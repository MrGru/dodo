//! Endpoint decorations (§8): the geometry of an arrow head, a triangle, a
//! diamond and a dot.
//!
//! # Allocation-free on purpose
//!
//! Every marker is at most four points, and there are up to two per edge. A
//! `Vec` per marker would be two allocations per edge per rebuild for eight
//! `f32`s, so [`ArrowPolygon`] is a fixed array with a length — `Copy`,
//! stack-sized, and the reason a route rebuild during a drag touches the
//! allocator exactly once (for the route's own segments) rather than three
//! times.
//!
//! # A dot is not a path
//!
//! [`ArrowGeometry::Dot`] is a centre and a radius rather than a tessellated
//! circle, because a circle is a **quad with a corner radius of half its
//! side** — Phase 0 measured 20,000 quads at 60 fps against the same count of
//! filled paths at 30, and a quad carries the radius for free. The same
//! decision `render::shapes::prefers_quad` makes for a rectangle, made here for
//! the marker.
//!
//! # The line is not trimmed back
//!
//! A filled marker is painted **over** the end of its edge rather than the edge
//! being shortened to meet it. Trimming would make the route's geometry depend
//! on the edge's *style*, which would put the markers into every geometry cache
//! key and rebuild the whole route when someone changed an arrow head. The
//! marker covers the overlap; the separation is worth more than the two pixels.
//!
//! **This file names no UI framework.**

use crate::{geometry::Vec2, models::ArrowMarker};

/// How long a marker is, along the edge, as a multiple of the stroke width.
///
/// A marker sized off the stroke rather than off the edge's length is what
/// keeps a short edge's arrow head from swallowing it, and a long edge's from
/// disappearing. The multiple is a judgement, matched by eye against the arrow
/// heads in the applications the requirements name.
pub const MARKER_LENGTH_PER_STROKE: f32 = 6.0;

/// The smallest a marker gets in world units, whatever the stroke width, so a
/// hairline edge still shows which way it points.
pub const MIN_MARKER_LENGTH: f32 = 6.0;

/// A marker's shape, ready to be transformed and painted.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ArrowGeometry {
    /// A V, a triangle or a diamond.
    Polygon(ArrowPolygon),
    /// A disc, painted as a quad with a corner radius. See the module doc.
    Dot { center: Vec2, radius: f32 },
}

/// Up to four points, without touching the allocator.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArrowPolygon {
    points: [Vec2; 4],
    len: u8,
    /// Whether the last point joins back to the first.
    pub closed: bool,
    /// Filled, or stroked at the edge's own width. An open arrow head is
    /// stroked; a triangle and a diamond are filled.
    pub filled: bool,
}

impl ArrowPolygon {
    pub fn points(&self) -> &[Vec2] {
        &self.points[..self.len as usize]
    }

    pub fn len(&self) -> usize {
        self.len as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

/// The world-space size of a marker on an edge of this stroke width.
pub fn marker_length(stroke_width: f32) -> f32 {
    (stroke_width * MARKER_LENGTH_PER_STROKE).max(MIN_MARKER_LENGTH)
}

/// The geometry for one marker.
///
/// - `tip` is where the edge ends — the point of the arrow.
/// - `direction` is the unit vector the edge is **travelling in** as it
///   arrives. A source marker passes the negated start tangent, which is what
///   makes it point back down the edge; [`EdgeRoute`](crate::geometry::EdgeRoute)
///   carries both tangents for exactly this.
/// - `length` is the marker's extent along `direction`; see [`marker_length`].
///
/// `None` for [`ArrowMarker::None`], for a degenerate direction and for a
/// non-positive length — all three are ordinary states rather than errors, and
/// returning a zero-area polygon instead would put a path in the frame that
/// paints nothing.
pub fn marker(kind: ArrowMarker, tip: Vec2, direction: Vec2, length: f32) -> Option<ArrowGeometry> {
    if kind == ArrowMarker::None || !length.is_finite() || length <= 0.0 {
        return None;
    }

    let magnitude = direction.length();
    if !magnitude.is_finite() || magnitude <= f32::EPSILON {
        return None;
    }
    let forward = direction / magnitude;
    // The left-hand normal. Which hand does not matter — the shapes below are
    // symmetric about `forward` — but it has to be consistent, or a diamond
    // would wind backwards and lyon would fill it inside out.
    let side = Vec2::new(-forward.y, forward.x);

    let back = tip - forward * length;

    Some(match kind {
        ArrowMarker::None => unreachable!("filtered above"),
        // An open V: the two wings, meeting at the tip. Stroked, so the joint
        // is the edge's own line width and it reads as part of the line.
        ArrowMarker::Arrow => {
            let wing = length * 0.5;
            ArrowGeometry::Polygon(ArrowPolygon {
                points: [back + side * wing, tip, back - side * wing, Vec2::ZERO],
                len: 3,
                closed: false,
                filled: false,
            })
        }
        ArrowMarker::ArrowClosed => {
            let wing = length * 0.4;
            ArrowGeometry::Polygon(ArrowPolygon {
                points: [tip, back + side * wing, back - side * wing, Vec2::ZERO],
                len: 3,
                closed: true,
                filled: true,
            })
        }
        ArrowMarker::Diamond => {
            let waist = tip - forward * (length * 0.5);
            let wing = length * 0.35;
            ArrowGeometry::Polygon(ArrowPolygon {
                points: [tip, waist + side * wing, back, waist - side * wing],
                len: 4,
                closed: true,
                filled: true,
            })
        }
        ArrowMarker::Dot => ArrowGeometry::Dot {
            center: tip - forward * (length * 0.5),
            radius: length * 0.5,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::{ArrowGeometry, MIN_MARKER_LENGTH, marker, marker_length};
    use crate::{geometry::Vec2, models::ArrowMarker};

    const RIGHT: Vec2 = Vec2::new(1.0, 0.0);

    fn polygon(kind: ArrowMarker, direction: Vec2) -> Vec<Vec2> {
        match marker(kind, Vec2::new(100.0, 50.0), direction, 10.0) {
            Some(ArrowGeometry::Polygon(polygon)) => polygon.points().to_vec(),
            other => panic!("{kind:?} is a polygon, got {other:?}"),
        }
    }

    #[test]
    fn no_marker_is_no_geometry() {
        assert_eq!(marker(ArrowMarker::None, Vec2::ZERO, RIGHT, 10.0), None);
    }

    /// Every marker's point sits exactly on the end of the edge — the property
    /// that makes an arrow look attached rather than floating near its line.
    #[test]
    fn every_pointed_marker_has_its_point_on_the_tip() {
        let tip = Vec2::new(100.0, 50.0);

        for kind in [
            ArrowMarker::Arrow,
            ArrowMarker::ArrowClosed,
            ArrowMarker::Diamond,
        ] {
            let points = polygon(kind, RIGHT);
            assert!(
                points.iter().any(|p| (*p - tip).length() < 1e-5),
                "{kind:?} does not touch the tip: {points:?}"
            );
        }
    }

    /// A marker's body lies **behind** its tip, along the direction of travel.
    /// Get this backwards and every arrow in the canvas points into its node.
    #[test]
    fn a_marker_lies_behind_the_tip_in_the_direction_of_travel() {
        for kind in [
            ArrowMarker::Arrow,
            ArrowMarker::ArrowClosed,
            ArrowMarker::Diamond,
        ] {
            let points = polygon(kind, RIGHT);
            assert!(
                points.iter().all(|p| p.x <= 100.0 + 1e-5),
                "{kind:?} pokes past the tip: {points:?}"
            );
            assert!(
                points.iter().any(|p| p.x < 100.0 - 1e-5),
                "{kind:?} has no body: {points:?}"
            );
        }
    }

    #[test]
    fn a_marker_is_symmetric_about_the_edge_it_sits_on() {
        for kind in [
            ArrowMarker::Arrow,
            ArrowMarker::ArrowClosed,
            ArrowMarker::Diamond,
        ] {
            let points = polygon(kind, RIGHT);
            let above: f32 = points.iter().map(|p| (p.y - 50.0).max(0.0)).sum();
            let below: f32 = points.iter().map(|p| (50.0 - p.y).max(0.0)).sum();

            assert!((above - below).abs() < 1e-4, "{kind:?} is lopsided");
        }
    }

    /// The open arrow head is two wings meeting at the tip: three points, not
    /// closed, not filled — so it paints as a stroke of the edge's own width.
    #[test]
    fn an_open_arrow_is_a_stroked_v() {
        let Some(ArrowGeometry::Polygon(polygon)) =
            marker(ArrowMarker::Arrow, Vec2::ZERO, RIGHT, 10.0)
        else {
            panic!("expected a polygon");
        };

        assert_eq!(polygon.len(), 3);
        assert!(!polygon.closed);
        assert!(!polygon.filled);
        assert_eq!(
            polygon.points()[1],
            Vec2::ZERO,
            "the tip is the middle point"
        );
    }

    #[test]
    fn a_closed_arrow_and_a_diamond_are_filled_and_closed() {
        for (kind, points) in [(ArrowMarker::ArrowClosed, 3), (ArrowMarker::Diamond, 4)] {
            let Some(ArrowGeometry::Polygon(polygon)) = marker(kind, Vec2::ZERO, RIGHT, 10.0)
            else {
                panic!("expected a polygon for {kind:?}");
            };

            assert_eq!(polygon.len(), points, "{kind:?}");
            assert!(polygon.closed, "{kind:?}");
            assert!(polygon.filled, "{kind:?}");
        }
    }

    /// A dot straddles the end of the edge rather than sitting past it, so the
    /// line disappears under it.
    #[test]
    fn a_dot_is_a_circle_centred_half_a_length_back() {
        let Some(ArrowGeometry::Dot { center, radius }) =
            marker(ArrowMarker::Dot, Vec2::new(100.0, 0.0), RIGHT, 10.0)
        else {
            panic!("expected a dot");
        };

        assert_eq!(center, Vec2::new(95.0, 0.0));
        assert_eq!(radius, 5.0);
    }

    /// Markers rotate with the edge. A marker built for a downward edge is the
    /// upward one turned a quarter turn, which is what the normal construction
    /// has to guarantee.
    #[test]
    fn a_marker_rotates_with_the_direction_it_is_given() {
        let down = polygon(ArrowMarker::ArrowClosed, Vec2::new(0.0, 1.0));

        assert!(
            down.iter().all(|p| p.y <= 50.0 + 1e-5),
            "a downward arrow's body is above its tip: {down:?}"
        );
        let widths: Vec<f32> = down.iter().map(|p| p.x).collect();
        assert!(
            widths.iter().any(|x| *x < 100.0) && widths.iter().any(|x| *x > 100.0),
            "a downward arrow spreads in x: {down:?}"
        );
    }

    /// A direction of zero length — a route whose ends coincide — must not
    /// produce a `NaN` polygon.
    #[test]
    fn a_degenerate_direction_or_length_yields_no_marker() {
        assert_eq!(
            marker(ArrowMarker::Arrow, Vec2::ZERO, Vec2::ZERO, 10.0),
            None
        );
        assert_eq!(marker(ArrowMarker::Arrow, Vec2::ZERO, RIGHT, 0.0), None);
        assert_eq!(
            marker(ArrowMarker::Arrow, Vec2::ZERO, RIGHT, f32::NAN),
            None
        );
    }

    /// A hairline edge still gets a visible arrow, and a thick one gets a
    /// proportionate one.
    #[test]
    fn marker_length_has_a_floor_and_then_follows_the_stroke() {
        assert_eq!(marker_length(0.1), MIN_MARKER_LENGTH);
        assert_eq!(marker_length(4.0), 24.0);
    }
}
