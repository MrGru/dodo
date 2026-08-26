//! [`Perimeter`] — **the one silhouette**, as the closed boundary of a shape
//! inscribed in the *unit square*, and the two questions a binding asks of it:
//! *where is parameter `t`?* and *which `t` is nearest the pointer?*
//!
//! # Why this exists at all
//!
//! A connector endpoint used to bind to a point on its target's **bounding
//! box**: [`Side::facing`](crate::geometry::Side) picked a face and the
//! endpoint slid along it. On a rectangle that is the right answer; on an
//! ellipse, a diamond or a rounded rectangle it is a lie the user can see —
//! the arrow stops in mid-air beside a circle, or plunges through the cut
//! corner of a rounded box. Excalidraw's arrows land on the drawn outline, and
//! that is what this module makes expressible.
//!
//! It is deliberately **the same geometry the painter draws**:
//! [`crate::render::shapes`]'s `rectangle`, `rounded_rectangle`, `ellipse`,
//! `diamond` and `triangle` are all built by walking a [`Perimeter`], so an
//! outline and a binding cannot disagree about where a shape's edge is. That is
//! the same rule [`handle_world_position`](crate::models::handle_world_position)
//! states for handles, applied to whole silhouettes:
//! `render::shapes`' `the_painters_outline_is_the_binding_silhouette` is the
//! assertion.
//!
//! # The unit square, and why the parameter is arc length *there*
//!
//! Every shape here is built inside `0.0..=1.0` on both axes, and a world point
//! is `bounds.origin + unit.scale(bounds.size)`, rotated about the centre. That
//! affine map is exact — including for a rounded corner, whose circular world
//! arc is an *elliptical* unit arc with `rx = radius / width` and
//! `ry = radius / height`, and scales straight back to a circle.
//!
//! The persisted binding is **one `f32`: a normalised position round the unit
//! silhouette**, measured from each shape's own start point and running the way
//! its outline is drawn.
//!
//! *Position*, and the exact definition matters: **segments are weighted by
//! their arc length, and within one segment the parameter is that segment's
//! own.** So a rounded rectangle's corner gets the share of the range its arc
//! actually occupies — which is the part that has to be arc length, or a
//! 12-pixel corner would claim an eighth of the perimeter — while inside the
//! corner the cubic's parameter is used directly.
//!
//! The alternative, true arc length all the way down, costs a Newton solve to
//! invert on the resolve path and buys a redistribution a quarter-circle cubic
//! keeps within about 2 % of uniform. What this definition buys instead is that
//! [`Perimeter::nearest`] and [`Perimeter::point_at`] are **exact inverses**:
//! the point a drag previews, the parameter it stores and the point a reload
//! resolves are the same point, with no round-off between them.
//! `a_binding_resolves_to_the_point_it_was_taken_from` is the assertion.
//!
//! Three properties follow, and each is one of the requirements:
//!
//! - **It is always on the boundary.** Any `f32` at all resolves to a point on
//!   the outline — there is no way to store a binding that resolves inside the
//!   shape, which a normalised *point* in the box could do the moment the shape
//!   or the file was slightly wrong.
//! - **A move, a resize and a rotation are exact.** The unit silhouette does
//!   not depend on the node's position, its size or its angle, so the parameter
//!   means the same visible spot after any of them — including a non-uniform
//!   resize, where a normalised arc length of the *world* outline would slide
//!   the endpoint along the edge it was pinned to.
//! - **A restyle is graceful.** Changing the corner radius changes the unit
//!   silhouette, and the endpoint re-resolves onto the new outline instead of
//!   floating off the corner that was just rounded away.
//!
//! The one place the parameter is not perfectly invariant is a rounded
//! rectangle under a *non-uniform* resize, where the corner arcs are a
//! different fraction of the unit perimeter than they were. The drift is a
//! fraction of a corner and it is monotone, which is the behaviour a user reads
//! as "it followed the shape".
//!
//! # Allocation-free, because of where it runs
//!
//! [`Perimeter::of`] is called once per bound endpoint whenever its target
//! moves, resizes or rotates, and once per frame per candidate while an
//! endpoint is being dragged. `budgets.rs` is explicit that a per-frame path
//! may not allocate, so a perimeter is a fixed `[PerimeterSegment; 8]` on the
//! stack — eight is a rounded rectangle, the busiest shape here — with its
//! cumulative arc lengths beside it. Nothing in this file heap-allocates.
//!
//! **This file names no UI framework.**

use crate::geometry::{CIRCLE_KAPPA as KAPPA, Rect, Side, Vec2, cubic_point};

/// The most segments any silhouette here needs.
///
/// A rounded rectangle is four straight sides and four corner cubics, and it is
/// the largest; an ellipse is four cubics, a rectangle and a diamond four
/// lines, a triangle three. The array is sized to the worst case so the whole
/// structure is `Copy` and lives on the stack.
pub const MAX_PERIMETER_SEGMENTS: usize = 8;

/// Five-point Gauss-Legendre abscissae and weights on `0.0..=1.0`.
///
/// **The arc-length table is on the hot path**, so it is not a chord sum.
/// Every bound endpoint re-measures its target's silhouette whenever that
/// target moves, resizes, rotates or is restyled, and a rounded rectangle has
/// four curves in it; sixteen chords each would be sixty-four cubic
/// evaluations for a number that only has to be *proportional*.
///
/// Five-point Gauss-Legendre integrates `|B'(t)|` — smooth and non-vanishing on
/// every arc here — to about one part in `1e-9` for five derivative
/// evaluations, which is both cheaper and far more accurate than the chord sum
/// it replaced. Arc length is a *parameter* rather than a length in any case:
/// it decides how evenly the binding is distributed round a curve, never
/// whether the point lands on it — [`Perimeter::point_at`] always evaluates the
/// true curve.
const GAUSS: [(f32, f32); 5] = [
    (0.046_910_077, 0.118_463_44),
    (0.230_765_34, 0.239_314_34),
    (0.5, 0.284_444_44),
    (0.769_234_66, 0.239_314_34),
    (0.953_089_9, 0.118_463_44),
];

/// How many candidate points per segment the nearest-point search starts from.
///
/// The search is a coarse scan followed by [`REFINE_STEPS`] ternary bisections
/// inside the winning bracket, which is the standard shape for "closest point
/// on a cubic" — the exact answer is the root of a fifth-degree polynomial and
/// nobody wants it. A straight segment skips both: its projection is exact.
const SCAN_SAMPLES: u32 = 12;

/// Ternary-search iterations used to refine the best scan sample.
///
/// Each iteration cuts the bracket by a third, so sixteen of them shrink a
/// twelfth of a segment to about 2e-5 of it — well under a millipixel on any
/// shape a person can see.
const REFINE_STEPS: u32 = 16;

/// A shape as this module draws it: the geometric vocabulary, with no element
/// kind, no style and no renderer in it.
///
/// Separate from [`ShapeKind`](crate::models::ShapeKind) and from
/// [`NodeShape`](crate::runtime::NodeShape) on purpose. `geometry/` sits below
/// both — it may not name a runtime projection — and the mapping is one
/// `match` in each of the two layers that has one
/// ([`NodeShape::perimeter`](crate::runtime::NodeShape::perimeter) and
/// `render::shapes`), which is where a new kind is routed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PerimeterShape {
    Rectangle,
    /// `radius` is in **world** units, like
    /// [`ElementStyle::corner_radius`](crate::models::ElementStyle::corner_radius);
    /// [`Perimeter::of`] normalises it against the size it is given.
    RoundedRectangle {
        radius: f32,
    },
    Ellipse,
    Diamond,
    Triangle,
}

/// One step of a silhouette: a straight side, or a cubic with two controls.
///
/// Not [`SubpathCommand`](crate::render::shapes::SubpathCommand): that is a
/// *drawing* vocabulary with a current point and a `Close`, and this is a list
/// of independent, self-contained pieces that can be measured and projected
/// onto in any order. `render::shapes` converts one into the other, in the one
/// direction that makes sense.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PerimeterSegment {
    pub from: Vec2,
    pub to: Vec2,
    /// `None` for a straight side; the two control points for a curved one.
    pub controls: Option<[Vec2; 2]>,
}

impl PerimeterSegment {
    fn line(from: Vec2, to: Vec2) -> PerimeterSegment {
        PerimeterSegment {
            from,
            to,
            controls: None,
        }
    }

    fn cubic(from: Vec2, c1: Vec2, c2: Vec2, to: Vec2) -> PerimeterSegment {
        PerimeterSegment {
            from,
            to,
            controls: Some([c1, c2]),
        }
    }

    /// The point at `t` along this segment, `t` in `0.0..=1.0`.
    pub fn point_at(self, t: f32) -> Vec2 {
        match self.controls {
            None => self.from + (self.to - self.from) * t,
            Some([c1, c2]) => cubic_point(self.from, c1, c2, self.to, t),
        }
    }

    /// The tangent at `t`, i.e. the derivative of the segment there.
    ///
    /// Not normalised: [`length`](PerimeterSegment::length) wants the magnitude
    /// and a caller asking for a direction can divide.
    pub fn tangent_at(self, t: f32) -> Vec2 {
        match self.controls {
            None => self.to - self.from,
            Some([c1, c2]) => {
                let u = 1.0 - t;
                (c1 - self.from) * (3.0 * u * u)
                    + (c2 - c1) * (6.0 * u * t)
                    + (self.to - c2) * (3.0 * t * t)
            }
        }
    }

    /// The segment's length after `size` is applied — i.e. in world units for a
    /// shape of that size, or its unit length for [`Vec2::ONE`].
    ///
    /// A straight side is exact; a cubic is `∫|B'(t)|dt` by five-point
    /// Gauss-Legendre quadrature — see the `GAUSS` table above.
    pub fn length(self, size: Vec2) -> f32 {
        match self.controls {
            None => (self.to - self.from).scale(size).length(),
            Some(_) => GAUSS
                .iter()
                .map(|(t, weight)| self.tangent_at(*t).scale(size).length() * weight)
                .sum(),
        }
    }

    /// The parameter of the point on this segment nearest `target`, measuring
    /// distance **after** `size` is applied.
    ///
    /// The weighting is what makes "nearest" mean nearest *on screen* rather
    /// than nearest in the unit square: on a 400×40 box the two are not the
    /// same point, and the user is aiming at the one they can see.
    fn nearest_parameter(self, target: Vec2, size: Vec2) -> (f32, f32) {
        let distance = |t: f32| ((self.point_at(t) - target).scale(size)).length_squared();

        if self.controls.is_none() {
            // A straight side has an exact projection, clamped to its own
            // extent so the answer stays on the segment rather than on the
            // infinite line through it.
            let along = (self.to - self.from).scale(size);
            let reach = (target - self.from).scale(size);
            let length_squared = along.length_squared();
            let t = if length_squared <= f32::EPSILON {
                0.0
            } else {
                (reach.x * along.x + reach.y * along.y) / length_squared
            }
            .clamp(0.0, 1.0);
            return (t, distance(t));
        }

        let mut best = (0.0f32, distance(0.0));
        for step in 1..=SCAN_SAMPLES {
            let t = step as f32 / SCAN_SAMPLES as f32;
            let d = distance(t);
            if d < best.1 {
                best = (t, d);
            }
        }

        // Ternary search inside the bracket the scan won. The distance along a
        // cubic is not unimodal in general, but it is inside one twelfth of one
        // of these corner arcs, and the scan is what picks the right basin.
        let span = 1.0 / SCAN_SAMPLES as f32;
        let (mut low, mut high) = ((best.0 - span).max(0.0), (best.0 + span).min(1.0));
        for _ in 0..REFINE_STEPS {
            let third = (high - low) / 3.0;
            let (a, b) = (low + third, high - third);
            if distance(a) < distance(b) {
                high = b;
            } else {
                low = a;
            }
        }
        let t = (low + high) * 0.5;
        let d = distance(t);
        if d < best.1 { (t, d) } else { best }
    }
}

/// A closed silhouette in the unit square, with its cumulative arc lengths.
///
/// Built by [`Perimeter::of`] and read by [`Perimeter::point_at`] and
/// [`Perimeter::nearest`]. See the module doc for why the parameter is arc
/// length here rather than in world space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Perimeter {
    segments: [PerimeterSegment; MAX_PERIMETER_SEGMENTS],
    len: u8,
    /// Cumulative unit arc length at the **end** of each segment. This is what
    /// gives each segment its share of `0.0..=1.0`; inside a segment the
    /// parameter is the segment's own. See the module doc.
    ends: [f32; MAX_PERIMETER_SEGMENTS],
    total: f32,
}

impl Perimeter {
    /// The unit-square silhouette of `shape` at a node of `size`.
    ///
    /// `size` is needed even though the result is in unit space, because a
    /// corner radius is a *world* length: the same 12 px radius is a different
    /// fraction of a wide box than of a tall one, and a unit silhouette that
    /// ignored that would scale back into an outline the painter never drew.
    pub fn of(shape: PerimeterShape, size: Vec2) -> Perimeter {
        let mut builder = Builder::new();
        let (w, h) = (size.x.abs(), size.y.abs());

        match shape {
            PerimeterShape::Rectangle => builder.polygon(&[
                Vec2::new(0.0, 0.0),
                Vec2::new(1.0, 0.0),
                Vec2::new(1.0, 1.0),
                Vec2::new(0.0, 1.0),
            ]),
            PerimeterShape::RoundedRectangle { radius } => {
                // The painter's clamp, stated once: a radius past half the
                // shorter side produces a stadium rather than an inverted
                // corner — the rule CSS and GPUI's own quad use.
                let limit = w.min(h) * 0.5;
                let r = radius.clamp(0.0, limit.max(0.0));
                if r <= 0.0 || w <= f32::EPSILON || h <= f32::EPSILON {
                    builder.polygon(&[
                        Vec2::new(0.0, 0.0),
                        Vec2::new(1.0, 0.0),
                        Vec2::new(1.0, 1.0),
                        Vec2::new(0.0, 1.0),
                    ]);
                } else {
                    builder.rounded_rectangle(r / w, r / h);
                }
            }
            PerimeterShape::Ellipse => builder.ellipse(),
            PerimeterShape::Diamond => builder.polygon(&[
                Vec2::new(0.5, 0.0),
                Vec2::new(1.0, 0.5),
                Vec2::new(0.5, 1.0),
                Vec2::new(0.0, 0.5),
            ]),
            PerimeterShape::Triangle => builder.polygon(&[
                Vec2::new(0.5, 0.0),
                Vec2::new(1.0, 1.0),
                Vec2::new(0.0, 1.0),
            ]),
        }

        builder.finish()
    }

    pub fn segments(&self) -> &[PerimeterSegment] {
        &self.segments[..self.len as usize]
    }

    /// The unit arc length all the way round. Never zero for a real shape, and
    /// guarded against a degenerate one by [`Perimeter::point_at`].
    pub fn total_length(&self) -> f32 {
        self.total
    }

    /// Whether this silhouette has a usable arc length. False only for a
    /// degenerate shape — a zero-sized node — where every query answers the
    /// centre rather than a `NaN`.
    fn has_length(&self) -> bool {
        self.total.is_finite() && self.total > 0.0
    }

    /// The unit-square point at normalised perimeter position `t` — the exact
    /// inverse of [`Perimeter::nearest`]'s first answer.
    ///
    /// `t` **wraps**: 1.25 and 0.25 are the same place, and so is −0.75. A
    /// stored parameter is therefore always meaningful, whatever a file or a
    /// migration put in it, which is half the reason the binding is a scalar.
    pub fn point_at(&self, t: f32) -> Vec2 {
        let segments = self.segments();
        if segments.is_empty() || !self.has_length() || !t.is_finite() {
            return Vec2::new(0.5, 0.5);
        }

        let wrapped = t.rem_euclid(1.0) * self.total;
        let mut start = 0.0;
        for (index, segment) in segments.iter().enumerate() {
            let end = self.ends[index];
            if wrapped <= end || index + 1 == segments.len() {
                let span = end - start;
                let local = if span <= f32::EPSILON {
                    0.0
                } else {
                    ((wrapped - start) / span).clamp(0.0, 1.0)
                };
                return segment.point_at(local);
            }
            start = end;
        }
        segments[0].from
    }

    /// The normalised perimeter position of the boundary point nearest
    /// `target`, and that point — both in unit space, with distance measured
    /// after `size`.
    ///
    /// This is the "aim anywhere" half of the feature: `target` is where the
    /// user is pointing, expressed in the target's unit square, and the answer
    /// is the visible spot they are pointing at.
    pub fn nearest(&self, target: Vec2, size: Vec2) -> (f32, Vec2) {
        let segments = self.segments();
        if segments.is_empty() || !self.has_length() {
            return (0.0, Vec2::new(0.5, 0.5));
        }

        let mut best: Option<(f32, f32, Vec2)> = None;
        let mut start = 0.0;
        for (index, segment) in segments.iter().enumerate() {
            let (local, distance) = segment.nearest_parameter(target, size);
            if best.is_none_or(|(_, closest, _)| distance < closest) {
                let end = self.ends[index];
                let along = start + (end - start) * local;
                best = Some((along / self.total, distance, segment.point_at(local)));
            }
            start = self.ends[index];
        }

        best.map_or((0.0, segments[0].from), |(t, _, point)| {
            (t.clamp(0.0, 1.0), point)
        })
    }
}

/// The world point at perimeter parameter `t` on `shape` inscribed in
/// `bounds`, after the element's own `angle`.
///
/// **The single resolver.** Every place that turns a stored binding back into a
/// position goes through it, so a bound endpoint painted in one place and
/// routed from another is not expressible — the same argument
/// [`handle_world_position`](crate::models::handle_world_position) makes for
/// handles.
pub fn perimeter_point(shape: PerimeterShape, bounds: Rect, angle: f32, t: f32) -> Vec2 {
    let bounds = bounds.normalized();
    let unit = Perimeter::of(shape, bounds.size).point_at(t);
    (bounds.origin + unit.scale(bounds.size)).rotated_about(bounds.center(), angle)
}

/// Where a boundary point sits, and the parameter that will be stored for it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PerimeterHit {
    /// Normalised position round the unit silhouette — the durable half.
    pub t: f32,
    /// The world point it resolves to right now, rotation included.
    pub point: Vec2,
}

/// The point of `shape` inscribed in `bounds` (rotated by `angle`) nearest
/// `aim`, in world space.
///
/// `aim` is **where the user is pointing**, not where the other end of the
/// connector is: that is the whole difference between Excalidraw's free
/// binding and the old "which side faces my partner" rule, and it is why the
/// preview during a drag shows exactly the point the release commits.
pub fn nearest_perimeter_point(
    shape: PerimeterShape,
    bounds: Rect,
    angle: f32,
    aim: Vec2,
) -> PerimeterHit {
    let bounds = bounds.normalized();
    let centre = bounds.center();
    let size = bounds.size;
    // The silhouette is axis-aligned in the element's own frame, so the query
    // is rotated *into* that frame and the answer rotated back out. One
    // rotation each way, rather than a rotated silhouette to search.
    let local = aim.rotated_about(centre, -angle);
    let unit_target = Vec2::new(
        if size.x.abs() > f32::EPSILON {
            (local.x - bounds.origin.x) / size.x
        } else {
            0.5
        },
        if size.y.abs() > f32::EPSILON {
            (local.y - bounds.origin.y) / size.y
        } else {
            0.5
        },
    );

    let (t, unit) = Perimeter::of(shape, size).nearest(unit_target, size);
    PerimeterHit {
        t,
        point: (bounds.origin + unit.scale(size)).rotated_about(centre, angle),
    }
}

/// **§4's floating connection point, kept on the silhouette** — the point of
/// `side` of `bounds` nearest `toward`, clamped to the part of that side the
/// shape actually occupies.
///
/// This is the *cheap* half of the module, and it is deliberately not the
/// search above. It routes a graph edge's whole-node endpoint, which is
/// rebuilt for **both ends of every stale edge**: `flow_graph_bench` routes
/// 500,000 of them in one pass, so the per-endpoint budget here is tens of
/// flops, not the ~7,000 a nearest-point search over four cubics costs. Every
/// answer below is a clamp.
///
/// What each shape contributes is the span of `side` that lies on its outline:
///
/// - a **rectangle** occupies the whole side, so this is the old behaviour
///   exactly, and every route in the crate is unchanged by it;
/// - a **rounded rectangle** occupies the side less a corner radius at each
///   end, so a route aimed into a corner stops where the flat run does instead
///   of entering through the curve that was rounded away — which is the case
///   that actually happens, because
///   [`is_connectable`](crate::runtime::NodeShape::is_connectable) makes a
///   graph node the only kind an edge may attach to and a graph node's body is
///   a rounded rectangle;
/// - an **ellipse** and a **diamond** touch each side at one point, its middle,
///   and that point is on both outlines;
/// - a **triangle** touches its base along the whole width and its other three
///   sides at one corner each.
///
/// A connector endpoint does **not** come through here — it aims at the
/// pointer, not at its partner, and it gets
/// [`nearest_perimeter_point`]'s exact answer.
pub fn floating_perimeter_point(
    shape: PerimeterShape,
    bounds: Rect,
    side: Side,
    toward: Vec2,
) -> Vec2 {
    let bounds = bounds.normalized();
    let (min, max) = (bounds.min(), bounds.max());
    let (w, h) = (bounds.size.x.abs(), bounds.size.y.abs());

    // The span of `side`, as a fraction of it, that lies on the outline.
    let (low, high) = match (shape, side) {
        (PerimeterShape::Rectangle, _) => (0.0, 1.0),
        (PerimeterShape::RoundedRectangle { radius }, _) => {
            let r = radius.clamp(0.0, (w.min(h) * 0.5).max(0.0));
            let extent = if side.is_horizontal() { h } else { w };
            let fraction = if extent > f32::EPSILON {
                (r / extent).clamp(0.0, 0.5)
            } else {
                0.5
            };
            (fraction, 1.0 - fraction)
        }
        (PerimeterShape::Ellipse | PerimeterShape::Diamond, _) => (0.5, 0.5),
        (PerimeterShape::Triangle, Side::Bottom) => (0.0, 1.0),
        (PerimeterShape::Triangle, Side::Top) => (0.5, 0.5),
        // The apex is up, so the left and right sides are touched only at the
        // base corners: the far end of each, in the direction the side runs.
        (PerimeterShape::Triangle, Side::Left | Side::Right) => (1.0, 1.0),
    };

    match side {
        Side::Top | Side::Bottom => {
            let y = if matches!(side, Side::Top) {
                min.y
            } else {
                max.y
            };
            let span = |f: f32| min.x + w * f;
            Vec2::new(
                toward
                    .x
                    .clamp(span(low).min(span(high)), span(low).max(span(high))),
                y,
            )
        }
        Side::Left | Side::Right => {
            let x = if matches!(side, Side::Left) {
                min.x
            } else {
                max.x
            };
            let span = |f: f32| min.y + h * f;
            Vec2::new(
                x,
                toward
                    .y
                    .clamp(span(low).min(span(high)), span(low).max(span(high))),
            )
        }
    }
}

/// Builds a [`Perimeter`] in the unit square, measuring as it goes.
struct Builder {
    segments: [PerimeterSegment; MAX_PERIMETER_SEGMENTS],
    len: usize,
}

impl Builder {
    fn new() -> Builder {
        Builder {
            segments: [PerimeterSegment::line(Vec2::ZERO, Vec2::ZERO); MAX_PERIMETER_SEGMENTS],
            len: 0,
        }
    }

    fn push(&mut self, segment: PerimeterSegment) {
        if self.len < MAX_PERIMETER_SEGMENTS {
            self.segments[self.len] = segment;
            self.len += 1;
        }
    }

    /// A closed polygon: one straight side per vertex, the last one back to the
    /// first.
    fn polygon(&mut self, points: &[Vec2]) {
        for (index, from) in points.iter().enumerate() {
            let to = points[(index + 1) % points.len()];
            self.push(PerimeterSegment::line(*from, to));
        }
    }

    /// A rounded rectangle with per-axis corner radii, starting at the top edge
    /// just past the top-left corner and running clockwise — **the exact order
    /// `render::shapes::rounded_rectangle` draws**, so the two cannot diverge.
    fn rounded_rectangle(&mut self, rx: f32, ry: f32) {
        let (kx, ky) = (rx * KAPPA, ry * KAPPA);
        self.push(PerimeterSegment::line(
            Vec2::new(rx, 0.0),
            Vec2::new(1.0 - rx, 0.0),
        ));
        self.push(PerimeterSegment::cubic(
            Vec2::new(1.0 - rx, 0.0),
            Vec2::new(1.0 - rx + kx, 0.0),
            Vec2::new(1.0, ry - ky),
            Vec2::new(1.0, ry),
        ));
        self.push(PerimeterSegment::line(
            Vec2::new(1.0, ry),
            Vec2::new(1.0, 1.0 - ry),
        ));
        self.push(PerimeterSegment::cubic(
            Vec2::new(1.0, 1.0 - ry),
            Vec2::new(1.0, 1.0 - ry + ky),
            Vec2::new(1.0 - rx + kx, 1.0),
            Vec2::new(1.0 - rx, 1.0),
        ));
        self.push(PerimeterSegment::line(
            Vec2::new(1.0 - rx, 1.0),
            Vec2::new(rx, 1.0),
        ));
        self.push(PerimeterSegment::cubic(
            Vec2::new(rx, 1.0),
            Vec2::new(rx - kx, 1.0),
            Vec2::new(0.0, 1.0 - ry + ky),
            Vec2::new(0.0, 1.0 - ry),
        ));
        self.push(PerimeterSegment::line(
            Vec2::new(0.0, 1.0 - ry),
            Vec2::new(0.0, ry),
        ));
        self.push(PerimeterSegment::cubic(
            Vec2::new(0.0, ry),
            Vec2::new(0.0, ry - ky),
            Vec2::new(rx - kx, 0.0),
            Vec2::new(rx, 0.0),
        ));
    }

    /// The standard four-cubic ellipse, from the top and running clockwise —
    /// again the order `render::shapes::ellipse` draws, and the reason that
    /// function is now four lines.
    fn ellipse(&mut self) {
        let (rx, ry) = (0.5, 0.5);
        let (kx, ky) = (rx * KAPPA, ry * KAPPA);
        let c = Vec2::new(0.5, 0.5);

        self.push(PerimeterSegment::cubic(
            Vec2::new(c.x, c.y - ry),
            Vec2::new(c.x + kx, c.y - ry),
            Vec2::new(c.x + rx, c.y - ky),
            Vec2::new(c.x + rx, c.y),
        ));
        self.push(PerimeterSegment::cubic(
            Vec2::new(c.x + rx, c.y),
            Vec2::new(c.x + rx, c.y + ky),
            Vec2::new(c.x + kx, c.y + ry),
            Vec2::new(c.x, c.y + ry),
        ));
        self.push(PerimeterSegment::cubic(
            Vec2::new(c.x, c.y + ry),
            Vec2::new(c.x - kx, c.y + ry),
            Vec2::new(c.x - rx, c.y + ky),
            Vec2::new(c.x - rx, c.y),
        ));
        self.push(PerimeterSegment::cubic(
            Vec2::new(c.x - rx, c.y),
            Vec2::new(c.x - rx, c.y - ky),
            Vec2::new(c.x - kx, c.y - ry),
            Vec2::new(c.x, c.y - ry),
        ));
    }

    fn finish(self) -> Perimeter {
        let mut ends = [0.0f32; MAX_PERIMETER_SEGMENTS];
        let mut total = 0.0;
        for (end, segment) in ends.iter_mut().zip(&self.segments[..self.len]) {
            total += segment.length(Vec2::ONE);
            *end = total;
        }

        Perimeter {
            segments: self.segments,
            len: self.len as u8,
            ends,
            total,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Perimeter, PerimeterShape, floating_perimeter_point, nearest_perimeter_point,
        perimeter_point,
    };
    use crate::geometry::{Rect, Side, Vec2};

    /// A tenth of a pixel on a shape a couple of hundred pixels across.
    ///
    /// The ellipse and the rounded corners are the standard four-cubic
    /// approximation of an arc, which is within about `2.7e-4` of the radius —
    /// `0.033` at the 120-unit radius used below. An exact comparison would be
    /// asserting the approximation rather than the geometry, and a tolerance
    /// three times its known error is the honest margin.
    const EPSILON: f32 = 0.1;

    fn bounds() -> Rect {
        Rect::new(Vec2::new(100.0, 200.0), Vec2::new(240.0, 160.0))
    }

    fn shapes() -> [PerimeterShape; 5] {
        [
            PerimeterShape::Rectangle,
            PerimeterShape::RoundedRectangle { radius: 24.0 },
            PerimeterShape::Ellipse,
            PerimeterShape::Diamond,
            PerimeterShape::Triangle,
        ]
    }

    /// **An independent answer to "is this on the outline?"** — written from
    /// the shapes' own equations rather than from the module under test, so a
    /// silhouette that is subtly wrong fails here instead of agreeing with
    /// itself.
    fn distance_to_outline(shape: PerimeterShape, bounds: Rect, point: Vec2) -> f32 {
        let (min, max) = (bounds.min(), bounds.max());
        let (w, h) = (bounds.width(), bounds.height());
        let centre = bounds.center();

        let to_segment = |a: Vec2, b: Vec2| {
            let along = b - a;
            let length_squared = along.length_squared();
            let t = if length_squared <= f32::EPSILON {
                0.0
            } else {
                ((point - a).x * along.x + (point - a).y * along.y) / length_squared
            }
            .clamp(0.0, 1.0);
            (a + along * t - point).length()
        };
        let polygon = |points: &[Vec2]| {
            (0..points.len())
                .map(|index| to_segment(points[index], points[(index + 1) % points.len()]))
                .fold(f32::INFINITY, f32::min)
        };

        match shape {
            PerimeterShape::Rectangle => {
                polygon(&[min, Vec2::new(max.x, min.y), max, Vec2::new(min.x, max.y)])
            }
            PerimeterShape::Diamond => polygon(&[
                Vec2::new(centre.x, min.y),
                Vec2::new(max.x, centre.y),
                Vec2::new(centre.x, max.y),
                Vec2::new(min.x, centre.y),
            ]),
            PerimeterShape::Triangle => {
                polygon(&[Vec2::new(centre.x, min.y), max, Vec2::new(min.x, max.y)])
            }
            // The nearest point on an axis-aligned ellipse has no closed form
            // either, so this scans it and then bisects the winning bracket.
            // A scan alone is not good enough: a thousand steps round a
            // 630-unit ellipse leaves half a step — 0.3 — between a point on
            // the curve and the nearest *sample*, which is six times the
            // tolerance being asserted.
            PerimeterShape::Ellipse => {
                let at = |angle: f32| {
                    Vec2::new(
                        centre.x + w * 0.5 * angle.cos(),
                        centre.y + h * 0.5 * angle.sin(),
                    )
                };
                let step = std::f32::consts::TAU / 1_024.0;
                let best = (0..1_024)
                    .map(|index| index as f32 * step)
                    .min_by(|a, b| {
                        (at(*a) - point)
                            .length()
                            .total_cmp(&(at(*b) - point).length())
                    })
                    .unwrap_or(0.0);
                let (mut low, mut high) = (best - step, best + step);
                for _ in 0..48 {
                    let third = (high - low) / 3.0;
                    if (at(low + third) - point).length() < (at(high - third) - point).length() {
                        high -= third;
                    } else {
                        low += third;
                    }
                }
                (at((low + high) * 0.5) - point).length()
            }
            PerimeterShape::RoundedRectangle { radius } => {
                let r = radius.clamp(0.0, w.min(h) * 0.5);
                // The four flat runs, then the four circular corners — the
                // rounded rectangle's actual definition, which is what the
                // unit-space elliptical arcs have to scale back into.
                let flats = [
                    (Vec2::new(min.x + r, min.y), Vec2::new(max.x - r, min.y)),
                    (Vec2::new(max.x, min.y + r), Vec2::new(max.x, max.y - r)),
                    (Vec2::new(max.x - r, max.y), Vec2::new(min.x + r, max.y)),
                    (Vec2::new(min.x, max.y - r), Vec2::new(min.x, min.y + r)),
                ]
                .into_iter()
                .map(|(a, b)| to_segment(a, b))
                .fold(f32::INFINITY, f32::min);
                let corners = [
                    Vec2::new(min.x + r, min.y + r),
                    Vec2::new(max.x - r, min.y + r),
                    Vec2::new(max.x - r, max.y - r),
                    Vec2::new(min.x + r, max.y - r),
                ]
                .into_iter()
                .map(|c| ((point - c).length() - r).abs())
                .fold(f32::INFINITY, f32::min);
                flats.min(corners)
            }
        }
    }

    /// **Every stored parameter is a point on the drawn outline.** Not near it,
    /// not inside it — on it. This is the property that makes the binding a
    /// scalar rather than a point in a box.
    #[test]
    fn every_parameter_lands_on_the_shapes_real_outline() {
        for shape in shapes() {
            for step in 0..256 {
                let t = step as f32 / 256.0;
                let point = perimeter_point(shape, bounds(), 0.0, t);
                let error = distance_to_outline(shape, bounds(), point);
                assert!(
                    error < EPSILON,
                    "{shape:?} at t={t} landed {error} away from its own outline"
                );
            }
        }
    }

    /// A bounding box is not a silhouette, and this is the difference the whole
    /// feature exists for: aim at the top-right corner of the box and a
    /// rectangle binds at the corner, while an ellipse and a diamond bind where
    /// they actually are — well inside it.
    #[test]
    fn a_curved_shape_does_not_bind_to_its_bounding_box() {
        let corner = bounds().max();
        for shape in [PerimeterShape::Ellipse, PerimeterShape::Diamond] {
            let hit = nearest_perimeter_point(shape, bounds(), 0.0, corner);
            assert!(
                (hit.point - corner).length() > 20.0,
                "{shape:?} bound at {:?}, on the box corner",
                hit.point
            );
            assert!(distance_to_outline(shape, bounds(), hit.point) < EPSILON);
        }

        let hit = nearest_perimeter_point(PerimeterShape::Rectangle, bounds(), 0.0, corner);
        assert!((hit.point - corner).length() < EPSILON);
    }

    /// **The round trip the drag depends on**: what the preview showed is what
    /// the release stores, and what a reload resolves is the same point again.
    #[test]
    fn a_binding_resolves_to_the_point_it_was_taken_from() {
        for shape in shapes() {
            for step in 0..64 {
                let angle = step as f32 / 64.0 * std::f32::consts::TAU;
                let aim = bounds().center() + Vec2::new(angle.cos(), angle.sin()) * 300.0;
                let hit = nearest_perimeter_point(shape, bounds(), 0.0, aim);
                let resolved = perimeter_point(shape, bounds(), 0.0, hit.t);
                assert!(
                    (resolved - hit.point).length() < 1e-3,
                    "{shape:?}: stored {} resolved to {resolved:?}, not {:?}",
                    hit.t,
                    hit.point
                );
            }
        }
    }

    /// The aim is honoured: pointing at a spot on the outline binds *there*,
    /// not at the nearest side centre and not at a corner.
    #[test]
    fn the_binding_follows_where_the_pointer_aims() {
        let shape = PerimeterShape::Rectangle;
        let cases = [
            (Vec2::new(160.0, 150.0), Vec2::new(160.0, 200.0)),
            (Vec2::new(400.0, 220.0), Vec2::new(340.0, 220.0)),
            (Vec2::new(300.0, 400.0), Vec2::new(300.0, 360.0)),
            (Vec2::new(60.0, 340.0), Vec2::new(100.0, 340.0)),
        ];
        for (aim, expected) in cases {
            let hit = nearest_perimeter_point(shape, bounds(), 0.0, aim);
            assert!(
                (hit.point - expected).length() < EPSILON,
                "aiming at {aim:?} bound at {:?}, not {expected:?}",
                hit.point
            );
        }
    }

    /// **A rotated node is bound in its own frame.** The parameter taken from a
    /// rotated shape resolves back to the point that was aimed at, and that
    /// point is on the rotated outline — which is the un-rotated outline turned
    /// about the same centre.
    #[test]
    fn a_rotation_is_carried_by_the_resolver_and_not_by_the_parameter() {
        let angle = 0.7;
        for shape in shapes() {
            for step in 0..32 {
                let t = step as f32 / 32.0;
                let upright = perimeter_point(shape, bounds(), 0.0, t);
                let rotated = perimeter_point(shape, bounds(), angle, t);
                assert!(
                    (rotated - upright.rotated_about(bounds().center(), angle)).length() < 1e-3,
                    "{shape:?} at t={t}: rotation is not a rotation"
                );
            }

            // And aiming at a rotated shape finds the rotated outline.
            let aim = Vec2::new(500.0, 500.0);
            let hit = nearest_perimeter_point(shape, bounds(), angle, aim);
            let local = hit.point.rotated_about(bounds().center(), -angle);
            assert!(
                distance_to_outline(shape, bounds(), local) < EPSILON,
                "{shape:?} bound off its rotated outline"
            );
        }
    }

    /// **The requirement, stated as an invariant**: move it, resize it, rotate
    /// it, restyle it — the endpoint stays on the same visible spot, and the
    /// spot is on the outline every time.
    ///
    /// "The same visible spot" is checked as the same *relative* position in
    /// the element's own frame, because that is what following the shape means
    /// when the shape itself has changed size.
    #[test]
    fn a_binding_survives_a_move_a_resize_and_a_rotation() {
        let shape = PerimeterShape::Ellipse;
        let start = bounds();
        let t = nearest_perimeter_point(shape, start, 0.0, Vec2::new(400.0, 210.0)).t;
        let relative = |bounds: Rect, angle: f32| {
            let point =
                perimeter_point(shape, bounds, angle, t).rotated_about(bounds.center(), -angle);
            Vec2::new(
                (point.x - bounds.origin.x) / bounds.width(),
                (point.y - bounds.origin.y) / bounds.height(),
            )
        };

        let original = relative(start, 0.0);
        let cases = [
            (start.translated(Vec2::new(-900.0, 40.0)), 0.0),
            (Rect::new(start.origin, Vec2::new(480.0, 160.0)), 0.0),
            (Rect::new(start.origin, Vec2::new(60.0, 400.0)), 0.0),
            (start, 1.9),
            (
                Rect::new(Vec2::new(-30.0, 12.0), Vec2::new(90.0, 90.0)),
                -0.4,
            ),
        ];
        for (bounds, angle) in cases {
            let moved = relative(bounds, angle);
            assert!(
                (moved - original).length() < 1e-3,
                "{bounds:?} at {angle}: the endpoint slid to {moved:?} from {original:?}"
            );
        }
    }

    /// **A restyle is the case a normalised point in the box gets wrong.**
    /// Rounding the corners of a box an arrow is bound *at* the corner of moves
    /// the outline out from under it; the parameter re-resolves onto the new
    /// one instead.
    #[test]
    fn rounding_the_corners_keeps_the_endpoint_on_the_shape() {
        let square = Rect::new(Vec2::ZERO, Vec2::new(200.0, 200.0));
        let t = nearest_perimeter_point(
            PerimeterShape::Rectangle,
            square,
            0.0,
            Vec2::new(260.0, -60.0),
        )
        .t;
        assert!(
            (perimeter_point(PerimeterShape::Rectangle, square, 0.0, t) - Vec2::new(200.0, 0.0))
                .length()
                < EPSILON,
            "the aim was the top-right corner"
        );

        for radius in [1.0, 12.0, 40.0, 100.0] {
            let rounded = PerimeterShape::RoundedRectangle { radius };
            let point = perimeter_point(rounded, square, 0.0, t);
            assert!(
                distance_to_outline(rounded, square, point) < EPSILON,
                "radius {radius}: the endpoint was left at {point:?}, off the outline"
            );
        }
    }

    /// §4's floating connection point — the cheap answer an *edge* gets — is
    /// also on the outline, for every shape and every side.
    #[test]
    fn the_edge_attachment_never_leaves_the_outline() {
        for shape in shapes() {
            for side in [Side::Top, Side::Right, Side::Bottom, Side::Left] {
                for step in 0..16 {
                    let toward = Vec2::new(-200.0 + step as f32 * 60.0, 150.0 + step as f32 * 40.0);
                    let point = floating_perimeter_point(shape, bounds(), side, toward);
                    assert!(
                        distance_to_outline(shape, bounds(), point) < EPSILON,
                        "{shape:?} {side:?} toward {toward:?} left the outline at {point:?}"
                    );
                }
            }
        }
    }

    /// A rectangle's edge attachment is exactly what it always was, so no route
    /// in the crate moved when the silhouette arrived.
    #[test]
    fn a_rectangles_edge_attachment_is_the_old_clamp() {
        let cases = [
            (Side::Top, Vec2::new(9_000.0, 0.0), Vec2::new(340.0, 200.0)),
            (
                Side::Left,
                Vec2::new(0.0, -9_000.0),
                Vec2::new(100.0, 200.0),
            ),
            (Side::Bottom, Vec2::new(180.0, 0.0), Vec2::new(180.0, 360.0)),
            (Side::Right, Vec2::new(0.0, 300.0), Vec2::new(340.0, 300.0)),
        ];
        for (side, toward, expected) in cases {
            assert_eq!(
                floating_perimeter_point(PerimeterShape::Rectangle, bounds(), side, toward),
                expected
            );
        }
    }

    /// A rounded body's edge attachment stops where the flat run does, so a
    /// route aimed into a graph node's corner does not enter through the curve
    /// that was rounded away.
    #[test]
    fn a_rounded_bodys_edge_attachment_stops_at_the_flat_run() {
        let shape = PerimeterShape::RoundedRectangle { radius: 24.0 };
        assert_eq!(
            floating_perimeter_point(shape, bounds(), Side::Top, Vec2::new(9_000.0, 0.0)),
            Vec2::new(316.0, 200.0),
        );
        assert_eq!(
            floating_perimeter_point(shape, bounds(), Side::Left, Vec2::new(0.0, -9_000.0)),
            Vec2::new(100.0, 224.0),
        );
    }

    /// The corner radius is a *world* length, so the unit silhouette has to be
    /// built against the size it will be scaled by — otherwise a wide box's
    /// corners come back elliptical.
    #[test]
    fn a_corner_stays_circular_however_the_box_is_stretched() {
        let shape = PerimeterShape::RoundedRectangle { radius: 20.0 };
        for size in [
            Vec2::new(400.0, 60.0),
            Vec2::new(60.0, 400.0),
            Vec2::new(120.0, 120.0),
        ] {
            let bounds = Rect::new(Vec2::ZERO, size);
            let centre = Vec2::splat(20.0);
            for step in 0..64 {
                let point = perimeter_point(shape, bounds, 0.0, step as f32 / 64.0);
                if point.x < 20.0 && point.y < 20.0 {
                    assert!(
                        ((point - centre).length() - 20.0).abs() < EPSILON,
                        "{size:?}: the top-left corner is not a circle at {point:?}"
                    );
                }
            }
        }
    }

    /// The parameter wraps, so nothing a file or a migration can put in it
    /// resolves to a point that is not on the shape.
    #[test]
    fn any_parameter_at_all_is_a_point_on_the_outline() {
        let shape = PerimeterShape::Diamond;
        for t in [-3.25, -0.5, 0.0, 1.0, 2.75, 1e6] {
            let point = perimeter_point(shape, bounds(), 0.0, t);
            assert!(
                distance_to_outline(shape, bounds(), point) < EPSILON,
                "t={t} left the outline at {point:?}"
            );
        }
        assert_eq!(
            perimeter_point(shape, bounds(), 0.0, 0.25),
            perimeter_point(shape, bounds(), 0.0, 5.25),
        );
        assert!(perimeter_point(shape, bounds(), 0.0, f32::NAN).is_finite());
    }

    /// A zero-sized element answers its own centre rather than a `NaN` that
    /// would poison every bounds union it later reaches.
    #[test]
    fn a_degenerate_element_answers_a_finite_point() {
        let flat = Rect::new(Vec2::new(5.0, 7.0), Vec2::ZERO);
        for shape in shapes() {
            assert!(perimeter_point(shape, flat, 0.0, 0.3).is_finite());
            assert!(
                nearest_perimeter_point(shape, flat, 0.4, Vec2::new(9.0, 9.0))
                    .point
                    .is_finite()
            );
        }
    }

    /// Eight is the worst case, and it is the rounded rectangle. If a shape
    /// ever needs a ninth the array silently drops it, so this is the tripwire.
    #[test]
    fn no_silhouette_overflows_the_fixed_segment_array() {
        for shape in shapes() {
            let perimeter = Perimeter::of(shape, Vec2::new(120.0, 80.0));
            assert!(!perimeter.segments().is_empty(), "{shape:?} built nothing");
            assert!(perimeter.total_length() > 0.0, "{shape:?} has no length");
            let closed = perimeter.segments().last().unwrap().to;
            let opened = perimeter.segments().first().unwrap().from;
            assert!(
                (closed - opened).length() < 1e-5,
                "{shape:?} is not closed: {opened:?} -> {closed:?}"
            );
        }
    }

    /// The arc-length share each segment gets is what makes a corner claim a
    /// corner's worth of the range rather than an eighth of it.
    #[test]
    fn a_corner_claims_only_its_own_share_of_the_range() {
        let square = Rect::new(Vec2::ZERO, Vec2::new(200.0, 200.0));
        let shape = PerimeterShape::RoundedRectangle { radius: 10.0 };
        // One corner arc is a tenth of a side's length and a quarter-circle of
        // it, so it is a small slice of the whole way round.
        let before = nearest_perimeter_point(shape, square, 0.0, Vec2::new(190.0, -50.0)).t;
        let after = nearest_perimeter_point(shape, square, 0.0, Vec2::new(250.0, 10.0)).t;
        let share = after - before;
        assert!(
            (0.005..0.03).contains(&share),
            "the corner claimed {share} of the perimeter"
        );
    }
}
