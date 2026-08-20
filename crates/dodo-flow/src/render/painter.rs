//! [`WindowPainter`] — the GPUI end of the paint-order contract.
//!
//! **This is the only file in the crate that paints**, and it is deliberately
//! dull. It implements [`PrimitiveSink`] and nothing else: three methods that
//! translate one pure primitive into one GPUI call. It cannot choose an order,
//! because [`PaintPlan::paint_into`] is what calls it and that is where the
//! order lives.
//!
//! Two things it *does* decide, both because only the GPUI side can:
//!
//! - **The pane→window offset.** Everything in a [`PaintPlan`] is in
//!   pane-relative screen pixels, matching [`crate::geometry::Viewport`]'s
//!   convention, so the canvas does not have to know where the sidebar put it.
//!   This painter adds the element's origin as the last step before painting.
//! - **The true vertex count.** `PathBuilder::build_path` pushes three vertices
//!   per triangle, so `path.vertices.len()` is exactly the number that is
//!   uploaded — 104 bytes each, against a 256 MiB instance buffer. That is the
//!   number [`crate::budgets`] is written about and the number the sink
//!   returns; the estimate in `render::shapes` is only ever a bound on it, and
//!   the calibration test at the bottom of this file is what keeps the bound
//!   true.
//!
//! # §9's text wraps, and where it wraps is decided elsewhere
//!
//! Phase 10 drew one line however much was typed; Phase 10.5 wraps. The
//! shaping call is [`shape_wrapped`], which carries the correction Phase 10's
//! own note needs — the argument it was passing as a wrap width was
//! `force_width`, a per-glyph advance, and it was scattering long labels rather
//! than truncating them.
//!
//! **Two of the three things wrapping needs are not in this file, on purpose.**
//! The width a run wraps into is [`crate::render::scene`]'s, because only the
//! scene knows a node's box; the height of a line and the lift that keeps a
//! block centred are [`TextPrimitive`]'s, because both are arithmetic and
//! arithmetic below the UI-framework line is arithmetic a test can assert. What
//! is left here is the call, the cache and the loop — which is what "this file
//! is deliberately dull" is supposed to mean.
//!
//! ## The limitation a user meets: text can outgrow its box downwards
//!
//! A wrapped paragraph is as tall as it needs to be, and nothing clamps it to
//! the element's height — so enough text in a short node spills above and below
//! it. Excalidraw grows the container instead, which is a resize on every
//! keystroke and therefore an edit through
//! [`FlowEditor`](crate::commands::FlowEditor) on every keystroke; that is a
//! decision about the document, not about painting, and it belongs with
//! whatever gives an element an auto-height.
//!
//! `shape_text` has a `line_clamp` that would cut the paragraph off at the
//! box's height instead. It is deliberately not used: **overflowing text is
//! visible and a user can fix it by dragging a corner; clamped text is words
//! that are simply gone**, with nothing on screen to say so.

use std::sync::Arc;

use gpui::{
    App, Background, BorderStyle, Bounds, Corners, Edges, FillOptions, Font, Hsla, Path,
    PathBuilder, PathStyle, Pixels, Point, Rgba, StrokeOptions, TextAlign, TextRun, Window,
    WrappedLine, point, px,
};

use crate::{
    geometry::{Rect, Vec2},
    models::{Color, FontFamily},
    render::{
        cache::{CachedGeometry, GeometryCache, ScreenAnchor, ShapedLineCache},
        plan::{PathPaint, PathPrimitive, PrimitiveSink, QuadPrimitive, TextPrimitive},
        shapes::{Outline, SubpathCommand},
    },
};

/// **The half of §23's geometry cache that has to know what a path is.**
///
/// Phase 0 §1.6 is what makes this three lines rather than a research project:
/// `Path<Pixels>` has a **public** `vertices: Vec<PathVertex<P>>` and a public
/// `bounds`, and `PathVertex` has a public `xy_position`. The private `start` /
/// `current` / `contour_count` fields are only used by `Path`'s own builder
/// methods and never by rendering, so a built path can be moved in place and
/// painted, exactly.
///
/// `PathBuilder::transform` / `translate` / `scale` do **not** help: they apply
/// to the *source* path before `build()` tessellates, so they always
/// re-tessellate. Mutating the vertex buffer is the only way to reuse a
/// tessellation, and it works — a path built at A and translated by d has the
/// same vertex count as one rebuilt at A+d, with a maximum deviation of
/// 0.000122 px.
impl CachedGeometry for Path<Pixels> {
    fn vertex_count(&self) -> u32 {
        self.vertices.len() as u32
    }

    fn transform(&mut self, scale: f32, offset: Vec2) {
        for vertex in self.vertices.iter_mut() {
            vertex.xy_position = point(
                px(vertex.xy_position.x.as_f32() * scale + offset.x),
                px(vertex.xy_position.y.as_f32() * scale + offset.y),
            );
        }

        // The bounds are what the renderer clips against, so they move with the
        // vertices or a translated path is rejected at the edge of the pane.
        self.bounds = Bounds {
            origin: point(
                px(self.bounds.origin.x.as_f32() * scale + offset.x),
                px(self.bounds.origin.y.as_f32() * scale + offset.y),
            ),
            size: gpui::size(
                px(self.bounds.size.width.as_f32() * scale),
                px(self.bounds.size.height.as_f32() * scale),
            ),
        };
    }
}

/// A pure [`Color`] as GPUI's.
///
/// Free-standing rather than a `From` impl because `Color` is defined in
/// `models/`, which may not name a UI framework — so the conversion has to live
/// on this side of the boundary. That is the boundary working, not chafing.
pub fn to_hsla(color: Color) -> Hsla {
    Rgba {
        r: color.r,
        g: color.g,
        b: color.b,
        a: color.a,
    }
    .into()
}

fn to_background(color: Color) -> Background {
    to_hsla(color).into()
}

fn to_point(v: Vec2) -> Point<Pixels> {
    point(px(v.x), px(v.y))
}

fn to_bounds(rect: Rect) -> Bounds<Pixels> {
    let rect = rect.normalized();
    Bounds {
        origin: to_point(rect.origin),
        size: gpui::size(px(rect.size.x), px(rect.size.y)),
    }
}

/// GPUI's colour as the crate's pure one — the inverse of [`to_hsla`], and it
/// lives here for the same reason: `models/` may not name a UI framework, so a
/// theme colour can only be converted on this side of the boundary.
///
/// Two callers, which is why it is here rather than in either of them: the
/// canvas resolves its ink from the theme every frame
/// ([`FlowView`](crate::views::FlowView)), and the tool palette resolves its
/// glyph colour the same way.
pub fn from_hsla(color: Hsla) -> Color {
    let rgba: Rgba = color.into();
    Color::rgba(rgba.r, rgba.g, rgba.b, rgba.a)
}

/// Turns an [`Outline`] into a built GPUI [`Path`], at the given tolerance.
///
/// Returns `None` for an outline lyon refuses or one that tessellates to
/// nothing — a zero-area shape, a stroke of width zero. Both are ordinary
/// states (a shape being dragged out from a single click starts at zero size),
/// so neither is an error.
///
/// Public because Phase 4's geometry cache needs exactly this and must not
/// grow a second copy of it: the cache stores what this returns, translates its
/// vertex buffer for pan, and rebuilds only when the outline or the tolerance
/// changes.
pub fn build_path(outline: &Outline, paint: PathPaint, tolerance: f32) -> Option<Path<Pixels>> {
    if outline.is_empty() {
        return None;
    }

    let style = match paint {
        PathPaint::Fill(_) => PathStyle::Fill(FillOptions::default().with_tolerance(tolerance)),
        PathPaint::Stroke { width, .. } | PathPaint::DashedStroke { width, .. } => {
            if width <= 0.0 {
                return None;
            }
            PathStyle::Stroke(
                StrokeOptions::default()
                    .with_line_width(width)
                    .with_tolerance(tolerance),
            )
        }
    };

    let mut builder = PathBuilder::fill().with_style(style);
    if let PathPaint::DashedStroke { dash, .. } = paint {
        // `dash_array` duplicates an odd-length array to make it even, so the
        // two-element form is passed through exactly as written.
        builder = builder.dash_array(&[px(dash.on.max(0.0)), px(dash.off.max(0.0))]);
    }
    for command in outline.commands() {
        match *command {
            SubpathCommand::MoveTo(p) => builder.move_to(to_point(p)),
            SubpathCommand::LineTo(p) => builder.line_to(to_point(p)),
            SubpathCommand::CubicTo { c1, c2, to } => {
                builder.cubic_bezier_to(to_point(to), to_point(c1), to_point(c2))
            }
            SubpathCommand::Close => builder.close(),
        }
    }

    match builder.build() {
        Ok(path) if !path.vertices.is_empty() => Some(path),
        // A tessellation failure is a shape GPUI would have dropped anyway.
        // Painting nothing is the same outcome without the panic, and dodo
        // installs no logger for a warning to reach.
        _ => None,
    }
}

/// **One label, shaped and wrapped** — what §23's text cache holds after
/// Phase 10.5.
///
/// `shape_text` returns one [`WrappedLine`] per *hard* newline, each of which
/// may itself have wrapped onto several visual lines. Both counts matter and
/// neither is derivable from the other, so both are measured once here, at
/// shaping time, rather than walked again on every frame that paints the label.
///
/// Held behind an [`Arc`] in the cache. A hit has to be cloned out before the
/// window can be borrowed to paint it — Phase 0 §3 correction 13, the same
/// constraint the geometry cache lives with — and a refcount bump is a strictly
/// cheaper clone than the `ShapedLine` copy this replaced.
#[derive(Debug)]
pub struct WrappedText {
    /// One entry per hard line; each carries its own wrap boundaries.
    pub lines: Vec<WrappedLine>,
    /// **Visual** lines: hard lines plus every wrap boundary inside them. What
    /// the vertical centring is computed from.
    pub visual_lines: u32,
    /// The widest line, in screen pixels, for
    /// [`TextAlign::offset`](crate::models::TextAlign::offset).
    pub width: f32,
    /// The count the sink reports, in the same units Phase 5 reported it: the
    /// laid-out byte length.
    pub glyphs: u32,
}

/// §23's text cache, as the canvas holds it.
///
/// An alias rather than the type written out, so the one file above the
/// UI-framework line that has to name it — [`FlowView`](crate::views::FlowView),
/// which owns the cache for the frame — names it in one place.
pub type TextCache = ShapedLineCache<Arc<WrappedText>>;

/// Paints a [`PaintPlan`](crate::render::plan::PaintPlan) into a window.
///
/// Borrows the window for the length of one frame, so it is constructed inside
/// the canvas paint closure and dropped there.
pub struct WindowPainter<'a> {
    window: &'a mut Window,
    /// The canvas element's top-left in window coordinates.
    origin: Vec2,
    cx: &'a mut App,
    /// §23's tessellation cache. **Keyed in window space** — the anchor below
    /// folds the element's origin in — so a pane that moves is just another
    /// translation and nothing has to be rebuilt for it.
    geometry: &'a mut GeometryCache<Path<Pixels>>,
    text: &'a mut TextCache,
    fonts: FontSet,
}

/// **The three faces §9's [`FontFamily`] resolves to**, chosen once per frame
/// rather than per label.
///
/// Resolved by the caller rather than here for the reason every colour in
/// `models/` is an `Option<Color>`: the theme is what knows which UI and
/// monospace faces this build is drawing with, and only `views/` may name a
/// theme. This painter is handed the answer.
///
/// **`hand_drawn` is honestly a preference, not a promise.** dodo ships no
/// hand-drawn face — see
/// [`FontFamily::preferred_faces`](crate::models::FontFamily::preferred_faces)
/// for why — so `views::flow` probes the text system for the platform's
/// candidates and falls back to `normal`. On a machine with none of them
/// installed, choosing Hand-drawn changes nothing on screen. That is a
/// limitation a user meets, and it is recorded here because this is where it is
/// caused.
#[derive(Debug, Clone)]
pub struct FontSet {
    pub normal: Font,
    pub hand_drawn: Font,
    pub code: Font,
}

impl FontSet {
    /// All three the same face — the honest default before a theme has been
    /// consulted, and what [`WindowPainter::new`] uses.
    pub fn uniform(font: Font) -> FontSet {
        FontSet {
            hand_drawn: font.clone(),
            code: font.clone(),
            normal: font,
        }
    }

    fn face(&self, family: FontFamily) -> &Font {
        match family {
            FontFamily::Normal => &self.normal,
            FontFamily::HandDrawn => &self.hand_drawn,
            FontFamily::Code => &self.code,
        }
    }
}

impl<'a> WindowPainter<'a> {
    /// A painter with no caches — every path tessellated fresh, every label
    /// shaped fresh.
    ///
    /// Kept because it is what a test or a one-off render wants, and because it
    /// is the honest baseline the cached path is measured against. The canvas
    /// itself uses [`WindowPainter::with_fonts`], which also carries §9's three
    /// faces.
    pub fn new(
        window: &'a mut Window,
        cx: &'a mut App,
        bounds: Bounds<Pixels>,
        geometry: &'a mut GeometryCache<Path<Pixels>>,
        text: &'a mut TextCache,
    ) -> WindowPainter<'a> {
        let fonts = FontSet::uniform(window.text_style().font());
        WindowPainter::with_fonts(window, cx, bounds, geometry, text, fonts)
    }

    /// The same painter with the theme's three faces (§9). What the canvas
    /// uses; [`WindowPainter::new`] is the one-face form.
    pub fn with_fonts(
        window: &'a mut Window,
        cx: &'a mut App,
        bounds: Bounds<Pixels>,
        geometry: &'a mut GeometryCache<Path<Pixels>>,
        text: &'a mut TextCache,
        fonts: FontSet,
    ) -> WindowPainter<'a> {
        WindowPainter {
            window,
            origin: Vec2::new(bounds.origin.x.as_f32(), bounds.origin.y.as_f32()),
            cx,
            geometry,
            text,
            fonts,
        }
    }

    /// The anchor a frame's caches should be started at: the camera, with the
    /// canvas element's origin folded in.
    ///
    /// Folding the origin in is what lets the cache hold **window-space**
    /// tessellations. The alternative — caching pane-relative and translating
    /// at paint time — would touch every vertex of every path on every frame,
    /// which is the cost the cache exists to remove.
    pub fn anchor(viewport: &crate::geometry::Viewport, bounds: Bounds<Pixels>) -> ScreenAnchor {
        let mut anchor = ScreenAnchor::of(viewport);
        anchor.origin += Vec2::new(bounds.origin.x.as_f32(), bounds.origin.y.as_f32());
        anchor
    }

    fn place(&self, rect: Rect) -> Bounds<Pixels> {
        to_bounds(rect.translated(self.origin))
    }

    /// The outline in window space, which is where the cache holds it.
    fn window_outline(&self, outline: &Outline) -> Outline {
        let mut moved = Outline::with_capacity(outline.commands().len());
        for command in outline.commands() {
            match *command {
                SubpathCommand::MoveTo(p) => moved.move_to(p + self.origin),
                SubpathCommand::LineTo(p) => moved.line_to(p + self.origin),
                SubpathCommand::CubicTo { c1, c2, to } => {
                    moved.cubic_to(c1 + self.origin, c2 + self.origin, to + self.origin)
                }
                SubpathCommand::Close => moved.close(),
            };
        }
        moved
    }
}

impl PrimitiveSink for WindowPainter<'_> {
    fn quad(&mut self, quad: &QuadPrimitive) {
        let bounds = self.place(quad.bounds);
        if bounds.size.width <= px(0.0) || bounds.size.height <= px(0.0) {
            return;
        }

        self.window.paint_quad(gpui::PaintQuad {
            bounds,
            corner_radii: Corners::all(px(quad.corner_radius.max(0.0))),
            background: to_background(quad.background),
            border_widths: Edges::all(px(quad.border_width.max(0.0))),
            border_color: to_hsla(quad.border_color),
            border_style: BorderStyle::Solid,
        });
    }

    /// **Paints one path, through §23's cache.**
    ///
    /// The cache is asked first; a hit comes back already repositioned for this
    /// frame's camera. `paint_path` consumes its argument, so the cached path is
    /// cloned into it — Phase 0 §3 correction 13: that clone is the floor and
    /// there is no borrow-based alternative without patching gpui. It is still
    /// far cheaper than re-tessellating: 1.29 µs to clone against ~5.2 µs to
    /// rebuild a 300-vertex path.
    ///
    /// A path with no key — an overlay — skips the cache entirely, because §23
    /// says not to cache what changes every frame.
    fn path(&mut self, path: &PathPrimitive) -> u32 {
        if path.paint.color().is_invisible() {
            return 0;
        }

        let background = to_background(path.paint.color());

        if let Some(cached) = path.key.and_then(|key| self.geometry.get(&key)) {
            let built = cached.clone();
            let vertices = built.vertices.len() as u32;
            self.window.paint_path(built, background);
            return vertices;
        }

        // Translated into window space *before* tessellation, so the built path
        // is the one the cache holds and no second transform is applied on the
        // way to the GPU.
        let outline = self.window_outline(&path.outline);
        let Some(built) = build_path(&outline, path.paint, path.quality.flattening_tolerance)
        else {
            return 0;
        };

        let vertices = built.vertices.len() as u32;
        if let Some(key) = path.key {
            self.geometry.insert(key, built.clone());
        }
        self.window.paint_path(built, background);
        vertices
    }

    /// **Paints one label, through the engine's own text cache** (§9), wrapped
    /// to its host's width (§9, Phase 10.5).
    ///
    /// The size arriving here is already quantised onto the LOD ladder and the
    /// wrap width onto [`TextKey::quantize_wrap_width`](crate::render::cache::TextKey::quantize_wrap_width),
    /// so a zoom gesture asks for a handful of distinct layouts rather than one
    /// per frame — both are part of the cache key, and GPUI's own layout cache
    /// is only two frames deep. Shaping is ~7–11 µs against ~1.7 µs to paint a
    /// cached line, which at a thousand labels is the difference between 11.1 ms
    /// and 3.7 ms a frame.
    ///
    /// Text is **last** in the paint order, and that is settled by
    /// [`PaintPlan::paint_into`](crate::render::plan::PaintPlan::paint_into)
    /// rather than here.
    fn text(&mut self, text: &TextPrimitive) -> u32 {
        if text.color.is_invisible() || text.font_size <= 0.0 {
            return 0;
        }

        let wrapped = match self.text.get(&text.key) {
            // A refcount bump. The borrow has to end before the window can be
            // borrowed to paint, which is why this is cloned rather than held.
            Some(cached) => Arc::clone(cached),
            None => {
                // One run: the canvas draws a node's label in one style. Rich
                // text inside a node is its element's business, where GPUI's
                // own layout does it properly.
                let run = TextRun {
                    len: text.text.len(),
                    font: self.fonts.face(text.family).clone(),
                    color: to_hsla(text.color),
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                };
                let shaped = Arc::new(shape_wrapped(
                    self.window,
                    text.text.as_ref(),
                    text.font_size,
                    text.wrap_width,
                    run,
                ));
                self.text.insert(text.key, Arc::clone(&shaped));
                shaped
            }
        };

        // **Alignment is applied here, from the shaped width**, and it could
        // not have been applied earlier: only the text system knows how wide a
        // run turned out, and `TextPrimitive::origin` is built before anything
        // is shaped. GPUI's own `TextAlign` is deliberately not used — it
        // aligns within the *wrap width* it is handed, which is a different
        // number from `max_width` and would therefore be a second, disagreeing
        // answer. One arithmetic offset, asserted with no window by
        // `TextAlign::offset`.
        let indent = text.align.offset(text.max_width, wrapped.width.max(0.0));
        // **And the vertical half of the same idea.** `origin` is the top-left
        // of a single line centred on the element, because that is all a scene
        // builder can know; the block has to rise by half of every line past
        // the first, which is `TextPrimitive::vertical_offset` and is zero for
        // one line.
        let lift = text.vertical_offset(wrapped.visual_lines);
        let mut origin = to_point(text.origin + Vec2::new(indent, lift) + self.origin);
        let line_height = px(text.line_height());

        for line in wrapped.lines.iter() {
            // `align` is `Left` and `bounds` `None` because both alignments are
            // already in `origin` above; handing GPUI a second answer here is
            // exactly how the two would drift.
            let _ = line.paint(
                origin,
                line_height,
                TextAlign::Left,
                None,
                self.window,
                self.cx,
            );
            // Each hard line advances by its own wrapped height, so a paragraph
            // after a long one starts below it rather than on top of it.
            let lines = line.wrap_boundaries.len() as f32 + 1.0;
            origin.y += line_height * lines;
        }

        wrapped.glyphs
    }
}

/// **The shaping call, and the change that made §9's text more than one line.**
///
/// `shape_text` rather than `shape_line`, and the difference is not only that
/// one wraps. Phase 10 called
/// `shape_line(text, size, &runs, Some(px(max_width)))` and recorded that a
/// label longer than its box was "truncated by the wrap width". **That fourth
/// argument is not a wrap width and never was**: it is `force_width`, the
/// per-glyph advance a terminal grid uses, and passing a box width there scatters
/// the glyphs of any label longer than half the box onto a lattice of
/// `max_width` steps. Nothing truncated, nothing wrapped, and a long label came
/// apart. `shape_text`'s fourth argument *is* the wrap width, which is why the
/// three changes that doc named — `shape_text`, a `WrappedLine` in the cache,
/// and a line-height model — were the right three even though the reason given
/// for the first was wrong.
///
/// Newlines are kept rather than flattened to spaces: `shape_text` splits on
/// them itself and returns one [`WrappedLine`] each, so a typed paragraph
/// survives as a paragraph. `\r` is still flattened — the splitter only knows
/// `\n`, and a stray carriage return would otherwise shape as a glyph.
fn shape_wrapped(
    window: &Window,
    text: &str,
    font_size: f32,
    wrap_width: f32,
    run: TextRun,
) -> WrappedText {
    let content: String = text
        .chars()
        .map(|c| if c == '\r' { ' ' } else { c })
        .collect();
    let mut run = run;
    run.len = content.len();

    let lines = window
        .text_system()
        .shape_text(
            content.into(),
            px(font_size),
            &[run],
            Some(px(wrap_width.max(1.0))),
            None,
        )
        .unwrap_or_default();

    let mut visual_lines = 0_u32;
    let mut width = 0.0_f32;
    let mut glyphs = 0_u32;
    for line in lines.iter() {
        visual_lines += line.wrap_boundaries.len() as u32 + 1;
        width = width.max(line.width().as_f32());
        glyphs += line.len() as u32;
    }

    WrappedText {
        lines: lines.into_iter().collect(),
        // A `shape_text` that returned nothing still occupies one line's worth
        // of space, and reporting zero would make `vertical_offset` lift an
        // empty block by half a line.
        visual_lines: visual_lines.max(1),
        width,
        glyphs,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{models::RenderQuality, render::shapes};

    /// Every shape this phase draws, filled and stroked, across the tolerance
    /// range. Tessellation needs lyon but not a window, so this runs as an
    /// ordinary unit test.
    fn each_shape() -> Vec<(&'static str, Outline)> {
        let rect = Rect::new(Vec2::new(0.0, 0.0), Vec2::new(160.0, 90.0));
        vec![
            ("rectangle", shapes::rectangle(rect)),
            ("rounded rectangle", shapes::rounded_rectangle(rect, 16.0)),
            ("ellipse", shapes::ellipse(rect)),
            ("diamond", shapes::diamond(rect)),
            ("triangle", shapes::triangle(rect)),
        ]
    }

    /// **The calibration that makes vertex accounting mean something.**
    ///
    /// `render::shapes`'s estimate is spent against a ceiling whose failure
    /// mode is a black window, so it must be an upper bound on what
    /// tessellation actually produces — never an under-count. This builds every
    /// shape for real and checks the bound holds, which is the only way that
    /// claim stays true as shapes are added.
    #[test]
    fn the_estimate_is_an_upper_bound_on_every_real_tessellation() {
        for quality in [
            RenderQuality::PRECISE,
            RenderQuality::BALANCED,
            RenderQuality::DRAFT,
        ] {
            for (name, outline) in each_shape() {
                for paint in [
                    PathPaint::Fill(Color::WHITE),
                    PathPaint::Stroke {
                        color: Color::WHITE,
                        width: 2.0,
                    },
                ] {
                    let estimated = outline.estimated_vertices(paint, quality);
                    let actual = build_path(&outline, paint, quality.flattening_tolerance)
                        .map(|path| path.vertices.len() as u32)
                        .unwrap_or(0);

                    assert!(
                        estimated >= actual,
                        "{name} {paint:?} at tolerance {} under-estimated: \
                         {estimated} < {actual} actual vertices",
                        quality.flattening_tolerance
                    );
                }
            }
        }
    }

    /// The bound has to be *useful*, not merely safe — an estimate ten times
    /// the truth would throw away shapes the frame could easily have afforded.
    #[test]
    fn the_estimate_is_not_wastefully_loose() {
        for (name, outline) in each_shape() {
            let paint = PathPaint::Fill(Color::WHITE);
            let quality = RenderQuality::BALANCED;
            let estimated = outline.estimated_vertices(paint, quality);
            let actual = build_path(&outline, paint, quality.flattening_tolerance)
                .map(|path| path.vertices.len() as u32)
                .unwrap_or(0);

            assert!(
                actual > 0 && estimated <= actual * 8,
                "{name}: estimate {estimated} against {actual} actual"
            );
        }
    }

    /// Phase 0's cost ordering, re-measured against real tessellations rather
    /// than taken on trust: an ellipse is several times a diamond, which is why
    /// LOD must degrade ellipses to quads before it touches polygons.
    #[test]
    fn an_ellipse_really_does_cost_several_times_a_diamond() {
        let tolerance = RenderQuality::BALANCED.flattening_tolerance;
        let rect = Rect::new(Vec2::ZERO, Vec2::new(160.0, 90.0));
        let paint = PathPaint::Fill(Color::WHITE);

        let ellipse = build_path(&shapes::ellipse(rect), paint, tolerance)
            .unwrap()
            .vertices
            .len();
        let diamond = build_path(&shapes::diamond(rect), paint, tolerance)
            .unwrap()
            .vertices
            .len();

        assert!(
            ellipse > diamond * 2,
            "ellipse {ellipse} vertices vs diamond {diamond}"
        );
    }

    /// The tolerance knob has to actually buy vertices, or
    /// [`RenderQuality`] is decoration and the 2× budget multiplier Phase 0
    /// measured does not exist.
    #[test]
    fn a_looser_tolerance_really_produces_fewer_vertices() {
        let rect = Rect::new(Vec2::ZERO, Vec2::new(400.0, 400.0));
        let paint = PathPaint::Fill(Color::WHITE);
        let outline = shapes::ellipse(rect);

        let precise = build_path(&outline, paint, RenderQuality::PRECISE.flattening_tolerance)
            .unwrap()
            .vertices
            .len();
        let draft = build_path(&outline, paint, RenderQuality::DRAFT.flattening_tolerance)
            .unwrap()
            .vertices
            .len();

        assert!(precise > draft, "precise {precise} vs draft {draft}");
    }

    /// `build_path` returns `None` rather than panicking on the shapes a user
    /// produces mid-gesture.
    #[test]
    fn degenerate_input_declines_instead_of_failing() {
        let zero = Rect::new(Vec2::splat(10.0), Vec2::ZERO);

        assert!(build_path(&Outline::new(), PathPaint::Fill(Color::WHITE), 0.25).is_none());
        assert!(
            build_path(
                &shapes::rectangle(zero),
                PathPaint::Stroke {
                    color: Color::WHITE,
                    width: 0.0
                },
                0.25
            )
            .is_none(),
            "a zero-width stroke has no geometry"
        );
        assert!(
            build_path(
                &shapes::rectangle(zero),
                PathPaint::Fill(Color::WHITE),
                0.25
            )
            .is_none(),
            "a zero-area fill tessellates to nothing"
        );
    }

    /// The claim the whole accounting model rests on: `Path::vertices` holds
    /// three entries per triangle, so its length is what gets uploaded.
    #[test]
    fn a_built_paths_vertex_count_is_a_multiple_of_three() {
        for (name, outline) in each_shape() {
            let path = build_path(&outline, PathPaint::Fill(Color::WHITE), 0.25)
                .unwrap_or_else(|| panic!("{name} built nothing"));
            assert_eq!(
                path.vertices.len() % 3,
                0,
                "{name} produced {} vertices",
                path.vertices.len()
            );
        }
    }

    #[test]
    fn colour_survives_the_round_trip_into_gpuis_space() {
        let hsla = to_hsla(Color::rgba(0.2, 0.4, 0.6, 0.8));
        let back: Rgba = hsla.into();

        assert!((back.r - 0.2).abs() < 1e-3);
        assert!((back.g - 0.4).abs() < 1e-3);
        assert!((back.b - 0.6).abs() < 1e-3);
        assert!((back.a - 0.8).abs() < 1e-3);
    }
}
