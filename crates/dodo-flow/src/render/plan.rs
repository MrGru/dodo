//! [`PaintPlan`] — **the paint-order contract, made inexpressible to break**.
//!
//! Phase 0 measured the trap this module exists to prevent. `Scene::batches()`
//! groups primitives by draw order, and a run of paths breaks the moment
//! another primitive kind sorts between them. On macOS every path batch costs a
//! render pass into a **full-viewport intermediate texture with a clear**, plus
//! a composite pass — about 0.09–0.26 ms of GPU each. Sixteen batches are free;
//! 128 still hold 60 fps; **192 drop to 30 and 256 to 15**. The same 256 paths
//! painted contiguously run at 60 fps with 1.11 ms of CPU.
//!
//! So "paint the node's quad, then the node's border path, then the node's
//! label" — the obvious per-element loop — spends the entire frame budget at
//! about 130 nodes, and it does so **invisibly to a CPU profiler**, because the
//! CPU cost of those batches is negligible. It is the kind of mistake that is
//! found six months later by someone bisecting a frame graph.
//!
//! # The shape of the fix
//!
//! A painter never touches a window. It pushes into one of three typed buckets
//! here, in whatever order suits it, and [`PaintPlan::paint_into`] is the
//! **only** thing that ever emits them:
//!
//! ```text
//! push_quad ─┐                    ┌─ every quad ─┐
//! push_path ─┼─> PaintPlan ─ paint_into ─> every path ─┼─> PrimitiveSink
//! push_text ─┘                    └─ every text ─┘
//! ```
//!
//! There is no accessor that hands the primitives back in insertion order, no
//! iterator over a mixed sequence, and the sink never sees the plan. A painter
//! that wanted to interleave would have to add a method to this file, which is
//! exactly the review moment the contract is for.
//!
//! The three buckets are also why `text` exists here with nothing pushing into
//! it yet: the *order* is the contract, and a phase that adds text later must
//! not get to choose where text goes.
//!
//! # Painted-vertex accounting
//!
//! [`PrimitiveSink::path`] returns the number of vertices it actually painted,
//! and [`PaintStats`] accumulates them. This is not instrumentation. macOS
//! stops presenting the drawable past ~2.58 M path vertices in a frame and the
//! window goes **solid black** — see [`crate::budgets`] — so the count is a
//! correctness signal, and [`PaintPlan::enforce_vertex_ceiling`] spends it
//! before the frame rather than after.
//!
//! **This file names no UI framework.** Coordinates are pane-relative screen
//! pixels as plain [`Vec2`]; the sink adds the element's origin.

use crate::render::shapes::Outline;
use crate::{
    budgets::RenderBudgets,
    geometry::{Rect, Vec2},
    models::{Color, RenderQuality},
};

/// An axis-aligned rectangle with optional corner radii and a border.
///
/// **The default primitive, not a special case.** Phase 0 measured 20,000 quads
/// holding 60 fps where the same count of filled rectangular paths dropped to
/// 30, and a quad is a fixed-size instance — no vertex buffer, no intermediate
/// render pass, no per-batch full-viewport clear. Corner radii and borders come
/// free. Grid dots, grid lines, the selection rectangle, node bodies and
/// handles are all quads; a path is for genuine curves and diagonals.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct QuadPrimitive {
    /// Pane-relative screen pixels.
    pub bounds: Rect,
    pub background: Color,
    /// Uniform, in screen pixels. A radius of half the shorter side is a
    /// circle, which is how a grid dot is drawn.
    pub corner_radius: f32,
    pub border_width: f32,
    pub border_color: Color,
}

impl QuadPrimitive {
    /// A plain filled rectangle: no radius, no border.
    pub fn filled(bounds: Rect, background: Color) -> QuadPrimitive {
        QuadPrimitive {
            bounds,
            background,
            corner_radius: 0.0,
            border_width: 0.0,
            border_color: Color::TRANSPARENT,
        }
    }

    pub fn with_corner_radius(mut self, radius: f32) -> QuadPrimitive {
        self.corner_radius = radius;
        self
    }

    pub fn with_border(mut self, width: f32, color: Color) -> QuadPrimitive {
        self.border_width = width;
        self.border_color = color;
        self
    }
}

/// How a path is turned into triangles: filled, or stroked at a width.
///
/// The distinction is not cosmetic — it changes the vertex count by roughly an
/// order of magnitude, because a stroke emits a quad per flattened segment
/// where a fill emits a fan over the whole outline. [`Outline::estimated_vertices`]
/// takes it for that reason.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PathPaint {
    Fill(Color),
    Stroke { color: Color, width: f32 },
}

impl PathPaint {
    pub fn color(&self) -> Color {
        match self {
            PathPaint::Fill(color) => *color,
            PathPaint::Stroke { color, .. } => *color,
        }
    }
}

/// A tessellated shape: the expensive primitive, and the one with a ceiling.
#[derive(Debug, Clone, PartialEq)]
pub struct PathPrimitive {
    /// Pane-relative screen pixels, already transformed by the viewport.
    /// Tessellation happens in screen space so the flattening tolerance means
    /// what it says — pixels of deviation on the display.
    pub outline: Outline,
    pub paint: PathPaint,
    /// The flattening tolerance this path is tessellated at. Carried per path
    /// rather than read globally because it is part of every geometry cache key
    /// and a later phase degrades it per element under LOD.
    pub quality: RenderQuality,
}

impl PathPrimitive {
    pub fn fill(outline: Outline, color: Color, quality: RenderQuality) -> PathPrimitive {
        PathPrimitive {
            outline,
            paint: PathPaint::Fill(color),
            quality,
        }
    }

    pub fn stroke(
        outline: Outline,
        color: Color,
        width: f32,
        quality: RenderQuality,
    ) -> PathPrimitive {
        PathPrimitive {
            outline,
            paint: PathPaint::Stroke { color, width },
            quality,
        }
    }

    /// The upper bound on the vertices this path will paint. See
    /// [`Outline::estimated_vertices`] for how good the bound is.
    pub fn estimated_vertices(&self) -> u32 {
        self.outline.estimated_vertices(self.paint, self.quality)
    }
}

/// A run of text on the canvas.
///
/// **Nothing pushes one of these yet** — text elements are Phase 5's, and
/// `ShapedLine` caching with them. The type exists now because the paint order
/// is the contract: text is last, and a phase that arrives with labels must
/// inherit that rather than decide it.
#[derive(Debug, Clone, PartialEq)]
pub struct TextPrimitive {
    /// Pane-relative screen pixels: the text's baseline origin.
    pub origin: Vec2,
    pub text: String,
    pub font_size: f32,
    pub color: Color,
}

/// What one frame actually painted.
///
/// `path_vertices` is the field that matters: it is measured against
/// [`RenderBudgets::hard_path_vertex_ceiling`], past which macOS stops
/// presenting the drawable entirely.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PaintStats {
    pub quads: u32,
    pub paths: u32,
    /// Vertices the sink reported actually painting — not the estimate.
    pub path_vertices: u32,
    pub glyphs: u32,
}

impl PaintStats {
    /// Whether this frame risked the black window. See [`crate::budgets`].
    pub fn exceeds_safe_vertices(&self, budgets: &RenderBudgets) -> bool {
        budgets.exceeds_safe_vertices(self.path_vertices)
    }

    pub fn within_frame_target(&self, budgets: &RenderBudgets) -> bool {
        budgets.within_frame_target(self.path_vertices)
    }
}

/// The three primitive kinds, in paint order. `Ord` **is** the contract: a
/// recording sink sorts its log by this and must find it already sorted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PrimitiveKind {
    Quad,
    Path,
    Text,
}

/// What a backend implements to receive a frame.
///
/// Deliberately narrow: three methods, called only by
/// [`PaintPlan::paint_into`], in the contract's order. A sink cannot ask the
/// plan for anything, so it cannot reorder what it is given.
pub trait PrimitiveSink {
    fn quad(&mut self, quad: &QuadPrimitive);

    /// Returns the number of vertices actually painted — zero if the sink
    /// declined the path (degenerate outline, failed tessellation).
    fn path(&mut self, path: &PathPrimitive) -> u32;

    /// Returns the number of glyphs actually painted.
    fn text(&mut self, text: &TextPrimitive) -> u32;
}

/// One frame's primitives, bucketed by kind.
///
/// Held on the view and `clear`ed each frame rather than reallocated —
/// requirements §40 rule 14, and pan is the hot path this exists to keep
/// allocation-free.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PaintPlan {
    quads: Vec<QuadPrimitive>,
    paths: Vec<PathPrimitive>,
    texts: Vec<TextPrimitive>,
}

impl PaintPlan {
    pub fn new() -> PaintPlan {
        PaintPlan::default()
    }

    /// Empties the buckets, **keeping their capacity**. The point of the type
    /// being owned by the view rather than built per frame.
    pub fn clear(&mut self) {
        self.quads.clear();
        self.paths.clear();
        self.texts.clear();
    }

    pub fn push_quad(&mut self, quad: QuadPrimitive) {
        self.quads.push(quad);
    }

    pub fn push_path(&mut self, path: PathPrimitive) {
        self.paths.push(path);
    }

    pub fn push_text(&mut self, text: TextPrimitive) {
        self.texts.push(text);
    }

    pub fn quad_count(&self) -> u32 {
        self.quads.len() as u32
    }

    pub fn path_count(&self) -> u32 {
        self.paths.len() as u32
    }

    pub fn text_count(&self) -> u32 {
        self.texts.len() as u32
    }

    pub fn is_empty(&self) -> bool {
        self.quads.is_empty() && self.paths.is_empty() && self.texts.is_empty()
    }

    /// The upper bound on the path vertices this frame will paint, available
    /// **before** any tessellation happens. That ordering is the whole point:
    /// the ceiling has to be respected before the vertices exist, because by
    /// the time they exist the frame is already over budget.
    pub fn estimated_path_vertices(&self) -> u32 {
        self.paths
            .iter()
            .map(PathPrimitive::estimated_vertices)
            .fold(0u32, u32::saturating_add)
    }

    /// **The black-window guard.** Drops paths from the end until the estimated
    /// vertex total fits inside [`RenderBudgets::safe_path_vertex_ceiling`],
    /// and returns how many were dropped.
    ///
    /// Dropping geometry is a bad frame; painting it is a window that renders
    /// nothing at all and gives no clue why (`metal_renderer.rs` logs and
    /// `break`s out of the draw loop, and dodo installs no logger). A visibly
    /// incomplete canvas is strictly better than a black one, and Phase 4's
    /// culling is what stops this from ever firing in practice — this is the
    /// backstop for when culling is wrong.
    ///
    /// Paths are dropped from the end because the painters push background
    /// first and overlays last, so the earliest entries are the ones the user
    /// most needs to see. A later phase replaces the truncation with LOD
    /// degradation; the guard stays either way.
    pub fn enforce_vertex_ceiling(&mut self, budgets: &RenderBudgets) -> u32 {
        let ceiling = budgets.safe_path_vertex_ceiling;
        let mut total = 0u32;
        let mut keep = 0usize;

        for path in &self.paths {
            let next = total.saturating_add(path.estimated_vertices());
            if next > ceiling {
                break;
            }
            total = next;
            keep += 1;
        }

        let dropped = self.paths.len() - keep;
        self.paths.truncate(keep);
        dropped as u32
    }

    /// **Emits every primitive, grouped by kind, in the contract's order.**
    ///
    /// The only method in the crate that hands primitives to anything. All
    /// quads, then all paths, then all text — one contiguous run each, so the
    /// scene sees at most one path batch from the canvas layer.
    pub fn paint_into<S: PrimitiveSink + ?Sized>(&self, sink: &mut S) -> PaintStats {
        let mut stats = PaintStats::default();

        for quad in &self.quads {
            sink.quad(quad);
            stats.quads += 1;
        }

        for path in &self.paths {
            let vertices = sink.path(path);
            if vertices > 0 {
                stats.paths += 1;
                stats.path_vertices = stats.path_vertices.saturating_add(vertices);
            }
        }

        for text in &self.texts {
            stats.glyphs = stats.glyphs.saturating_add(sink.text(text));
        }

        stats
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::budgets::{RenderBackend, for_backend};
    use crate::render::shapes;

    /// Records what it was handed, in the order it was handed it.
    #[derive(Default)]
    struct RecordingSink {
        log: Vec<PrimitiveKind>,
        vertices_per_path: u32,
    }

    impl PrimitiveSink for RecordingSink {
        fn quad(&mut self, _quad: &QuadPrimitive) {
            self.log.push(PrimitiveKind::Quad);
        }

        fn path(&mut self, _path: &PathPrimitive) -> u32 {
            self.log.push(PrimitiveKind::Path);
            self.vertices_per_path
        }

        fn text(&mut self, text: &TextPrimitive) -> u32 {
            self.log.push(PrimitiveKind::Text);
            text.text.chars().count() as u32
        }
    }

    fn rect(x: f32, y: f32) -> Rect {
        Rect::new(Vec2::new(x, y), Vec2::new(10.0, 10.0))
    }

    fn a_quad() -> QuadPrimitive {
        QuadPrimitive::filled(rect(0.0, 0.0), Color::WHITE)
    }

    fn a_path() -> PathPrimitive {
        PathPrimitive::fill(
            shapes::diamond(rect(0.0, 0.0)),
            Color::WHITE,
            RenderQuality::BALANCED,
        )
    }

    fn a_text() -> TextPrimitive {
        TextPrimitive {
            origin: Vec2::ZERO,
            text: "abc".into(),
            font_size: 12.0,
            color: Color::WHITE,
        }
    }

    /// **The paint-order contract.** Pushed in a deliberately hostile
    /// interleaving; emitted grouped, quads then paths then text.
    #[test]
    fn primitives_are_emitted_grouped_by_kind_whatever_the_push_order() {
        let mut plan = PaintPlan::new();
        plan.push_path(a_path());
        plan.push_text(a_text());
        plan.push_quad(a_quad());
        plan.push_path(a_path());
        plan.push_quad(a_quad());
        plan.push_text(a_text());
        plan.push_quad(a_quad());

        let mut sink = RecordingSink {
            vertices_per_path: 7,
            ..RecordingSink::default()
        };
        plan.paint_into(&mut sink);

        assert_eq!(
            sink.log,
            vec![
                PrimitiveKind::Quad,
                PrimitiveKind::Quad,
                PrimitiveKind::Quad,
                PrimitiveKind::Path,
                PrimitiveKind::Path,
                PrimitiveKind::Text,
                PrimitiveKind::Text,
            ]
        );
    }

    /// The property the batching cost actually depends on: whatever is pushed,
    /// the emitted sequence never returns to an earlier kind. One quad run, one
    /// path run, one text run — so the scene sees one path batch.
    #[test]
    fn emitted_kinds_never_go_backwards() {
        let mut plan = PaintPlan::new();
        for i in 0..40 {
            match i % 3 {
                0 => plan.push_text(a_text()),
                1 => plan.push_quad(a_quad()),
                _ => plan.push_path(a_path()),
            }
        }

        let mut sink = RecordingSink {
            vertices_per_path: 1,
            ..RecordingSink::default()
        };
        plan.paint_into(&mut sink);

        assert!(
            sink.log.windows(2).all(|pair| pair[0] <= pair[1]),
            "paint order left one contiguous run per kind: {:?}",
            sink.log
        );
        assert_eq!(sink.log.len(), 40);
    }

    #[test]
    fn stats_count_what_the_sink_reported() {
        let mut plan = PaintPlan::new();
        plan.push_quad(a_quad());
        plan.push_quad(a_quad());
        plan.push_path(a_path());
        plan.push_path(a_path());
        plan.push_text(a_text());

        let mut sink = RecordingSink {
            vertices_per_path: 33,
            ..RecordingSink::default()
        };
        let stats = plan.paint_into(&mut sink);

        assert_eq!(stats.quads, 2);
        assert_eq!(stats.paths, 2);
        assert_eq!(stats.path_vertices, 66);
        assert_eq!(stats.glyphs, 3);
    }

    /// A path the sink declined is not counted — the accounting is of what was
    /// painted, not of what was planned.
    #[test]
    fn a_declined_path_counts_neither_as_a_path_nor_as_vertices() {
        let mut plan = PaintPlan::new();
        plan.push_path(a_path());

        let mut sink = RecordingSink::default();
        let stats = plan.paint_into(&mut sink);

        assert_eq!(sink.log, vec![PrimitiveKind::Path]);
        assert_eq!(stats.paths, 0);
        assert_eq!(stats.path_vertices, 0);
    }

    #[test]
    fn clear_keeps_capacity_so_a_pan_frame_allocates_nothing() {
        let mut plan = PaintPlan::new();
        for _ in 0..64 {
            plan.push_quad(a_quad());
            plan.push_path(a_path());
        }
        let quad_capacity = plan.quads.capacity();
        let path_capacity = plan.paths.capacity();

        plan.clear();

        assert!(plan.is_empty());
        assert_eq!(plan.quads.capacity(), quad_capacity);
        assert_eq!(plan.paths.capacity(), path_capacity);
    }

    #[test]
    fn the_vertex_ceiling_drops_paths_until_the_frame_fits() {
        let budgets = for_backend(RenderBackend::Metal);
        let one = a_path().estimated_vertices();
        assert!(one > 0, "the estimator has to have an opinion");

        // Deliberately far past the ceiling.
        let count = (budgets.safe_path_vertex_ceiling / one) as usize + 500;
        let mut plan = PaintPlan::new();
        for _ in 0..count {
            plan.push_path(a_path());
        }
        assert!(plan.estimated_path_vertices() > budgets.safe_path_vertex_ceiling);

        let dropped = plan.enforce_vertex_ceiling(&budgets);

        assert!(dropped > 0);
        assert_eq!(plan.path_count() + dropped, count as u32);
        assert!(
            plan.estimated_path_vertices() <= budgets.safe_path_vertex_ceiling,
            "the guard exists so the window is never black"
        );
    }

    #[test]
    fn the_vertex_ceiling_leaves_an_ordinary_frame_alone() {
        let budgets = for_backend(RenderBackend::Metal);
        let mut plan = PaintPlan::new();
        for _ in 0..500 {
            plan.push_path(a_path());
        }

        assert_eq!(plan.enforce_vertex_ceiling(&budgets), 0);
        assert_eq!(plan.path_count(), 500);
    }

    /// The unmeasured backends are discounted, so the same frame is guarded
    /// harder there — a property of `budgets`, asserted here because this is
    /// the code that spends it.
    #[test]
    fn an_unmeasured_backend_guards_the_same_frame_sooner() {
        let metal = for_backend(RenderBackend::Metal);
        let windows = for_backend(RenderBackend::Windows);
        let one = a_path().estimated_vertices();
        let count = (metal.safe_path_vertex_ceiling / one) as usize;

        let mut on_metal = PaintPlan::new();
        let mut on_windows = PaintPlan::new();
        for _ in 0..count {
            on_metal.push_path(a_path());
            on_windows.push_path(a_path());
        }

        assert_eq!(on_metal.enforce_vertex_ceiling(&metal), 0);
        assert!(on_windows.enforce_vertex_ceiling(&windows) > 0);
    }
}
