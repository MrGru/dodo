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

use gpui::{
    App, Background, BorderStyle, Bounds, Corners, Edges, FillOptions, Font, Hsla, Path,
    PathBuilder, PathStyle, Pixels, Point, Rgba, ShapedLine, StrokeOptions, TextAlign, TextRun,
    Window, point, px,
};

use crate::{
    geometry::{Rect, Vec2},
    models::Color,
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
    text: &'a mut ShapedLineCache<ShapedLine>,
    font: Font,
}

impl<'a> WindowPainter<'a> {
    /// A painter with no caches — every path tessellated fresh, every label
    /// shaped fresh.
    ///
    /// Kept because it is what a test or a one-off render wants, and because it
    /// is the honest baseline the cached path is measured against. The canvas
    /// itself uses [`WindowPainter::cached`].
    pub fn new(
        window: &'a mut Window,
        cx: &'a mut App,
        bounds: Bounds<Pixels>,
        geometry: &'a mut GeometryCache<Path<Pixels>>,
        text: &'a mut ShapedLineCache<ShapedLine>,
    ) -> WindowPainter<'a> {
        let font = window.text_style().font();
        WindowPainter {
            window,
            origin: Vec2::new(bounds.origin.x.as_f32(), bounds.origin.y.as_f32()),
            cx,
            geometry,
            text,
            font,
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

    /// **Paints one label, through the engine's own shaped-line cache** (§9).
    ///
    /// The size arriving here is already quantised onto the LOD ladder, so a
    /// zoom gesture asks for a handful of distinct sizes rather than one per
    /// frame — `font_size` is part of GPUI's own layout cache key, and that
    /// cache is only two frames deep. Shaping is ~7–11 µs against ~1.7 µs to
    /// paint a cached line, which at a thousand labels is the difference
    /// between 11.1 ms and 3.7 ms a frame.
    ///
    /// Text is **last** in the paint order, and that is settled by
    /// [`PaintPlan::paint_into`](crate::render::plan::PaintPlan::paint_into)
    /// rather than here.
    fn text(&mut self, text: &TextPrimitive) -> u32 {
        if text.color.is_invisible() || text.font_size <= 0.0 {
            return 0;
        }

        let line = match self.text.get(&text.key) {
            Some(line) => line.clone(),
            None => {
                // One run: the canvas draws a node's label in one style. Rich
                // text inside a node is its element's business, where GPUI's
                // own layout does it properly.
                let run = TextRun {
                    len: text.text.len(),
                    font: self.font.clone(),
                    color: to_hsla(text.color),
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                };
                // A newline would make `shape_line` panic in a debug build, and
                // a node label is a single line by definition — so it is
                // flattened here rather than trusted.
                let flattened: String = text
                    .text
                    .chars()
                    .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
                    .collect();
                let shaped = self.window.text_system().shape_line(
                    flattened.into(),
                    px(text.font_size),
                    &[run],
                    Some(px(text.max_width.max(1.0))),
                );
                self.text.insert(text.key, shaped.clone());
                shaped
            }
        };

        let glyphs = line.len() as u32;
        let origin = to_point(text.origin + self.origin);
        // The line height is the font size plus a little leading, which is what
        // a single-line label wants; a full line-height model belongs with §9's
        // multi-line text, which is a later phase's.
        let _ = line.paint(
            origin,
            px(text.font_size * 1.3),
            TextAlign::Left,
            Some(px(text.max_width.max(1.0))),
            self.window,
            self.cx,
        );
        glyphs
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
