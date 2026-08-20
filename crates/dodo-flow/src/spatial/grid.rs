//! [`UniformGrid`] — §21's spatial hash, over `u32` item keys and [`Rect`]
//! bounds.
//!
//! §21 names four candidates (uniform hash/grid, quadtree, R-tree, BVH), says
//! to "start with the simplest approach that performs well for interactive 2D
//! editor workloads, likely a spatial hash/grid, **then benchmark**". This is
//! that grid, and `examples/flow_scene_bench.rs` is the benchmark — it builds
//! the same scene through this, through a dense array grid and through a brute
//! force scan, and the numbers are recorded in [`crate::spatial`]'s module doc
//! rather than claimed here.
//!
//! # The layout, and why it is three flat arrays
//!
//! The obvious spatial hash is `HashMap<(i32, i32), Vec<u32>>`, which is two
//! allocations per occupied cell and a `Vec` header per cell that a 100,000
//! node scene pays 60,000 times over. This is the same structure without
//! either:
//!
//! ```text
//! buckets  [u32; 2^k]     head entry of each hash chain, or NONE
//! entries  Vec<Entry>     { cell, item, next } — one per (item, cell) pair
//! placed   Vec<Placement> where each item currently sits, for removal
//! ```
//!
//! One item that covers four cells is four `entries` and one `placed` row. A
//! removal reads `placed[item]`, walks those four chains and unlinks; nothing
//! scans. Freed entries go on a free list and are reused, so a drag that moves
//! a node across cells sixty times a second allocates nothing after the first
//! few frames (§40 rules 13 and 14).
//!
//! # Dedup without a visited set
//!
//! An item spanning several cells is found once per cell the query touches, and
//! a query that returned it four times would make every caller pay for a
//! `HashSet`. The fix is arithmetic rather than a set: the query knows the cell
//! it is reading, `placed[item]` knows the item's own cell range, and the item
//! is emitted **only from the first cell of the overlap** —
//! `(max(item.x0, query.x0), max(item.y0, query.y0))`. That cell is always
//! inside the query (otherwise the item would not have been found there), so
//! every item is emitted exactly once, in O(1), and the query needs no `&mut`
//! and no scratch state.
//!
//! # Oversized items
//!
//! An item wider than [`MAX_CELLS_PER_ITEM`] cells is not linked into cells at
//! all; it goes into `oversized` and is returned by every query. That bounds
//! the entry count against the one input that would otherwise explode it — a
//! single edge spanning the whole document is 10,000 cells at 100,000 nodes —
//! and it makes the failure mode a slower query rather than a gigabyte of
//! links. The count is public ([`UniformGrid::oversized_count`]) because it is
//! the number that says the cell size is wrong for this document.
//!
//! **This file names no UI framework.**

use crate::geometry::{Rect, Vec2};

/// The sentinel for "no entry" in a chain and in the bucket table.
const NONE: u32 = u32::MAX;

/// How many cells one item may be linked into before it is treated as
/// oversized instead.
///
/// An 8×8 span. A node is normally one or two cells and an edge between
/// neighbours a handful; a long edge crossing a large document is thousands,
/// and linking it would cost more than scanning it. See the module doc.
pub const MAX_CELLS_PER_ITEM: i64 = 64;

/// The initial bucket count. A power of two, because the hash is masked rather
/// than divided.
const INITIAL_BUCKETS: usize = 64;

/// The inclusive cell range an item covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CellRange {
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
}

impl CellRange {
    /// How many cells this range covers, as an `i64` so a pathological range
    /// cannot overflow the check that rejects it.
    fn cell_count(&self) -> i64 {
        let width = (self.x1 as i64 - self.x0 as i64) + 1;
        let height = (self.y1 as i64 - self.y0 as i64) + 1;
        width.max(0) * height.max(0)
    }
}

/// Where one item currently sits. Read on removal, on update and by the
/// query's dedup rule — see the module doc.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Placement {
    /// Never inserted, or removed.
    #[default]
    Absent,
    /// Linked into every cell of this inclusive range.
    Cells(CellRange),
    /// Too large to link; in `oversized`.
    Oversized,
}

/// One link between an item and a cell.
#[derive(Debug, Clone, Copy)]
struct Entry {
    cell_x: i32,
    cell_y: i32,
    item: u32,
    next: u32,
}

/// A uniform spatial hash over axis-aligned rectangles.
///
/// Items are `u32` keys — a [`NodeIndex`](crate::models::NodeIndex) or an
/// [`EdgeIndex`](crate::models::EdgeIndex) raw value — because the grid has no
/// business knowing which store it indexes. [`crate::spatial::SpatialIndex`] is
/// the typed layer above it.
#[derive(Debug, Clone)]
pub struct UniformGrid {
    cell_size: f32,
    inverse_cell_size: f32,
    buckets: Vec<u32>,
    entries: Vec<Entry>,
    /// Head of the free list threaded through `entries[..].next`.
    free: u32,
    live_entries: usize,
    placed: Vec<Placement>,
    oversized: Vec<u32>,
    len: usize,
}

impl UniformGrid {
    /// A grid whose cells are `cell_size` world units square.
    ///
    /// The cell size is the one tuning knob and it is not guessed:
    /// [`crate::spatial::SpatialIndex::cell_size_for`] derives it from the
    /// scene, and the benchmark sweeps it.
    pub fn new(cell_size: f32) -> UniformGrid {
        let cell_size = if cell_size.is_finite() && cell_size > 0.0 {
            cell_size
        } else {
            1.0
        };

        UniformGrid {
            cell_size,
            inverse_cell_size: 1.0 / cell_size,
            buckets: vec![NONE; INITIAL_BUCKETS],
            entries: Vec::new(),
            free: NONE,
            live_entries: 0,
            placed: Vec::new(),
            oversized: Vec::new(),
            len: 0,
        }
    }

    pub fn cell_size(&self) -> f32 {
        self.cell_size
    }

    /// How many items are in the index.
    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// How many (item, cell) links are live. The number that says whether the
    /// cell size fits the document: one or two per item is healthy.
    pub fn entry_count(&self) -> usize {
        self.live_entries
    }

    /// Items too large to link into cells, and therefore visited by every
    /// query. See the module doc.
    pub fn oversized_count(&self) -> usize {
        self.oversized.len()
    }

    pub fn bucket_count(&self) -> usize {
        self.buckets.len()
    }

    /// The grid's own heap footprint, in bytes. §41 asks for bounded memory
    /// behaviour to be a number rather than a caution, so this is a number.
    pub fn memory_bytes(&self) -> usize {
        self.buckets.capacity() * size_of::<u32>()
            + self.entries.capacity() * size_of::<Entry>()
            + self.placed.capacity() * size_of::<Placement>()
            + self.oversized.capacity() * size_of::<u32>()
    }

    /// Sizes the per-item table up front. The bucket table is sized with it, so
    /// a bulk build rehashes once rather than a dozen times.
    pub fn reserve(&mut self, items: usize) {
        self.placed
            .resize(self.placed.len().max(items), Placement::Absent);
        self.entries.reserve(items);
        // One bucket per item keeps the load factor near one for the common
        // case of an item covering one or two cells.
        let wanted = items.next_power_of_two().max(INITIAL_BUCKETS);
        if wanted > self.buckets.len() {
            self.rehash(wanted);
        }
    }

    pub fn clear(&mut self) {
        self.buckets.iter_mut().for_each(|head| *head = NONE);
        self.entries.clear();
        self.free = NONE;
        self.live_entries = 0;
        self.placed
            .iter_mut()
            .for_each(|slot| *slot = Placement::Absent);
        self.oversized.clear();
        self.len = 0;
    }

    /// Adds an item, or moves one that is already there.
    pub fn insert(&mut self, item: u32, bounds: Rect) {
        self.update(item, bounds);
    }

    /// Moves an item to new bounds, and reports whether the index actually
    /// changed.
    ///
    /// **`false` is the common case during a drag** and is the reason this
    /// returns anything: a node moved a few world units usually stays in the
    /// same cells, so the update is one range comparison and no link work at
    /// all. The benchmark prints the ratio.
    pub fn update(&mut self, item: u32, bounds: Rect) -> bool {
        let slot = item as usize;
        if slot >= self.placed.len() {
            self.placed.resize(slot + 1, Placement::Absent);
        }

        let next = self.placement_for(bounds);
        let previous = self.placed[slot];
        if previous == next {
            return false;
        }

        self.unplace(item, previous);
        self.place(item, next);
        if previous == Placement::Absent {
            self.len += 1;
        }
        self.placed[slot] = next;
        true
    }

    /// Removes an item. Absent items are ignored, so a caller draining a queue
    /// does not have to know what it has already removed.
    pub fn remove(&mut self, item: u32) {
        let slot = item as usize;
        let Some(&previous) = self.placed.get(slot) else {
            return;
        };
        if previous == Placement::Absent {
            return;
        }

        self.unplace(item, previous);
        self.placed[slot] = Placement::Absent;
        self.len -= 1;
    }

    pub fn contains(&self, item: u32) -> bool {
        self.placed
            .get(item as usize)
            .is_some_and(|slot| *slot != Placement::Absent)
    }

    // ---- queries ---------------------------------------------------------

    /// **The broad phase**: every item whose cells overlap `rect`, appended to
    /// `out` and each one exactly once.
    ///
    /// The result is a *candidate* set — cell granularity means an item whose
    /// bounds miss the rectangle can still share a cell with it. §21 asks for
    /// broad phase plus precise narrow phase, and the narrow phase is the
    /// caller's: [`GraphWorld::hit_test`](crate::runtime::GraphWorld::hit_test)
    /// for a point, [`Rect::intersects`] for a rectangle.
    ///
    /// `out` is not cleared. Callers hold one buffer across frames (§40 rule
    /// 14) and clear it themselves when they mean to.
    pub fn query_rect(&self, rect: Rect, out: &mut Vec<u32>) {
        let Some(query) = self.cell_range(rect) else {
            out.extend_from_slice(&self.oversized);
            return;
        };

        for cell_y in query.y0..=query.y1 {
            for cell_x in query.x0..=query.x1 {
                let mut cursor = self.buckets[self.bucket_of(cell_x, cell_y)];
                while cursor != NONE {
                    let entry = self.entries[cursor as usize];
                    cursor = entry.next;

                    if entry.cell_x != cell_x || entry.cell_y != cell_y {
                        // A different cell that happens to hash here.
                        continue;
                    }
                    if self.is_first_overlap_cell(entry.item, &query, cell_x, cell_y) {
                        out.push(entry.item);
                    }
                }
            }
        }

        out.extend_from_slice(&self.oversized);
    }

    /// The broad phase for a point (§29). One cell, plus the oversized list.
    pub fn query_point(&self, point: Vec2, out: &mut Vec<u32>) {
        self.query_rect(Rect::new(point, Vec2::ZERO), out);
    }

    /// §21's **nearby-element query**: the broad phase over a circle, which is
    /// the circle's bounding square. Snapping, proximity connection and "what
    /// is near the cursor" all want this, and all of them narrow it themselves.
    pub fn query_near(&self, center: Vec2, radius: f32, out: &mut Vec<u32>) {
        let radius = radius.max(0.0);
        self.query_rect(
            Rect::new(center - Vec2::splat(radius), Vec2::splat(radius * 2.0)),
            out,
        );
    }

    // ---- internals -------------------------------------------------------

    /// Whether `(cell_x, cell_y)` is the cell an item should be emitted from
    /// for this query — the module doc's dedup rule.
    fn is_first_overlap_cell(
        &self,
        item: u32,
        query: &CellRange,
        cell_x: i32,
        cell_y: i32,
    ) -> bool {
        match self.placed[item as usize] {
            Placement::Cells(item_cells) => {
                cell_x == item_cells.x0.max(query.x0) && cell_y == item_cells.y0.max(query.y0)
            }
            // An oversized item is never in a chain, and an absent one is never
            // reachable from one; both are unreachable here, and answering
            // `true` rather than panicking keeps a corrupted index visible as a
            // duplicate rather than as a crash in a paint loop.
            _ => true,
        }
    }

    fn cell_of(&self, coordinate: f32) -> Option<i32> {
        let cell = (coordinate * self.inverse_cell_size).floor();
        // A coordinate beyond `i32`'s range, or a NaN from a degenerate
        // document, is not clamped into a real cell — it is refused, and the
        // caller's item becomes oversized. Clamping would put every such item
        // in one corner cell and make that cell the whole index.
        (cell.is_finite() && cell >= i32::MIN as f32 && cell <= i32::MAX as f32)
            .then_some(cell as i32)
    }

    fn cell_range(&self, bounds: Rect) -> Option<CellRange> {
        let bounds = bounds.normalized();
        let min = bounds.min();
        let max = bounds.max();
        Some(CellRange {
            x0: self.cell_of(min.x)?,
            y0: self.cell_of(min.y)?,
            x1: self.cell_of(max.x)?,
            y1: self.cell_of(max.y)?,
        })
    }

    fn placement_for(&self, bounds: Rect) -> Placement {
        match self.cell_range(bounds) {
            Some(range) if range.cell_count() <= MAX_CELLS_PER_ITEM => Placement::Cells(range),
            _ => Placement::Oversized,
        }
    }

    fn bucket_of(&self, cell_x: i32, cell_y: i32) -> usize {
        // A multiply-xor mix, in the crate rather than from a dependency: dodo
        // is deliberate about every package in its graph, and what is needed
        // here is a few nanoseconds of avalanche over two `i32`s, not a hash
        // function with properties.
        let x = (cell_x as u32).wrapping_mul(0x9E37_79B1);
        let y = (cell_y as u32).wrapping_mul(0x85EB_CA77);
        let mixed = x ^ y.rotate_left(16);
        (mixed ^ (mixed >> 15)) as usize & (self.buckets.len() - 1)
    }

    fn place(&mut self, item: u32, placement: Placement) {
        match placement {
            Placement::Absent => {}
            Placement::Oversized => self.oversized.push(item),
            Placement::Cells(range) => {
                if self.live_entries + range.cell_count() as usize > self.buckets.len() {
                    self.rehash((self.buckets.len() * 2).max(INITIAL_BUCKETS));
                }
                for cell_y in range.y0..=range.y1 {
                    for cell_x in range.x0..=range.x1 {
                        self.link(item, cell_x, cell_y);
                    }
                }
            }
        }
    }

    fn unplace(&mut self, item: u32, placement: Placement) {
        match placement {
            Placement::Absent => {}
            Placement::Oversized => {
                if let Some(at) = self.oversized.iter().position(|&other| other == item) {
                    self.oversized.swap_remove(at);
                }
            }
            Placement::Cells(range) => {
                for cell_y in range.y0..=range.y1 {
                    for cell_x in range.x0..=range.x1 {
                        self.unlink(item, cell_x, cell_y);
                    }
                }
            }
        }
    }

    fn link(&mut self, item: u32, cell_x: i32, cell_y: i32) {
        let bucket = self.bucket_of(cell_x, cell_y);
        let head = self.buckets[bucket];

        let index = if self.free != NONE {
            let index = self.free;
            self.free = self.entries[index as usize].next;
            self.entries[index as usize] = Entry {
                cell_x,
                cell_y,
                item,
                next: head,
            };
            index
        } else {
            self.entries.push(Entry {
                cell_x,
                cell_y,
                item,
                next: head,
            });
            (self.entries.len() - 1) as u32
        };

        self.buckets[bucket] = index;
        self.live_entries += 1;
    }

    fn unlink(&mut self, item: u32, cell_x: i32, cell_y: i32) {
        let bucket = self.bucket_of(cell_x, cell_y);
        let mut previous = NONE;
        let mut cursor = self.buckets[bucket];

        while cursor != NONE {
            let entry = self.entries[cursor as usize];
            if entry.item == item && entry.cell_x == cell_x && entry.cell_y == cell_y {
                if previous == NONE {
                    self.buckets[bucket] = entry.next;
                } else {
                    self.entries[previous as usize].next = entry.next;
                }
                self.entries[cursor as usize].next = self.free;
                self.free = cursor;
                self.live_entries -= 1;
                return;
            }
            previous = cursor;
            cursor = entry.next;
        }
    }

    /// Rebuilds the bucket table at a new size, keeping every live entry.
    ///
    /// Walks the existing chains rather than the `entries` array, because the
    /// array also holds the free list and a freed entry re-linked here would
    /// resurrect a removed item.
    fn rehash(&mut self, buckets: usize) {
        let mut live = Vec::with_capacity(self.live_entries);
        for bucket in 0..self.buckets.len() {
            let mut cursor = self.buckets[bucket];
            while cursor != NONE {
                live.push(cursor);
                cursor = self.entries[cursor as usize].next;
            }
        }

        self.buckets = vec![NONE; buckets];
        for index in live {
            let entry = self.entries[index as usize];
            let bucket = self.bucket_of(entry.cell_x, entry.cell_y);
            self.entries[index as usize].next = self.buckets[bucket];
            self.buckets[bucket] = index;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The oracle every correctness test here is checked against.
    ///
    /// **This is the one legitimate place in the crate for a linear scan** —
    /// as the reference a fast structure is compared to, never as the
    /// structure. `render/scene.rs` and `views/flow.rs` must never grow one.
    fn brute_force(items: &[(u32, Rect)], rect: Rect) -> Vec<u32> {
        let mut found: Vec<u32> = items
            .iter()
            .filter(|(_, bounds)| bounds.intersects(rect))
            .map(|(item, _)| *item)
            .collect();
        found.sort_unstable();
        found
    }

    /// A deterministic scatter of rectangles, from a linear congruential
    /// generator written out rather than depended on.
    fn scatter(count: u32, extent: f32, max_size: f32) -> Vec<(u32, Rect)> {
        let mut state = 0x2545_F491u32;
        let mut next = move || {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state >> 8) as f32 / (1 << 24) as f32
        };

        (0..count)
            .map(|item| {
                let x = next() * extent - extent * 0.5;
                let y = next() * extent - extent * 0.5;
                let w = 4.0 + next() * max_size;
                let h = 4.0 + next() * max_size;
                (item, Rect::new(Vec2::new(x, y), Vec2::new(w, h)))
            })
            .collect()
    }

    fn queried(grid: &UniformGrid, items: &[(u32, Rect)], rect: Rect) -> Vec<u32> {
        let mut candidates = Vec::new();
        grid.query_rect(rect, &mut candidates);

        // The narrow phase, which is what makes the answer comparable to the
        // oracle: the grid returns candidates, not hits.
        let mut hits: Vec<u32> = candidates
            .iter()
            .filter(|item| items[**item as usize].1.intersects(rect))
            .copied()
            .collect();
        hits.sort_unstable();
        hits
    }

    #[test]
    fn a_rect_query_agrees_with_brute_force_everywhere() {
        let items = scatter(2_000, 4_000.0, 120.0);
        let mut grid = UniformGrid::new(128.0);
        grid.reserve(items.len());
        for (item, bounds) in &items {
            grid.insert(*item, *bounds);
        }

        for step in 0..40 {
            let offset = step as f32 * 97.0 - 2_000.0;
            let rect = Rect::new(
                Vec2::new(offset, offset * 0.7),
                Vec2::new(200.0 + step as f32 * 13.0, 150.0),
            );
            assert_eq!(
                queried(&grid, &items, rect),
                brute_force(&items, rect),
                "cell query disagreed with the oracle at {rect:?}"
            );
        }
    }

    #[test]
    fn every_candidate_is_returned_exactly_once() {
        // Cells far smaller than the items, so every item spans many cells and
        // the dedup rule is actually exercised.
        let items = scatter(500, 1_000.0, 200.0);
        let mut grid = UniformGrid::new(32.0);
        for (item, bounds) in &items {
            grid.insert(*item, *bounds);
        }

        let mut candidates = Vec::new();
        grid.query_rect(
            Rect::new(Vec2::splat(-600.0), Vec2::splat(1_200.0)),
            &mut candidates,
        );

        let mut sorted = candidates.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), candidates.len(), "an item was returned twice");
    }

    #[test]
    fn a_point_query_finds_what_covers_it_and_nothing_else() {
        let items = scatter(1_000, 2_000.0, 80.0);
        let mut grid = UniformGrid::new(64.0);
        for (item, bounds) in &items {
            grid.insert(*item, *bounds);
        }

        for step in 0..50 {
            let point = Vec2::new(step as f32 * 37.0 - 900.0, step as f32 * -21.0 + 400.0);
            let mut candidates = Vec::new();
            grid.query_point(point, &mut candidates);

            let mut hits: Vec<u32> = candidates
                .iter()
                .filter(|item| items[**item as usize].1.contains_point(point))
                .copied()
                .collect();
            hits.sort_unstable();

            let mut expected: Vec<u32> = items
                .iter()
                .filter(|(_, bounds)| bounds.contains_point(point))
                .map(|(item, _)| *item)
                .collect();
            expected.sort_unstable();

            assert_eq!(hits, expected, "point query disagreed at {point:?}");
        }
    }

    #[test]
    fn a_nearby_query_covers_the_whole_circle() {
        let mut grid = UniformGrid::new(50.0);
        grid.insert(0, Rect::new(Vec2::new(0.0, 0.0), Vec2::splat(10.0)));
        grid.insert(1, Rect::new(Vec2::new(90.0, 0.0), Vec2::splat(10.0)));
        grid.insert(2, Rect::new(Vec2::new(400.0, 0.0), Vec2::splat(10.0)));

        let mut candidates = Vec::new();
        grid.query_near(Vec2::new(5.0, 5.0), 100.0, &mut candidates);
        candidates.sort_unstable();

        assert!(candidates.contains(&0));
        assert!(
            candidates.contains(&1),
            "an item inside the radius was missed"
        );
        assert!(!candidates.contains(&2), "an item far outside was returned");
    }

    #[test]
    fn moving_an_item_moves_what_the_query_finds() {
        let mut grid = UniformGrid::new(100.0);
        let start = Rect::new(Vec2::new(0.0, 0.0), Vec2::splat(50.0));
        let end = Rect::new(Vec2::new(900.0, 900.0), Vec2::splat(50.0));
        grid.insert(7, start);

        let mut found = Vec::new();
        grid.update(7, end);
        grid.query_rect(start, &mut found);
        assert!(found.is_empty(), "the item was still in its old cells");

        found.clear();
        grid.query_rect(end, &mut found);
        assert_eq!(found, vec![7]);
        assert_eq!(grid.len(), 1, "an update must not duplicate the item");
    }

    /// The property the drag path depends on: a small move inside one cell is
    /// not an index write at all.
    #[test]
    fn a_move_that_stays_in_the_same_cells_is_free() {
        let mut grid = UniformGrid::new(256.0);
        grid.insert(3, Rect::new(Vec2::new(10.0, 10.0), Vec2::splat(40.0)));

        assert!(!grid.update(3, Rect::new(Vec2::new(11.0, 12.0), Vec2::splat(40.0))));
        assert!(grid.update(3, Rect::new(Vec2::new(1_000.0, 12.0), Vec2::splat(40.0))));
    }

    #[test]
    fn removal_leaves_nothing_behind_and_reuses_its_entries() {
        let mut grid = UniformGrid::new(64.0);
        for item in 0..200u32 {
            grid.insert(
                item,
                Rect::new(Vec2::splat(item as f32 * 30.0), Vec2::splat(90.0)),
            );
        }
        let peak = grid.entry_count();

        for item in 0..200u32 {
            grid.remove(item);
        }
        assert_eq!(grid.len(), 0);
        assert_eq!(grid.entry_count(), 0);

        // Re-inserting the same items must reuse the freed entries rather than
        // growing the array.
        let entries_before = grid.entries.len();
        for item in 0..200u32 {
            grid.insert(
                item,
                Rect::new(Vec2::splat(item as f32 * 30.0), Vec2::splat(90.0)),
            );
        }
        assert_eq!(grid.entry_count(), peak);
        assert_eq!(
            grid.entries.len(),
            entries_before,
            "the free list was not reused"
        );
    }

    #[test]
    fn removing_an_absent_item_is_a_no_op() {
        let mut grid = UniformGrid::new(64.0);
        grid.remove(0);
        grid.remove(9_999);
        assert_eq!(grid.len(), 0);
    }

    #[test]
    fn an_item_larger_than_the_cell_budget_becomes_oversized_and_is_always_found() {
        let mut grid = UniformGrid::new(10.0);
        // 1,000 × 1,000 units at a 10-unit cell is 10,000 cells, far past the
        // budget.
        grid.insert(1, Rect::new(Vec2::ZERO, Vec2::splat(1_000.0)));

        assert_eq!(grid.oversized_count(), 1);
        assert_eq!(
            grid.entry_count(),
            0,
            "an oversized item must not be linked"
        );

        let mut found = Vec::new();
        grid.query_rect(
            Rect::new(Vec2::splat(-9_000.0), Vec2::splat(5.0)),
            &mut found,
        );
        assert_eq!(
            found,
            vec![1],
            "an oversized item is returned by every query"
        );

        // And it can leave the oversized list again.
        grid.update(1, Rect::new(Vec2::ZERO, Vec2::splat(5.0)));
        assert_eq!(grid.oversized_count(), 0);
        assert_eq!(grid.entry_count(), 1);
    }

    #[test]
    fn a_non_finite_bound_is_oversized_rather_than_a_cell_at_infinity() {
        let mut grid = UniformGrid::new(32.0);
        grid.insert(0, Rect::new(Vec2::new(f32::NAN, 0.0), Vec2::splat(10.0)));
        grid.insert(1, Rect::new(Vec2::ZERO, Vec2::new(f32::INFINITY, 10.0)));

        assert_eq!(grid.oversized_count(), 2);
        assert_eq!(grid.len(), 2);
    }

    #[test]
    fn a_query_with_a_degenerate_rect_still_returns_the_oversized_items() {
        let mut grid = UniformGrid::new(32.0);
        grid.insert(0, Rect::new(Vec2::ZERO, Vec2::splat(10_000.0)));

        let mut found = Vec::new();
        grid.query_rect(Rect::new(Vec2::new(f32::NAN, 0.0), Vec2::ZERO), &mut found);
        assert_eq!(found, vec![0]);
    }

    #[test]
    fn the_index_survives_growing_past_its_bucket_table() {
        let items = scatter(5_000, 20_000.0, 60.0);
        let mut grid = UniformGrid::new(200.0);
        for (item, bounds) in &items {
            grid.insert(*item, *bounds);
        }
        assert!(grid.bucket_count() >= grid.entry_count() / 2);

        let rect = Rect::new(Vec2::splat(-3_000.0), Vec2::splat(6_000.0));
        assert_eq!(queried(&grid, &items, rect), brute_force(&items, rect));
    }

    #[test]
    fn a_rehash_does_not_resurrect_a_removed_item() {
        let mut grid = UniformGrid::new(10.0);
        for item in 0..40u32 {
            grid.insert(
                item,
                Rect::new(Vec2::splat(item as f32 * 25.0), Vec2::splat(5.0)),
            );
        }
        for item in 0..20u32 {
            grid.remove(item);
        }
        // Force the table to grow with the free list populated.
        for item in 100..900u32 {
            grid.insert(
                item,
                Rect::new(Vec2::splat(item as f32 * 25.0), Vec2::splat(5.0)),
            );
        }

        let mut found = Vec::new();
        grid.query_rect(
            Rect::new(Vec2::splat(-1_000.0), Vec2::splat(50_000.0)),
            &mut found,
        );
        found.sort_unstable();
        assert!(
            !found.iter().any(|item| *item < 20),
            "a removed item came back through a rehash: {found:?}"
        );
        assert_eq!(found.len(), grid.len());
    }

    #[test]
    fn a_zero_or_negative_cell_size_does_not_divide_by_zero() {
        for cell in [0.0, -8.0, f32::NAN] {
            let mut grid = UniformGrid::new(cell);
            grid.insert(0, Rect::new(Vec2::ZERO, Vec2::splat(1.0)));
            let mut found = Vec::new();
            grid.query_rect(Rect::new(Vec2::ZERO, Vec2::splat(1.0)), &mut found);
            assert_eq!(found, vec![0], "cell size {cell} lost its item");
        }
    }

    #[test]
    fn memory_is_reported_and_grows_with_the_index() {
        let mut grid = UniformGrid::new(64.0);
        let empty = grid.memory_bytes();
        for item in 0..1_000u32 {
            grid.insert(
                item,
                Rect::new(Vec2::splat(item as f32 * 10.0), Vec2::splat(40.0)),
            );
        }
        assert!(grid.memory_bytes() > empty);
    }
}
