//! [`Outline`] — the four canvas shapes as flattenable geometry, and the rule
//! for when a shape should not be a path at all.
//!
//! # Which shapes are paths, and which are quads
//!
//! Phase 0 measured a rectangle three ways and the answer was not close:
//! 20,000 quads hold 60 fps where 20,000 filled rectangular paths drop to 30,
//! and a quad carries corner radii, a border width and a border colour for
//! free. So an **axis-aligned rectangle or rounded rectangle is a quad**, and
//! [`prefers_quad`] is the single place that decides it. Ellipses and diamonds
//! have no quad form and are paths.
//!
//! That leaves an obvious question: why does this module build rectangle and
//! rounded-rectangle outlines at all, if they are painted as quads? Because
//! "axis-aligned" is a condition, not a property of the kind — a rotated
//! rectangle, a dashed border, a sketch-rendered body (§13) and a shape used as
//! a clip all need the real outline, and every one of those arrives in a later
//! phase. [`prefers_quad`] is the routing decision; the outlines are the
//! fallback it routes away from.
//!
//! # Why an ellipse is not two arcs
//!
//! Phase 0 measured an ellipse built from two `PathBuilder::arc_to` calls at
//! **337 vertices** — as expensive as a full-window Bézier, and about fourteen
//! times a diamond. [`ellipse`] uses the standard four-cubic construction with
//! the circular magic constant instead, which lyon flattens against the
//! *tolerance* rather than against whatever step `arc_to` picked. The shape is
//! within a quarter of a pixel of a true ellipse at any size a canvas shows and
//! it costs what the tolerance says it costs, which is the property the budget
//! needs.
//!
//! # Screen space, not world space
//!
//! Outlines are built in **pane-relative screen pixels**, already transformed
//! by the viewport. Tessellating in screen space is what makes the flattening
//! tolerance mean what it says — pixels of deviation on the display — and it is
//! why [`crate::models::RenderQuality`] is expressed in pixels.
//!
//! **This file names no UI framework.**

use crate::{
    geometry::{Rect, Vec2},
    models::{RenderQuality, ShapeKind},
    render::plan::PathPaint,
};

/// The circular magic constant, `4/3 * (sqrt(2) - 1)`: the control-point offset
/// that makes a cubic Bézier approximate a quarter circle to within about
/// 0.02 % of the radius.
const KAPPA: f32 = 0.552_284_8;

/// One step of an outline. Deliberately smaller than SVG's vocabulary — every
/// shape the canvas draws reduces to lines and cubics, and quadratics and arcs
/// would be two more cases in the flattening estimate for no new shapes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SubpathCommand {
    MoveTo(Vec2),
    LineTo(Vec2),
    /// A cubic Bézier from the current point, with two control points.
    CubicTo {
        c1: Vec2,
        c2: Vec2,
        to: Vec2,
    },
    Close,
}

/// A shape's boundary, in pane-relative screen pixels.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Outline {
    commands: Vec<SubpathCommand>,
}

impl Outline {
    pub fn new() -> Outline {
        Outline::default()
    }

    pub fn with_capacity(commands: usize) -> Outline {
        Outline {
            commands: Vec::with_capacity(commands),
        }
    }

    pub fn commands(&self) -> &[SubpathCommand] {
        &self.commands
    }

    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    pub fn move_to(&mut self, to: Vec2) -> &mut Outline {
        self.commands.push(SubpathCommand::MoveTo(to));
        self
    }

    pub fn line_to(&mut self, to: Vec2) -> &mut Outline {
        self.commands.push(SubpathCommand::LineTo(to));
        self
    }

    pub fn cubic_to(&mut self, c1: Vec2, c2: Vec2, to: Vec2) -> &mut Outline {
        self.commands.push(SubpathCommand::CubicTo { c1, c2, to });
        self
    }

    pub fn close(&mut self) -> &mut Outline {
        self.commands.push(SubpathCommand::Close);
        self
    }

    /// The outline's bounding box, control points included.
    ///
    /// A cubic never leaves its control hull, so this is a true bound rather
    /// than a tight one — which is what a cull test wants, since a false
    /// "visible" costs one wasted path and a false "hidden" is a missing shape.
    pub fn bounds(&self) -> Option<Rect> {
        let mut points = Vec::with_capacity(self.commands.len() * 3);
        for command in &self.commands {
            match *command {
                SubpathCommand::MoveTo(p) | SubpathCommand::LineTo(p) => points.push(p),
                SubpathCommand::CubicTo { c1, c2, to } => {
                    points.push(c1);
                    points.push(c2);
                    points.push(to);
                }
                SubpathCommand::Close => {}
            }
        }
        Rect::of_points(points)
    }

    /// How many points this outline flattens to at `tolerance`.
    ///
    /// Lines contribute their endpoint; a cubic contributes the segments lyon
    /// will split it into. The cubic term is the approximation — see
    /// [`cubic_segments`].
    pub fn flattened_points(&self, tolerance: f32) -> u32 {
        let tolerance = tolerance.max(RenderQuality::MIN_TOLERANCE);
        let mut current = Vec2::ZERO;
        let mut points = 0u32;

        for command in &self.commands {
            match *command {
                SubpathCommand::MoveTo(p) => {
                    points += 1;
                    current = p;
                }
                SubpathCommand::LineTo(p) => {
                    points += 1;
                    current = p;
                }
                SubpathCommand::CubicTo { c1, c2, to } => {
                    points += cubic_segments(current, c1, c2, to, tolerance);
                    current = to;
                }
                SubpathCommand::Close => {}
            }
        }

        points
    }

    /// **The pre-tessellation vertex bound the budget is spent against.**
    ///
    /// `Path<Pixels>` stores three vertices per triangle —
    /// `PathBuilder::build_path` pushes `v0, v1, v2` for every index triple, so
    /// `path.vertices.len()` is exactly `indices.len()`. That fixes the model:
    ///
    /// - a **fill** of an outline flattening to `n` points is a triangulation of
    ///   an `n`-gon, at most `n - 2` triangles, so `3(n - 2)` vertices;
    /// - a **stroke** emits a quad per segment plus join and cap geometry —
    ///   two triangles per segment before joins, so roughly `6n`.
    ///
    /// Both are multiplied by [`SAFETY_MARGIN`], because this number is spent
    /// on a ceiling whose failure mode is a black window: over-estimating costs
    /// a shape that could have been drawn, under-estimating costs the frame.
    /// `render::painter`'s tests check the bound against real tessellations of
    /// every shape here.
    pub fn estimated_vertices(&self, paint: PathPaint, quality: RenderQuality) -> u32 {
        let points = self.flattened_points(quality.flattening_tolerance);
        if points < 2 {
            return 0;
        }

        let triangles = match paint {
            PathPaint::Fill(_) => points.saturating_sub(2),
            PathPaint::Stroke { .. } => points.saturating_mul(2),
        };

        ((triangles.saturating_mul(3)) as f32 * SAFETY_MARGIN).ceil() as u32
    }
}

/// How much the estimate is inflated over the geometric model.
///
/// Lyon's fill tessellator adds vertices at self-intersections and its stroke
/// tessellator adds them at joins and caps, and neither count is predictable
/// from the outline alone. 1.6 is what `render::painter`'s calibration test
/// found sufficient for every shape in this module across the tolerance range,
/// with room left over — and the test fails if a future shape breaks it.
pub const SAFETY_MARGIN: f32 = 1.6;

/// The most segments one cubic is allowed to contribute.
///
/// A guard, not a tuning knob: without it a shape scaled to a huge zoom would
/// let a single curve's estimate run away, and the estimate is what the black-
/// window guard spends. Any real curve at any real tolerance is far below it.
pub const MAX_CUBIC_SEGMENTS: u32 = 256;

/// Segments a cubic flattens into at `tolerance`.
///
/// Flattening error for a cubic falls as the square of the segment count, so
/// the count grows as the square root of (curve size / tolerance). `hull` — the
/// control polygon's length — is the standard stand-in for curve size: it is
/// cheap, it never underestimates the arc length, and it degrades correctly for
/// a degenerate curve.
///
/// The 0.6 coefficient is fitted, not derived, and `render::painter`'s
/// calibration test is what keeps it honest.
pub fn cubic_segments(from: Vec2, c1: Vec2, c2: Vec2, to: Vec2, tolerance: f32) -> u32 {
    let hull = (c1 - from).length() + (c2 - c1).length() + (to - c2).length();
    if !hull.is_finite() || hull <= 0.0 {
        return 1;
    }

    let segments = ((hull / tolerance.max(RenderQuality::MIN_TOLERANCE)).sqrt() * 0.6).ceil();
    (segments as u32).clamp(1, MAX_CUBIC_SEGMENTS)
}

/// **The routing decision: is this shape cheaper as a quad than as a path?**
///
/// The one place that answers it, so a new call site cannot quietly decide a
/// rectangle is a path. `rotation` is in radians and is what disqualifies an
/// otherwise-quad shape — GPUI's quad is axis-aligned and there is no rotated
/// form of it.
pub fn prefers_quad(kind: ShapeKind, rotation: f32) -> bool {
    if rotation.abs() > 1e-4 {
        return false;
    }

    matches!(kind, ShapeKind::Rectangle | ShapeKind::RoundedRectangle)
}

/// An axis-aligned rectangle, counter-clockwise from the top-left.
pub fn rectangle(rect: Rect) -> Outline {
    let rect = rect.normalized();
    let min = rect.min();
    let max = rect.max();

    let mut outline = Outline::with_capacity(5);
    outline
        .move_to(min)
        .line_to(Vec2::new(max.x, min.y))
        .line_to(max)
        .line_to(Vec2::new(min.x, max.y))
        .close();
    outline
}

/// A rectangle with uniform rounded corners.
///
/// The radius is clamped to half the shorter side, so a radius larger than the
/// rectangle produces a stadium rather than an inverted corner — the same rule
/// CSS and GPUI's own quad use, and the reason the shape stays valid when a
/// user drags a node smaller than its own corner radius.
pub fn rounded_rectangle(rect: Rect, radius: f32) -> Outline {
    let rect = rect.normalized();
    let limit = rect.width().min(rect.height()) * 0.5;
    let r = radius.clamp(0.0, limit.max(0.0));

    if r <= 0.0 {
        return rectangle(rect);
    }

    let min = rect.min();
    let max = rect.max();
    let k = r * KAPPA;

    let mut outline = Outline::with_capacity(9);
    outline
        .move_to(Vec2::new(min.x + r, min.y))
        .line_to(Vec2::new(max.x - r, min.y))
        .cubic_to(
            Vec2::new(max.x - r + k, min.y),
            Vec2::new(max.x, min.y + r - k),
            Vec2::new(max.x, min.y + r),
        )
        .line_to(Vec2::new(max.x, max.y - r))
        .cubic_to(
            Vec2::new(max.x, max.y - r + k),
            Vec2::new(max.x - r + k, max.y),
            Vec2::new(max.x - r, max.y),
        )
        .line_to(Vec2::new(min.x + r, max.y))
        .cubic_to(
            Vec2::new(min.x + r - k, max.y),
            Vec2::new(min.x, max.y - r + k),
            Vec2::new(min.x, max.y - r),
        )
        .line_to(Vec2::new(min.x, min.y + r))
        .cubic_to(
            Vec2::new(min.x, min.y + r - k),
            Vec2::new(min.x + r - k, min.y),
            Vec2::new(min.x + r, min.y),
        )
        .close();
    outline
}

/// An ellipse inscribed in `rect`, as four cubics. See the module doc for why
/// this is not two `arc_to` calls.
pub fn ellipse(rect: Rect) -> Outline {
    let rect = rect.normalized();
    let center = rect.center();
    let rx = rect.width() * 0.5;
    let ry = rect.height() * 0.5;
    let kx = rx * KAPPA;
    let ky = ry * KAPPA;

    let mut outline = Outline::with_capacity(6);
    outline
        .move_to(Vec2::new(center.x, center.y - ry))
        .cubic_to(
            Vec2::new(center.x + kx, center.y - ry),
            Vec2::new(center.x + rx, center.y - ky),
            Vec2::new(center.x + rx, center.y),
        )
        .cubic_to(
            Vec2::new(center.x + rx, center.y + ky),
            Vec2::new(center.x + kx, center.y + ry),
            Vec2::new(center.x, center.y + ry),
        )
        .cubic_to(
            Vec2::new(center.x - kx, center.y + ry),
            Vec2::new(center.x - rx, center.y + ky),
            Vec2::new(center.x - rx, center.y),
        )
        .cubic_to(
            Vec2::new(center.x - rx, center.y - ky),
            Vec2::new(center.x - kx, center.y - ry),
            Vec2::new(center.x, center.y - ry),
        )
        .close();
    outline
}

/// A diamond inscribed in `rect`: the flowchart decision shape (§6).
pub fn diamond(rect: Rect) -> Outline {
    let rect = rect.normalized();
    let center = rect.center();
    let min = rect.min();
    let max = rect.max();

    let mut outline = Outline::with_capacity(5);
    outline
        .move_to(Vec2::new(center.x, min.y))
        .line_to(Vec2::new(max.x, center.y))
        .line_to(Vec2::new(center.x, max.y))
        .line_to(Vec2::new(min.x, center.y))
        .close();
    outline
}

/// A triangle inscribed in `rect`, apex up.
///
/// Present because [`ShapeKind`] has the variant and a `match` that silently
/// fell through to a rectangle would be a wrong drawing rather than a missing
/// one. It costs four lines and it is the same polygon machinery.
pub fn triangle(rect: Rect) -> Outline {
    let rect = rect.normalized();
    let min = rect.min();
    let max = rect.max();

    let mut outline = Outline::with_capacity(4);
    outline
        .move_to(Vec2::new(rect.center().x, min.y))
        .line_to(max)
        .line_to(Vec2::new(min.x, max.y))
        .close();
    outline
}

/// The outline for a shape kind. `corner_radius` is used only by
/// [`ShapeKind::RoundedRectangle`].
///
/// A custom kind falls back to a rectangle: the registry that knows how to draw
/// it is Phase 5's, and until then a visible box is a better answer than an
/// invisible element the user cannot find or delete.
pub fn outline_for(kind: &ShapeKind, rect: Rect, corner_radius: f32) -> Outline {
    match kind {
        ShapeKind::Rectangle => rectangle(rect),
        ShapeKind::RoundedRectangle => rounded_rectangle(rect, corner_radius),
        ShapeKind::Ellipse => ellipse(rect),
        ShapeKind::Diamond => diamond(rect),
        ShapeKind::Triangle => triangle(rect),
        ShapeKind::Custom(_) => rectangle(rect),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Color, CustomKind};

    const EPSILON: f32 = 1e-3;

    fn rect() -> Rect {
        Rect::new(Vec2::new(10.0, 20.0), Vec2::new(100.0, 60.0))
    }

    fn fill() -> PathPaint {
        PathPaint::Fill(Color::WHITE)
    }

    fn stroke() -> PathPaint {
        PathPaint::Stroke {
            color: Color::WHITE,
            width: 2.0,
        }
    }

    fn assert_near(a: Vec2, b: Vec2) {
        assert!((a - b).length() < EPSILON, "expected {b:?}, got {a:?}");
    }

    #[test]
    fn every_shape_stays_inside_its_rectangle() {
        for kind in [
            ShapeKind::Rectangle,
            ShapeKind::RoundedRectangle,
            ShapeKind::Ellipse,
            ShapeKind::Diamond,
            ShapeKind::Triangle,
        ] {
            let bounds = outline_for(&kind, rect(), 12.0)
                .bounds()
                .expect("a shape has bounds");

            assert!(
                rect().inflate(EPSILON).contains_rect(bounds),
                "{kind:?} escaped its rectangle: {bounds:?}"
            );
        }
    }

    /// Control points included: an ellipse's cubic hull must not bulge out of
    /// the rectangle, or a cull test built on [`Outline::bounds`] would clip a
    /// visible shape.
    #[test]
    fn the_ellipse_touches_all_four_edges_and_leaves_none() {
        let bounds = ellipse(rect()).bounds().expect("an ellipse has bounds");

        assert!((bounds.min().x - rect().min().x).abs() < EPSILON);
        assert!((bounds.min().y - rect().min().y).abs() < EPSILON);
        assert!((bounds.max().x - rect().max().x).abs() < EPSILON);
        assert!((bounds.max().y - rect().max().y).abs() < EPSILON);
    }

    #[test]
    fn a_diamond_is_its_rectangles_four_edge_midpoints() {
        let outline = diamond(rect());
        let center = rect().center();

        assert_eq!(outline.commands().len(), 5);
        match outline.commands()[0] {
            SubpathCommand::MoveTo(p) => assert_near(p, Vec2::new(center.x, rect().min().y)),
            other => panic!("expected a move, got {other:?}"),
        }
        assert_eq!(outline.commands()[4], SubpathCommand::Close);
    }

    #[test]
    fn a_zero_radius_rounded_rectangle_is_a_rectangle() {
        assert_eq!(rounded_rectangle(rect(), 0.0), rectangle(rect()));
    }

    /// A node dragged smaller than its own corner radius must not invert its
    /// corners — the radius clamps to a stadium instead.
    #[test]
    fn an_over_large_radius_clamps_to_half_the_shorter_side() {
        let huge = rounded_rectangle(rect(), 1_000.0);
        let stadium = rounded_rectangle(rect(), rect().height() * 0.5);

        assert_eq!(huge, stadium);
        assert!(
            rect()
                .inflate(EPSILON)
                .contains_rect(huge.bounds().unwrap()),
            "a clamped radius still stays inside the rectangle"
        );
    }

    #[test]
    fn a_degenerate_rectangle_produces_an_outline_rather_than_a_panic() {
        let flat = Rect::new(Vec2::new(5.0, 5.0), Vec2::ZERO);
        for kind in [
            ShapeKind::Rectangle,
            ShapeKind::RoundedRectangle,
            ShapeKind::Ellipse,
            ShapeKind::Diamond,
            ShapeKind::Triangle,
        ] {
            let outline = outline_for(&kind, flat, 4.0);
            assert!(!outline.is_empty());
            assert!(outline.flattened_points(0.25) > 0);
        }
    }

    /// The whole point of §33's routing rule, pinned: a plain rectangle is
    /// never a path, and a curve never becomes a quad.
    #[test]
    fn only_axis_aligned_rectangles_prefer_a_quad() {
        assert!(prefers_quad(ShapeKind::Rectangle, 0.0));
        assert!(prefers_quad(ShapeKind::RoundedRectangle, 0.0));
        assert!(!prefers_quad(ShapeKind::Ellipse, 0.0));
        assert!(!prefers_quad(ShapeKind::Diamond, 0.0));
        assert!(!prefers_quad(ShapeKind::Triangle, 0.0));
        assert!(!prefers_quad(
            ShapeKind::Custom(CustomKind::new("thing")),
            0.0
        ));
    }

    #[test]
    fn a_rotated_rectangle_needs_a_path_because_a_quad_cannot_rotate() {
        assert!(!prefers_quad(ShapeKind::Rectangle, 0.4));
        assert!(!prefers_quad(ShapeKind::RoundedRectangle, -0.4));
        assert!(
            prefers_quad(ShapeKind::Rectangle, 1e-6),
            "floating-point noise is not a rotation"
        );
    }

    /// The measured relationship the LOD ladder depends on: a shape made of
    /// lines costs the same at any tolerance, a shape made of curves does not.
    #[test]
    fn tolerance_moves_the_curve_shapes_and_leaves_the_polygons_alone() {
        let precise = RenderQuality::PRECISE.flattening_tolerance;
        let draft = RenderQuality::DRAFT.flattening_tolerance;

        assert_eq!(
            diamond(rect()).flattened_points(precise),
            diamond(rect()).flattened_points(draft),
        );
        assert!(
            ellipse(rect()).flattened_points(precise) > ellipse(rect()).flattened_points(draft),
            "a looser tolerance has to buy fewer vertices, or the knob is a lie"
        );
    }

    #[test]
    fn a_stroke_costs_more_than_a_fill_of_the_same_outline() {
        let outline = ellipse(rect());
        let quality = RenderQuality::BALANCED;

        assert!(
            outline.estimated_vertices(stroke(), quality)
                > outline.estimated_vertices(fill(), quality)
        );
    }

    /// Phase 0's ordering, reproduced by the estimator: a polygon is cheap and
    /// an ellipse is not. The ratio is what makes LOD degrade ellipses before
    /// rectangles.
    #[test]
    fn an_ellipse_costs_several_times_a_diamond() {
        let quality = RenderQuality::BALANCED;
        let diamond = diamond(rect()).estimated_vertices(fill(), quality);
        let ellipse = ellipse(rect()).estimated_vertices(fill(), quality);

        assert!(
            ellipse > diamond * 3,
            "ellipse {ellipse} vs diamond {diamond}"
        );
    }

    /// The runaway guard. Without it a shape at extreme zoom lets one curve's
    /// estimate dominate the frame's whole ceiling.
    #[test]
    fn one_cubic_can_never_estimate_past_the_clamp() {
        let vast = Rect::new(Vec2::ZERO, Vec2::splat(1.0e7));
        let segments = cubic_segments(
            Vec2::ZERO,
            Vec2::new(vast.width(), 0.0),
            Vec2::new(vast.width(), vast.height()),
            Vec2::new(0.0, vast.height()),
            RenderQuality::MIN_TOLERANCE,
        );

        assert_eq!(segments, MAX_CUBIC_SEGMENTS);
    }

    #[test]
    fn a_degenerate_cubic_still_costs_one_segment() {
        assert_eq!(
            cubic_segments(Vec2::ZERO, Vec2::ZERO, Vec2::ZERO, Vec2::ZERO, 0.25),
            1
        );
    }

    #[test]
    fn an_empty_outline_estimates_nothing() {
        assert_eq!(
            Outline::new().estimated_vertices(fill(), RenderQuality::BALANCED),
            0
        );
        assert_eq!(Outline::new().bounds(), None);
    }

    #[test]
    fn a_custom_kind_falls_back_to_something_visible() {
        let outline = outline_for(&ShapeKind::Custom(CustomKind::new("mystery")), rect(), 0.0);
        assert_eq!(outline, rectangle(rect()));
    }
}
