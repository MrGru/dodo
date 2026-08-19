//! [`Rect`] — an axis-aligned rectangle in world space.
//!
//! This is the type culling asks its questions in, and culling is a
//! *correctness* precondition rather than an optimisation: 16,000 fully
//! offscreen paths were measured still costing 6.3 ms of CPU per frame, because
//! GPUI rejects them only after `paint_path` has copied and scaled their vertex
//! buffers. So [`Rect::intersects`] is the predicate the whole render budget
//! rests on, and it is written to be obviously right.
//!
//! **Origin/size rather than min/max.** A node stores a position and a size, so
//! origin/size is the representation that needs no conversion at the call site
//! that matters most. `min()` and `max()` are one addition away when a query
//! wants them.
//!
//! **Empty rectangles are legal and are not "no rectangle".** A zero-size rect
//! is a degenerate point — a collapsed node, a single-point selection — and it
//! participates in unions correctly. "No rectangle at all" is `Option<Rect>`,
//! which is what [`Rect::of_points`] and the document's content bounds return.

use serde::{Deserialize, Serialize};

use crate::geometry::Vec2;

/// An axis-aligned rectangle: an origin (its minimum corner) and a size.
///
/// A negative size component is not normalised away — a drag that runs up and
/// left produces one, and [`Rect::normalized`] is the explicit fix. Every
/// predicate below assumes a normalised rectangle and says so.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Rect {
    pub origin: Vec2,
    pub size: Vec2,
}

impl Rect {
    /// The degenerate rectangle at the world origin.
    pub const ZERO: Rect = Rect {
        origin: Vec2::ZERO,
        size: Vec2::ZERO,
    };

    pub const fn new(origin: Vec2, size: Vec2) -> Rect {
        Rect { origin, size }
    }

    /// From two opposite corners, in any order. This is the box-select
    /// constructor: the drag's anchor and the pointer, normalised on the way in.
    pub fn from_corners(a: Vec2, b: Vec2) -> Rect {
        let min = a.min(b);
        let max = a.max(b);
        Rect::new(min, max - min)
    }

    /// The smallest rectangle containing every point, or `None` for an empty
    /// iterator — a document with no elements has no content bounds, and
    /// answering `Rect::ZERO` would silently make zoom-to-fit frame the origin.
    pub fn of_points(points: impl IntoIterator<Item = Vec2>) -> Option<Rect> {
        let mut points = points.into_iter();
        let first = points.next()?;
        let (min, max) = points.fold((first, first), |(min, max), p| (min.min(p), max.max(p)));
        Some(Rect::new(min, max - min))
    }

    /// The smallest rectangle containing every rectangle, or `None` for an empty
    /// iterator.
    pub fn of_rects(rects: impl IntoIterator<Item = Rect>) -> Option<Rect> {
        let mut rects = rects.into_iter();
        let first = rects.next()?;
        Some(rects.fold(first, |acc, r| acc.union(r)))
    }

    /// The minimum corner. Equal to `origin` when normalised.
    pub fn min(&self) -> Vec2 {
        self.origin.min(self.origin + self.size)
    }

    /// The maximum corner.
    pub fn max(&self) -> Vec2 {
        self.origin.max(self.origin + self.size)
    }

    pub fn center(&self) -> Vec2 {
        self.origin + self.size / 2.0
    }

    pub fn width(&self) -> f32 {
        self.size.x
    }

    pub fn height(&self) -> f32 {
        self.size.y
    }

    /// The same rectangle with a non-negative size.
    pub fn normalized(&self) -> Rect {
        let min = self.min();
        Rect::new(min, self.max() - min)
    }

    /// Zero area. Note that a degenerate rectangle still *contains* its own
    /// origin and still intersects a rectangle that covers it — emptiness is
    /// about area, not about existence.
    pub fn is_empty(&self) -> bool {
        self.size.x == 0.0 || self.size.y == 0.0
    }

    /// Inclusive on every edge. A point exactly on a node's right edge belongs
    /// to that node; hit-testing resolves the resulting overlap by z-order, not
    /// by an exclusive bound that would leave a one-unit dead strip between
    /// touching elements.
    ///
    /// Assumes a normalised rectangle.
    pub fn contains_point(&self, point: Vec2) -> bool {
        let min = self.origin;
        let max = self.origin + self.size;
        point.x >= min.x && point.x <= max.x && point.y >= min.y && point.y <= max.y
    }

    /// `other` lies entirely inside `self`. This is box-select's "fully
    /// enclosed" mode; [`Rect::intersects`] is its "touched" mode.
    ///
    /// Assumes normalised rectangles.
    pub fn contains_rect(&self, other: Rect) -> bool {
        self.contains_point(other.min()) && self.contains_point(other.max())
    }

    /// Any overlap at all, edges included. **This is the culling predicate**;
    /// see the module doc for why it is load-bearing.
    ///
    /// Assumes normalised rectangles.
    pub fn intersects(&self, other: Rect) -> bool {
        let (a_min, a_max) = (self.origin, self.origin + self.size);
        let (b_min, b_max) = (other.origin, other.origin + other.size);
        a_min.x <= b_max.x && b_min.x <= a_max.x && a_min.y <= b_max.y && b_min.y <= a_max.y
    }

    /// The smallest rectangle containing both. Assumes normalised rectangles.
    pub fn union(&self, other: Rect) -> Rect {
        let min = self.min().min(other.min());
        let max = self.max().max(other.max());
        Rect::new(min, max - min)
    }

    /// The overlap, or `None` when they do not touch. Assumes normalised
    /// rectangles.
    pub fn intersection(&self, other: Rect) -> Option<Rect> {
        if !self.intersects(other) {
            return None;
        }
        let min = self.min().max(other.min());
        let max = self.max().min(other.max());
        Some(Rect::new(min, max - min))
    }

    /// Grown by `amount` on every side (shrunk, for a negative amount). Used to
    /// pad a viewport query so an element whose stroke or shadow overhangs its
    /// bounds is still considered visible.
    pub fn inflate(&self, amount: f32) -> Rect {
        Rect::new(
            self.origin - Vec2::splat(amount),
            self.size + Vec2::splat(amount * 2.0),
        )
    }

    pub fn translated(&self, delta: Vec2) -> Rect {
        Rect::new(self.origin + delta, self.size)
    }

    /// Every component is finite — see [`Vec2::is_finite`] for why a loaded
    /// document is checked.
    pub fn is_finite(&self) -> bool {
        self.origin.is_finite() && self.size.is_finite()
    }
}

/// Whether the segment `a`→`b` touches `rect`, exactly.
///
/// §28's box selection needs it (an edge crossing the rectangle is selected
/// even though neither of its ends is inside it) and §29's edge hit test will
/// need it again. Exact rather than bounds-based: the segment's own bounding box
/// overlaps the rectangle for any diagonal that merely passes nearby, and a
/// selection that grabbed those would feel broken in exactly the way a rubber
/// band must not.
///
/// The test is the standard three-step one — endpoint inside, then the
/// separating-axis check on the segment's normal, having already rejected on the
/// two axis-aligned separations. `rect` is normalised first, so a rectangle
/// dragged up and to the left behaves like one dragged down and to the right.
pub fn segment_intersects_rect(a: Vec2, b: Vec2, rect: Rect) -> bool {
    let rect = rect.normalized();
    if rect.contains_point(a) || rect.contains_point(b) {
        return true;
    }

    // Axis-aligned separation: the segment's own bounds miss the rectangle.
    let min = a.min(b);
    let max = a.max(b);
    if max.x < rect.min().x || min.x > rect.max().x || max.y < rect.min().y || min.y > rect.max().y
    {
        return false;
    }

    // The remaining axis is the segment's normal. If all four corners fall on
    // the same side of the infinite line through `a` and `b`, the rectangle and
    // the segment are separated by it; otherwise, with the two axis-aligned
    // separations already ruled out, they touch.
    let direction = b - a;
    let normal = Vec2::new(-direction.y, direction.x);
    let offset = normal.x * a.x + normal.y * a.y;

    let mut positive = false;
    let mut negative = false;
    for corner in [
        rect.min(),
        Vec2::new(rect.max().x, rect.min().y),
        rect.max(),
        Vec2::new(rect.min().x, rect.max().y),
    ] {
        let side = normal.x * corner.x + normal.y * corner.y - offset;
        positive |= side > 0.0;
        negative |= side < 0.0;
        // A corner exactly on the line is a touch, which counts.
        if side == 0.0 {
            return true;
        }
    }

    positive && negative
}

#[cfg(test)]
mod tests {
    use super::{Rect, segment_intersects_rect};
    use crate::geometry::Vec2;

    fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect::new(Vec2::new(x, y), Vec2::new(w, h))
    }

    #[test]
    fn corners_and_center() {
        let r = rect(10.0, 20.0, 30.0, 40.0);

        assert_eq!(r.min(), Vec2::new(10.0, 20.0));
        assert_eq!(r.max(), Vec2::new(40.0, 60.0));
        assert_eq!(r.center(), Vec2::new(25.0, 40.0));
        assert_eq!(r.width(), 30.0);
        assert_eq!(r.height(), 40.0);
    }

    #[test]
    fn from_corners_normalizes_either_drag_direction() {
        let down_right = Rect::from_corners(Vec2::new(1.0, 2.0), Vec2::new(5.0, 8.0));
        let up_left = Rect::from_corners(Vec2::new(5.0, 8.0), Vec2::new(1.0, 2.0));

        assert_eq!(down_right, rect(1.0, 2.0, 4.0, 6.0));
        assert_eq!(up_left, down_right);
    }

    #[test]
    fn negative_sizes_normalize() {
        let backwards = rect(40.0, 60.0, -30.0, -40.0);

        assert_eq!(backwards.normalized(), rect(10.0, 20.0, 30.0, 40.0));
        assert_eq!(backwards.min(), Vec2::new(10.0, 20.0));
        assert_eq!(backwards.max(), Vec2::new(40.0, 60.0));
    }

    #[test]
    fn contains_point_is_inclusive_on_every_edge() {
        let r = rect(0.0, 0.0, 10.0, 10.0);

        assert!(r.contains_point(Vec2::new(5.0, 5.0)));
        assert!(r.contains_point(Vec2::ZERO));
        assert!(r.contains_point(Vec2::new(10.0, 10.0)));
        assert!(!r.contains_point(Vec2::new(10.001, 5.0)));
        assert!(!r.contains_point(Vec2::new(-0.001, 5.0)));
    }

    #[test]
    fn contains_rect_requires_full_enclosure() {
        let outer = rect(0.0, 0.0, 10.0, 10.0);

        assert!(outer.contains_rect(rect(2.0, 2.0, 3.0, 3.0)));
        assert!(outer.contains_rect(outer));
        assert!(!outer.contains_rect(rect(8.0, 8.0, 5.0, 5.0)));
    }

    #[test]
    fn intersects_counts_a_shared_edge_but_not_a_gap() {
        let r = rect(0.0, 0.0, 10.0, 10.0);

        assert!(r.intersects(rect(5.0, 5.0, 10.0, 10.0)));
        assert!(
            r.intersects(rect(10.0, 0.0, 5.0, 5.0)),
            "shared edge touches"
        );
        assert!(!r.intersects(rect(10.001, 0.0, 5.0, 5.0)));
        assert!(!r.intersects(rect(-20.0, 0.0, 5.0, 5.0)));
        assert!(
            r.intersects(rect(-5.0, -5.0, 100.0, 100.0)),
            "fully covered"
        );
    }

    #[test]
    fn a_degenerate_rect_still_intersects_and_contains() {
        let point = rect(5.0, 5.0, 0.0, 0.0);
        let area = rect(0.0, 0.0, 10.0, 10.0);

        assert!(point.is_empty());
        assert!(area.intersects(point));
        assert!(area.contains_rect(point));
        assert!(point.contains_point(Vec2::new(5.0, 5.0)));
    }

    #[test]
    fn union_and_intersection() {
        let a = rect(0.0, 0.0, 10.0, 10.0);
        let b = rect(5.0, 5.0, 10.0, 10.0);

        assert_eq!(a.union(b), rect(0.0, 0.0, 15.0, 15.0));
        assert_eq!(a.intersection(b), Some(rect(5.0, 5.0, 5.0, 5.0)));
        assert_eq!(a.intersection(rect(50.0, 50.0, 1.0, 1.0)), None);
    }

    #[test]
    fn of_points_and_of_rects_answer_none_when_empty() {
        assert_eq!(Rect::of_points(std::iter::empty()), None);
        assert_eq!(Rect::of_rects(std::iter::empty()), None);

        let points = [
            Vec2::new(3.0, -1.0),
            Vec2::new(-2.0, 4.0),
            Vec2::new(0.0, 0.0),
        ];
        assert_eq!(Rect::of_points(points), Some(rect(-2.0, -1.0, 5.0, 5.0)));

        let rects = [rect(0.0, 0.0, 1.0, 1.0), rect(9.0, 9.0, 1.0, 1.0)];
        assert_eq!(Rect::of_rects(rects), Some(rect(0.0, 0.0, 10.0, 10.0)));
    }

    #[test]
    fn inflate_grows_on_every_side_and_shrinks_for_a_negative_amount() {
        let r = rect(10.0, 10.0, 20.0, 20.0);

        assert_eq!(r.inflate(5.0), rect(5.0, 5.0, 30.0, 30.0));
        assert_eq!(r.inflate(-5.0), rect(15.0, 15.0, 10.0, 10.0));
        assert_eq!(r.inflate(0.0), r);
    }

    #[test]
    fn translate_moves_the_origin_and_keeps_the_size() {
        let r = rect(1.0, 2.0, 3.0, 4.0);

        assert_eq!(
            r.translated(Vec2::new(10.0, 20.0)),
            rect(11.0, 22.0, 3.0, 4.0)
        );
    }

    /// The oracle for the exact segment test: sample the segment densely and
    /// ask whether any sample is inside. Slow and obviously right, which is
    /// what a reference is for.
    fn segment_touches_by_sampling(a: Vec2, b: Vec2, r: Rect) -> bool {
        (0..=2_000).any(|step| {
            let t = step as f32 / 2_000.0;
            r.contains_point(a + (b - a) * t)
        })
    }

    #[test]
    fn a_segment_crossing_a_rectangle_is_detected_from_outside_it() {
        let r = rect(0.0, 0.0, 100.0, 100.0);

        // Straight through, both ends outside.
        assert!(segment_intersects_rect(
            Vec2::new(-50.0, 50.0),
            Vec2::new(150.0, 50.0),
            r
        ));
        // Diagonally through a corner.
        assert!(segment_intersects_rect(
            Vec2::new(-10.0, 10.0),
            Vec2::new(10.0, -10.0),
            r
        ));
        // One end inside.
        assert!(segment_intersects_rect(
            Vec2::new(50.0, 50.0),
            Vec2::new(500.0, 500.0),
            r
        ));
    }

    #[test]
    fn a_segment_that_only_passes_nearby_is_not_a_hit() {
        let r = rect(0.0, 0.0, 100.0, 100.0);

        // The classic false positive: a diagonal whose *bounding box* covers
        // the rectangle entirely but which passes outside its far corner. A
        // bounds-only test says yes; the exact test says no.
        assert!(!segment_intersects_rect(
            Vec2::new(-10.0, 260.0),
            Vec2::new(260.0, -10.0),
            r
        ));
        // And the same diagonal shifted in until it does clip the corner.
        assert!(segment_intersects_rect(
            Vec2::new(-10.0, 120.0),
            Vec2::new(120.0, -10.0),
            r
        ));
        assert!(!segment_intersects_rect(
            Vec2::new(200.0, 0.0),
            Vec2::new(200.0, 100.0),
            r
        ));
    }

    #[test]
    fn the_segment_test_agrees_with_dense_sampling() {
        let r = rect(-30.0, -20.0, 60.0, 40.0);
        let mut state = 0x1234_5678u32;
        let mut next = move || {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state >> 8) as f32 / (1 << 24) as f32 * 200.0 - 100.0
        };

        for _ in 0..2_000 {
            let a = Vec2::new(next(), next());
            let b = Vec2::new(next(), next());
            if segment_intersects_rect(a, b, r) != segment_touches_by_sampling(a, b, r) {
                // Sampling can miss a grazing crossing, so only a *false
                // negative* from the exact test is a failure.
                assert!(
                    !segment_touches_by_sampling(a, b, r),
                    "the exact test missed a crossing: {a:?} -> {b:?}"
                );
            }
        }
    }

    #[test]
    fn a_degenerate_segment_is_a_point_test() {
        let r = rect(0.0, 0.0, 10.0, 10.0);
        let inside = Vec2::new(5.0, 5.0);
        let outside = Vec2::new(50.0, 5.0);

        assert!(segment_intersects_rect(inside, inside, r));
        assert!(!segment_intersects_rect(outside, outside, r));
    }

    #[test]
    fn the_rectangle_is_normalized_before_it_is_tested() {
        let backwards = Rect::new(Vec2::new(10.0, 10.0), Vec2::new(-10.0, -10.0));
        assert!(segment_intersects_rect(
            Vec2::new(-5.0, 5.0),
            Vec2::new(15.0, 5.0),
            backwards
        ));
    }
}
