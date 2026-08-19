//! [`SpatialIndex`] — the typed layer over [`UniformGrid`], and the thing that
//! makes painting correct rather than fast.
//!
//! # Why this is a correctness precondition
//!
//! macOS stops presenting the drawable past ~2.58 M path vertices in a frame
//! and the window goes **solid black** ([`crate::budgets`]). GPUI's content
//! mask does not save us: rejection happens *after* `paint_path` has cloned and
//! scaled the vertex buffer, so Phase 0 measured 16,000 fully offscreen paths
//! still costing 6.3 ms of CPU per frame. The visible set computed here is what
//! stops both, and [`crate::render::scene`] is where it is spent.
//!
//! # Painted bounds, not geometric bounds
//!
//! An element is indexed at the bounds it **paints**, not the bounds it
//! occupies: a node's rectangle plus half its stroke, an edge's control hull
//! plus half its stroke plus its arrow markers. That is done at insert time,
//! where the style is already in hand, and it is what lets the viewport query
//! be a plain rectangle test instead of carrying a running maximum stroke width
//! around the engine.
//!
//! The only overhang left is the part that is constant in **screen** pixels and
//! therefore changes with zoom — a handle dot, a selection outline. That is
//! [`SCREEN_PAINT_MARGIN_PIXELS`], applied to the query rectangle at the
//! current zoom rather than to the index.
//!
//! # What must mark an element for re-indexing
//!
//! [`SpatialIndex::sync`] drains
//! [`DirtyState::spatial_updates`](crate::runtime::DirtyState::spatial_updates)
//! and its edge twin, so anything that changes an element's *painted* bounds
//! has to set `NodeDirty::SPATIAL` / `EdgeDirty::SPATIAL`. Position and size do
//! (`GraphWorld::move_node`, `set_node_position`, `set_node_size`), and an edge
//! whose geometry is invalidated does. **A style change that widens a stroke
//! does not yet**, because nothing in the crate mutates a style through the
//! world — `NodeStore::style_mut` is the only route and it bypasses the dirty
//! state entirely. Phase 7's command layer is what closes that, and this
//! paragraph is the note it needs.
//!
//! **This file names no UI framework.**

use crate::{
    geometry::{Rect, Vec2, Viewport, arrow},
    models::{EdgeIndex, NodeIndex},
    runtime::GraphWorld,
    spatial::UniformGrid,
};

/// The screen-constant overhang a query rectangle must allow for, in pixels.
///
/// Handles are drawn at a fixed screen radius so they stay grabbable at any
/// zoom, and a selected element's outline is a fixed screen width; both are
/// therefore invisible to the world-space bounds in the index. Twelve pixels
/// covers the 4.5 px handle dot, the 2 px selection stroke and a little slack,
/// and it costs candidates rather than correctness — the narrow phase in
/// [`crate::render::scene`] rejects whatever it lets through.
pub const SCREEN_PAINT_MARGIN_PIXELS: f32 = 12.0;

/// The cell size used when a document is too small to derive one from.
pub const DEFAULT_CELL_SIZE: f32 = 256.0;

/// What one [`SpatialIndex::sync`] did. Printed by the benchmark harness and
/// asserted by the drag tests: **`moved` is the number that matters**, because
/// a node that stays inside its cells is not an index write at all.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SyncReport {
    /// Nodes the dirty state queued.
    pub nodes_queued: u32,
    /// Of those, the ones whose cells actually changed.
    pub nodes_moved: u32,
    pub edges_queued: u32,
    pub edges_moved: u32,
}

impl SyncReport {
    pub fn is_empty(&self) -> bool {
        self.nodes_queued == 0 && self.edges_queued == 0
    }
}

/// What the viewport can see, as compact runtime indices (§28, §41) — never
/// cloned elements.
///
/// Owned by the caller and refilled per frame, so a pan allocates nothing after
/// the first few frames (§40 rules 13 and 14). The scratch candidate buffer
/// lives here for the same reason.
#[derive(Debug, Clone, Default)]
pub struct VisibleSet {
    nodes: Vec<NodeIndex>,
    edges: Vec<EdgeIndex>,
    scratch: Vec<u32>,
    /// The world rectangle this set answers for — the viewport plus the screen
    /// margin. Kept so a caller can tell what it was culled against.
    query: Rect,
    /// Broad-phase candidates before the narrow phase rejected any. The
    /// benchmark prints the ratio; a ratio far from 1 means the cell size is
    /// wrong for the document.
    candidates: u32,
}

impl VisibleSet {
    pub fn new() -> VisibleSet {
        VisibleSet::default()
    }

    /// **Painted in insertion order**, which is what keeps overlapping elements
    /// stacked the way the document says. The broad phase returns them in cell
    /// order, so [`SpatialIndex::query_visible`] sorts.
    pub fn nodes(&self) -> &[NodeIndex] {
        &self.nodes
    }

    pub fn edges(&self) -> &[EdgeIndex] {
        &self.edges
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty() && self.edges.is_empty()
    }

    /// The world rectangle the set was culled against — the visible world
    /// rectangle inflated by [`SCREEN_PAINT_MARGIN_PIXELS`] at the current
    /// zoom.
    pub fn query_rect(&self) -> Rect {
        self.query
    }

    /// How many candidates the broad phase produced, against
    /// [`node_count`](VisibleSet::node_count) +
    /// [`edge_count`](VisibleSet::edge_count) that survived the narrow phase.
    pub fn candidate_count(&self) -> u32 {
        self.candidates
    }

    fn clear(&mut self) {
        self.nodes.clear();
        self.edges.clear();
        self.scratch.clear();
        self.candidates = 0;
    }
}

/// §21's spatial index: one uniform grid for nodes, one for edges.
///
/// Two grids rather than one because the two are queried separately and their
/// bounds have wildly different sizes — a node is a rectangle a couple of cells
/// wide, an edge's control hull can be a screen across — and mixing them would
/// make one cell size wrong for both.
#[derive(Debug, Clone)]
pub struct SpatialIndex {
    nodes: UniformGrid,
    edges: UniformGrid,
}

impl SpatialIndex {
    pub fn new(cell_size: f32) -> SpatialIndex {
        SpatialIndex {
            nodes: UniformGrid::new(cell_size),
            // Edges are indexed by their control hull, which is routinely
            // several node-widths long, so their grid is coarser: a finer one
            // links each edge into more cells for no better rejection.
            edges: UniformGrid::new(cell_size * 2.0),
        }
    }

    /// An index sized for this world and built from it. See [`cell_size_for`],
    /// and [`rebuild`](SpatialIndex::rebuild) for the call that must follow.
    pub fn for_world(world: &GraphWorld) -> SpatialIndex {
        let mut index = SpatialIndex::new(cell_size_for(world));
        index.rebuild(world);
        index
    }

    pub fn nodes(&self) -> &UniformGrid {
        &self.nodes
    }

    pub fn edges(&self) -> &UniformGrid {
        &self.edges
    }

    /// The index's own heap footprint, in bytes (§41).
    pub fn memory_bytes(&self) -> usize {
        self.nodes.memory_bytes() + self.edges.memory_bytes()
    }

    /// **Builds the index from scratch.** For a freshly loaded document; never
    /// on a frame path, which is what [`sync`](SpatialIndex::sync) is for.
    ///
    /// Edges with no current route are skipped rather than indexed at a stale
    /// place: `GraphWorld::rebuild_all_geometry` is the caller's job first.
    ///
    /// **Follow it with [`GraphWorld::clear_spatial_updates`].** Building the
    /// document queued every element for a spatial update, and a rebuild has
    /// already answered all of them — leaving the queues full would make the
    /// next `sync` re-index the whole document one element at a time.
    pub fn rebuild(&mut self, world: &GraphWorld) {
        self.nodes.clear();
        self.edges.clear();
        self.nodes.reserve(world.nodes().len());
        self.edges.reserve(world.edges().len());

        for node in world.nodes().indices() {
            self.nodes
                .insert(node.raw(), node_painted_bounds(world, node));
        }
        for edge in world.edges().indices() {
            if let Some(bounds) = edge_painted_bounds(world, edge) {
                self.edges.insert(edge.raw(), bounds);
            }
        }
    }

    /// **Applies what changed since the last frame**, from the dirty state's
    /// spatial queues.
    ///
    /// Takes the world by shared reference so it can read the queues and the
    /// stores at once; the caller clears the queues afterwards with
    /// [`GraphWorld::clear_spatial_updates`]. It must run **after**
    /// [`GraphWorld::rebuild_dirty_geometry`], because an edge's new bounds
    /// come from its new route.
    pub fn sync(&mut self, world: &GraphWorld) -> SyncReport {
        let mut report = SyncReport::default();

        for &node in world.dirty().spatial_updates() {
            report.nodes_queued += 1;
            if world.nodes().contains(node) {
                if self
                    .nodes
                    .update(node.raw(), node_painted_bounds(world, node))
                {
                    report.nodes_moved += 1;
                }
            } else {
                self.nodes.remove(node.raw());
            }
        }

        for &edge in world.dirty().edge_spatial_updates() {
            report.edges_queued += 1;
            match edge_painted_bounds(world, edge) {
                Some(bounds) => {
                    if self.edges.update(edge.raw(), bounds) {
                        report.edges_moved += 1;
                    }
                }
                None => self.edges.remove(edge.raw()),
            }
        }

        report
    }

    /// Records a newly added node. The world's stores grow without telling the
    /// index, so this is the one call an add path has to make.
    pub fn insert_node(&mut self, world: &GraphWorld, node: NodeIndex) {
        self.nodes
            .insert(node.raw(), node_painted_bounds(world, node));
    }

    /// Records a newly added edge, if its route is current.
    pub fn insert_edge(&mut self, world: &GraphWorld, edge: EdgeIndex) {
        if let Some(bounds) = edge_painted_bounds(world, edge) {
            self.edges.insert(edge.raw(), bounds);
        }
    }

    // ---- queries ---------------------------------------------------------

    /// **§16's query**: the nodes and edges the viewport can paint, refilled
    /// into `out`.
    ///
    /// Broad phase from the grid, then a world-space narrow phase against the
    /// same rectangle — cell granularity always over-reports, and paying one
    /// rectangle test per candidate here is much cheaper than paying a
    /// tessellation for it later.
    ///
    /// Hidden elements are dropped here rather than in the painter: "visible"
    /// means "could be painted", and a caller that wants the geometric answer
    /// wants [`node_candidates`](SpatialIndex::node_candidates).
    pub fn query_visible(&self, world: &GraphWorld, viewport: &Viewport, out: &mut VisibleSet) {
        out.clear();
        let query = query_rect(viewport);
        out.query = query;

        self.nodes.query_rect(query, &mut out.scratch);
        out.candidates += out.scratch.len() as u32;
        for &raw in &out.scratch {
            let node = NodeIndex::new(raw);
            if world.nodes().is_hidden(node) {
                continue;
            }
            if node_painted_bounds(world, node).intersects(query) {
                out.nodes.push(node);
            }
        }

        out.scratch.clear();
        self.edges.query_rect(query, &mut out.scratch);
        out.candidates += out.scratch.len() as u32;
        for &raw in &out.scratch {
            let edge = EdgeIndex::new(raw);
            if world.edges().is_hidden(edge) {
                continue;
            }
            if edge_painted_bounds(world, edge).is_some_and(|bounds| bounds.intersects(query)) {
                out.edges.push(edge);
            }
        }
        out.scratch.clear();

        // The grid answers in cell order; the painter needs insertion order so
        // that overlapping elements stack the way the document says.
        out.nodes.sort_unstable();
        out.edges.sort_unstable();
    }

    /// The broad phase for a rectangle, as node indices — §28's box selection
    /// and anything else that starts from an area.
    ///
    /// `out` is not cleared, so a caller can accumulate across several
    /// rectangles.
    pub fn node_candidates(&self, rect: Rect, out: &mut Vec<NodeIndex>) {
        self.collect(&self.nodes, rect, out, NodeIndex::new);
    }

    pub fn edge_candidates(&self, rect: Rect, out: &mut Vec<EdgeIndex>) {
        self.collect(&self.edges, rect, out, EdgeIndex::new);
    }

    /// **§29's broad phase**: the nodes whose cells cover `point`, generously
    /// enough that a handle just outside a node's own bounds is still found.
    ///
    /// `tolerance` is the world-space hit tolerance the narrow phase will use;
    /// passing it here is what stops the broad phase from rejecting a handle
    /// the narrow phase would have accepted.
    pub fn nodes_at(&self, point: Vec2, tolerance: f32, out: &mut Vec<NodeIndex>) {
        let tolerance = tolerance.max(0.0);
        self.node_candidates(
            Rect::new(point - Vec2::splat(tolerance), Vec2::splat(tolerance * 2.0)),
            out,
        );
    }

    /// §21's **nearby-node query**: everything within `radius` of `center`,
    /// broad phase only. Snapping and proximity connection are its callers,
    /// and both narrow it themselves.
    pub fn nodes_near(&self, center: Vec2, radius: f32, out: &mut Vec<NodeIndex>) {
        let mut scratch = Vec::new();
        self.nodes.query_near(center, radius, &mut scratch);
        out.extend(scratch.into_iter().map(NodeIndex::new));
    }

    fn collect<T>(
        &self,
        grid: &UniformGrid,
        rect: Rect,
        out: &mut Vec<T>,
        wrap: impl Fn(u32) -> T,
    ) {
        let mut scratch = Vec::new();
        grid.query_rect(rect, &mut scratch);
        out.extend(scratch.into_iter().map(wrap));
    }
}

/// The world rectangle a viewport must query, including the screen-constant
/// overhang. See [`SCREEN_PAINT_MARGIN_PIXELS`].
pub fn query_rect(viewport: &Viewport) -> Rect {
    viewport
        .visible_world_rect()
        .inflate(viewport.screen_to_world_length(SCREEN_PAINT_MARGIN_PIXELS))
}

/// A node's bounds **as painted**: its rectangle plus half its stroke.
pub fn node_painted_bounds(world: &GraphWorld, node: NodeIndex) -> Rect {
    let style = world.nodes().style(node);
    world
        .nodes()
        .bounds(node)
        .inflate(style.stroke.width.max(0.0) * 0.5)
}

/// An edge's bounds **as painted**: its route's control hull plus half its
/// stroke plus whatever an arrow marker adds, or `None` when its route is
/// stale or absent.
///
/// The control hull is already a true bound rather than a tight one — a cubic
/// never leaves it — so a false "visible" costs one wasted path and a false
/// "hidden" would be a missing edge. Erring outward here is the whole point.
pub fn edge_painted_bounds(world: &GraphWorld, edge: EdgeIndex) -> Option<Rect> {
    let route = world.route(edge)?;
    let style = world.edges().style(edge);
    let width = style.stroke.width.max(0.0);
    let marker = if style.start_marker == crate::models::ArrowMarker::None
        && style.end_marker == crate::models::ArrowMarker::None
    {
        0.0
    } else {
        arrow::marker_length(width)
    };

    Some(route.bounds().inflate(width * 0.5 + marker))
}

/// A cell size derived from the document rather than guessed.
///
/// Twice the mean node width, which puts a typical node in one or two cells and
/// keeps the entry count near the item count. §21 asks for the structure to be
/// benchmarked rather than asserted, and `examples/flow_scene_bench.rs` sweeps
/// this — the sweep is what says twice is the right multiple, and
/// [`crate::spatial`]'s module doc records the numbers.
pub fn cell_size_for(world: &GraphWorld) -> f32 {
    let sizes = world.nodes().sizes();
    if sizes.is_empty() {
        return DEFAULT_CELL_SIZE;
    }

    // Sampled rather than summed: a 100,000-node document does not need an
    // exact mean to pick a cell size, and this runs on every document open.
    let step = (sizes.len() / 1_000).max(1);
    let mut total = 0.0f32;
    let mut count = 0u32;
    let mut cursor = 0usize;
    while cursor < sizes.len() {
        let size = sizes[cursor];
        if size.x.is_finite() && size.y.is_finite() {
            total += size.x.max(size.y);
            count += 1;
        }
        cursor += step;
    }

    if count == 0 {
        return DEFAULT_CELL_SIZE;
    }
    (total / count as f32 * 2.0).clamp(16.0, 8_192.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        geometry::Vec2,
        models::{ElementKind, GraphNodeKind},
        runtime::{ConnectionRules, EdgeEnd},
    };

    fn grid_world(columns: u32, rows: u32) -> GraphWorld {
        let mut world = GraphWorld::new();
        world.set_rules(ConnectionRules::PERMISSIVE);
        for row in 0..rows {
            for column in 0..columns {
                world.create_node(
                    ElementKind::GraphNode(GraphNodeKind::Default),
                    Vec2::new(column as f32 * 240.0, row as f32 * 140.0),
                    Vec2::new(160.0, 60.0),
                );
            }
        }
        for index in 0..(columns * rows).saturating_sub(1) {
            world
                .connect(
                    EdgeEnd::node(NodeIndex::new(index)),
                    EdgeEnd::node(NodeIndex::new(index + 1)),
                )
                .expect("permissive rules accept it");
        }
        world.rebuild_all_geometry();
        // Building the document queued every element; a caller that then
        // rebuilds the index owes this call. See `SpatialIndex::rebuild`.
        world.clear_spatial_updates();
        world
    }

    /// **The oracle.** A linear scan is legitimate exactly here — as the
    /// reference a spatial query is checked against — and nowhere in the
    /// engine.
    fn visible_by_scan(
        world: &GraphWorld,
        viewport: &Viewport,
    ) -> (Vec<NodeIndex>, Vec<EdgeIndex>) {
        let query = query_rect(viewport);
        let nodes = world
            .nodes()
            .indices()
            .filter(|node| !world.nodes().is_hidden(*node))
            .filter(|node| node_painted_bounds(world, *node).intersects(query))
            .collect();
        let edges = world
            .edges()
            .indices()
            .filter(|edge| !world.edges().is_hidden(*edge))
            .filter(|edge| {
                edge_painted_bounds(world, *edge).is_some_and(|bounds| bounds.intersects(query))
            })
            .collect();
        (nodes, edges)
    }

    #[test]
    fn the_visible_set_matches_a_brute_force_scan_at_every_camera() {
        let world = grid_world(40, 40);
        let index = SpatialIndex::for_world(&world);
        let mut visible = VisibleSet::new();

        for step in 0..25 {
            let mut viewport = Viewport::new(
                Vec2::new(step as f32 * -310.0, step as f32 * -190.0),
                if step % 3 == 0 { 0.35 } else { 1.0 },
                Vec2::new(1_440.0, 900.0),
            );
            viewport.set_size(Vec2::new(1_440.0, 900.0));

            index.query_visible(&world, &viewport, &mut visible);
            let (nodes, edges) = visible_by_scan(&world, &viewport);

            assert_eq!(
                visible.nodes(),
                nodes.as_slice(),
                "nodes differed at step {step}"
            );
            assert_eq!(
                visible.edges(),
                edges.as_slice(),
                "edges differed at step {step}"
            );
        }
    }

    #[test]
    fn a_hundred_thousand_node_document_yields_a_screenful() {
        let world = grid_world(400, 250);
        assert_eq!(world.nodes().len(), 100_000);

        let index = SpatialIndex::for_world(&world);
        let mut visible = VisibleSet::new();
        let viewport = Viewport::new(Vec2::ZERO, 1.0, Vec2::new(1_440.0, 900.0));
        index.query_visible(&world, &viewport, &mut visible);

        // §16's rule, as a number: a 100,000-node document must not produce
        // thousands of visible elements at 1:1.
        assert!(
            visible.node_count() < 100,
            "a 1440x900 viewport saw {} of 100,000 nodes",
            visible.node_count()
        );
        assert!(visible.node_count() > 0, "the viewport saw nothing at all");
    }

    #[test]
    fn the_visible_set_is_in_insertion_order() {
        let world = grid_world(20, 20);
        let index = SpatialIndex::for_world(&world);
        let mut visible = VisibleSet::new();
        index.query_visible(
            &world,
            &Viewport::new(Vec2::ZERO, 0.5, Vec2::new(1_440.0, 900.0)),
            &mut visible,
        );

        assert!(visible.nodes().windows(2).all(|pair| pair[0] < pair[1]));
        assert!(visible.edges().windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn a_hidden_element_is_not_visible() {
        let mut world = grid_world(6, 6);
        let index = SpatialIndex::for_world(&world);
        let viewport = Viewport::new(Vec2::ZERO, 1.0, Vec2::new(1_440.0, 900.0));

        let mut visible = VisibleSet::new();
        index.query_visible(&world, &viewport, &mut visible);
        let before = visible.node_count();

        world.set_node_hidden(NodeIndex::new(0), true);
        index.query_visible(&world, &viewport, &mut visible);
        assert_eq!(visible.node_count(), before - 1);
    }

    #[test]
    fn moving_a_node_moves_it_in_the_index_and_takes_its_edges_with_it() {
        let mut world = grid_world(10, 10);
        let mut index = SpatialIndex::for_world(&world);
        let subject = NodeIndex::new(0);

        world.move_node(subject, Vec2::new(50_000.0, 50_000.0));
        world.rebuild_dirty_geometry();
        let report = index.sync(&world);
        world.clear_spatial_updates();

        assert_eq!(report.nodes_queued, 1);
        assert_eq!(report.nodes_moved, 1);
        assert_eq!(
            report.edges_queued, 1,
            "only its own edge should be re-indexed"
        );
        assert_eq!(report.edges_moved, 1);

        let mut visible = VisibleSet::new();
        index.query_visible(
            &world,
            &Viewport::new(Vec2::ZERO, 1.0, Vec2::new(1_440.0, 900.0)),
            &mut visible,
        );
        assert!(
            !visible.nodes().contains(&subject),
            "the node was still indexed where it used to be"
        );
    }

    #[test]
    fn a_drag_inside_one_cell_costs_no_index_writes() {
        let mut world = grid_world(10, 10);
        let mut index = SpatialIndex::for_world(&world);
        let subject = NodeIndex::new(45);

        // A pixel-sized move, sixty times, the shape of a real drag.
        let mut moved = 0;
        for _ in 0..60 {
            world.move_node(subject, Vec2::new(0.5, 0.0));
            world.rebuild_dirty_geometry();
            moved += index.sync(&world).nodes_moved;
            world.clear_spatial_updates();
        }

        assert!(
            moved < 10,
            "a 30-unit drag re-linked the node {moved} times in a 320-unit cell"
        );
    }

    /// **Query cost scaling, asserted without a clock.**
    ///
    /// A timing assertion in a unit test is a flake waiting for a loaded CI
    /// machine, so this measures the *work* instead: how many candidates the
    /// broad phase visits and how many cells it walks. Both are what the time
    /// is proportional to, and neither depends on the machine.
    ///
    /// The harness prints the wall-clock version — 2.3 µs at 5,000 nodes and
    /// 2.4 µs at 100,000 — and this is what stops that from regressing.
    #[test]
    fn query_cost_is_set_by_the_viewport_and_not_by_the_document() {
        let viewport = Viewport::new(Vec2::ZERO, 1.0, Vec2::new(1_440.0, 900.0));
        let query = query_rect(&viewport);

        let mut counts = Vec::new();
        for side in [10u32, 40, 120] {
            let world = grid_world(side, side);
            let index = SpatialIndex::for_world(&world);

            let mut candidates = Vec::new();
            index.node_candidates(query, &mut candidates);
            counts.push((world.nodes().len(), candidates.len()));
        }

        // A 144x larger document, at the same camera, must visit the same
        // candidates. Not "about the same" — exactly the same, because the
        // cells the query touches are a property of the query.
        let first = counts[0].1;
        for (nodes, candidates) in &counts {
            assert_eq!(
                *candidates, first,
                "{nodes} nodes produced {candidates} candidates against {first}"
            );
        }
        assert!(
            counts[2].0 > counts[0].0 * 100,
            "the documents were not scaled"
        );
        assert!(first > 0, "the camera saw nothing");
    }

    /// The same for the visible set, which adds the narrow phase and the sort.
    #[test]
    fn the_visible_set_does_not_grow_with_the_document() {
        let viewport = Viewport::new(Vec2::ZERO, 1.0, Vec2::new(1_440.0, 900.0));

        let mut sizes = Vec::new();
        for side in [12u32, 60] {
            let world = grid_world(side, side);
            let index = SpatialIndex::for_world(&world);
            let mut visible = VisibleSet::new();
            index.query_visible(&world, &viewport, &mut visible);
            sizes.push((visible.node_count(), visible.candidate_count()));
        }

        assert_eq!(
            sizes[0].0, sizes[1].0,
            "a 25x document changed the visible set"
        );
        assert_eq!(
            sizes[0].1, sizes[1].1,
            "a 25x document changed the candidates"
        );
    }

    #[test]
    fn the_cell_size_follows_the_documents_node_size() {
        let mut small = GraphWorld::new();
        small.create_node(
            ElementKind::GraphNode(GraphNodeKind::Default),
            Vec2::ZERO,
            Vec2::new(40.0, 20.0),
        );
        let mut large = GraphWorld::new();
        large.create_node(
            ElementKind::GraphNode(GraphNodeKind::Default),
            Vec2::ZERO,
            Vec2::new(800.0, 400.0),
        );

        assert!(cell_size_for(&small) < cell_size_for(&large));
        assert_eq!(cell_size_for(&GraphWorld::new()), DEFAULT_CELL_SIZE);
    }

    #[test]
    fn an_empty_world_has_an_empty_visible_set() {
        let world = GraphWorld::new();
        let index = SpatialIndex::for_world(&world);
        let mut visible = VisibleSet::new();
        index.query_visible(
            &world,
            &Viewport::new(Vec2::ZERO, 1.0, Vec2::new(800.0, 600.0)),
            &mut visible,
        );
        assert!(visible.is_empty());
    }

    #[test]
    fn a_new_node_is_indexed_by_the_call_that_adds_it() {
        let mut world = grid_world(4, 4);
        let mut index = SpatialIndex::for_world(&world);

        let added = world.create_node(
            ElementKind::GraphNode(GraphNodeKind::Default),
            Vec2::new(20.0, 20.0),
            Vec2::new(100.0, 40.0),
        );
        index.insert_node(&world, added);
        world.clear_spatial_updates();

        let mut candidates = Vec::new();
        index.nodes_at(Vec2::new(60.0, 40.0), 1.0, &mut candidates);
        assert!(candidates.contains(&added));
    }

    #[test]
    fn a_nearby_query_is_a_broad_phase_over_the_circle() {
        let world = grid_world(10, 10);
        let index = SpatialIndex::for_world(&world);

        let mut near = Vec::new();
        index.nodes_near(Vec2::new(80.0, 30.0), 300.0, &mut near);
        assert!(near.contains(&NodeIndex::new(0)));

        let mut far = Vec::new();
        index.nodes_near(Vec2::new(80.0, 30.0), 10.0, &mut far);
        assert!(far.len() < near.len());
    }

    #[test]
    fn painted_bounds_are_wider_than_geometric_bounds_when_a_stroke_is_wide() {
        let mut world = GraphWorld::new();
        let id = world.next_id();
        let mut spec = crate::runtime::NodeSpec::new(
            id,
            ElementKind::GraphNode(GraphNodeKind::Default),
            Vec2::ZERO,
            Vec2::new(100.0, 50.0),
        );
        spec.style.stroke.width = 20.0;
        let node = world.add_node(spec);

        let painted = node_painted_bounds(&world, node);
        assert_eq!(painted.width(), 120.0, "half the stroke on each side");
        assert_eq!(painted.height(), 70.0);
    }

    #[test]
    fn an_edge_with_no_route_has_no_painted_bounds() {
        let mut world = grid_world(3, 3);
        let edge = crate::models::EdgeIndex::new(0);
        world.set_edge_routing(edge, crate::models::EdgeRouting::Step);
        // Invalidated and not yet rebuilt.
        assert!(edge_painted_bounds(&world, edge).is_none());
    }
}
