//! **The one named place for every render ceiling and LOD threshold.**
//!
//! Nothing in the engine may spell one of these numbers inline. That is not
//! tidiness. The decision taken on 2026-08-16 was to build the canvas
//! **macOS-first with per-platform constants**, because every number below was
//! measured against `gpui_macos/src/metal_renderer.rs`, and `gpui_windows`,
//! `gpui_linux` and `gpui_wgpu` are separate implementations with their own
//! instance-buffer and batching behaviour — and because two of dodo's four
//! release targets cannot be built on macOS at all, so nobody here could have
//! measured them. Keeping every number in this file means another backend's
//! numbers drop in by editing it, and no engine logic changes.
//!
//! # The ceiling is a black window, not a slow frame
//!
//! This is the fact the whole module exists for. macOS's Metal instance buffer
//! doubles on overflow up to a hard 256 MiB cap, at which point the renderer
//! does `log::error!(…); break` — *leaving the draw loop without presenting the
//! drawable* (`metal_renderer.rs:508-511`). At 104 bytes per path vertex that is
//! [`RenderBudgets::hard_path_vertex_ceiling`] = 2,581,110 vertices, and it was
//! reproduced on the bracketing scene: 12,000 200-vertex edges render, 13,000
//! produce a **uniformly black window**. No panic, no warning — dodo installs
//! no logger, so the `log::error!` goes nowhere.
//!
//! So exceeding the budget is a *correctness* failure, and two things follow
//! that shape the rest of the engine:
//!
//! - **Culling is mandatory**, not an optimisation. GPUI's content-mask
//!   clipping does not substitute for it — rejection happens after
//!   `paint_path` has already cloned and scaled the vertex buffer, so 16,000
//!   fully offscreen paths still cost 6.3 ms of CPU per frame.
//! - **The renderer counts the vertices it is about to paint** and degrades to
//!   a simplified representation when the frame approaches
//!   [`RenderBudgets::safe_path_vertex_ceiling`], rather than painting nothing
//!   at all.
//!
//! # Provenance is part of the data
//!
//! Every [`RenderBudgets`] carries a [`Provenance`]. The macOS numbers say
//! where and when they were measured; the others say **`Unmeasured`** and mean
//! it. They are not "assumed equal to macOS" — they are macOS's numbers with a
//! deliberate safety discount ([`UNMEASURED_DISCOUNT`]), chosen so that an
//! unmeasured backend fails *slow* rather than fails *black*. Re-running the
//! same measurements on a Windows and a Linux runner is what replaces those
//! rows, and the discount goes with them when it does. The `METAL` constant's
//! comments record what each macOS number was measured on, so the same scenes
//! can be rebuilt: paint N cached paths of M vertices per frame and sweep both.
//!
//! # Why `HostOs` and `cfg!`, never `#[cfg]`
//!
//! dodo's root `AGENTS.md` states the invariant and this module is one of its
//! clearest cases: **a platform-conditional answer is a value chosen by
//! `HostOs` or `cfg!`, not an item behind `#[cfg]`**. [`for_host`] is a
//! function of a [`HostOs`], so every platform's budget typechecks and is
//! asserted from any machine — including the two nobody here can build for.
//! [`current`] is the single place that reads `cfg!`.
//! Every constant below states what it was measured on and when. Where a
//! number is derived rather than measured, it says so.

use dodo_paths::HostOs;

/// Which GPUI renderer backend a platform uses. The budgets are properties of
/// *this*, not of the operating system — they are named separately because the
/// day dodo ships a `gpui_wgpu` build on a platform that today uses a native
/// backend, the numbers move with the renderer and not with the OS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RenderBackend {
    /// `gpui_macos`, Metal. The only one measured.
    Metal,
    /// `gpui_windows`.
    Windows,
    /// `gpui_linux`.
    Linux,
}

/// Where a set of budgets came from. See the module doc — this is data, not a
/// comment, so a caller (a benchmark harness, a diagnostics page) can say out
/// loud that it is running on guessed numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provenance {
    /// Measured on real hardware; the row says which, and when.
    Measured {
        /// The machine, briefly. Budgets are hardware-dependent and pretending
        /// otherwise is how a constant outlives its truth.
        machine: &'static str,
        /// ISO date of the measurement.
        date: &'static str,
    },
    /// Not measured on this backend. The numbers are macOS's, discounted by
    /// [`UNMEASURED_DISCOUNT`].
    Unmeasured,
}

impl Provenance {
    pub fn is_measured(&self) -> bool {
        matches!(self, Provenance::Measured { .. })
    }
}

/// What an unmeasured backend keeps of macOS's measured headroom.
///
/// Half. There is no measurement behind the number and it is not pretending to
/// be one — it is a decision about which way to be wrong. An unmeasured backend
/// that overestimates its ceiling renders a black window, which is total and
/// silent; one that underestimates drops to a simplified representation
/// earlier than it needed to, which is visible, recoverable and merely ugly.
pub const UNMEASURED_DISCOUNT: f32 = 0.5;

/// The bytes `gpui_macos` uploads per path vertex.
///
/// `PathRasterizationVertex` is `{ xy: Point<ScaledPixels>, st: Point<f32>,
/// color: Background, bounds: Bounds<ScaledPixels> }` = 8 + 8 + 72 + 16 = **104
/// bytes**, measured with `size_of` against the pinned gpui. The 72-byte
/// `Background` is a full gradient descriptor replicated onto every vertex,
/// which is why the ceiling arrives so much sooner than a vertex count
/// suggests.
pub const METAL_BYTES_PER_UPLOADED_VERTEX: u32 = 104;

/// The bytes one cached vertex costs *in dodo's own geometry cache*.
///
/// `PathVertex<Pixels>` is **32 bytes** (measured), not the 104 the renderer
/// uploads — the cache holds gpui's `Path`, and the 104-byte expansion happens
/// inside the Metal renderer. This is the number
/// [`RenderBudgets::geometry_cache_max_bytes`] is denominated against: a
/// 200-vertex Bézier is 6.4 KB, and a 300,000-edge scene would be ~1.9 GB if
/// it were all cached, which is why the cache is byte-bounded and
/// viewport-scoped from day one rather than "watched".
pub const CACHE_BYTES_PER_VERTEX: u32 = 32;

/// The zoom ladder (§15), as data rather than as literals in the renderer.
///
/// §15 asks for exactly this and says the thresholds must be configurable and
/// later tuned by benchmarks — so they are fields, and the LOD renderer is where
/// they first do anything.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LodThresholds {
    /// At or above this zoom, visible nodes get real GPUI elements: text,
    /// controls, interactive handles.
    pub full_detail_zoom: f32,
    /// At or above this zoom (and below `full_detail_zoom`), nodes are drawn on
    /// the canvas with a label and no controls.
    pub compact_zoom: f32,
    /// Below `compact_zoom`, everything is a box. §15: do not lay out rich text
    /// that cannot be read, do not create buttons, do not create handles unless
    /// interaction needs them.
    ///
    /// An ellipse degrades **earlier** than a rectangle. Measured on the M1
    /// above: an `arc_to` ellipse is 337 vertices and 7.2 µs to build — as much
    /// as a Bézier spanning the whole window — against 24 vertices and 0.8 µs
    /// for a rectangle or diamond via `add_polygon`. This is the zoom below which a curved shape
    /// is painted as its bounding quad.
    pub curve_to_quad_zoom: f32,
    /// The discrete font sizes labels are shaped at, smallest first.
    ///
    /// **Quantisation is mandatory, not a nicety.** `font_size` is part of
    /// GPUI's shaped-line cache key, so a continuous zoom re-shapes every
    /// visible label on every frame; snapping to a handful of sizes turns that
    /// into a cache hit. GPUI's own layout cache is only two frames deep, so
    /// the engine owns a `ShapedLine` cache on top.
    pub font_size_ladder: &'static [f32],
    /// Below this rendered height in screen pixels, a label is not drawn at
    /// all — it cannot be read, and shaping it is pure cost.
    pub min_readable_font_px: f32,
    /// The font size a node label is nominally drawn at in **world** units, so
    /// its rendered size is this times the zoom before quantisation. Here
    /// rather than in the renderer because it is the input to the quantisation
    /// above, and the pair only makes sense read together.
    pub nominal_label_size: f32,
    /// Below this rendered side in screen pixels, a node is a plain box: no
    /// label, no border, no handles, whatever the zoom rung says.
    ///
    /// Independent of zoom on purpose — a 20-unit node at zoom 1 and a
    /// 200-unit node at zoom 0.1 are the same legibility problem, and §15's
    /// "merge/simplify visual details" is about what reaches the eye rather
    /// than about the camera.
    pub min_detailed_node_px: f32,
    /// Below this on-screen length in pixels, an edge is not drawn at all.
    ///
    /// A three-pixel edge is a smudge on a node's border, and it costs a whole
    /// path — [`RenderBudgets::nanos_per_path`] of fixed CPU regardless of how
    /// few vertices it has. This is the cheapest rung of §15's ladder and the
    /// only one that costs literally nothing.
    pub min_edge_screen_px: f32,
}

impl LodThresholds {
    /// §15's own ladder, unchanged. It is a starting point the requirements
    /// describe as tunable, and the benchmark scenes are what will tune it.
    pub const DEFAULT: LodThresholds = LodThresholds {
        full_detail_zoom: 0.6,
        compact_zoom: 0.2,
        curve_to_quad_zoom: 0.35,
        font_size_ladder: &[9.0, 11.0, 13.0, 16.0, 20.0, 28.0],
        min_readable_font_px: 6.0,
        nominal_label_size: 13.0,
        // Two node bodies' worth of border and a label's line height do not fit
        // in less; below it the quad is the whole node.
        min_detailed_node_px: 24.0,
        // Half a handle's diameter. Shorter than that and the edge is inside
        // the dots it joins.
        min_edge_screen_px: 4.0,
    };

    /// Snaps a rendered font size onto the ladder. Sizes above the ladder's top
    /// are clamped to it — a label rendered at 400 px is shaped at 28 and drawn
    /// scaled, because shaping every zoom level of a zoomed-in label is exactly
    /// the per-frame re-shaping the ladder exists to prevent.
    pub fn quantize_font_size(&self, size: f32) -> f32 {
        let mut best = self.font_size_ladder[0];
        for &candidate in self.font_size_ladder {
            if candidate <= size {
                best = candidate;
            }
        }
        best
    }

    /// Which rung of the ladder a zoom level lands on.
    pub fn detail(&self, zoom: f32) -> DetailLevel {
        if zoom >= self.full_detail_zoom {
            DetailLevel::Full
        } else if zoom >= self.compact_zoom {
            DetailLevel::Compact
        } else {
            DetailLevel::Overview
        }
    }

    /// Whether a curved shape should be painted as its bounding quad at this
    /// zoom.
    pub fn degrade_curves_to_quads(&self, zoom: f32) -> bool {
        zoom < self.curve_to_quad_zoom
    }
}

/// §15's three rungs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DetailLevel {
    /// Real GPUI elements for visible nodes.
    Full,
    /// Canvas-drawn with a label, no controls.
    Compact,
    /// Boxes.
    Overview,
}

/// Every render ceiling for one backend.
///
/// Constructed only by [`for_host`] / [`for_backend`]; the fields are public so
/// a benchmark harness can print them and a diagnostics surface can show them,
/// but nothing should build one by hand outside this module — that would be the
/// scattered literal this whole file exists to prevent.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderBudgets {
    pub backend: RenderBackend,
    pub provenance: Provenance,

    /// **The cliff.** Painting more path vertices than this in one frame does
    /// not slow the window down; it makes the window black. See the module doc.
    pub hard_path_vertex_ceiling: u32,

    /// Where the renderer starts degrading instead of painting. Below the hard
    /// ceiling with real headroom, because the count is an estimate made before
    /// tessellation and because quads and sprites share the same instance
    /// buffer.
    pub safe_path_vertex_ceiling: u32,

    /// What a frame can paint and still hold 60 fps. An order of magnitude
    /// below the cliff: the cliff is about *rendering at all*, this is about
    /// rendering *smoothly*, and they are different numbers for different
    /// reasons.
    pub target_path_vertices_per_frame: u32,

    /// **The budget Phase 4's tables did not have, and the one a hairball
    /// reaches first.**
    ///
    /// Vertices are not the only per-frame ceiling. `Window::paint_path`
    /// consumes its argument, so a cached path is cloned into it, and the
    /// vertex array is traversed four times per painted path per frame — a
    /// *fixed* [`RenderBudgets::nanos_per_path`] whatever the path's length.
    /// Phase 4's scattered scene put 61,104 edges genuinely in the viewport;
    /// at 1.5 µs each that is **92 ms of CPU before a single vertex is
    /// counted**, and it would still be 92 ms if every one of those edges were
    /// a two-point line.
    ///
    /// So a level-of-detail ladder that only simplifies geometry does not bound
    /// that frame, and [`crate::render::lod`] spends this budget alongside the
    /// vertex one. **Derived, not measured**: half a 16.7 ms frame divided by
    /// `nanos_per_path`, which is 5,566, rounded down to 5,000. The dense scene
    /// paints 3,104 paths and holds its frame, which is the closest measured
    /// point to it.
    pub target_paths_per_frame: u32,

    /// Contiguous runs of paths per frame, which is a hard cost no CPU
    /// profiler shows.
    ///
    /// Each run is a full-viewport intermediate render pass with a clear, at
    /// ~0.09–0.26 ms of GPU; 130–190 of them consume a whole frame at 60 fps,
    /// with negligible CPU. **The rule this implies is a paint-order contract,
    /// not a budget to spend**: paint all quads, then all paths, then all text,
    /// never interleaved per element. 256 paths painted contiguously is one
    /// batch and 1.11 ms; the same 256 interleaved with quads is 256 batches
    /// and 15 fps.
    pub max_path_batches_per_frame: u32,

    /// Axis-aligned rectangles painted as quads that hold 60 fps.
    ///
    /// A `Quad` is a fixed-size instance — no vertex buffer, no intermediate
    /// pass — and carries corner radii, borders and border style for free.
    /// 20,000 quads cost 1.5 ms at 60 fps where 20,000 filled rect paths cost
    /// 3.0 ms at 30. **Every axis-aligned rectangle in this engine is a quad**:
    /// the grid, node bodies, the selection rectangle, handles, overview boxes.
    pub target_quads_per_frame: u32,

    /// Rich interactive GPUI elements that hold 60 fps.
    ///
    /// Measured at ~1,600, against the requirements document's assumed "~70
    /// visible nodes" — the platform is roomier here than the specification
    /// expected. It is still a ceiling on *visible* nodes, and §16's rule that
    /// a 100,000-node document produces tens of elements is unaffected.
    pub max_rich_elements: u32,

    /// The geometry cache's hard byte bound.
    ///
    /// 64 MiB ≈ 2 M cached vertices at [`CACHE_BYTES_PER_VERTEX`], which is
    /// roughly six frames' worth of the sustainable vertex target — enough that
    /// a pan reuses nearly everything, small enough that a 300,000-edge
    /// document cannot turn the cache into the ~1.9 GB it would reach if every
    /// edge were held (300,000 edges × 200 vertices ×
    /// [`CACHE_BYTES_PER_VERTEX`]). Bounded memory behaviour has to be a bound,
    /// not a caution, and this is it.
    pub geometry_cache_max_bytes: usize,

    /// The most shaped labels the engine's own text cache holds.
    ///
    /// GPUI has a line-layout cache and it is **two frames deep** — current
    /// frame plus previous, unused keys evicted — so a label that leaves the
    /// viewport for a single frame is re-shaped on return, at ~7–11 µs against
    /// ~1.7 µs to paint a cached one. Phase 0 measured 5,000 cached labels
    /// holding 60 fps at 8.7 ms and 5,000 freshly shaped ones at 18 fps, which
    /// is why [`crate::render::cache::ShapedLineCache`] exists at all.
    ///
    /// 4,096 is above any plausible visible label count — the dense scene shows
    /// 1,584 nodes — and bounded, so a document with 100,000 labels holds the
    /// visible ones rather than all of them.
    pub max_shaped_lines: u32,

    /// How far the zoom may drift from the zoom a cached tessellation was built
    /// at before it is rebuilt, as a factor either way.
    ///
    /// Scaling a cached tessellation keeps its vertex count, so the flattening
    /// error grows with the zoom (error ≈ tolerance × k) and the stroke width
    /// scales with it. Measurement saw no visible difference at 8× on 12 px
    /// corners, so the headroom is real — ±2× is the conservative band that
    /// buys responsiveness during a live pinch without ever showing a polygon.
    pub retessellation_zoom_band: f32,

    /// The fixed per-path CPU cost, in nanoseconds: `paint_path` consumes the
    /// path, so a cached one must be cloned into it, and the vertex array is
    /// traversed four times per painted path per frame.
    ///
    /// Fitted over the degraded region of the sweep — see
    /// [`RenderBudgets::predicted_paint_micros`] for what that means and why
    /// the pair is an upper bound rather than a prediction.
    pub nanos_per_path: u32,

    /// The marginal CPU cost per painted vertex, in nanoseconds. Same caveat as
    /// [`RenderBudgets::nanos_per_path`].
    pub nanos_per_vertex: u32,

    pub lod: LodThresholds,
}

impl RenderBudgets {
    /// Whether a frame carrying this many path vertices should be degraded
    /// before it is painted. **The check that prevents a black window.**
    pub fn exceeds_safe_vertices(&self, vertices: u32) -> bool {
        vertices > self.safe_path_vertex_ceiling
    }

    /// Whether a frame of this size is expected to hold 60 fps.
    pub fn within_frame_target(&self, vertices: u32) -> bool {
        vertices <= self.target_path_vertices_per_frame
    }

    /// A **conservative upper bound** on the CPU cost of painting `paths` paths
    /// totalling `vertices` vertices, in microseconds.
    ///
    /// One place for the cost model, so a benchmark harness and the renderer's
    /// own degradation logic cannot disagree — but read what it is before
    /// trusting a number out of it. The two coefficients come from a
    /// decomposition sweep over path count and vertex count, and that fit was
    /// taken over the
    /// *degraded* region, where the frame time is no longer vsync-quantised.
    /// Applied to the healthy 60 fps frame recorded on
    /// [`target_path_vertices_per_frame`](RenderBudgets::target_path_vertices_per_frame)
    /// — 1,800 paths and 328,836 vertices, measured at 5.0 ms — it predicts
    /// ~13 ms, about **2.6× the measurement**.
    ///
    /// That gap is real and it is documented here rather than tuned away,
    /// because nothing in this slice can measure the healthy region again and a
    /// fitted-to-nothing constant would be worse than an honest over-estimate.
    /// Use it as a ceiling — "this frame will not cost more than" — never as a
    /// prediction.
    ///
    /// # Phase 4 could not re-fit it, and this is why
    ///
    /// Phase 1 asked Phase 4's harness to re-fit these two coefficients.
    /// **It cannot, and the reason is structural rather than an omission.**
    /// They describe `Window::paint_path` — the clone, the `scale`, the
    /// `insert_primitive` clone and the Metal renderer's per-batch expansion,
    /// four traversals of the vertex array ending in a 104-bytes-per-vertex GPU
    /// upload. Every one of those needs a real `Window`, and
    /// `examples/flow_scene_bench.rs` is a headless example.
    ///
    /// What that harness *can* measure, because `render::painter::build_path`
    /// needs no window, is **tessellation**: 1.01 µs per path on the dense
    /// scene and 2.74 µs on the large one, at 3.12 ms and 0.35 ms per frame
    /// respectively. That is a different cost from this one and is recorded in
    /// [`crate::render::plan`] rather than folded in here.
    ///
    /// Re-fitting these two therefore needs a windowed spike of the shape Phase
    /// 0 used, and it is worth doing when a phase has a window to hand anyway —
    /// Phase 5 is the first that does. Until then the pair stays an honest
    /// over-estimate.
    pub fn predicted_paint_micros(&self, paths: u32, vertices: u32) -> f32 {
        let nanos = paths as f32 * self.nanos_per_path as f32
            + vertices as f32 * self.nanos_per_vertex as f32;
        nanos / 1_000.0
    }

    /// How many cached vertices fit in the geometry cache's byte bound.
    pub fn geometry_cache_max_vertices(&self) -> usize {
        self.geometry_cache_max_bytes / CACHE_BYTES_PER_VERTEX as usize
    }
}

/// The macOS/Metal budgets — **the only measured row**.
///
/// Measured on 2026-08-16 on an **Apple M1 laptop**, in a 1440×900 logical
/// window at scale factor 2, built in dodo's shipping release profile against
/// gpui `a1230fc`. Every field below carries the scene it came from, so each
/// can be re-measured independently on other hardware — they are properties of
/// a machine and a window size as much as of a renderer, and treating them as
/// universal is how a constant outlives its truth.
const METAL: RenderBudgets = RenderBudgets {
    backend: RenderBackend::Metal,
    provenance: Provenance::Measured {
        machine: "Apple M1, 1440x900 @2x",
        date: "2026-08-16",
    },
    // 256 MiB instance-buffer cap / 104 bytes per vertex, confirmed empirically
    // between 12,000 and 13,000 200-vertex edges.
    hard_path_vertex_ceiling: 2_581_110,
    // ~93 % of the cliff: enough headroom for the estimate to be made before
    // tessellation and for quads and sprites sharing the same instance buffer.
    safe_path_vertex_ceiling: 2_400_000,
    // A realistic frame — 600 nodes as rounded quads plus a stroke path each,
    // two 180 px Bézier edges each and one cached label each — is 1,800 paths
    // and 328,836 vertices, measured at 5.0 ms of paint CPU and holding 60 fps;
    // 1,200 nodes (655,758 vertices) drops to 29 fps. 350,000 is the working
    // budget that sits between them, and halving the flattening tolerance
    // quality roughly doubles it.
    target_path_vertices_per_frame: 350_000,
    // Derived rather than measured — see the field's doc. Half a 16.7 ms frame
    // at `nanos_per_path` is 5,566; 5,000 is that rounded down, and the dense
    // scene's 3,104 paths are the nearest measured point below it.
    target_paths_per_frame: 5_000,
    // Measured degradation sets in between 130 and 190 batches (128 holds 60
    // fps, 192 drops to 30); 64 is where the cost is still negligible, and is
    // the number the culling phase asserts against.
    max_path_batches_per_frame: 64,
    target_quads_per_frame: 20_000,
    max_rich_elements: 1_600,
    geometry_cache_max_bytes: 64 * 1024 * 1024,
    max_shaped_lines: 4_096,
    retessellation_zoom_band: 2.0,
    nanos_per_path: 1_500,
    nanos_per_vertex: 32,
    lod: LodThresholds::DEFAULT,
};

/// An unmeasured backend: macOS's shape, discounted. See
/// [`UNMEASURED_DISCOUNT`].
///
/// A `const fn` so every backend's row is built the same way and the discount
/// is applied in exactly one place. The LOD ladder is **not** discounted — it
/// is a legibility judgement about human eyes and screen pixels, not a property
/// of a renderer, so it is the same everywhere until a measurement says
/// otherwise.
const fn unmeasured(backend: RenderBackend) -> RenderBudgets {
    RenderBudgets {
        backend,
        provenance: Provenance::Unmeasured,
        // `const fn` cannot do float arithmetic on these, and would not want
        // to: the discount is a halving, spelled as a halving.
        hard_path_vertex_ceiling: METAL.hard_path_vertex_ceiling / 2,
        safe_path_vertex_ceiling: METAL.safe_path_vertex_ceiling / 2,
        target_path_vertices_per_frame: METAL.target_path_vertices_per_frame / 2,
        target_paths_per_frame: METAL.target_paths_per_frame / 2,
        max_path_batches_per_frame: METAL.max_path_batches_per_frame / 2,
        target_quads_per_frame: METAL.target_quads_per_frame / 2,
        max_rich_elements: METAL.max_rich_elements / 2,
        geometry_cache_max_bytes: METAL.geometry_cache_max_bytes / 2,
        max_shaped_lines: METAL.max_shaped_lines / 2,
        retessellation_zoom_band: METAL.retessellation_zoom_band,
        nanos_per_path: METAL.nanos_per_path,
        nanos_per_vertex: METAL.nanos_per_vertex,
        lod: LodThresholds::DEFAULT,
    }
}

/// The budgets for one backend.
pub const fn for_backend(backend: RenderBackend) -> RenderBudgets {
    match backend {
        RenderBackend::Metal => METAL,
        RenderBackend::Windows => unmeasured(RenderBackend::Windows),
        RenderBackend::Linux => unmeasured(RenderBackend::Linux),
    }
}

/// Which GPUI renderer a host uses.
///
/// A total function of a [`HostOs`], so the Windows and Linux answers are
/// asserted from a Mac. `HostOs::Unix` maps to [`RenderBackend::Linux`]: dodo
/// ships no BSD target, and `gpui_linux` is what a Unix build links.
pub const fn backend_for_host(host: HostOs) -> RenderBackend {
    match host {
        HostOs::MacOs => RenderBackend::Metal,
        HostOs::Windows => RenderBackend::Windows,
        HostOs::Unix => RenderBackend::Linux,
    }
}

/// The budgets for a host platform. **Every platform's answer is available from
/// every platform** — that is the whole point, see the module doc.
pub const fn for_host(host: HostOs) -> RenderBudgets {
    for_backend(backend_for_host(host))
}

/// The budgets for the platform this build targets.
///
/// The one place in the crate that reads `cfg!`. Everything else takes a
/// [`HostOs`] or a [`RenderBudgets`], which is what keeps the engine testable
/// against all three.
pub fn current() -> RenderBudgets {
    for_host(current_host())
}

/// The platform this build targets, as a value.
///
/// `cfg!` rather than `#[cfg]`, so all three arms compile everywhere — the same
/// seam `dodo-docker`'s `paths::current()` uses, and for the same reason.
pub const fn current_host() -> HostOs {
    if cfg!(target_os = "macos") {
        HostOs::MacOs
    } else if cfg!(target_os = "windows") {
        HostOs::Windows
    } else {
        HostOs::Unix
    }
}

#[cfg(test)]
mod tests {
    use dodo_paths::HostOs;

    use super::{
        CACHE_BYTES_PER_VERTEX, DetailLevel, LodThresholds, METAL_BYTES_PER_UPLOADED_VERTEX,
        Provenance, RenderBackend, current, current_host, for_backend, for_host,
    };

    const EVERY_HOST: [HostOs; 3] = [HostOs::MacOs, HostOs::Windows, HostOs::Unix];

    #[test]
    fn every_platforms_budget_is_answerable_from_this_one() {
        // The invariant this module exists to satisfy: two of dodo's four
        // release targets cannot be built here, so their budgets must still be
        // values this machine can produce and assert on.
        for host in EVERY_HOST {
            let budgets = for_host(host);
            assert!(budgets.hard_path_vertex_ceiling > 0, "{host:?}");
        }
    }

    #[test]
    fn only_macos_is_measured_and_it_says_where_from() {
        let metal = for_host(HostOs::MacOs);

        match metal.provenance {
            Provenance::Measured { machine, date } => {
                assert!(machine.contains("M1"));
                assert_eq!(date, "2026-08-16");
            }
            other => panic!("macOS must carry its measurement, got {other:?}"),
        }

        for host in [HostOs::Windows, HostOs::Unix] {
            assert_eq!(
                for_host(host).provenance,
                Provenance::Unmeasured,
                "{host:?} has not been measured and must not claim to be"
            );
        }
    }

    #[test]
    fn the_measured_macos_ceiling_is_the_instance_buffer_divided_by_the_vertex_size() {
        // 256 MiB / 104 bytes. If either constant is edited without the other,
        // this catches it.
        let expected = (256 * 1024 * 1024) / METAL_BYTES_PER_UPLOADED_VERTEX;

        assert_eq!(for_host(HostOs::MacOs).hard_path_vertex_ceiling, expected);
    }

    #[test]
    fn an_unmeasured_backend_is_conservative_rather_than_optimistic() {
        let metal = for_host(HostOs::MacOs);

        for host in [HostOs::Windows, HostOs::Unix] {
            let other = for_host(host);
            assert!(
                other.hard_path_vertex_ceiling < metal.hard_path_vertex_ceiling,
                "{host:?} must not claim macOS's measured headroom"
            );
            assert!(other.safe_path_vertex_ceiling < metal.safe_path_vertex_ceiling);
            assert!(other.max_rich_elements < metal.max_rich_elements);
        }
    }

    #[test]
    fn the_lod_ladder_is_a_legibility_judgement_and_does_not_vary_by_backend() {
        let metal = for_host(HostOs::MacOs);

        for host in EVERY_HOST {
            assert_eq!(for_host(host).lod, metal.lod, "{host:?}");
        }
    }

    #[test]
    fn every_backends_ceilings_are_ordered_target_then_safe_then_hard() {
        for host in EVERY_HOST {
            let b = for_host(host);

            assert!(
                b.target_path_vertices_per_frame < b.safe_path_vertex_ceiling,
                "{host:?}: the smooth-frame target must sit well below the degradation trigger"
            );
            assert!(
                b.safe_path_vertex_ceiling < b.hard_path_vertex_ceiling,
                "{host:?}: degradation must trigger before the cliff, not at it"
            );
        }
    }

    #[test]
    fn the_degradation_check_fires_before_the_cliff() {
        let b = for_host(HostOs::MacOs);

        assert!(!b.exceeds_safe_vertices(b.target_path_vertices_per_frame));
        assert!(!b.exceeds_safe_vertices(b.safe_path_vertex_ceiling));
        assert!(b.exceeds_safe_vertices(b.safe_path_vertex_ceiling + 1));
        assert!(
            b.exceeds_safe_vertices(b.hard_path_vertex_ceiling),
            "a frame at the cliff must already have been degraded"
        );
    }

    #[test]
    fn the_frame_target_is_the_smoothness_question_not_the_cliff_one() {
        let b = for_host(HostOs::MacOs);

        assert!(b.within_frame_target(b.target_path_vertices_per_frame));
        assert!(!b.within_frame_target(b.target_path_vertices_per_frame + 1));
        assert!(
            !b.exceeds_safe_vertices(b.target_path_vertices_per_frame + 1),
            "over the smooth target is a slow frame, not a black one"
        );
    }

    #[test]
    fn the_cost_model_is_an_upper_bound_on_the_measured_realistic_frame() {
        let b = for_host(HostOs::MacOs);

        // The realistic frame recorded on `METAL`: 600 nodes, 1,800 paths,
        // 328,836 path vertices, **measured at 5.0 ms** of paint CPU and
        // holding 60 fps. The model is fitted to the degraded region and
        // overestimates a healthy frame — this pins how much, so a re-fit
        // against new measurements is a visible change rather than a silent
        // one.
        const MEASURED_MICROS: f32 = 5_000.0;
        let micros = b.predicted_paint_micros(1_800, 328_836);

        assert!(
            micros > MEASURED_MICROS,
            "the model must never under-predict a measured frame: {micros} us"
        );
        assert!(
            micros < MEASURED_MICROS * 3.0,
            "the model overestimates by more than 3x ({micros} us); re-fit it"
        );
    }

    #[test]
    fn the_geometry_cache_bound_is_stated_in_bytes_and_converts_to_vertices() {
        let b = for_host(HostOs::MacOs);

        assert_eq!(b.geometry_cache_max_bytes, 64 * 1024 * 1024);
        assert_eq!(
            b.geometry_cache_max_vertices(),
            b.geometry_cache_max_bytes / CACHE_BYTES_PER_VERTEX as usize
        );
        assert!(
            b.geometry_cache_max_vertices() > b.target_path_vertices_per_frame as usize,
            "the cache must hold more than one frame or a pan reuses nothing"
        );
    }

    #[test]
    fn the_lod_ladder_is_the_one_the_requirements_describe() {
        let lod = LodThresholds::DEFAULT;

        assert_eq!(lod.detail(1.0), DetailLevel::Full);
        assert_eq!(lod.detail(0.6), DetailLevel::Full);
        assert_eq!(lod.detail(0.59), DetailLevel::Compact);
        assert_eq!(lod.detail(0.2), DetailLevel::Compact);
        assert_eq!(lod.detail(0.19), DetailLevel::Overview);
        assert_eq!(lod.detail(0.01), DetailLevel::Overview);
    }

    #[test]
    fn curves_degrade_to_quads_earlier_than_rectangles_do() {
        let lod = LodThresholds::DEFAULT;

        assert!(
            lod.curve_to_quad_zoom > lod.compact_zoom,
            "an ellipse costs 337 vertices against a rectangle's 24, so it must \
             give up its curve before the node gives up its label"
        );
        assert!(lod.degrade_curves_to_quads(0.3));
        assert!(!lod.degrade_curves_to_quads(0.4));
    }

    #[test]
    fn font_sizes_snap_onto_the_ladder_so_a_zoom_does_not_reshape_every_label() {
        let lod = LodThresholds::DEFAULT;

        assert_eq!(lod.quantize_font_size(13.0), 13.0);
        assert_eq!(lod.quantize_font_size(13.4), 13.0);
        assert_eq!(lod.quantize_font_size(15.9), 13.0);
        assert_eq!(lod.quantize_font_size(16.0), 16.0);
        assert_eq!(
            lod.quantize_font_size(400.0),
            28.0,
            "clamped to the top rung"
        );
        assert_eq!(
            lod.quantize_font_size(0.5),
            9.0,
            "clamped to the bottom rung"
        );
    }

    #[test]
    fn a_continuous_zoom_produces_only_a_handful_of_shaped_sizes() {
        // The property the ladder exists for: sweeping the zoom must not
        // produce a new `font_size` — and therefore a new shaped line — per
        // frame.
        let lod = LodThresholds::DEFAULT;
        let mut sizes: Vec<f32> = (1..=400)
            .map(|i| lod.quantize_font_size(14.0 * i as f32 / 100.0))
            .collect();
        sizes.sort_by(f32::total_cmp);
        sizes.dedup();

        assert!(
            sizes.len() <= lod.font_size_ladder.len(),
            "400 zoom steps produced {} distinct font sizes",
            sizes.len()
        );
    }

    #[test]
    fn the_host_to_backend_mapping_is_total() {
        assert_eq!(for_host(HostOs::MacOs).backend, RenderBackend::Metal);
        assert_eq!(for_host(HostOs::Windows).backend, RenderBackend::Windows);
        assert_eq!(for_host(HostOs::Unix).backend, RenderBackend::Linux);

        for host in EVERY_HOST {
            assert_eq!(for_host(host), for_backend(for_host(host).backend));
        }
    }

    #[test]
    fn current_agrees_with_the_host_it_is_built_for() {
        assert_eq!(current(), for_host(current_host()));

        // The `cfg!` seam itself, asserted against the compiler's own view.
        let expected = if cfg!(target_os = "macos") {
            HostOs::MacOs
        } else if cfg!(target_os = "windows") {
            HostOs::Windows
        } else {
            HostOs::Unix
        };
        assert_eq!(current_host(), expected);
    }
}
