//! Cubic Bézier evaluation and flattening, in **world** space.
//!
//! [`cubic_segments`] moved here from `render::shapes` in Phase 4, and the move
//! is the point: the step-count formula is now reachable from the layer that
//! asks geometric questions about a route — *does this edge pass through the
//! selection rectangle?* — as well as from the layer that estimates a vertex
//! count. `render::shapes` re-exports it, so no call site changed, and there is
//! exactly one formula rather than two that drift.
//!
//! Flattening a curve is the only honest way to answer "does it cross this
//! rectangle": the control hull is a bound, not the curve, and a selection that
//! used the hull would grab edges that visibly miss the rectangle.
//!
//! **This file names no UI framework.**

use crate::{geometry::Vec2, models::RenderQuality};

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

/// The point at parameter `t` on a cubic, by de Casteljau.
///
/// Written as the nested-lerp form rather than the expanded polynomial: it is
/// the numerically stable one, and at `t = 0` and `t = 1` it returns the
/// endpoints exactly, which a hit test at the end of an edge depends on.
pub fn cubic_point(from: Vec2, c1: Vec2, c2: Vec2, to: Vec2, t: f32) -> Vec2 {
    let lerp = |a: Vec2, b: Vec2| a + (b - a) * t;

    let a = lerp(from, c1);
    let b = lerp(c1, c2);
    let c = lerp(c2, to);
    lerp(lerp(a, b), lerp(b, c))
}

/// Walks the cubic as a polyline at `tolerance`, calling `point` for every
/// vertex **after** `from` — so a caller that already has the start emits a
/// closed chain without a duplicate.
pub fn flatten_cubic(
    from: Vec2,
    c1: Vec2,
    c2: Vec2,
    to: Vec2,
    tolerance: f32,
    mut point: impl FnMut(Vec2),
) {
    let steps = cubic_segments(from, c1, c2, to, tolerance);
    for step in 1..=steps {
        point(cubic_point(from, c1, c2, to, step as f32 / steps as f32));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cubic_starts_and_ends_exactly_on_its_endpoints() {
        let from = Vec2::new(-3.0, 11.0);
        let to = Vec2::new(120.0, -40.0);
        let c1 = Vec2::new(40.0, 200.0);
        let c2 = Vec2::new(90.0, -300.0);

        assert_eq!(cubic_point(from, c1, c2, to, 0.0), from);
        assert_eq!(cubic_point(from, c1, c2, to, 1.0), to);
    }

    #[test]
    fn a_degenerate_cubic_is_its_own_point() {
        let p = Vec2::new(4.0, 4.0);
        assert_eq!(cubic_point(p, p, p, p, 0.37), p);
        assert_eq!(cubic_segments(p, p, p, p, 0.1), 1);
    }

    /// A straight "curve" — control points on the line — must stay on it.
    #[test]
    fn a_straight_cubic_stays_straight() {
        let from = Vec2::ZERO;
        let to = Vec2::new(100.0, 0.0);
        let mid = cubic_point(from, Vec2::new(33.0, 0.0), Vec2::new(66.0, 0.0), to, 0.5);
        assert!(mid.y.abs() < 1e-5);
        // Not 50: the control points are not evenly spaced, and the Bernstein
        // weights at t = 0.5 are 1/8, 3/8, 3/8, 1/8.
        assert!((mid.x - 49.625).abs() < 1e-3, "midpoint was {mid:?}");
    }

    #[test]
    fn flattening_ends_on_the_endpoint_and_never_repeats_the_start() {
        let from = Vec2::ZERO;
        let c1 = Vec2::new(0.0, 100.0);
        let c2 = Vec2::new(200.0, 100.0);
        let to = Vec2::new(200.0, 0.0);

        let mut points = Vec::new();
        flatten_cubic(from, c1, c2, to, 0.25, |p| points.push(p));

        assert!(!points.is_empty());
        assert_ne!(points[0], from);
        assert_eq!(*points.last().unwrap(), to);
        assert_eq!(
            points.len(),
            cubic_segments(from, c1, c2, to, 0.25) as usize
        );
    }

    #[test]
    fn a_finer_tolerance_asks_for_more_segments() {
        let from = Vec2::ZERO;
        let c1 = Vec2::new(0.0, 400.0);
        let c2 = Vec2::new(400.0, 400.0);
        let to = Vec2::new(400.0, 0.0);

        assert!(cubic_segments(from, c1, c2, to, 0.1) > cubic_segments(from, c1, c2, to, 2.0));
        assert!(cubic_segments(from, c1, c2, to, 1e-9) <= MAX_CUBIC_SEGMENTS);
    }

    #[test]
    fn a_non_finite_control_point_does_not_produce_a_runaway_count() {
        let from = Vec2::ZERO;
        let to = Vec2::new(10.0, 0.0);
        assert_eq!(
            cubic_segments(from, Vec2::new(f32::NAN, 0.0), Vec2::ZERO, to, 0.1),
            1
        );
    }
}
