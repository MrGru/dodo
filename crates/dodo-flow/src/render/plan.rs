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
//! push_quad ──┐                     ┌─ every quad ──┐
//! push_image ─┤                     ├─ every image ─┤
//! push_path ──┼─> PaintPlan ─ paint_into ─> every path ──┼─> PrimitiveSink
//! push_text ──┘                     └─ every text ──┘
//! ```
//!
//! # Where §10's images sit in that order, and what it costs
//!
//! **One contiguous run of their own, and the run moves.** A picture is a
//! *body*, so its neighbours in the paint order are the other bodies — but
//! unlike every other kind here it has no second form: a quad-bodied shape that
//! a depth needs above a path is *promoted* into the path run (Phase 11's
//! `promotes_to_path`), and a picture has no outline to promote, exactly as a
//! glyph run has none.
//!
//! So the run's position is chosen per frame instead:
//!
//! - **Before the paths by default**, which is where a picture belongs — the
//!   overwhelmingly common thing to do with one is put a screenshot down and
//!   annotate over it, and everything drawn as a path (ellipses, diamonds,
//!   every edge, everything §13's hand touches) is then above it for free.
//! - **After the paths when the document asks**, which is
//!   [`PaintPlan::set_images_after_paths`] and is decided by
//!   [`render::scene`](crate::render::scene) from the *depths actually
//!   planned*: if the topmost picture sits above the topmost path-bodied
//!   element, the whole run moves.
//!
//! The batching contract is untouched either way — one run each, in one of two
//! orders — and what the frame paints is still decided in one place.
//!
//! **The limitation this leaves is narrow and is a user's to meet**: the run
//! moves as a whole, so two pictures on opposite sides of one path-bodied body
//! cannot both be satisfied — the one the document puts *lower* wins, because
//! the flag is set from the topmost picture. A canvas with one screenshot behind
//! a diagram and a second logo on top of it draws the logo behind the ellipse it
//! was meant to cover.
//!
//! An image is **cheap**: no tessellation, no path batch and no path vertices at
//! all. It is a textured quad, and its batching cost is one sprite batch per
//! *atlas texture* rather than one per picture — small images share an atlas
//! page, a large one gets a page to itself. A sprite batch is a draw call with a
//! texture bind, not the full-viewport intermediate pass with a clear that a
//! path batch costs, which is why the budget this run spends is
//! [`RenderBudgets::max_rich_elements`](crate::budgets::RenderBudgets::max_rich_elements)-shaped
//! rather than vertex-shaped.
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
//! # The clip is a property of the plan, not of the painters
//!
//! Phase 0's other measurement: 16,000 fully **offscreen** paths still cost
//! 6.3 ms of CPU per frame, because GPUI's content-mask rejection happens after
//! `paint_path` has cloned and scaled the vertex buffer. So "no offscreen path
//! reaches the painter" is a correctness property, and it is made structural
//! the same way the paint order was — [`PaintPlan::clear`] **takes the frame's
//! clip rectangle**, and [`PaintPlan::push_path`] rejects anything whose
//! painted extent misses it.
//!
//! That is deliberately the plan's job rather than each painter's. A painter
//! that forgot the check would be a silent 6 ms; a painter that cannot express
//! the mistake is a contract. [`PaintStats::culled_paths`] counts what the
//! rejection caught, so a scene extractor whose own culling is wrong shows up
//! as a number rather than as a slow frame.
//!
//! The broad phase in [`crate::spatial`] is what keeps this from being the
//! *only* culling: rejecting here still costs a rectangle test per element, and
//! a document with 100,000 nodes must not pay 100,000 of them per frame.
//!
//! # What a frame actually costs, measured
//!
//! Apple M1, release, 1440×900, 2026-08-19, through
//! `examples/flow_scene_bench.rs` — which implements [`PrimitiveSink`] itself
//! and calls `render::painter::build_path`, so these are real tessellations
//! rather than estimates:
//!
//! | scene | quads | paths | estimate | **painted** | batches | culled | dropped |
//! |---|---:|---:|---:|---:|---:|---:|---:|
//! | small (100 n) | 3,321 | 76 | 16,276 | 9,972 | 1 | 0 | 0 |
//! | medium (5 k) | 3,321 | 117 | 24,498 | 14,796 | 1 | 0 | 0 |
//! | large (100 k) | 3,321 | 126 | 31,188 | 19,242 | 1 | 0 | 0 |
//! | **dense (1,584 visible)** | 4,843 | 3,104 | 239,900 | **132,888** | 1 | 78 | 0 |
//!
//! The worst realistic frame is **132,888 painted vertices — 5.5 % of the
//! 2.4 M safe ceiling and 38 % of the 350,000 that holds 60 fps**, with 18×
//! headroom to the cliff. One path batch everywhere against a budget of 64.
//! Nothing was dropped by [`PaintPlan::enforce_vertex_ceiling`] on any of the
//! four scenes.
//!
//! # The estimate is a bound, and it is loose by about 1.6×
//!
//! `estimate` above is what [`PaintPlan::estimated_path_vertices`] spends and
//! `painted` is what lyon produced. The ratio is 1.63 on small, 1.66 on medium,
//! 1.62 on large and 1.81 on dense — consistently high, never low, which is the
//! direction a black-window guard has to err in. It is recorded rather than
//! tuned out: shaving it would buy nothing (the guard is 18× away from firing)
//! and would spend the safety margin that makes it a guard.
//!
//! # Tessellation, and the case for §23's geometry cache
//!
//! The same run, timing `build_path` alone:
//!
//! | scene | paths | tessellation | per path |
//! |---|---:|---:|---:|
//! | large | 126 | 0.35 ms | 2.74 µs |
//! | **dense** | 3,104 | **3.12 ms** | 1.01 µs |
//!
//! **3.12 ms is 19 % of a 16.7 ms frame, spent rebuilding geometry that did not
//! change.** Phase 0 measured that translating a cached tessellation for a pan
//! is about 12× cheaper than rebuilding it, and the dense scene's whole visible
//! set would be 4.25 MB of cache against
//! [`RenderBudgets::geometry_cache_max_bytes`]'s 64 MiB. So the cache is worth
//! building and it fits — it is simply not built yet. See [`crate::spatial`]
//! for the phase's full results.
//!
//! **This file names no UI framework.** Coordinates are pane-relative screen
//! pixels as plain [`Vec2`]; the sink adds the element's origin.

use std::sync::Arc;

use crate::render::shapes::Outline;
use crate::{
    budgets::RenderBudgets,
    geometry::{Rect, Vec2},
    models::{Color, FontFamily, NodeImage, NodeIndex, RenderQuality, TextAlign},
    render::cache::{GeometryKey, TextKey},
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

/// A dash on, then a dash off, in screen pixels.
///
/// Two lengths rather than an arbitrary pattern, and the type is `Copy` because
/// of it — [`PathPaint`] sits in an array with one entry per painted path, and
/// a `Vec` in there would allocate per dashed edge per frame.
/// [`DashPattern`](crate::models::DashPattern) in the document may hold a
/// longer pattern; [`DashPattern::spec`](crate::models::DashPattern::spec)
/// takes the first two entries, which is every pattern anyone has asked for and
/// covers dashed and dotted alike.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DashSpec {
    pub on: f32,
    pub off: f32,
}

impl DashSpec {
    pub fn new(on: f32, off: f32) -> DashSpec {
        DashSpec { on, off }
    }

    /// One dash plus one gap. Never zero, so a caller dividing a length by it
    /// cannot produce an infinity.
    pub fn period(&self) -> f32 {
        (self.on.max(0.0) + self.off.max(0.0)).max(f32::EPSILON)
    }
}

/// How a path is turned into triangles: filled, stroked, or **stroked with a
/// dash pattern, which is its own kind rather than a flag on a stroke**.
///
/// The distinction between fill and stroke is not cosmetic — it changes the
/// vertex count by roughly an order of magnitude, because a stroke emits a quad
/// per flattened segment where a fill emits a fan over the whole outline.
///
/// The dashed case is a bigger step again, and it is the reason it is a variant
/// rather than an `Option<DashSpec>` on [`PathPaint::Stroke`]. Phase 0 measured
/// a dashed straight line at **376 vertices and 11.8 µs against 6 vertices and
/// 0.8 µs for the same line solid** — 63× the vertices and 14× the CPU, because
/// lyon splits the path into one stroked subpath per dash, each with its own
/// caps. A style flag would make that a free-looking checkbox that quietly
/// costs a sixty-fourth of the frame's whole vertex budget per edge;
/// [`Outline::estimated_vertices`] charges it properly, so the black-window
/// guard sees it coming.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PathPaint {
    Fill(Color),
    Stroke {
        color: Color,
        width: f32,
    },
    DashedStroke {
        color: Color,
        width: f32,
        dash: DashSpec,
    },
}

impl PathPaint {
    pub fn color(&self) -> Color {
        match self {
            PathPaint::Fill(color) => *color,
            PathPaint::Stroke { color, .. } => *color,
            PathPaint::DashedStroke { color, .. } => *color,
        }
    }

    /// The stroke width, or `None` for a fill.
    pub fn width(&self) -> Option<f32> {
        match self {
            PathPaint::Fill(_) => None,
            PathPaint::Stroke { width, .. } | PathPaint::DashedStroke { width, .. } => Some(*width),
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
    /// and the LOD ladder degrades it per element.
    pub quality: RenderQuality,
    /// **Where the geometry cache should file this tessellation**, or `None`
    /// for a path not worth caching.
    ///
    /// The key is the plan's rather than the painter's because only the planner
    /// knows *which element* a path belongs to — by the time a painter sees an
    /// outline it is a bag of screen coordinates, and screen coordinates change
    /// on every pan, which is exactly the case the cache exists to serve. See
    /// [`crate::render::cache`].
    ///
    /// `None` is right for the overlays: a rubber band and a connection preview
    /// change every frame by definition, and §23 says not to cache what changes
    /// every frame.
    pub key: Option<GeometryKey>,
}

impl PathPrimitive {
    pub fn fill(outline: Outline, color: Color, quality: RenderQuality) -> PathPrimitive {
        PathPrimitive {
            outline,
            paint: PathPaint::Fill(color),
            quality,
            key: None,
        }
    }

    /// Files this path in the geometry cache under `key`. See the field.
    pub fn keyed(mut self, key: GeometryKey) -> PathPrimitive {
        self.key = Some(key);
        self
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
            key: None,
        }
    }

    /// A dashed stroke. **Expensive** — see [`PathPaint`] for the measurement.
    pub fn dashed_stroke(
        outline: Outline,
        color: Color,
        width: f32,
        dash: DashSpec,
        quality: RenderQuality,
    ) -> PathPrimitive {
        PathPrimitive {
            outline,
            paint: PathPaint::DashedStroke { color, width, dash },
            quality,
            key: None,
        }
    }

    /// The upper bound on the vertices this path will paint. See
    /// [`Outline::estimated_vertices`] for how good the bound is.
    pub fn estimated_vertices(&self) -> u32 {
        self.outline.estimated_vertices(self.paint, self.quality)
    }
}

/// A run of text on the canvas (§9).
///
/// Text is **last** in the paint order and that is the contract, not a
/// preference: a run of paths breaks the moment another primitive kind sorts
/// between them, and each contiguous run is a full-viewport render pass.
#[derive(Debug, Clone, PartialEq)]
pub struct TextPrimitive {
    /// Pane-relative screen pixels: the text's top-left.
    pub origin: Vec2,
    /// `Arc<str>` rather than `String`: this is built per visible label per
    /// frame, and a `String` here would be an allocation per label per frame —
    /// 1,584 of them on Phase 4's dense scene. See
    /// [`NodeCold::label`](crate::runtime::NodeCold::label).
    pub text: Arc<str>,
    /// **Already quantised onto the LOD ladder.** `font_size` is part of GPUI's
    /// shaped-line cache key, so an unquantised size re-shapes every visible
    /// label on every frame of a zoom (Phase 0 §1.9).
    pub font_size: f32,
    pub color: Color,
    /// Where the shaped-line cache files this run. See
    /// [`crate::render::cache::ShapedLineCache`].
    pub key: TextKey,
    /// The element's inner width in screen pixels — **the box, exactly**, and
    /// what [`align`](TextPrimitive::align) positions the run inside.
    ///
    /// Kept exact rather than quantised, because a right-aligned label snapped
    /// onto an eight-pixel grid would sit visibly short of its own border.
    pub max_width: f32,
    /// **The width the text wraps into**, in screen pixels — already snapped
    /// onto [`TextKey::quantize_wrap_width`](crate::render::cache::TextKey::quantize_wrap_width),
    /// which is the same number [`key`](TextPrimitive::key) records.
    ///
    /// Its own field rather than a reuse of [`max_width`](TextPrimitive::max_width)
    /// because the two answer different questions and only one of them is part
    /// of the shaped result: where a line breaks is baked into the cache entry,
    /// and where the block sits inside its box is arithmetic applied after.
    pub wrap_width: f32,
    /// Which face to shape with. Resolved against the theme by the painter,
    /// which is the only layer that knows what is installed — see
    /// [`FontFamily`](crate::models::FontFamily).
    pub family: FontFamily,
    /// Where the run sits inside [`max_width`](TextPrimitive::max_width).
    ///
    /// **Carried rather than baked into `origin`** because alignment needs the
    /// run's *measured* width, and only the painter has shaped it. Baking it
    /// here would mean shaping twice or guessing.
    pub align: TextAlign,
}

impl TextPrimitive {
    /// **§9's line-height model**, and it is one number on purpose.
    ///
    /// Phase 10 painted a single line at `font_size * 1.3` and said a real
    /// model belonged with multi-line text. This is that model: the same 1.3,
    /// promoted from a literal in the painter to the constant both the painter
    /// and the vertical centring read, so a block of four lines is exactly four
    /// times as tall as the one line it replaced.
    ///
    /// It is deliberately not a style field. A per-element line height is a
    /// document-format change and a fifth thing in every text cache key; the
    /// canvas has no control that would set one, and a constant that is wrong
    /// for nobody beats a field that is unreachable for everybody.
    pub const LINE_HEIGHT_RATIO: f32 = 1.3;

    /// One line's height in screen pixels.
    pub fn line_height(&self) -> f32 {
        self.font_size * TextPrimitive::LINE_HEIGHT_RATIO
    }

    /// **How far up a block of `lines` has to move to stay centred where one
    /// line was.**
    ///
    /// [`origin`](TextPrimitive::origin) is built before anything is shaped, so
    /// it is the top-left of a *single* line centred on the element — the only
    /// answer available to a scene builder that does not know how many lines
    /// the text will take. The painter knows, and applies this.
    ///
    /// Zero for one line, which is what makes wrapping a superset of Phase 10's
    /// placement rather than a change to it: an unwrapped label lands on the
    /// same pixel it always did.
    pub fn vertical_offset(&self, lines: u32) -> f32 {
        -(lines.saturating_sub(1) as f32) * self.line_height() * 0.5
    }
}

/// **§10's picture**, as the plan sees it: a rectangle, a handle and a crop.
///
/// It carries no pixels and no decoded image, which is what keeps this file
/// below the UI-framework line. Which bytes the handle names is the world's
/// answer; turning them into something a GPU can sample is the painter's, and
/// the two meet at [`PrimitiveSink::image`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ImagePrimitive {
    /// Pane-relative screen pixels: the element's whole frame. **The crop does
    /// not shrink it** — a crop chooses what is shown *inside* this rectangle,
    /// so the frame is where the picture is drawn either way.
    pub bounds: Rect,
    /// Which element this is. The painter's only way to find the picture it
    /// laid out for this frame, and the reason it is here rather than an
    /// `Arc<RenderImage>`: a decoded image is a GPUI type.
    pub node: NodeIndex,
    /// The handle and the crop, straight from the document.
    pub image: NodeImage,
    /// `0.0..=1.0`, from the element's style — the panel's Opacity row.
    pub opacity: f32,
    /// The frame's corner radius in **screen** pixels — the panel's Edges row.
    pub corner_radius: f32,
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

    /// **Contiguous runs of paths the scene saw**, which is what each
    /// full-viewport intermediate render pass costs ~0.1 ms for.
    ///
    /// Always 0 or 1, because [`PaintPlan::paint_into`] is the only emitter and
    /// emits every path in one run. It is reported rather than assumed because
    /// [`RenderBudgets::max_path_batches_per_frame`] is an exit criterion, and
    /// a criterion nobody measures is a criterion nobody keeps.
    pub path_batches: u32,

    /// Paths the clip rejected before the painter saw them.
    ///
    /// **Zero in a healthy frame**: the spatial broad phase should have
    /// rejected them already, and this is what notices when it did not.
    pub culled_paths: u32,

    /// Quads the clip rejected. A quad is cheap, so this is a diagnostic
    /// rather than a budget.
    pub culled_quads: u32,

    /// **Pictures the sink actually painted** (§10). Fewer than were planned
    /// means a picture the painter had nothing decoded for — a broken file, or
    /// a document whose resource table lost an entry — and the difference is
    /// what makes that visible as a number rather than as a hole on screen.
    pub images: u32,

    /// Images the clip rejected. Diagnostic, like [`culled_quads`](PaintStats::culled_quads).
    pub culled_images: u32,
}

impl PaintStats {
    /// Whether this frame risked the black window. See [`crate::budgets`].
    pub fn exceeds_safe_vertices(&self, budgets: &RenderBudgets) -> bool {
        budgets.exceeds_safe_vertices(self.path_vertices)
    }

    pub fn within_frame_target(&self, budgets: &RenderBudgets) -> bool {
        budgets.within_frame_target(self.path_vertices)
    }

    /// Whether this frame stayed inside
    /// [`RenderBudgets::max_path_batches_per_frame`] — Phase 0's second
    /// structural finding, and the one no CPU profiler shows.
    pub fn within_batch_budget(&self, budgets: &RenderBudgets) -> bool {
        self.path_batches <= budgets.max_path_batches_per_frame
    }

    /// **The culling property**: no offscreen path reached the painter, and
    /// none had to be rejected on the way. See [`PaintPlan::push_path`].
    pub fn culled_nothing(&self) -> bool {
        self.culled_paths == 0
    }
}

/// The four primitive kinds, in their **default** paint order.
///
/// `Ord` is the contract for three of them: a recording sink sorts its log by
/// this and must find it already sorted. [`Image`](PrimitiveKind::Image) is the
/// exception and says so here rather than in a comment somewhere — its run
/// moves to the far side of the paths when a depth order asks
/// ([`PaintPlan::set_images_after_paths`]), so what holds for it is the weaker
/// and more important property: **the sequence never returns to a kind it has
/// left**.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PrimitiveKind {
    Quad,
    /// §10's pictures, between the quads and the paths — see the module doc for
    /// why there, and what it costs.
    Image,
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

    /// Paints one picture, and answers whether it could — zero for a sink with
    /// nothing decoded for this element, which is the honest report of a broken
    /// or missing resource.
    fn image(&mut self, image: &ImagePrimitive) -> u32;

    /// Returns the number of glyphs actually painted.
    fn text(&mut self, text: &TextPrimitive) -> u32;
}

/// One frame's primitives, bucketed by kind.
///
/// Held on the view and `clear`ed each frame rather than reallocated —
/// requirements §40 rule 14, and pan is the hot path this exists to keep
/// allocation-free.
#[derive(Debug, Clone, PartialEq)]
pub struct PaintPlan {
    quads: Vec<QuadPrimitive>,
    images: Vec<ImagePrimitive>,
    paths: Vec<PathPrimitive>,
    texts: Vec<TextPrimitive>,
    /// The pane, in the same pane-relative screen pixels the primitives use.
    /// Nothing outside it is kept — see the module doc.
    clip: Rect,
    culled_paths: u32,
    culled_quads: u32,
    culled_images: u32,
    /// Whether §10's pictures are emitted after the paths rather than before
    /// them. See [`PaintPlan::set_images_after_paths`].
    images_after_paths: bool,
}

impl Default for PaintPlan {
    fn default() -> PaintPlan {
        PaintPlan {
            quads: Vec::new(),
            images: Vec::new(),
            paths: Vec::new(),
            texts: Vec::new(),
            clip: PaintPlan::UNBOUNDED,
            culled_paths: 0,
            culled_quads: 0,
            culled_images: 0,
            images_after_paths: false,
        }
    }
}

impl PaintPlan {
    /// The clip a plan starts with: everything. A plan is useless until
    /// [`clear`](PaintPlan::clear) gives it the frame's real pane, and this is
    /// what it does in the meantime — keeping everything is the answer that
    /// cannot silently lose geometry.
    pub const UNBOUNDED: Rect = Rect::new(
        Vec2::new(f32::MIN / 4.0, f32::MIN / 4.0),
        Vec2::new(f32::MAX / 2.0, f32::MAX / 2.0),
    );

    pub fn new() -> PaintPlan {
        PaintPlan::default()
    }

    /// **Starts a frame**: empties the buckets, keeping their capacity, and
    /// takes the pane every primitive will be clipped against.
    ///
    /// The clip is a parameter rather than a setter because a frame that
    /// forgot to set it would paint the whole document — see the module doc for
    /// what that costs and why it is a correctness question. `clip` is
    /// pane-relative screen pixels, so it is normally
    /// `Rect::new(Vec2::ZERO, pane_size)`.
    pub fn clear(&mut self, clip: Rect) {
        self.quads.clear();
        self.images.clear();
        self.paths.clear();
        self.texts.clear();
        self.clip = clip.normalized();
        self.culled_paths = 0;
        self.culled_quads = 0;
        self.culled_images = 0;
        self.images_after_paths = false;
    }

    /// The pane this frame is clipped against.
    pub fn clip(&self) -> Rect {
        self.clip
    }

    /// Adds a quad, unless it misses the clip.
    pub fn push_quad(&mut self, quad: QuadPrimitive) {
        // The border is drawn inside the bounds, so the bounds are the whole
        // painted extent — unlike a stroked path, which straddles its outline.
        if !quad.bounds.normalized().intersects(self.clip) {
            self.culled_quads += 1;
            return;
        }
        self.quads.push(quad);
    }

    /// Adds a path, **unless its painted extent misses the clip**.
    ///
    /// The extent is the outline's bounds grown by half the stroke width, which
    /// is exactly how far a stroke reaches outside the geometry it follows; a
    /// fill reaches no further than its outline. An outline with no bounds at
    /// all — an empty path — is dropped, because it would paint nothing.
    pub fn push_path(&mut self, path: PathPrimitive) {
        let extent = path
            .outline
            .bounds()
            .map(|bounds| bounds.inflate(path.paint.width().unwrap_or(0.0) * 0.5));

        match extent {
            Some(extent) if extent.intersects(self.clip) => self.paths.push(path),
            _ => self.culled_paths += 1,
        }
    }

    /// Adds a picture, unless its frame misses the clip. Culled like a quad
    /// and for the same reason: the frame is the whole painted extent, because
    /// the picture is drawn inside it and clipped to it.
    pub fn push_image(&mut self, image: ImagePrimitive) {
        if !image.bounds.normalized().intersects(self.clip) {
            self.culled_images += 1;
            return;
        }
        self.images.push(image);
    }

    /// **Moves §10's image run to the far side of the paths** — the whole of
    /// how a picture is brought in front of an ellipse.
    ///
    /// Set by the scene from the depths it actually planned, and reset by
    /// [`clear`](PaintPlan::clear) with everything else, so a frame cannot
    /// inherit the previous one's answer. See the module doc for the rule and
    /// for the case it cannot satisfy.
    pub fn set_images_after_paths(&mut self, after: bool) {
        self.images_after_paths = after;
    }

    pub fn images_after_paths(&self) -> bool {
        self.images_after_paths
    }

    pub fn push_text(&mut self, text: TextPrimitive) {
        self.texts.push(text);
    }

    /// Paths the clip rejected this frame. **Zero means the spatial broad phase
    /// did its job**; see the module doc.
    pub fn culled_paths(&self) -> u32 {
        self.culled_paths
    }

    pub fn culled_quads(&self) -> u32 {
        self.culled_quads
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

    pub fn image_count(&self) -> u32 {
        self.images.len() as u32
    }

    pub fn culled_images(&self) -> u32 {
        self.culled_images
    }

    pub fn is_empty(&self) -> bool {
        self.quads.is_empty()
            && self.images.is_empty()
            && self.paths.is_empty()
            && self.texts.is_empty()
    }

    /// The planned paths, **for tests only**.
    ///
    /// Deliberately not part of the crate's surface. [`PaintPlan::paint_into`]
    /// is the only way primitives leave a plan in a release build, and an
    /// accessor that survived compilation would be the first step back toward a
    /// painter choosing its own order — which is the whole thing this file
    /// exists to make inexpressible. A test asserting what an edge planned is a
    /// different matter, and `#[cfg(test)]` is the difference.
    #[cfg(test)]
    pub(crate) fn paths(&self) -> &[PathPrimitive] {
        &self.paths
    }

    /// The planned quads. Test-only, for the same reason as
    /// [`PaintPlan::paths`].
    #[cfg(test)]
    pub(crate) fn quads(&self) -> &[QuadPrimitive] {
        &self.quads
    }

    /// The planned text runs. Test-only, for the same reason as
    /// [`PaintPlan::paths`].
    #[cfg(test)]
    pub(crate) fn texts(&self) -> &[TextPrimitive] {
        &self.texts
    }

    /// **The planned pictures**, for the one caller that has to lay them out
    /// before they can be painted.
    ///
    /// The exception to this file's rule that primitives leave a plan only
    /// through [`paint_into`](PaintPlan::paint_into), and it is narrow on
    /// purpose: an [`ImagePrimitive`] is a rectangle and a handle, not
    /// something that can be painted, and GPUI will only lay an element out in
    /// the *prepaint* phase — a frame earlier than the sink runs. So
    /// `views::images` reads this during prepaint and the painter still emits
    /// them in the contract's order. Handing back a slice cannot reorder
    /// anything, which is the property that mattered.
    pub fn planned_images(&self) -> &[ImagePrimitive] {
        &self.images
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
    /// quads, then all images, then all paths, then all text — one contiguous
    /// run each, so the scene sees at most one path batch from the canvas
    /// layer.
    pub fn paint_into<S: PrimitiveSink + ?Sized>(&self, sink: &mut S) -> PaintStats {
        let mut stats = PaintStats {
            culled_paths: self.culled_paths,
            culled_quads: self.culled_quads,
            ..PaintStats::default()
        };

        for quad in &self.quads {
            sink.quad(quad);
            stats.quads += 1;
        }

        // §10's pictures, on whichever side of the path run this frame's
        // depths asked for — see `set_images_after_paths`. One run either way.
        if !self.images_after_paths {
            self.paint_images_into(sink, &mut stats);
        }

        for path in &self.paths {
            let vertices = sink.path(path);
            if vertices > 0 {
                stats.paths += 1;
                stats.path_vertices = stats.path_vertices.saturating_add(vertices);
            }
        }

        // One run, because this loop is the only emitter and it does not stop
        // to paint anything else. The `PrimitiveKind` ordering test is what
        // proves the claim; this is what reports it.
        stats.path_batches = u32::from(stats.paths > 0);

        if self.images_after_paths {
            self.paint_images_into(sink, &mut stats);
        }

        for text in &self.texts {
            stats.glyphs = stats.glyphs.saturating_add(sink.text(text));
        }

        stats
    }

    /// The image run, wherever [`paint_into`](PaintPlan::paint_into) puts it.
    ///
    /// A method rather than the loop written twice, because "one contiguous
    /// run" is the property the whole file exists to make structural and two
    /// copies of the loop is exactly how a frame ends up emitting half of it in
    /// each place.
    fn paint_images_into<S: PrimitiveSink + ?Sized>(&self, sink: &mut S, stats: &mut PaintStats) {
        for image in &self.images {
            stats.images = stats.images.saturating_add(sink.image(image));
        }
        stats.culled_images = self.culled_images;
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

        fn image(&mut self, _image: &ImagePrimitive) -> u32 {
            self.log.push(PrimitiveKind::Image);
            1
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

    /// A pane large enough for the fixtures above and small enough that
    /// "offscreen" is easy to write.
    fn a_pane() -> Rect {
        Rect::new(Vec2::ZERO, Vec2::new(400.0, 300.0))
    }

    fn a_text() -> TextPrimitive {
        TextPrimitive {
            origin: Vec2::ZERO,
            text: "abc".into(),
            key: TextKey::node(NodeIndex::new(0), 1, 12.0, 100.0),
            max_width: 100.0,
            wrap_width: 100.0,
            font_size: 12.0,
            color: Color::WHITE,
            family: FontFamily::default(),
            align: TextAlign::default(),
        }
    }

    /// **The line-height model** (Phase 10.5), and the property that makes
    /// wrapping a superset of Phase 10's single line rather than a change to
    /// it.
    ///
    /// A scene builder centres one line on its element, because that is all it
    /// can know before anything is shaped. The painter learns how many lines
    /// there really were and lifts the block by half of every line past the
    /// first — so one line lands on the pixel it always did, and a block of any
    /// height stays centred on the same point.
    #[test]
    fn a_wrapped_block_stays_centred_where_one_line_was() {
        let text = a_text();
        let line = text.line_height();

        assert_eq!(line, 12.0 * TextPrimitive::LINE_HEIGHT_RATIO);
        assert_eq!(
            text.vertical_offset(1),
            0.0,
            "an unwrapped label moved, so every Phase 10 placement shifted"
        );
        assert_eq!(
            text.vertical_offset(0),
            0.0,
            "no lines is not a negative lift"
        );

        // Two lines: the block is one line taller, so it rises half a line.
        assert_eq!(text.vertical_offset(2), -line * 0.5);
        assert_eq!(text.vertical_offset(5), -line * 2.0);

        // The centre of the block is the centre of the single line it replaced,
        // whatever the count — the statement the arithmetic above is for.
        for lines in 1..8_u32 {
            let top = text.vertical_offset(lines);
            let centre = top + line * lines as f32 * 0.5;
            assert!(
                (centre - line * 0.5).abs() < 1e-4,
                "{lines} lines centred at {centre} instead of {}",
                line * 0.5
            );
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

    fn an_image() -> ImagePrimitive {
        ImagePrimitive {
            bounds: rect(0.0, 0.0),
            node: NodeIndex::new(0),
            image: crate::models::NodeImage::new(crate::models::ImageHandle::of(b"x")),
            opacity: 1.0,
            corner_radius: 0.0,
        }
    }

    /// **§10's run, on either side of the paths, and one run either way.**
    ///
    /// The position is a depth question and the *contiguity* is the contract —
    /// see the module doc. This asserts both halves, because a frame that moved
    /// the run by emitting half of it in each place would satisfy the depth and
    /// cost the batch the whole file exists to save.
    #[test]
    fn the_image_run_moves_as_a_whole_or_not_at_all() {
        for after in [false, true] {
            let mut plan = PaintPlan::new();
            plan.clear(a_pane());
            plan.set_images_after_paths(after);
            plan.push_quad(a_quad());
            plan.push_image(an_image());
            plan.push_path(a_path());
            plan.push_image(an_image());
            plan.push_text(a_text());

            let mut sink = RecordingSink {
                vertices_per_path: 7,
                ..RecordingSink::default()
            };
            let stats = plan.paint_into(&mut sink);

            assert_eq!(stats.images, 2, "a picture was dropped at after={after}");
            let expected = if after {
                vec![
                    PrimitiveKind::Quad,
                    PrimitiveKind::Path,
                    PrimitiveKind::Image,
                    PrimitiveKind::Image,
                    PrimitiveKind::Text,
                ]
            } else {
                vec![
                    PrimitiveKind::Quad,
                    PrimitiveKind::Image,
                    PrimitiveKind::Image,
                    PrimitiveKind::Path,
                    PrimitiveKind::Text,
                ]
            };
            assert_eq!(sink.log, expected, "after={after}");
        }
    }

    /// A picture that misses the pane is rejected before the painter, exactly
    /// as a quad is and for the same reason.
    #[test]
    fn an_offscreen_picture_is_culled() {
        let mut plan = PaintPlan::new();
        plan.clear(a_pane());
        plan.push_image(ImagePrimitive {
            bounds: Rect::new(Vec2::new(5_000.0, 5_000.0), Vec2::new(10.0, 10.0)),
            ..an_image()
        });

        assert_eq!(plan.image_count(), 0);
        assert_eq!(plan.culled_images(), 1);
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

        plan.clear(a_pane());

        assert!(plan.is_empty());
        assert_eq!(plan.quads.capacity(), quad_capacity);
        assert_eq!(plan.paths.capacity(), path_capacity);
        assert_eq!(plan.clip(), a_pane());
    }

    /// **The culling property, as a contract of the plan itself.** A painter
    /// cannot hand an offscreen path to the sink, whatever it believes.
    #[test]
    fn a_path_outside_the_clip_never_reaches_the_sink() {
        let mut plan = PaintPlan::new();
        plan.clear(a_pane());

        plan.push_path(a_path());
        plan.push_path(PathPrimitive::fill(
            shapes::diamond(rect(-9_000.0, -9_000.0)),
            Color::WHITE,
            RenderQuality::BALANCED,
        ));
        plan.push_quad(QuadPrimitive::filled(rect(50_000.0, 0.0), Color::WHITE));

        let mut sink = RecordingSink {
            vertices_per_path: 3,
            ..RecordingSink::default()
        };
        let stats = plan.paint_into(&mut sink);

        assert_eq!(sink.log, vec![PrimitiveKind::Path]);
        assert_eq!(stats.culled_paths, 1);
        assert_eq!(stats.culled_quads, 1);
        assert!(!stats.culled_nothing());
    }

    /// A stroke straddles its outline, so a path just outside the pane can
    /// still paint into it. Culling on the outline alone would clip a border.
    #[test]
    fn a_stroke_that_reaches_into_the_pane_survives() {
        let mut plan = PaintPlan::new();
        plan.clear(a_pane());

        // Outline sits 6 units left of the pane; a 20-unit stroke reaches 10
        // units past its own outline, so it paints into the pane.
        let outline = shapes::rectangle(Rect::new(Vec2::new(-26.0, 10.0), Vec2::new(20.0, 20.0)));
        plan.push_path(PathPrimitive::stroke(
            outline.clone(),
            Color::WHITE,
            20.0,
            RenderQuality::BALANCED,
        ));
        // The same outline filled reaches no further than itself.
        plan.push_path(PathPrimitive::fill(
            outline,
            Color::WHITE,
            RenderQuality::BALANCED,
        ));

        assert_eq!(plan.path_count(), 1);
        assert_eq!(plan.culled_paths(), 1);
    }

    /// A fresh plan keeps everything, because a plan that silently dropped
    /// geometry before its first `clear` would be a trap.
    #[test]
    fn an_unbounded_plan_culls_nothing() {
        let mut plan = PaintPlan::new();
        plan.push_path(PathPrimitive::fill(
            shapes::diamond(rect(-1e6, 1e6)),
            Color::WHITE,
            RenderQuality::BALANCED,
        ));

        assert_eq!(plan.path_count(), 1);
        assert_eq!(plan.culled_paths(), 0);
    }

    /// Phase 0's second structural finding, reported rather than assumed: the
    /// canvas layer is one contiguous run of paths, so it is one batch.
    #[test]
    fn a_frame_is_one_path_batch_and_an_empty_one_is_none() {
        let budgets = for_backend(RenderBackend::Metal);
        let mut plan = PaintPlan::new();
        plan.clear(a_pane());

        let mut sink = RecordingSink {
            vertices_per_path: 5,
            ..RecordingSink::default()
        };
        assert_eq!(plan.paint_into(&mut sink).path_batches, 0);

        for _ in 0..300 {
            plan.push_quad(a_quad());
            plan.push_path(a_path());
        }
        let stats = plan.paint_into(&mut sink);

        assert_eq!(stats.paths, 300);
        assert_eq!(stats.path_batches, 1);
        assert!(stats.within_batch_budget(&budgets));
        assert!(stats.culled_nothing());
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
