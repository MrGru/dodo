//! §23's geometry cache — **byte-bounded and viewport-scoped from day one**,
//! and the item Phase 4 handed forward with a number attached.
//!
//! # The measurement this exists for
//!
//! Phase 4's dense scene spends **3.12 ms per frame — 19 % of a 16.7 ms
//! frame — tessellating geometry that did not change.** 3,104 paths at 1.01 µs
//! each, rebuilt from scratch, sixty times a second, for a picture that is
//! identical to the last one except that the camera moved.
//!
//! Phase 0 measured the alternative exactly (§1.6): a built `Path<Pixels>` can
//! be translated in place by mutating its vertex buffer, **exactly** — same
//! vertex count as a rebuild, max deviation 0.000122 px, pure `f32` rounding —
//! at 1.7 ns per vertex against ~5.2 µs to re-tessellate the same path. So the
//! cache is worth building. The question was only how to bound it.
//!
//! # Why "bounded" has to be a bound and not a caution
//!
//! `PathVertex<Pixels>` is 32 bytes ([`CACHE_BYTES_PER_VERTEX`]). A 200-vertex
//! Bézier edge is 6.4 KB. **A fully cached 300,000-edge scene would be
//! ~1.9 GB** — Phase 0 §3 correction 14 derived it, and it is the reason
//! [`RenderBudgets::geometry_cache_max_bytes`] exists as a hard 64 MiB cap
//! rather than as a warning in a doc.
//!
//! Two mechanisms hold it, and they are different:
//!
//! - **Viewport scoping**, which is structural: an entry is only ever *touched*
//!   by a frame that planned it, and only visible elements are planned. An
//!   entry nothing has asked for in [`RETAIN_FRAMES`] frames is dropped. The
//!   working set is therefore the screen, not the document, whatever the
//!   document's size.
//! - **The byte cap**, which is the backstop: if the visible set alone were
//!   somehow larger than 64 MiB, the least-recently-used entries go until it
//!   fits. `a_cache_cannot_exceed_its_byte_bound_however_much_is_inserted` is
//!   the test, and it inserts far more than the bound on purpose.
//!
//! # The zoom policy, which is the part the plan did not have
//!
//! Phase 0 §3 correction 4 specifies it and this module implements exactly
//! that:
//!
//! | camera change | what happens | cost |
//! |---|---|---|
//! | **pure pan** | translate the vertex buffer in place | exact, 1.7 ns/vertex |
//! | **zoom, gesture in progress** | scale + translate in place | responsive, slightly under-tessellated |
//! | **zoom, gesture settled** | re-tessellate | correct |
//! | **zoom past ±[`RenderBudgets::retessellation_zoom_band`]×** | re-tessellate | correct |
//!
//! The gesture distinction is the one that matters and it is easy to get
//! backwards. Scaling a cached tessellation keeps its vertex count, so the
//! flattening error grows with the zoom factor (error ≈ tolerance × k) **and
//! the stroke width scales with it**, which is wrong for a stroke that should
//! stay a constant screen thickness. During a live pinch that is invisible and
//! responsiveness is worth more; once the gesture stops, a canvas left with 1.9×
//! strokes is simply wrong. So [`GeometryCache::begin_frame`] takes whether a
//! zoom gesture is in progress, and a settled frame at a new zoom misses on
//! purpose.
//!
//! # The key, and why it is versions rather than geometry
//!
//! [`GeometryKey`] is *(owner, part, version, quality)*. The version comes from
//! [`NodeStore::version`](crate::runtime::NodeStore::version) /
//! [`EdgeGeometryStore::version`](crate::runtime::EdgeGeometryStore::version),
//! which §23 asks for in as many words; the quality is
//! [`RenderQuality::cache_key`], which is Phase 0 §3 correction 5 — **flattening
//! tolerance is part of the key**, because it is a 2× budget multiplier and two
//! tessellations of the same outline at different tolerances are different
//! pictures. The LOD rung folds into the quality, since
//! [`EdgeDetail::quality`](crate::render::lod::EdgeDetail::quality) multiplies
//! the tolerance — so changing rung is a miss by construction rather than by
//! remembering to add a field.
//!
//! Nothing in the key is the geometry itself. Comparing control points per
//! element per frame is the cost the version exists to replace.
//!
//! # Why this file is generic and names no UI framework
//!
//! What it caches is a `gpui::Path<Pixels>` — and if that type appeared here,
//! the byte accounting, the eviction order, the pan policy and the zoom band
//! would all need a window to test. So the cache is generic over
//! [`CachedGeometry`], a three-method trait describing what a built path can
//! do, and `render::painter` implements it for GPUI's. Every property above is
//! asserted in this file against a plain `Vec<Vec2>`.
//!
//! **This file names no UI framework.**

use std::collections::HashMap;

use crate::{
    budgets::{CACHE_BYTES_PER_VERTEX, RenderBudgets},
    geometry::{Vec2, Viewport},
    models::{EdgeIndex, NodeIndex, RenderQuality},
};

/// How many frames an untouched entry survives.
///
/// Not one. A node that leaves the viewport for a single frame — a fast pan
/// that overshoots and settles back, an element that straddles the margin —
/// would otherwise pay a full re-tessellation to come back, which is the exact
/// cost the cache exists to avoid. Four frames is 67 ms at 60 fps: long enough
/// to cover a jitter, short enough that the working set is still the screen.
pub const RETAIN_FRAMES: u64 = 4;

/// What fraction of the byte bound an overflow eviction leaves behind.
///
/// **Not 1.0, and the reason is a complexity one rather than a taste one.**
/// Evicting exactly enough to fit would re-sort the whole entry set on every
/// subsequent insert, turning a document that overflows the cache into an
/// O(n² log n) frame — the opposite of what a cache is for. Dropping to 90 %
/// means one sort buys roughly ten thousand inserts' worth of headroom, so the
/// eviction cost is amortised to nothing and the bound still holds absolutely.
pub const EVICT_TO_FRACTION: f32 = 0.9;

/// What part of an element a cached tessellation is.
///
/// An element can own several paths — a shape has a fill and a stroke, an edge
/// has a line and up to two markers — and they are separate entries because
/// they are separately expensive and separately reusable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum GeometryPart {
    /// A shape's filled body.
    Fill,
    /// A shape's outline, or an edge's line.
    Stroke,
    StartMarker,
    EndMarker,
}

/// Which element a cached tessellation belongs to.
///
/// Compact runtime indices, never [`ElementId`](crate::models::ElementId) and
/// never a document reference — §24's "compact IDs rather than cloning
/// metadata", and the reason a cache can never reach the document format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum GeometryOwner {
    Node(NodeIndex),
    Edge(EdgeIndex),
}

/// **The cache key.** See the module doc for why each field is here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct GeometryKey {
    pub owner: GeometryOwner,
    pub part: GeometryPart,
    /// The element's geometry/appearance version — §23's versioning.
    pub version: u32,
    /// [`RenderQuality::cache_key`] — the flattening tolerance, quantised.
    /// Phase 0 §3 correction 5.
    pub quality: u32,
}

impl GeometryKey {
    pub fn node(node: NodeIndex, part: GeometryPart, version: u32, quality: RenderQuality) -> Self {
        GeometryKey {
            owner: GeometryOwner::Node(node),
            part,
            version,
            quality: quality.cache_key(),
        }
    }

    pub fn edge(edge: EdgeIndex, part: GeometryPart, version: u32, quality: RenderQuality) -> Self {
        GeometryKey {
            owner: GeometryOwner::Edge(edge),
            part,
            version,
            quality: quality.cache_key(),
        }
    }
}

/// **Where the camera was when a tessellation was built**, reduced to the two
/// numbers that reposition it.
///
/// World→screen is a similarity: `s = origin + w · zoom`. So the map from one
/// camera's screen space to another's is `p ↦ a·p + b` with `a = z₁/z₀` and
/// `b = o₁ − a·o₀`, and those two numbers are all a cached vertex buffer needs
/// to move. Keeping the anchor rather than the whole [`Viewport`] is what makes
/// an entry 12 bytes of bookkeeping instead of a camera.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScreenAnchor {
    pub zoom: f32,
    /// Where world (0, 0) sits, in pane-relative screen pixels.
    pub origin: Vec2,
}

impl ScreenAnchor {
    pub fn of(viewport: &Viewport) -> ScreenAnchor {
        ScreenAnchor {
            zoom: viewport.zoom(),
            origin: viewport.world_to_screen(Vec2::ZERO),
        }
    }

    /// The `(scale, offset)` that maps a point built at `self` into `now`.
    ///
    /// A pure pan gives `(1.0, delta)` exactly, which is the case the whole
    /// cache is built around.
    pub fn transform_to(&self, now: &ScreenAnchor) -> (f32, Vec2) {
        let scale = if self.zoom.abs() > f32::EPSILON {
            now.zoom / self.zoom
        } else {
            1.0
        };
        (scale, now.origin - self.origin * scale)
    }

    /// Whether the two cameras are at the same zoom, so a repositioning is an
    /// exact translation.
    pub fn same_zoom(&self, other: &ScreenAnchor) -> bool {
        // Relative rather than absolute: zoom spans several orders of
        // magnitude, and an absolute epsilon is either useless at 0.05 or
        // wrong at 20.
        (self.zoom - other.zoom).abs() <= self.zoom.abs() * 1e-6
    }
}

/// What a built path must be able to do to live in this cache.
///
/// Three methods, because that is genuinely all the policy above needs — and
/// because a wider trait would be a place for GPUI's vocabulary to leak back
/// in. `render::painter` implements it for `gpui::Path<Pixels>`; the tests here
/// implement it for a `Vec<Vec2>`.
pub trait CachedGeometry {
    /// The vertices this holds. **Denominates the byte bound** — see
    /// [`CACHE_BYTES_PER_VERTEX`].
    fn vertex_count(&self) -> u32;

    /// Maps every point: `p ↦ p · scale + offset`. One pass, because a
    /// separate scale and translate would be two.
    fn transform(&mut self, scale: f32, offset: Vec2);
}

/// What one entry cost and where it was left.
#[derive(Debug, Clone)]
struct Entry<G> {
    geometry: G,
    /// Where the geometry currently sits, updated by every reposition.
    anchor: ScreenAnchor,
    /// The zoom the geometry was **tessellated** at, which never changes. The
    /// ±band is measured against this rather than against `anchor`, or a
    /// gesture that scaled a little sixty times would drift arbitrarily far
    /// from the tessellation it started as.
    built_zoom: f32,
    bytes: usize,
    last_used: u64,
}

/// How a lookup was answered. Reported rather than inferred, because
/// "cache hit rate during pure pan" is one of this phase's exit numbers and a
/// number nobody measures is a number nobody keeps.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CacheStats {
    /// Repositioned by an exact translation — the pure-pan case.
    pub translated: u32,
    /// Reused at the same camera, with nothing to do at all.
    pub reused: u32,
    /// Repositioned by a scale during a live zoom gesture. Slightly
    /// under-tessellated by design; see the module doc.
    pub scaled: u32,
    /// Not in the cache, or present at a camera it could not be reused from.
    pub misses: u32,
    /// Entries dropped for being untouched, or to stay inside the byte bound.
    pub evictions: u32,
}

impl CacheStats {
    pub fn hits(&self) -> u32 {
        self.translated + self.reused + self.scaled
    }

    pub fn lookups(&self) -> u32 {
        self.hits() + self.misses
    }

    /// Hits as a fraction of lookups, or 1.0 for a frame that asked nothing —
    /// a frame with no geometry has not missed anything.
    pub fn hit_rate(&self) -> f32 {
        match self.lookups() {
            0 => 1.0,
            total => self.hits() as f32 / total as f32,
        }
    }
}

/// **The cache.** See the module doc for the policy; this is the mechanism.
#[derive(Debug, Clone)]
pub struct GeometryCache<G> {
    entries: HashMap<GeometryKey, Entry<G>>,
    bytes: usize,
    max_bytes: usize,
    zoom_band: f32,
    frame: u64,
    /// Whether a zoom gesture is in progress this frame. See the module doc:
    /// this is what separates "scale it, the user is pinching" from
    /// "re-tessellate, the camera has settled".
    zooming: bool,
    anchor: ScreenAnchor,
    frame_stats: CacheStats,
    total_stats: CacheStats,
}

impl<G: CachedGeometry> GeometryCache<G> {
    /// A cache sized by the platform's budgets. **The only constructor a
    /// renderer should use** — a hand-chosen byte bound is exactly the
    /// scattered literal [`crate::budgets`] exists to prevent.
    pub fn new(budgets: &RenderBudgets) -> GeometryCache<G> {
        GeometryCache {
            entries: HashMap::new(),
            bytes: 0,
            max_bytes: budgets.geometry_cache_max_bytes,
            zoom_band: budgets.retessellation_zoom_band.max(1.0),
            frame: 0,
            zooming: false,
            anchor: ScreenAnchor {
                zoom: 1.0,
                origin: Vec2::ZERO,
            },
            frame_stats: CacheStats::default(),
            total_stats: CacheStats::default(),
        }
    }

    /// Starts a frame at this camera.
    ///
    /// `zooming` says whether a zoom gesture is in progress — a pinch, a
    /// wheel-notch zoom, an animated zoom-to-fit. It is the caller's because
    /// only the caller sees the input; the cache cannot tell a gesture from a
    /// settled camera by looking at two numbers.
    pub fn begin_frame(&mut self, anchor: ScreenAnchor, zooming: bool) {
        self.frame += 1;
        self.anchor = anchor;
        self.zooming = zooming;
        self.frame_stats = CacheStats::default();
    }

    /// **Looks up a tessellation and repositions it to this frame's camera.**
    ///
    /// A hit is returned ready to paint — already translated, or already
    /// scaled. A miss returns `None` and *removes* the stale entry, so the
    /// caller's insert does not have to think about replacing it.
    pub fn get(&mut self, key: &GeometryKey) -> Option<&G> {
        let (frame, anchor, zooming, band) =
            (self.frame, self.anchor, self.zooming, self.zoom_band);

        let outcome = match self.entries.get_mut(key) {
            None => Outcome::Miss,
            Some(entry) => {
                entry.last_used = frame;
                if entry.anchor.same_zoom(&anchor) {
                    let delta = anchor.origin - entry.anchor.origin;
                    if delta == Vec2::ZERO {
                        Outcome::Reused
                    } else {
                        // The exact case Phase 0 measured: same zoom, moved
                        // camera, translate the vertex buffer and nothing else.
                        entry.geometry.transform(1.0, delta);
                        entry.anchor = anchor;
                        Outcome::Translated
                    }
                } else if zooming && within_band(entry.built_zoom, anchor.zoom, band) {
                    let (scale, offset) = entry.anchor.transform_to(&anchor);
                    entry.geometry.transform(scale, offset);
                    entry.anchor = anchor;
                    Outcome::Scaled
                } else {
                    Outcome::Stale
                }
            }
        };

        match outcome {
            Outcome::Reused => {
                self.record(|stats| stats.reused += 1);
                self.entries.get(key).map(|entry| &entry.geometry)
            }
            Outcome::Translated => {
                self.record(|stats| stats.translated += 1);
                self.entries.get(key).map(|entry| &entry.geometry)
            }
            Outcome::Scaled => {
                self.record(|stats| stats.scaled += 1);
                self.entries.get(key).map(|entry| &entry.geometry)
            }
            Outcome::Stale => {
                self.remove(key);
                self.record(|stats| stats.misses += 1);
                None
            }
            Outcome::Miss => {
                self.record(|stats| stats.misses += 1);
                None
            }
        }
    }

    /// Stores a tessellation built at this frame's camera.
    ///
    /// **Refuses anything larger than the whole bound** rather than evicting
    /// everything to make room for it: one pathological path must not be able
    /// to empty a cache that is serving the rest of the frame.
    pub fn insert(&mut self, key: GeometryKey, geometry: G) {
        let bytes = geometry.vertex_count() as usize * CACHE_BYTES_PER_VERTEX as usize;
        if bytes > self.max_bytes {
            return;
        }

        self.remove(&key);
        self.bytes += bytes;
        self.entries.insert(
            key,
            Entry {
                geometry,
                anchor: self.anchor,
                built_zoom: self.anchor.zoom,
                bytes,
                last_used: self.frame,
            },
        );
        self.enforce_byte_bound();
    }

    /// Ends the frame: drops what nothing has asked for in [`RETAIN_FRAMES`]
    /// frames. **This is the viewport scoping** — see the module doc.
    pub fn end_frame(&mut self) {
        let (frame, mut evicted, mut freed) = (self.frame, 0u32, 0usize);
        self.entries.retain(|_, entry| {
            let keep = frame.saturating_sub(entry.last_used) < RETAIN_FRAMES;
            if !keep {
                evicted += 1;
                freed += entry.bytes;
            }
            keep
        });
        self.bytes -= freed;
        self.record(|stats| stats.evictions += evicted);
    }

    /// What this frame's lookups did. **The pure-pan hit rate**, from outside.
    pub fn frame_stats(&self) -> CacheStats {
        self.frame_stats
    }

    /// What every lookup since construction did.
    pub fn total_stats(&self) -> CacheStats {
        self.total_stats
    }

    /// The bytes currently held. **Never above
    /// [`RenderBudgets::geometry_cache_max_bytes`]** — that is the point.
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    pub fn max_bytes(&self) -> usize {
        self.max_bytes
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Drops everything. For a document swap, where every index means something
    /// else and every cached tessellation is a lie.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.bytes = 0;
    }

    fn remove(&mut self, key: &GeometryKey) {
        if let Some(entry) = self.entries.remove(key) {
            self.bytes -= entry.bytes;
        }
    }

    /// The backstop. Drops least-recently-used entries until the bound holds.
    ///
    /// Sorting on overflow rather than maintaining an LRU list: overflow is the
    /// rare path by construction — the viewport scoping is what normally keeps
    /// the cache small — and an intrusive list would cost a pointer pair per
    /// entry on every frame to make the rare path faster.
    fn enforce_byte_bound(&mut self) {
        if self.bytes <= self.max_bytes {
            return;
        }

        let mut order: Vec<(u64, GeometryKey)> = self
            .entries
            .iter()
            .map(|(key, entry)| (entry.last_used, *key))
            .collect();
        order.sort_unstable();

        // Down to the low-water mark, not just under the bound — see
        // `EVICT_TO_FRACTION`.
        let target = (self.max_bytes as f32 * EVICT_TO_FRACTION) as usize;
        let mut evicted = 0;
        for (_, key) in order {
            if self.bytes <= target {
                break;
            }
            self.remove(&key);
            evicted += 1;
        }
        self.record(|stats| stats.evictions += evicted);
    }

    fn record(&mut self, update: impl Fn(&mut CacheStats)) {
        update(&mut self.frame_stats);
        update(&mut self.total_stats);
    }
}

enum Outcome {
    Reused,
    Translated,
    Scaled,
    Stale,
    Miss,
}

/// Whether `now` is within a factor of `band` either way of `built`.
fn within_band(built: f32, now: f32, band: f32) -> bool {
    if built.abs() <= f32::EPSILON {
        return false;
    }
    let ratio = now / built;
    ratio >= 1.0 / band && ratio <= band
}

// ---- text -------------------------------------------------------------

/// The cache key for one shaped label.
///
/// `font_size` is in the key because **it is in GPUI's own shaped-line cache
/// key**, so a continuous zoom re-shapes every visible label on every frame
/// unless the size is quantised first (Phase 0 §1.9 and §3 correction 11). The
/// quantisation is
/// [`LodThresholds::quantize_font_size`](crate::budgets::LodThresholds::quantize_font_size);
/// this key is what makes the quantisation pay.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TextKey {
    pub node: NodeIndex,
    /// The node's appearance version, so an edited label is a different line.
    pub version: u32,
    /// The quantised font size, in tenths of a pixel. An integer because `f32`
    /// is neither `Eq` nor `Hash`, and for the same reason
    /// [`RenderQuality::cache_key`] is one.
    pub font_size: u32,
}

impl TextKey {
    pub fn new(node: NodeIndex, version: u32, font_size: f32) -> TextKey {
        TextKey {
            node,
            version,
            font_size: (font_size.max(0.0) * 10.0).round() as u32,
        }
    }
}

/// **The engine's own shaped-line cache** (§9, Phase 0 §3 correction 11).
///
/// GPUI has one, and it is **two frames deep** — current frame plus previous,
/// with unused keys evicted (`line_layout.rs:577`). A label that leaves the
/// viewport for a single frame is re-shaped on return, and shaping is
/// ~7–11 µs against ~1.7 µs to paint a cached line. At 1,000 labels that is the
/// difference Phase 0 measured as 11.1 ms against 3.7 ms, and at 5,000 it is
/// 51.3 ms (18 fps) against 8.7 ms (60 fps). So the engine owns this one, and
/// it survives more than two frames.
///
/// Generic over the shaped line for the same reason [`GeometryCache`] is
/// generic over the path: `ShapedLine` is GPUI's, and the retention policy
/// should be assertable without a window.
#[derive(Debug, Clone)]
pub struct ShapedLineCache<L> {
    entries: HashMap<TextKey, (L, u64)>,
    max_entries: usize,
    frame: u64,
    stats: CacheStats,
}

impl<L> ShapedLineCache<L> {
    pub fn new(budgets: &RenderBudgets) -> ShapedLineCache<L> {
        ShapedLineCache {
            entries: HashMap::new(),
            max_entries: budgets.max_shaped_lines as usize,
            frame: 0,
            stats: CacheStats::default(),
        }
    }

    pub fn begin_frame(&mut self) {
        self.frame += 1;
    }

    pub fn get(&mut self, key: &TextKey) -> Option<&L> {
        let frame = self.frame;
        match self.entries.get_mut(key) {
            Some((line, last_used)) => {
                *last_used = frame;
                self.stats.reused += 1;
                // Reborrowed rather than returned from the arm, because the
                // mutable borrow above has to end before the shared one.
                let _ = line;
                self.entries.get(key).map(|(line, _)| line)
            }
            None => {
                self.stats.misses += 1;
                None
            }
        }
    }

    pub fn insert(&mut self, key: TextKey, line: L) {
        self.entries.insert(key, (line, self.frame));
        self.enforce_entry_bound();
    }

    /// Drops lines nothing has asked for in [`RETAIN_FRAMES`] frames.
    ///
    /// The same viewport scoping the geometry cache uses, and the same reason:
    /// a document with 100,000 labels must hold the visible ones, not all of
    /// them.
    pub fn end_frame(&mut self) {
        let frame = self.frame;
        let mut evicted = 0;
        self.entries.retain(|_, (_, last_used)| {
            let keep = frame.saturating_sub(*last_used) < RETAIN_FRAMES;
            if !keep {
                evicted += 1;
            }
            keep
        });
        self.stats.evictions += evicted;
    }

    pub fn stats(&self) -> CacheStats {
        self.stats
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    fn enforce_entry_bound(&mut self) {
        if self.entries.len() <= self.max_entries {
            return;
        }

        let mut order: Vec<(u64, TextKey)> = self
            .entries
            .iter()
            .map(|(key, (_, last_used))| (*last_used, *key))
            .collect();
        order.sort_unstable();

        let mut evicted = 0;
        for (_, key) in order {
            if self.entries.len() <= self.max_entries {
                break;
            }
            self.entries.remove(&key);
            evicted += 1;
        }
        self.stats.evictions += evicted;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::budgets::{RenderBackend, for_backend};

    /// A stand-in for `gpui::Path<Pixels>`: a vertex buffer and nothing else,
    /// which is exactly what the cache's policy actually operates on.
    #[derive(Debug, Clone, PartialEq)]
    struct FakePath(Vec<Vec2>);

    impl CachedGeometry for FakePath {
        fn vertex_count(&self) -> u32 {
            self.0.len() as u32
        }

        fn transform(&mut self, scale: f32, offset: Vec2) {
            for point in &mut self.0 {
                *point = *point * scale + offset;
            }
        }
    }

    fn path(vertices: usize) -> FakePath {
        FakePath((0..vertices).map(|i| Vec2::new(i as f32, 0.0)).collect())
    }

    fn budgets() -> RenderBudgets {
        for_backend(RenderBackend::Metal)
    }

    fn cache() -> GeometryCache<FakePath> {
        GeometryCache::new(&budgets())
    }

    fn anchor(zoom: f32, x: f32) -> ScreenAnchor {
        ScreenAnchor {
            zoom,
            origin: Vec2::new(x, 0.0),
        }
    }

    fn key(version: u32) -> GeometryKey {
        GeometryKey::edge(
            EdgeIndex::new(0),
            GeometryPart::Stroke,
            version,
            RenderQuality::BALANCED,
        )
    }

    /// **The number this phase owes: the hit rate during a pure pan.**
    ///
    /// Sixty frames of camera movement at a fixed zoom, over a working set that
    /// does not change. Everything after the first frame must be a hit, and
    /// every one of those hits must be the *exact* kind — a translation, not a
    /// scale.
    #[test]
    fn a_pure_pan_hits_everything_it_looks_at_and_translates_it_exactly() {
        let mut cache = cache();
        let keys: Vec<GeometryKey> = (0..200)
            .map(|i| {
                GeometryKey::edge(
                    EdgeIndex::new(i),
                    GeometryPart::Stroke,
                    1,
                    RenderQuality::BALANCED,
                )
            })
            .collect();

        // Frame 1: cold. Every lookup misses and is filled.
        cache.begin_frame(anchor(1.0, 0.0), false);
        for key in &keys {
            assert!(cache.get(key).is_none());
            cache.insert(*key, path(200));
        }
        assert_eq!(cache.frame_stats().misses, 200);
        cache.end_frame();

        // Frames 2..60: pan, same zoom.
        for frame in 1..60 {
            cache.begin_frame(anchor(1.0, frame as f32 * 7.0), false);
            for key in &keys {
                assert!(cache.get(key).is_some(), "frame {frame} missed");
            }
            let stats = cache.frame_stats();
            assert_eq!(stats.misses, 0, "frame {frame}");
            assert_eq!(stats.translated, 200, "frame {frame} did not translate");
            assert_eq!(stats.scaled, 0, "a pan must never scale a tessellation");
            assert_eq!(stats.hit_rate(), 1.0);
            cache.end_frame();
        }
    }

    /// Translation is *exact* — Phase 0 measured the deviation at 0.000122 px
    /// against a rebuild. Here the arithmetic is checked directly: a path
    /// translated across sixty frames lands where a path built at the final
    /// camera would.
    #[test]
    fn a_translated_path_lands_exactly_where_a_rebuilt_one_would() {
        let mut cache = cache();
        cache.begin_frame(anchor(1.0, 0.0), false);
        cache.insert(key(1), FakePath(vec![Vec2::new(10.0, 20.0)]));
        cache.end_frame();

        for frame in 1..=60 {
            cache.begin_frame(anchor(1.0, frame as f32), false);
            let cached = cache.get(&key(1)).expect("hit").clone();
            assert_eq!(
                cached.0[0],
                Vec2::new(10.0 + frame as f32, 20.0),
                "drifted by frame {frame}"
            );
            cache.end_frame();
        }
    }

    /// The zoom policy's first half: a **settled** camera at a new zoom must
    /// re-tessellate, because a scaled tessellation carries a scaled stroke
    /// width and a canvas left like that is simply wrong.
    #[test]
    fn a_settled_zoom_misses_rather_than_scaling() {
        let mut cache = cache();
        cache.begin_frame(anchor(1.0, 0.0), false);
        cache.insert(key(1), path(100));
        cache.end_frame();

        cache.begin_frame(anchor(1.5, 0.0), false);
        assert!(cache.get(&key(1)).is_none(), "a settled zoom must rebuild");
        assert_eq!(cache.frame_stats().misses, 1);
        assert_eq!(cache.frame_stats().scaled, 0);
    }

    /// The other half: **during** a gesture the cache scales for responsiveness.
    #[test]
    fn a_zoom_gesture_scales_the_cache_instead_of_rebuilding_it() {
        let mut cache = cache();
        cache.begin_frame(anchor(1.0, 0.0), false);
        cache.insert(key(1), FakePath(vec![Vec2::new(10.0, 0.0)]));
        cache.end_frame();

        cache.begin_frame(anchor(1.5, 0.0), true);
        let cached = cache.get(&key(1)).expect("scaled hit").clone();
        assert_eq!(cache.frame_stats().scaled, 1);
        assert_eq!(cached.0[0], Vec2::new(15.0, 0.0));
    }

    /// And the band: past ±`retessellation_zoom_band` the tessellation is too
    /// coarse for the zoom it is being shown at, gesture or no gesture.
    #[test]
    fn a_gesture_past_the_band_rebuilds_anyway() {
        let budgets = budgets();
        let mut cache = cache();
        cache.begin_frame(anchor(1.0, 0.0), false);
        cache.insert(key(1), path(100));
        cache.end_frame();

        let inside = budgets.retessellation_zoom_band * 0.9;
        cache.begin_frame(anchor(inside, 0.0), true);
        assert!(cache.get(&key(1)).is_some(), "{inside}x is inside the band");
        cache.end_frame();

        let outside = budgets.retessellation_zoom_band * 1.1;
        cache.begin_frame(anchor(outside, 0.0), true);
        assert!(cache.get(&key(1)).is_none(), "{outside}x is past the band");
    }

    /// **The band is measured against the tessellation zoom, not against where
    /// the geometry currently sits.** Otherwise a gesture that scales by 1 % a
    /// hundred times drifts arbitrarily far from what was actually flattened,
    /// one imperceptible step at a time.
    #[test]
    fn a_long_gesture_cannot_creep_past_the_band_in_small_steps() {
        let budgets = budgets();
        let mut cache = cache();
        cache.begin_frame(anchor(1.0, 0.0), false);
        cache.insert(key(1), path(100));
        cache.end_frame();

        let mut zoom = 1.0f32;
        let mut rebuilt_at = None;
        for _ in 0..400 {
            zoom *= 1.01;
            cache.begin_frame(anchor(zoom, 0.0), true);
            if cache.get(&key(1)).is_none() {
                rebuilt_at = Some(zoom);
                break;
            }
            cache.end_frame();
        }

        let rebuilt_at = rebuilt_at.expect("the band must eventually force a rebuild");
        assert!(
            rebuilt_at <= budgets.retessellation_zoom_band * 1.02,
            "crept to {rebuilt_at}x before rebuilding, past the \
             {}x band",
            budgets.retessellation_zoom_band
        );
    }

    /// §23's versioning, end to end: an element whose geometry changed gets a
    /// new key, so the stale tessellation is never painted.
    #[test]
    fn a_new_version_is_a_different_entry() {
        let mut cache = cache();
        cache.begin_frame(anchor(1.0, 0.0), false);
        cache.insert(key(1), path(100));
        assert!(cache.get(&key(1)).is_some());
        assert!(cache.get(&key(2)).is_none(), "a moved element must miss");
    }

    /// Phase 0 §3 correction 5: the flattening tolerance is part of the key.
    #[test]
    fn the_flattening_tolerance_is_part_of_the_key() {
        let precise = GeometryKey::edge(
            EdgeIndex::new(0),
            GeometryPart::Stroke,
            1,
            RenderQuality::PRECISE,
        );
        let draft = GeometryKey::edge(
            EdgeIndex::new(0),
            GeometryPart::Stroke,
            1,
            RenderQuality::DRAFT,
        );
        assert_ne!(precise, draft);

        let mut cache = cache();
        cache.begin_frame(anchor(1.0, 0.0), false);
        cache.insert(precise, path(100));
        assert!(
            cache.get(&draft).is_none(),
            "two tolerances are two pictures"
        );
    }

    /// The two parts of one element are two entries, because they are
    /// separately expensive.
    #[test]
    fn a_fill_and_a_stroke_of_one_element_are_separate_entries() {
        let fill = GeometryKey::node(
            NodeIndex::new(0),
            GeometryPart::Fill,
            1,
            RenderQuality::BALANCED,
        );
        let stroke = GeometryKey::node(
            NodeIndex::new(0),
            GeometryPart::Stroke,
            1,
            RenderQuality::BALANCED,
        );
        assert_ne!(fill, stroke);
    }

    /// **The bound Phase 0 §3 correction 14 asked for**, tested by trying hard
    /// to break it.
    ///
    /// 20,000 edges of 200 vertices is 128 MB of tessellation offered to a
    /// 64 MiB cache — twice the bound and several eviction rounds, on the same
    /// slope as the
    /// ~1.9 GB a fully cached 300,000-edge document would reach. The assertion
    /// runs on **every** insert rather than at the end, so an implementation
    /// that overshoots and then tidies up would still fail.
    #[test]
    fn a_cache_cannot_exceed_its_byte_bound_however_much_is_inserted() {
        let budgets = budgets();
        let mut cache = cache();
        cache.begin_frame(anchor(1.0, 0.0), false);

        for index in 0..20_000u32 {
            cache.insert(
                GeometryKey::edge(
                    EdgeIndex::new(index),
                    GeometryPart::Stroke,
                    1,
                    RenderQuality::BALANCED,
                ),
                path(200),
            );
            assert!(
                cache.bytes() <= budgets.geometry_cache_max_bytes,
                "cache reached {} bytes at entry {index}, past the {} bound",
                cache.bytes(),
                budgets.geometry_cache_max_bytes
            );
        }

        assert!(
            cache.total_stats().evictions > 0,
            "nothing was ever evicted"
        );
    }

    /// A single pathological path must not empty a cache that is serving the
    /// rest of the frame.
    #[test]
    fn one_oversized_path_is_refused_rather_than_evicting_everything() {
        let budgets = budgets();
        let mut cache = cache();
        cache.begin_frame(anchor(1.0, 0.0), false);
        cache.insert(key(1), path(100));
        let before = cache.len();

        let too_big = budgets.geometry_cache_max_vertices() + 1;
        cache.insert(key(2), path(too_big));

        assert_eq!(cache.len(), before, "the oversized path was stored");
        assert!(cache.get(&key(1)).is_some(), "and it evicted the rest");
    }

    /// **The viewport scoping.** An entry nothing asks for goes, so the working
    /// set is the screen rather than the document.
    #[test]
    fn an_entry_nothing_asks_for_is_dropped_after_the_retention_window() {
        let mut cache = cache();
        cache.begin_frame(anchor(1.0, 0.0), false);
        cache.insert(key(1), path(100));
        cache.end_frame();

        for _ in 0..RETAIN_FRAMES {
            cache.begin_frame(anchor(1.0, 0.0), false);
            cache.end_frame();
        }

        assert!(cache.is_empty(), "an unused entry outlived its window");
        assert_eq!(cache.bytes(), 0, "and its bytes were not reclaimed");
    }

    /// The other half of retention: a single frame's absence must not cost a
    /// re-tessellation, because a pan that overshoots and settles back is
    /// ordinary.
    #[test]
    fn a_one_frame_absence_does_not_cost_a_retessellation() {
        let mut cache = cache();
        cache.begin_frame(anchor(1.0, 0.0), false);
        cache.insert(key(1), path(100));
        cache.end_frame();

        cache.begin_frame(anchor(1.0, 0.0), false);
        cache.end_frame();

        cache.begin_frame(anchor(1.0, 0.0), false);
        assert!(
            cache.get(&key(1)).is_some(),
            "one frame away and it was gone"
        );
    }

    #[test]
    fn the_anchor_transform_is_the_identity_for_an_unchanged_camera() {
        let camera = anchor(1.3, 40.0);
        let (scale, offset) = camera.transform_to(&camera);

        assert_eq!(scale, 1.0);
        assert_eq!(offset, Vec2::ZERO);
    }

    /// The claim the anchor rests on: two cameras' screen spaces really are
    /// related by one scale and one offset, so a cached buffer can be moved
    /// between them without knowing any world coordinates.
    #[test]
    fn the_anchor_transform_matches_a_full_viewport_conversion() {
        let mut from = Viewport::default();
        from.set_size(Vec2::new(800.0, 600.0));
        from.pan_by(Vec2::new(13.0, -7.0));

        let mut to = from;
        to.pan_by(Vec2::new(50.0, 25.0));
        to.zoom_by(Vec2::new(400.0, 300.0), 1.4);

        let (scale, offset) = ScreenAnchor::of(&from).transform_to(&ScreenAnchor::of(&to));

        for world in [
            Vec2::ZERO,
            Vec2::new(120.0, -340.0),
            Vec2::new(-9_000.0, 4_500.0),
        ] {
            let moved = from.world_to_screen(world) * scale + offset;
            let rebuilt = to.world_to_screen(world);
            assert!(
                (moved - rebuilt).length() < 0.01,
                "{world:?}: moved {moved:?} against rebuilt {rebuilt:?}"
            );
        }
    }

    #[test]
    fn a_cleared_cache_reports_no_bytes() {
        let mut cache = cache();
        cache.begin_frame(anchor(1.0, 0.0), false);
        cache.insert(key(1), path(500));
        assert!(cache.bytes() > 0);

        cache.clear();
        assert_eq!(cache.bytes(), 0);
        assert!(cache.is_empty());
    }

    // ---- text ---------------------------------------------------------

    /// The quantisation is what makes the shaped-line cache pay: a continuous
    /// zoom must produce a handful of keys, not one per frame.
    #[test]
    fn quantised_sizes_collapse_a_zoom_sweep_into_a_few_text_keys() {
        let lod = budgets().lod;
        let mut keys: Vec<TextKey> = (1..=400)
            .map(|step| {
                let rendered = lod.nominal_label_size * step as f32 / 100.0;
                TextKey::new(NodeIndex::new(0), 1, lod.quantize_font_size(rendered))
            })
            .collect();
        keys.sort_unstable();
        keys.dedup();

        assert!(
            keys.len() <= lod.font_size_ladder.len(),
            "400 zoom steps produced {} distinct shaped-line keys",
            keys.len()
        );
    }

    /// The whole reason the engine owns this cache: GPUI's is two frames deep,
    /// so a label that leaves the viewport for one frame is re-shaped on
    /// return. This one is not.
    #[test]
    fn a_shaped_line_survives_longer_than_the_frameworks_two_frames() {
        let mut cache: ShapedLineCache<u32> = ShapedLineCache::new(&budgets());
        let key = TextKey::new(NodeIndex::new(0), 1, 13.0);

        cache.begin_frame();
        cache.insert(key, 42);
        cache.end_frame();

        for _ in 0..(RETAIN_FRAMES - 1) {
            cache.begin_frame();
            cache.end_frame();
        }

        cache.begin_frame();
        assert_eq!(cache.get(&key), Some(&42));
    }

    #[test]
    fn an_edited_label_is_a_different_shaped_line() {
        let mut cache: ShapedLineCache<u32> = ShapedLineCache::new(&budgets());
        cache.begin_frame();
        cache.insert(TextKey::new(NodeIndex::new(0), 1, 13.0), 42);

        assert_eq!(cache.get(&TextKey::new(NodeIndex::new(0), 2, 13.0)), None);
        assert_eq!(cache.get(&TextKey::new(NodeIndex::new(0), 1, 16.0)), None);
    }

    #[test]
    fn the_shaped_line_cache_is_bounded_by_its_entry_cap() {
        let budgets = budgets();
        let mut cache: ShapedLineCache<u32> = ShapedLineCache::new(&budgets);
        cache.begin_frame();

        for index in 0..(budgets.max_shaped_lines * 4) {
            cache.insert(TextKey::new(NodeIndex::new(index), 1, 13.0), index);
            assert!(cache.len() <= budgets.max_shaped_lines as usize);
        }
    }

    #[test]
    fn an_unused_shaped_line_is_dropped_after_the_retention_window() {
        let mut cache: ShapedLineCache<u32> = ShapedLineCache::new(&budgets());
        cache.begin_frame();
        cache.insert(TextKey::new(NodeIndex::new(0), 1, 13.0), 42);
        cache.end_frame();

        for _ in 0..RETAIN_FRAMES {
            cache.begin_frame();
            cache.end_frame();
        }

        assert!(cache.is_empty());
    }
}
