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
    Background, BorderStyle, Bounds, Corners, Edges, FillOptions, Hsla, Path, PathBuilder,
    PathStyle, Pixels, Point, Rgba, StrokeOptions, Window, point, px,
};

use crate::{
    geometry::{Rect, Vec2},
    models::Color,
    render::{
        plan::{PathPaint, PathPrimitive, PrimitiveSink, QuadPrimitive, TextPrimitive},
        shapes::{Outline, SubpathCommand},
    },
};

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
}

impl<'a> WindowPainter<'a> {
    pub fn new(window: &'a mut Window, bounds: Bounds<Pixels>) -> WindowPainter<'a> {
        WindowPainter {
            window,
            origin: Vec2::new(bounds.origin.x.as_f32(), bounds.origin.y.as_f32()),
        }
    }

    fn place(&self, rect: Rect) -> Bounds<Pixels> {
        to_bounds(rect.translated(self.origin))
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

    fn path(&mut self, path: &PathPrimitive) -> u32 {
        if path.paint.color().is_invisible() {
            return 0;
        }

        // Translated into window space *before* tessellation, so the built
        // path is the one the geometry cache will later hold and no second
        // transform is applied on the way to the GPU.
        let mut outline = Outline::with_capacity(path.outline.commands().len());
        for command in path.outline.commands() {
            match *command {
                SubpathCommand::MoveTo(p) => outline.move_to(p + self.origin),
                SubpathCommand::LineTo(p) => outline.line_to(p + self.origin),
                SubpathCommand::CubicTo { c1, c2, to } => {
                    outline.cubic_to(c1 + self.origin, c2 + self.origin, to + self.origin)
                }
                SubpathCommand::Close => outline.close(),
            };
        }

        let Some(built) = build_path(&outline, path.paint, path.quality.flattening_tolerance)
        else {
            return 0;
        };

        let vertices = built.vertices.len() as u32;
        self.window
            .paint_path(built, to_background(path.paint.color()));
        vertices
    }

    /// **Not implemented, and that is the honest state of it.**
    ///
    /// Nothing pushes a [`TextPrimitive`] in this phase — canvas text is Phase
    /// 5's, along with the `ShapedLine` cache it needs, because GPUI keys its
    /// shaped-line cache on `font_size` and continuous zoom would otherwise
    /// re-shape every label every frame. What matters now is that text is
    /// *last* in the paint order, and that is settled by
    /// [`PaintPlan::paint_into`](crate::render::plan::PaintPlan::paint_into)
    /// rather than here.
    ///
    /// Returning zero means "painted no glyphs", which is exactly true, and it
    /// keeps [`PaintStats`](crate::render::plan::PaintStats) honest.
    fn text(&mut self, _text: &TextPrimitive) -> u32 {
        0
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
