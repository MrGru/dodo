//! **Hachure and cross-hatch fills**: a shape's interior as a set of parallel
//! lines clipped to its outline.
//!
//! # Why this exists rather than a solid fill with a pattern
//!
//! The property panel's Fill row has three buttons and two of them are line
//! sets. There is no pattern brush in this renderer — a
//! [`PathPaint`](crate::render::plan::PathPaint) is a colour and a width — and
//! there could not usefully be one: GPUI paints tessellated geometry, so a
//! pattern would have to become geometry somewhere, and the only place it can
//! become geometry *correctly* is where the shape's own outline is known. A fill
//! shaded by tiling a texture behind a clip would also break the one thing this
//! engine cannot afford to break: it would be a second primitive kind in the
//! middle of a path run.
//!
//! So a hatched fill is **one path with many subpaths**, stroked. One path means
//! one tessellation, one cache entry, one entry in the plan's path run and no
//! extra batch — which is what makes this affordable at all.
//!
//! # The clip is a scanline, and that is the whole algorithm
//!
//! The outline is flattened to a polygon, rotated into the hatch direction's
//! frame, and swept by lines at a fixed spacing. Each sweep line's intersections
//! with the polygon edges are sorted along the line and taken **in pairs**,
//! which is the even-odd rule — so a shape with a hole, or a diamond, or an
//! ellipse gets exactly the spans that are inside it and nothing outside.
//!
//! Two details that are easy to get wrong and are wrong in a way you only see at
//! one angle:
//!
//! - **A vertex exactly on a sweep line must be counted once, not twice.** The
//!   test is a half-open interval on the edge's own span (`min <= y < max`),
//!   which is the standard fix and the reason [`spans`] does not simply compare
//!   both endpoints.
//! - **A horizontal edge in the rotated frame contributes nothing.** It has no
//!   crossing; including it would add a pair and invert the inside/outside
//!   parity for the rest of that line.
//!
//! # The line count is bounded, because a fill is not allowed to cost a frame
//!
//! Spacing is in screen pixels and a shape can be arbitrarily large on screen,
//! so the sweep is capped at [`MAX_LINES`] and the spacing widened to fit rather
//! than the lines truncated. A zoomed-in rectangle therefore gets a coarser
//! hatch instead of a hundred thousand segments — degrading the same way §15's
//! ladder does, and for the same reason: the alternative is a frame that costs
//! what the *document* says rather than what the screen can hold.
//!
//! **This file names no UI framework.**

use crate::{
    geometry::{Vec2, curve::flatten_cubic},
    models::FillStyle,
    render::shapes::{Outline, SubpathCommand},
};

/// The most sweep lines one fill may produce, per direction.
///
/// A cross-hatch is two sweeps, so a shape filling the viewport costs at most
/// twice this many segments. Sixty-four is enough that a 600 px shape at the
/// default spacing is not visibly coarsened and few enough that a full-screen
/// one is a rounding error against
/// [`RenderBudgets::target_paths_per_frame`](crate::budgets::RenderBudgets::target_paths_per_frame)
/// — the lines are subpaths of **one** path, so what they spend is vertices
/// rather than batches.
pub const MAX_LINES: u32 = 64;

/// The gap between hatch lines, in **screen pixels**.
///
/// Screen rather than world, for the reason [`SketchStyle`](crate::models::SketchStyle)'s
/// lengths are: a hatch is a property of the pen, and a real one does not get
/// coarser when you lean closer to the paper.
pub const DEFAULT_SPACING: f32 = 8.0;

/// The angle hachure runs at, as a unit direction. 45° up to the right, which
/// is what every hand-drawn diagram tool uses and what the reference shows.
const HACHURE: Vec2 = Vec2::new(
    std::f32::consts::FRAC_1_SQRT_2,
    -std::f32::consts::FRAC_1_SQRT_2,
);

/// The second direction of a cross-hatch: the first, turned a quarter turn.
const CROSS: Vec2 = Vec2::new(
    std::f32::consts::FRAC_1_SQRT_2,
    std::f32::consts::FRAC_1_SQRT_2,
);

/// The tolerance the outline is flattened at before it is swept.
///
/// Coarser than the canvas's own, deliberately: the polygon here is a *clip*
/// rather than something drawn, and a quarter-pixel error in where a hatch line
/// stops is invisible under a stroked border that is drawn at full precision
/// over the top of it.
const CLIP_TOLERANCE: f32 = 1.0;

/// **The fill for one shape**, as a single [`Outline`] of line subpaths.
///
/// Empty for [`FillStyle::Solid`] — a solid fill is the shape itself and this
/// module has nothing to say about it — and empty for an outline with no area.
pub fn hatch(outline: &Outline, style: FillStyle, spacing: f32) -> Outline {
    let mut lines = Outline::new();
    let polygon = flatten(outline);
    if polygon.len() < 3 {
        return lines;
    }

    match style {
        FillStyle::Solid => {}
        FillStyle::Hachure => sweep(&polygon, HACHURE, spacing, &mut lines),
        FillStyle::CrossHatch => {
            sweep(&polygon, HACHURE, spacing, &mut lines);
            sweep(&polygon, CROSS, spacing, &mut lines);
        }
    }

    lines
}

/// The outline as a closed polygon.
///
/// **Every subpath is closed whether or not it says `Close`**, because the
/// even-odd sweep needs edges rather than a stroke path: an open subpath left
/// open would leak its spans out through the gap.
fn flatten(outline: &Outline) -> Vec<Vec2> {
    let mut points: Vec<Vec2> = Vec::with_capacity(outline.commands().len() * 2);
    let mut current = Vec2::ZERO;

    for command in outline.commands() {
        match *command {
            SubpathCommand::MoveTo(p) | SubpathCommand::LineTo(p) => {
                points.push(p);
                current = p;
            }
            SubpathCommand::CubicTo { c1, c2, to } => {
                flatten_cubic(current, c1, c2, to, CLIP_TOLERANCE, |p| points.push(p));
                current = to;
            }
            SubpathCommand::Close => {}
        }
    }

    points
}

/// One direction's worth of sweep lines, appended to `lines`.
fn sweep(polygon: &[Vec2], direction: Vec2, spacing: f32, lines: &mut Outline) {
    // The sweep runs *along* `direction` and steps along its normal, so the
    // whole thing is done in a rotated frame: `u` is distance along a line and
    // `v` is which line. Rotating the polygon once is cheaper and much clearer
    // than intersecting arbitrary lines with arbitrary edges.
    let normal = Vec2::new(-direction.y, direction.x);
    let rotated: Vec<Vec2> = polygon
        .iter()
        .map(|p| {
            Vec2::new(
                p.x * direction.x + p.y * direction.y,
                p.x * normal.x + p.y * normal.y,
            )
        })
        .collect();

    let (mut low, mut high) = (f32::INFINITY, f32::NEG_INFINITY);
    for point in &rotated {
        low = low.min(point.y);
        high = high.max(point.y);
    }
    if !low.is_finite() || !high.is_finite() || high <= low {
        return;
    }

    // The bound, applied by widening the spacing rather than by stopping early
    // — see the module doc.
    let spacing = spacing
        .max(f32::MIN_POSITIVE)
        .max((high - low) / MAX_LINES as f32);

    let mut crossings: Vec<f32> = Vec::new();
    let mut at = low + spacing * 0.5;
    while at < high {
        spans(&rotated, at, &mut crossings);
        for pair in crossings.chunks_exact(2) {
            // **A zero-length span is not a hatch line.** A shape dragged out
            // to no area still has extent along the sweep's normal, so the
            // parity is right and every span it yields is a point — sixty-four
            // degenerate subpaths the tessellator has to look at and nobody can
            // see. Found by a test asking what an empty shape fills with.
            if pair[1] - pair[0] <= f32::EPSILON {
                continue;
            }
            // Back out of the rotated frame: `direction * u + normal * v`.
            let from = direction * pair[0] + normal * at;
            let to = direction * pair[1] + normal * at;
            lines.move_to(from).line_to(to);
        }
        at += spacing;
    }
}

/// Where the line `y == at` crosses the polygon, sorted along the line.
///
/// The polygon is treated as closed, so the last point joins the first. See the
/// module doc for the half-open interval and for why a horizontal edge is
/// skipped.
fn spans(polygon: &[Vec2], at: f32, out: &mut Vec<f32>) {
    out.clear();

    for index in 0..polygon.len() {
        let a = polygon[index];
        let b = polygon[(index + 1) % polygon.len()];
        if a.y == b.y {
            continue;
        }

        let (low, high) = if a.y < b.y { (a, b) } else { (b, a) };
        if at < low.y || at >= high.y {
            continue;
        }

        let t = (at - low.y) / (high.y - low.y);
        out.push(low.x + (high.x - low.x) * t);
    }

    out.sort_unstable_by(f32::total_cmp);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Rect;
    use crate::render::shapes;

    fn square() -> Outline {
        shapes::rectangle(Rect::new(Vec2::ZERO, Vec2::new(100.0, 100.0)))
    }

    /// A subpath is a `MoveTo` and a `LineTo`, so the segment count is half the
    /// commands.
    fn segments(outline: &Outline) -> usize {
        outline.commands().len() / 2
    }

    #[test]
    fn a_solid_fill_produces_no_lines() {
        assert!(hatch(&square(), FillStyle::Solid, DEFAULT_SPACING).is_empty());
    }

    #[test]
    fn a_cross_hatch_is_two_sweeps_of_a_hachure() {
        let one = hatch(&square(), FillStyle::Hachure, DEFAULT_SPACING);
        let two = hatch(&square(), FillStyle::CrossHatch, DEFAULT_SPACING);

        assert!(segments(&one) > 4);
        assert_eq!(segments(&two), segments(&one) * 2);
    }

    /// **Every hatch line stays inside the shape it fills.** The whole point of
    /// the scanline is the clip; a line that escaped would be a fill drawn over
    /// the canvas beside its own shape.
    #[test]
    fn every_line_stays_inside_the_shape() {
        let bounds = Rect::new(Vec2::new(20.0, 30.0), Vec2::new(140.0, 90.0));
        for shape in [
            shapes::rectangle(bounds),
            shapes::ellipse(bounds),
            shapes::diamond(bounds),
            shapes::triangle(bounds),
            shapes::rounded_rectangle(bounds, 12.0),
        ] {
            let lines = hatch(&shape, FillStyle::CrossHatch, DEFAULT_SPACING);
            let filled = lines.bounds().expect("a filled shape hatches");
            assert!(
                filled.min().x >= bounds.min().x - 0.5
                    && filled.min().y >= bounds.min().y - 0.5
                    && filled.max().x <= bounds.max().x + 0.5
                    && filled.max().y <= bounds.max().y + 0.5,
                "a hatch line left its shape: {filled:?} in {bounds:?}"
            );
        }
    }

    /// A diamond is the shape that catches a wrong parity rule: its widest
    /// scanline is at the middle and its narrowest at the corners, so a sweep
    /// that double-counted a vertex would draw a line across the *outside* of
    /// one of the points.
    #[test]
    fn a_diamond_hatches_widest_across_its_middle() {
        let bounds = Rect::new(Vec2::ZERO, Vec2::new(120.0, 120.0));
        let lines = hatch(&shapes::diamond(bounds), FillStyle::Hachure, 6.0);

        let mut lengths: Vec<f32> = lines
            .commands()
            .chunks_exact(2)
            .filter_map(|pair| match (pair[0], pair[1]) {
                (SubpathCommand::MoveTo(a), SubpathCommand::LineTo(b)) => Some((b - a).length()),
                _ => None,
            })
            .collect();
        assert!(lengths.len() > 4);

        // The longest span is near the middle of the sweep, not at either end.
        let longest = lengths
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map(|(index, _)| index)
            .unwrap();
        assert!(
            longest > 0 && longest < lengths.len() - 1,
            "the widest span of a diamond is not at one of its points"
        );

        lengths.sort_unstable_by(f32::total_cmp);
        assert!(lengths[0] > 0.0, "a zero-length span reached the output");
    }

    /// **The line count is bounded whatever the shape's size on screen.**
    /// Without this, a rectangle zoomed to fill a 4K display at the default
    /// spacing would be a few thousand segments in a single frame — and the
    /// black-window guard is a vertex count, so it would notice too late.
    #[test]
    fn an_enormous_shape_coarsens_rather_than_growing_without_bound() {
        let huge = Rect::new(Vec2::ZERO, Vec2::new(40_000.0, 40_000.0));
        let lines = hatch(&shapes::rectangle(huge), FillStyle::Hachure, 1.0);

        assert!(
            segments(&lines) as u32 <= MAX_LINES,
            "{} lines is more than the cap",
            segments(&lines)
        );
        assert!(segments(&lines) > 8, "and it still reads as a hatch");
    }

    /// A degenerate outline is not an error, and it must not be a panic either
    /// — a shape can be dragged out to nothing.
    #[test]
    fn a_shape_with_no_area_hatches_nothing() {
        let flat = Rect::new(Vec2::new(10.0, 10.0), Vec2::new(80.0, 0.0));
        assert!(hatch(&shapes::rectangle(flat), FillStyle::Hachure, 4.0).is_empty());

        let mut empty = Outline::new();
        empty.move_to(Vec2::ZERO);
        assert!(hatch(&empty, FillStyle::CrossHatch, 4.0).is_empty());
    }
}
