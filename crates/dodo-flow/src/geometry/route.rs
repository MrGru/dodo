//! [`EdgeRoute`] — **where an edge actually runs**, derived from where its ends
//! are and never stored with the edge (§8).
//!
//! # The separation this file exists to keep
//!
//! §8: *the data model must separate the logical edge from derived path
//! geometry*. [`EdgeStore`](crate::runtime::EdgeStore) holds the connection;
//! this holds the shape; [`EdgeGeometry`](crate::runtime::EdgeGeometry) holds
//! the shapes and the rule for when one is stale. Nothing here knows what an
//! edge *is* — [`route_into`] takes two [`Attachment`]s and a
//! [`EdgeRouting`], which is why every routing below is asserted from a pair of
//! points with no graph anywhere near it.
//!
//! # World space, and why the routes are not tessellated here
//!
//! A route is world-space cubics and lines. It is flattened into triangles only
//! at paint, in screen space, at the viewport's current zoom — because the
//! flattening tolerance is expressed in *pixels of deviation on the display*
//! ([`RenderQuality`](crate::models::RenderQuality)), so a route tessellated in
//! world units would be too coarse zoomed in and wasteful zoomed out. Keeping
//! the route as a handful of control points also makes the rebuild cheap, which
//! is the operation §19's target counts.
//!
//! # Rebuilding reuses the buffer
//!
//! [`route_into`] refills an existing [`EdgeRoute`] rather than returning a new
//! one. Dragging a node rebuilds its incident routes on every mouse move, and
//! §40 rules 13 and 14 are explicit about allocation on that path; the segment
//! `Vec` is cleared and refilled, so a drag allocates once per edge and never
//! again.
//!
//! **This file names no UI framework.**

use crate::{
    geometry::{CIRCLE_KAPPA, Rect, Vec2, bounds, curve},
    models::EdgeRouting,
};

/// Which side of a node an edge leaves from or arrives at.
///
/// Distinct from [`HandlePlacement`](crate::models::HandlePlacement), which is
/// a *document* fact about where a handle sits; this is a *geometry* fact about
/// which way the route sets off. They coincide for a handle endpoint and do not
/// for a floating one, where the side is chosen from the other end's position —
/// which is why the routing functions take this and never a placement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Side {
    Top,
    #[default]
    Right,
    Bottom,
    Left,
}

impl Side {
    /// The unit vector pointing away from the node.
    pub fn outward(self) -> Vec2 {
        match self {
            Side::Top => Vec2::new(0.0, -1.0),
            Side::Right => Vec2::new(1.0, 0.0),
            Side::Bottom => Vec2::new(0.0, 1.0),
            Side::Left => Vec2::new(-1.0, 0.0),
        }
    }

    pub fn is_horizontal(self) -> bool {
        matches!(self, Side::Left | Side::Right)
    }

    pub fn opposite(self) -> Side {
        match self {
            Side::Top => Side::Bottom,
            Side::Right => Side::Left,
            Side::Bottom => Side::Top,
            Side::Left => Side::Right,
        }
    }

    /// The side of `rect` that faces `toward` — §4's floating connection point.
    ///
    /// Chosen by which face the direction vector leaves through, scaled by the
    /// rectangle's own aspect: a wide node reached from slightly above should
    /// still be entered from the top, and comparing raw dx against dy would
    /// enter it from the side instead.
    pub fn facing(rect: Rect, toward: Vec2) -> Side {
        let rect = rect.normalized();
        let delta = toward - rect.center();
        let half = Vec2::new(rect.width().max(1e-3) * 0.5, rect.height().max(1e-3) * 0.5);
        // Normalising by the half-extents turns the rectangle into a unit
        // square, where "which face" is simply the larger component.
        let normalized = Vec2::new(delta.x / half.x, delta.y / half.y);

        if normalized.x.abs() >= normalized.y.abs() {
            if normalized.x >= 0.0 {
                Side::Right
            } else {
                Side::Left
            }
        } else if normalized.y >= 0.0 {
            Side::Bottom
        } else {
            Side::Top
        }
    }
}

/// Where a route starts or ends, and which way it sets off from there.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Attachment {
    pub point: Vec2,
    pub side: Side,
}

impl Attachment {
    pub fn new(point: Vec2, side: Side) -> Attachment {
        Attachment { point, side }
    }
}

/// One step of a route, from wherever the previous one ended.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RouteSegment {
    Line(Vec2),
    Cubic { c1: Vec2, c2: Vec2, to: Vec2 },
}

impl RouteSegment {
    pub fn end(self) -> Vec2 {
        match self {
            RouteSegment::Line(to) => to,
            RouteSegment::Cubic { to, .. } => to,
        }
    }
}

/// The knobs the five routings share. Fields rather than constants because §8
/// asks for editable waypoints and per-edge routing behaviour later, and a
/// value that is already a parameter does not have to be lifted out of a
/// function body when it does.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RouteOptions {
    /// How far a step or smooth-step route runs straight out of its handle
    /// before turning, in world units.
    pub step_offset: f32,
    /// The corner radius of a smooth-step route, in world units. Clamped per
    /// corner to half the shorter adjacent leg, so a tight route rounds less
    /// rather than overshooting.
    pub corner_radius: f32,
    /// How far a Bézier bows when its target is *behind* its source. React
    /// Flow's own coefficient, kept because the shape it produces is the one
    /// users of that library expect.
    pub curvature: f32,
}

impl RouteOptions {
    pub const DEFAULT: RouteOptions = RouteOptions {
        step_offset: 20.0,
        corner_radius: 8.0,
        curvature: 0.25,
    };
}

impl Default for RouteOptions {
    fn default() -> RouteOptions {
        RouteOptions::DEFAULT
    }
}

/// A derived edge path, in world space.
///
/// Construct with [`route`] or refill with [`route_into`]; the fields are
/// readable and the segments are not, because a route that could be edited in
/// place would be a route that no longer follows from its endpoints.
#[derive(Debug, Clone, PartialEq)]
pub struct EdgeRoute {
    routing: EdgeRouting,
    start: Vec2,
    end: Vec2,
    start_side: Side,
    end_side: Side,
    /// Unit vector, the direction the route leaves the source in.
    start_tangent: Vec2,
    /// Unit vector, the direction the route is travelling as it reaches the
    /// target — what a target arrow marker points along.
    end_tangent: Vec2,
    segments: Vec<RouteSegment>,
    bounds: Rect,
}

impl Default for EdgeRoute {
    fn default() -> EdgeRoute {
        EdgeRoute {
            routing: EdgeRouting::Straight,
            start: Vec2::ZERO,
            end: Vec2::ZERO,
            start_side: Side::Right,
            end_side: Side::Left,
            start_tangent: Vec2::new(1.0, 0.0),
            end_tangent: Vec2::new(1.0, 0.0),
            segments: Vec::new(),
            bounds: Rect::ZERO,
        }
    }
}

impl EdgeRoute {
    pub fn routing(&self) -> EdgeRouting {
        self.routing
    }

    pub fn start(&self) -> Vec2 {
        self.start
    }

    pub fn end(&self) -> Vec2 {
        self.end
    }

    pub fn start_side(&self) -> Side {
        self.start_side
    }

    pub fn end_side(&self) -> Side {
        self.end_side
    }

    /// The direction the route leaves its source in. A **source** arrow marker
    /// points along the negative of this — back down the way the edge came.
    pub fn start_tangent(&self) -> Vec2 {
        self.start_tangent
    }

    /// The direction the route is travelling when it arrives. A **target**
    /// arrow marker points along this.
    pub fn end_tangent(&self) -> Vec2 {
        self.end_tangent
    }

    pub fn segments(&self) -> &[RouteSegment] {
        &self.segments
    }

    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }

    /// The route's bounding box, **control points included**, so it is a true
    /// bound rather than a tight one. A cubic never leaves its control hull,
    /// and a cull test wants a bound it can trust: a false "visible" costs one
    /// wasted path, a false "hidden" is a missing edge.
    pub fn bounds(&self) -> Rect {
        self.bounds
    }

    /// The polyline the route reduces to when its curves are ignored — the
    /// coarse form a hit test starts from, and the overview-LOD form.
    pub fn corner_points(&self) -> impl Iterator<Item = Vec2> + '_ {
        std::iter::once(self.start).chain(self.segments.iter().map(|segment| segment.end()))
    }

    /// Walks the route as a world-space polyline at `tolerance`, calling
    /// `point` for the start and then for every flattened vertex.
    ///
    /// The narrow phase for anything that asks a geometric question about an
    /// edge — §28's box selection today, §29's edge hit test next. It allocates
    /// nothing: the caller decides what to do with each point.
    pub fn for_each_point(&self, tolerance: f32, mut point: impl FnMut(Vec2)) {
        let mut cursor = self.start;
        point(cursor);

        for segment in &self.segments {
            match *segment {
                RouteSegment::Line(to) => {
                    point(to);
                    cursor = to;
                }
                RouteSegment::Cubic { c1, c2, to } => {
                    curve::flatten_cubic(cursor, c1, c2, to, tolerance, &mut point);
                    cursor = to;
                }
            }
        }
    }

    /// **The point halfway along the route, by arc length** — where §9's edge
    /// label goes.
    ///
    /// By length rather than by parameter, and the difference is visible on
    /// every routing but `Straight`: a Bézier's `t = 0.5` sits wherever the
    /// control points put it, and a step route's is on whichever leg happens to
    /// be the second of three. Halfway along the *drawn* line is where a reader
    /// looks for a label, and it is the only definition that agrees with itself
    /// across the five routings.
    ///
    /// Derived every frame rather than stored, which is the same decision
    /// [`FlowEdge`](crate::models::FlowEdge) makes about the route itself: a
    /// cached midpoint is one more thing that has to be invalidated when an
    /// endpoint moves, and this costs one walk of a polyline the route already
    /// knows how to produce.
    pub fn midpoint(&self, tolerance: f32) -> Vec2 {
        // Two passes over the same flattening rather than one pass into a
        // `Vec`: this runs per labelled visible edge per frame, and §40 rule 14
        // is about exactly this allocation. The flattening is deterministic, so
        // the second walk sees the same points as the first.
        let mut total = 0.0;
        let mut previous: Option<Vec2> = None;
        self.for_each_point(tolerance, |point| {
            if let Some(from) = previous {
                total += (point - from).length();
            }
            previous = Some(point);
        });

        if total <= 0.0 {
            // A degenerate route — zero length, or a single point — has its
            // midpoint at its start. Not an error: two handles in the same
            // place is an ordinary transient while a node is being dragged.
            return self.start;
        }

        let half = total * 0.5;
        let mut walked = 0.0;
        let mut found = self.end;
        let mut previous: Option<Vec2> = None;
        let mut done = false;
        self.for_each_point(tolerance, |point| {
            if done {
                return;
            }
            if let Some(from) = previous {
                let length = (point - from).length();
                if walked + length >= half {
                    let along = if length > 0.0 {
                        (half - walked) / length
                    } else {
                        0.0
                    };
                    found = from + (point - from) * along;
                    done = true;
                    return;
                }
                walked += length;
            }
            previous = Some(point);
        });

        found
    }

    /// **The distance from `point` to the route's drawn line**, in world units
    /// — §29's narrow phase for an edge.
    ///
    /// The control hull rejects first, exactly as [`intersects_rect`](EdgeRoute::intersects_rect)
    /// does and for the same reason: it is one rectangle test and it is right
    /// for almost every edge. The hull is inflated by the tolerance before the
    /// test, because a point just outside a hull can still be within grabbing
    /// distance of the curve inside it.
    ///
    /// `None` means "not within `tolerance`", rather than a distance the caller
    /// then has to compare — so the early rejection is expressible and a caller
    /// cannot forget the comparison.
    pub fn distance_to_point(&self, point: Vec2, tolerance: f32, flatten: f32) -> Option<f32> {
        if !self.bounds.inflate(tolerance).contains_point(point) {
            return None;
        }

        let mut best = f32::INFINITY;
        let mut previous: Option<Vec2> = None;
        self.for_each_point(flatten, |vertex| {
            if let Some(from) = previous {
                best = best.min(distance_to_segment(point, from, vertex));
            }
            previous = Some(vertex);
        });

        // A route with no segments is its start point, which is still a thing
        // a user can aim at while an edge is being formed.
        if self.segments.is_empty() {
            best = best.min((point - self.start).length());
        }

        (best <= tolerance).then_some(best)
    }

    /// **§28's exact test**: whether the route's *curve* passes through `rect`.
    ///
    /// The control hull ([`bounds`](EdgeRoute::bounds)) rejects first, because
    /// it is one rectangle test and it is right for the overwhelming majority
    /// of edges. Only what survives is flattened — a Bézier that bows far away
    /// from the rectangle its hull overlaps is exactly the case a hull-only
    /// test would select wrongly, and a rubber band that grabs edges it visibly
    /// misses is the kind of wrongness a user reports as "selection is broken".
    pub fn intersects_rect(&self, rect: Rect, tolerance: f32) -> bool {
        let rect = rect.normalized();
        if !self.bounds.intersects(rect) {
            return false;
        }

        let mut previous: Option<Vec2> = None;
        let mut hit = false;
        self.for_each_point(tolerance, |point| {
            if hit {
                return;
            }
            if let Some(from) = previous
                && bounds::segment_intersects_rect(from, point, rect)
            {
                hit = true;
            }
            previous = Some(point);
        });

        // A route with no segments at all is a point, and its start is the
        // only thing there is to test.
        hit || (self.segments.is_empty() && rect.contains_point(self.start))
    }
}

/// The distance from `point` to the segment `a`–`b`.
///
/// Free-standing and here rather than in `geometry::bounds` beside
/// [`segment_intersects_rect`](crate::geometry::segment_intersects_rect),
/// because a route is the only thing that asks it and a distance is not a
/// bounds question. Degenerate segments (`a == b`) answer the distance to the
/// point, which is what a zero-length leg of a step route should give.
fn distance_to_segment(point: Vec2, a: Vec2, b: Vec2) -> f32 {
    let span = b - a;
    let length_squared = span.x * span.x + span.y * span.y;
    if length_squared <= f32::EPSILON {
        return (point - a).length();
    }

    let offset = point - a;
    let along = ((offset.x * span.x + offset.y * span.y) / length_squared).clamp(0.0, 1.0);
    (point - (a + span * along)).length()
}

/// Builds the route for one edge into a fresh [`EdgeRoute`].
pub fn route(
    routing: EdgeRouting,
    source: Attachment,
    target: Attachment,
    options: &RouteOptions,
) -> EdgeRoute {
    let mut route = EdgeRoute::default();
    route_into(&mut route, routing, source, target, options);
    route
}

/// **Rebuilds `route` in place**, reusing its segment buffer. See the module
/// doc for why the reuse is the default rather than the optimisation.
pub fn route_into(
    route: &mut EdgeRoute,
    routing: EdgeRouting,
    source: Attachment,
    target: Attachment,
    options: &RouteOptions,
) {
    route.routing = routing;
    route.start = source.point;
    route.end = target.point;
    route.start_side = source.side;
    route.end_side = target.side;
    route.segments.clear();

    match routing {
        EdgeRouting::Straight => straight(route, source, target),
        EdgeRouting::Bezier => bezier(route, source, target, options),
        EdgeRouting::SimpleBezier => simple_bezier(route, source, target),
        EdgeRouting::Step => step(route, source, target, options, 0.0),
        EdgeRouting::SmoothStep => step(route, source, target, options, options.corner_radius),
    }

    finish(route, source, target);
}

// ---- the five routings ---------------------------------------------------

fn straight(route: &mut EdgeRoute, _source: Attachment, target: Attachment) {
    route.segments.push(RouteSegment::Line(target.point));
}

/// §8's `Bezier`: control points pushed out along each end's own side, by a
/// distance that grows with how far the other end is in that direction.
///
/// The two-case offset is React Flow's. When the target is *ahead* of the
/// source in the source's own direction, half the gap gives the familiar gentle
/// S. When it is behind — a right handle connecting to something to its left —
/// half the gap would be negative and the curve would fold back through the
/// node, so the offset grows with the square root of the overshoot instead:
/// enough to bow clear, sublinear so a distant backwards target does not
/// produce an absurd loop.
fn bezier(route: &mut EdgeRoute, source: Attachment, target: Attachment, options: &RouteOptions) {
    let c1 = source.point + source.side.outward() * control_offset(source, target.point, options);
    let c2 = target.point + target.side.outward() * control_offset(target, source.point, options);

    route.segments.push(RouteSegment::Cubic {
        c1,
        c2,
        to: target.point,
    });
}

fn control_offset(from: Attachment, toward: Vec2, options: &RouteOptions) -> f32 {
    let delta = toward - from.point;
    let outward = from.side.outward();
    // How far `toward` lies in the direction this end sets off in. Negative
    // means the route has to double back.
    let distance = delta.x * outward.x + delta.y * outward.y;

    if distance >= 0.0 {
        0.5 * distance
    } else {
        options.curvature * 25.0 * (-distance).sqrt()
    }
}

/// §8's `SimpleBezier`: **the sides are deliberately not consulted**.
///
/// The control points come from the endpoints alone, on whichever axis the two
/// are further apart, which is what makes this the routing to use when the
/// endpoints have no meaningful handle direction — a floating connection, or an
/// edge being dragged out to nowhere in particular.
fn simple_bezier(route: &mut EdgeRoute, source: Attachment, target: Attachment) {
    let delta = target.point - source.point;

    let (c1, c2) = if delta.x.abs() >= delta.y.abs() {
        let half = delta.x * 0.5;
        (
            Vec2::new(source.point.x + half, source.point.y),
            Vec2::new(target.point.x - half, target.point.y),
        )
    } else {
        let half = delta.y * 0.5;
        (
            Vec2::new(source.point.x, source.point.y + half),
            Vec2::new(target.point.x, target.point.y - half),
        )
    };

    route.segments.push(RouteSegment::Cubic {
        c1,
        c2,
        to: target.point,
    });
}

/// §8's `Step` (`radius == 0.0`) and `SmoothStep` (`radius > 0.0`).
///
/// One function for both because they are one route with two corner
/// treatments — and because the alternative, two functions, is two places for
/// the leg maths to be wrong in different ways.
fn step(
    route: &mut EdgeRoute,
    source: Attachment,
    target: Attachment,
    options: &RouteOptions,
    radius: f32,
) {
    let points = orthogonal_points(source, target, options.step_offset);

    if radius <= 0.0 {
        for point in points.iter().skip(1) {
            route.segments.push(RouteSegment::Line(*point));
        }
        return;
    }

    // Every interior point is a corner: run up to it, round it, carry on.
    for index in 1..points.len() {
        let previous = points[index - 1];
        let current = points[index];

        let Some(next) = points.get(index + 1).copied() else {
            route.segments.push(RouteSegment::Line(current));
            break;
        };

        let incoming = current - previous;
        let outgoing = next - current;
        let in_length = incoming.length();
        let out_length = outgoing.length();

        if in_length <= f32::EPSILON || out_length <= f32::EPSILON {
            continue;
        }

        let r = radius.min(in_length * 0.5).min(out_length * 0.5);
        let u = incoming / in_length;
        let v = outgoing / out_length;

        let corner_start = current - u * r;
        let corner_end = current + v * r;

        route.segments.push(RouteSegment::Line(corner_start));
        route.segments.push(RouteSegment::Cubic {
            // A quarter-circle's control points, pulled toward the corner by
            // `CIRCLE_KAPPA` — the same construction the rounded rectangle
            // uses, which is why the constant lives in `geometry`.
            c1: corner_start + u * (r * CIRCLE_KAPPA),
            c2: corner_end - v * (r * CIRCLE_KAPPA),
            to: corner_end,
        });
    }
}

/// The corner points of an orthogonal route, source first and target last.
///
/// Both ends leave along their own side for `offset`, and the middle is one of
/// three shapes depending on how the two sides relate: a Z when they face along
/// the same axis, an L when they face along different ones. Collinear and
/// coincident points are dropped at the end, so a straight run does not arrive
/// as five identical corners.
fn orthogonal_points(source: Attachment, target: Attachment, offset: f32) -> Vec<Vec2> {
    let start = source.point;
    let end = target.point;
    let start_stub = start + source.side.outward() * offset;
    let end_stub = end + target.side.outward() * offset;

    let mut points = Vec::with_capacity(6);
    points.push(start);
    points.push(start_stub);

    match (source.side.is_horizontal(), target.side.is_horizontal()) {
        (true, true) => {
            let mid = (start_stub.x + end_stub.x) * 0.5;
            points.push(Vec2::new(mid, start_stub.y));
            points.push(Vec2::new(mid, end_stub.y));
        }
        (false, false) => {
            let mid = (start_stub.y + end_stub.y) * 0.5;
            points.push(Vec2::new(start_stub.x, mid));
            points.push(Vec2::new(end_stub.x, mid));
        }
        (true, false) => points.push(Vec2::new(end_stub.x, start_stub.y)),
        (false, true) => points.push(Vec2::new(start_stub.x, end_stub.y)),
    }

    points.push(end_stub);
    points.push(end);
    simplify(points)
}

/// Drops coincident and collinear points. Both matter: a coincident pair is a
/// zero-length leg that a corner radius would divide by, and a collinear
/// triple is a corner that would be rounded for no reason.
fn simplify(points: Vec<Vec2>) -> Vec<Vec2> {
    const EPSILON: f32 = 1e-4;

    let mut out: Vec<Vec2> = Vec::with_capacity(points.len());
    for point in points {
        if let Some(&last) = out.last()
            && (point - last).length() <= EPSILON
        {
            continue;
        }
        out.push(point);
    }

    let mut index = 1;
    while index + 1 < out.len() {
        let before = out[index - 1];
        let current = out[index];
        let after = out[index + 1];
        let a = current - before;
        let b = after - current;
        // The cross product of two collinear steps is zero; the dot product
        // being positive keeps a genuine reversal (a doubling-back stub) from
        // being flattened away with it.
        if (a.x * b.y - a.y * b.x).abs() <= EPSILON && a.x * b.x + a.y * b.y > 0.0 {
            out.remove(index);
        } else {
            index += 1;
        }
    }

    out
}

// ---- shared finishing ----------------------------------------------------

/// Fills in the tangents and the bounds once the segments exist.
fn finish(route: &mut EdgeRoute, source: Attachment, target: Attachment) {
    route.start_tangent = start_tangent(route).unwrap_or_else(|| source.side.outward());
    route.end_tangent = end_tangent(route).unwrap_or_else(|| target.side.opposite().outward());
    route.bounds = compute_bounds(route);
}

fn start_tangent(route: &EdgeRoute) -> Option<Vec2> {
    let first = *route.segments.first()?;
    let direction = match first {
        RouteSegment::Line(to) => to - route.start,
        // The tangent of a cubic at t = 0 is toward its first control point —
        // unless that control point sits on the start, in which case the curve
        // sets off toward the second.
        RouteSegment::Cubic { c1, c2, to } => {
            let first_leg = c1 - route.start;
            if first_leg.length() > f32::EPSILON {
                first_leg
            } else {
                let second_leg = c2 - route.start;
                if second_leg.length() > f32::EPSILON {
                    second_leg
                } else {
                    to - route.start
                }
            }
        }
    };

    normalize(direction)
}

fn end_tangent(route: &EdgeRoute) -> Option<Vec2> {
    let last = *route.segments.last()?;
    let previous = if route.segments.len() >= 2 {
        route.segments[route.segments.len() - 2].end()
    } else {
        route.start
    };

    let direction = match last {
        RouteSegment::Line(to) => to - previous,
        RouteSegment::Cubic { c1, c2, to } => {
            let last_leg = to - c2;
            if last_leg.length() > f32::EPSILON {
                last_leg
            } else {
                let earlier = to - c1;
                if earlier.length() > f32::EPSILON {
                    earlier
                } else {
                    to - previous
                }
            }
        }
    };

    normalize(direction)
}

fn normalize(v: Vec2) -> Option<Vec2> {
    let length = v.length();
    if length.is_finite() && length > f32::EPSILON {
        Some(v / length)
    } else {
        None
    }
}

fn compute_bounds(route: &EdgeRoute) -> Rect {
    let mut points = Vec::with_capacity(route.segments.len() * 3 + 1);
    points.push(route.start);
    for segment in &route.segments {
        match *segment {
            RouteSegment::Line(to) => points.push(to),
            RouteSegment::Cubic { c1, c2, to } => {
                points.push(c1);
                points.push(c2);
                points.push(to);
            }
        }
    }

    Rect::of_points(points).unwrap_or_else(|| Rect::new(route.start, Vec2::ZERO))
}

#[cfg(test)]
mod tests {
    use super::{
        Attachment, EdgeRoute, RouteOptions, RouteSegment, Side, orthogonal_points, route,
        route_into,
    };
    use crate::{
        geometry::{Rect, Vec2},
        models::EdgeRouting,
    };

    fn options() -> RouteOptions {
        RouteOptions::DEFAULT
    }

    fn from(x: f32, y: f32) -> Attachment {
        Attachment::new(Vec2::new(x, y), Side::Right)
    }

    fn to(x: f32, y: f32) -> Attachment {
        Attachment::new(Vec2::new(x, y), Side::Left)
    }

    fn routed(routing: EdgeRouting) -> EdgeRoute {
        route(routing, from(0.0, 0.0), to(200.0, 100.0), &options())
    }

    /// Whatever the routing, the route runs between the two attachment points.
    /// The property every consumer relies on and the one a clever router is
    /// most likely to break.
    #[test]
    fn every_routing_starts_and_ends_where_it_was_told_to() {
        for routing in [
            EdgeRouting::Straight,
            EdgeRouting::Bezier,
            EdgeRouting::SimpleBezier,
            EdgeRouting::Step,
            EdgeRouting::SmoothStep,
        ] {
            let route = routed(routing);

            assert_eq!(route.start(), Vec2::new(0.0, 0.0), "{routing:?}");
            assert_eq!(route.end(), Vec2::new(200.0, 100.0), "{routing:?}");
            assert_eq!(
                route.segments().last().map(|s| s.end()),
                Some(Vec2::new(200.0, 100.0)),
                "{routing:?}"
            );
            assert!(!route.is_empty(), "{routing:?}");
        }
    }

    #[test]
    fn a_straight_route_is_one_line_and_nothing_else() {
        let route = routed(EdgeRouting::Straight);

        assert_eq!(
            route.segments(),
            &[RouteSegment::Line(Vec2::new(200.0, 100.0))]
        );
        assert_eq!(
            route.bounds(),
            Rect::new(Vec2::new(0.0, 0.0), Vec2::new(200.0, 100.0))
        );
    }

    /// A Bézier's control points leave along each end's own side — the property
    /// that distinguishes it from a simple Bézier, and the reason it looks
    /// right against a handle.
    #[test]
    fn a_bezier_leaves_along_the_side_it_was_given() {
        let route = routed(EdgeRouting::Bezier);

        let [RouteSegment::Cubic { c1, c2, .. }] = route.segments() else {
            panic!("a bezier is one cubic: {:?}", route.segments());
        };

        // Source faces right, so its control point is to the right of it.
        assert!(c1.x > route.start().x, "{c1:?}");
        assert_eq!(c1.y, route.start().y);
        // Target faces left, so its control point is to the left of it.
        assert!(c2.x < route.end().x, "{c2:?}");
        assert_eq!(c2.y, route.end().y);
    }

    /// The backwards case: a right-facing source connecting to something to its
    /// left must still bow outward rather than folding back through the node.
    #[test]
    fn a_backwards_bezier_still_bows_away_from_its_node() {
        let route = route(
            EdgeRouting::Bezier,
            from(0.0, 0.0),
            to(-200.0, 40.0),
            &options(),
        );

        let [RouteSegment::Cubic { c1, c2, .. }] = route.segments() else {
            panic!("a bezier is one cubic");
        };

        assert!(c1.x > 0.0, "the source control point stays to the right");
        assert!(c2.x < -200.0, "the target control point stays to its left");
    }

    /// §8 and `EdgeRouting::SimpleBezier`'s own doc: the handle directions are
    /// not consulted. Two routes with opposite sides must be identical.
    #[test]
    fn a_simple_bezier_ignores_the_sides_entirely() {
        let a = route(
            EdgeRouting::SimpleBezier,
            Attachment::new(Vec2::ZERO, Side::Right),
            Attachment::new(Vec2::new(200.0, 0.0), Side::Left),
            &options(),
        );
        let b = route(
            EdgeRouting::SimpleBezier,
            Attachment::new(Vec2::ZERO, Side::Top),
            Attachment::new(Vec2::new(200.0, 0.0), Side::Bottom),
            &options(),
        );

        assert_eq!(a.segments(), b.segments());
    }

    #[test]
    fn a_simple_bezier_bows_along_the_longer_axis() {
        let horizontal = route(
            EdgeRouting::SimpleBezier,
            from(0.0, 0.0),
            to(200.0, 20.0),
            &options(),
        );
        let vertical = route(
            EdgeRouting::SimpleBezier,
            from(0.0, 0.0),
            to(20.0, 200.0),
            &options(),
        );

        let [RouteSegment::Cubic { c1, .. }] = horizontal.segments() else {
            panic!("one cubic");
        };
        assert_eq!(*c1, Vec2::new(100.0, 0.0));

        let [RouteSegment::Cubic { c1, .. }] = vertical.segments() else {
            panic!("one cubic");
        };
        assert_eq!(*c1, Vec2::new(0.0, 100.0));
    }

    /// A step route is axis-aligned everywhere: every leg moves in x or in y,
    /// never in both. The defining property of the routing.
    #[test]
    fn every_leg_of_a_step_route_is_axis_aligned() {
        let route = routed(EdgeRouting::Step);

        let mut previous = route.start();
        for segment in route.segments() {
            let RouteSegment::Line(to) = *segment else {
                panic!("a step route has no curves: {segment:?}");
            };
            let delta = to - previous;
            assert!(
                delta.x.abs() < 1e-4 || delta.y.abs() < 1e-4,
                "leg {previous:?} -> {to:?} is diagonal"
            );
            previous = to;
        }
    }

    /// A smooth-step route is the same route with rounded corners: the corners
    /// become cubics, and the straight legs stay straight.
    #[test]
    fn a_smooth_step_route_rounds_the_corners_of_the_step_route() {
        let step = routed(EdgeRouting::Step);
        let smooth = routed(EdgeRouting::SmoothStep);

        let corners = step.segments().len() - 1;
        let curves = smooth
            .segments()
            .iter()
            .filter(|segment| matches!(segment, RouteSegment::Cubic { .. }))
            .count();

        assert!(corners > 0, "the fixture must actually turn");
        assert_eq!(curves, corners);
        assert_eq!(smooth.start(), step.start());
        assert_eq!(smooth.end(), step.end());
    }

    /// The corner radius is clamped per corner, so a route whose legs are
    /// shorter than the radius rounds less rather than overshooting its own
    /// corner — which would put the curve outside the route's bounds.
    #[test]
    fn a_tight_smooth_step_rounds_less_rather_than_overshooting() {
        let mut tight = RouteOptions::DEFAULT;
        tight.corner_radius = 500.0;

        let route = route(
            EdgeRouting::SmoothStep,
            from(0.0, 0.0),
            to(60.0, 30.0),
            &tight,
        );

        let mut previous = route.start();
        for segment in route.segments() {
            if let RouteSegment::Cubic { to, .. } = *segment {
                let leg = (to - previous).length();
                assert!(leg <= 60.0, "a rounded corner ran {leg} past its legs");
            }
            previous = segment.end();
        }
    }

    /// A source and target on the same axis with nothing to route around must
    /// not produce a stack of coincident corners.
    #[test]
    fn a_collinear_step_route_collapses_to_one_leg() {
        let points = orthogonal_points(from(0.0, 0.0), to(200.0, 0.0), 20.0);

        assert_eq!(points, vec![Vec2::new(0.0, 0.0), Vec2::new(200.0, 0.0)]);
    }

    #[test]
    fn the_tangents_point_the_way_the_route_travels() {
        let straight = route(
            EdgeRouting::Straight,
            from(0.0, 0.0),
            to(100.0, 0.0),
            &options(),
        );

        assert!((straight.start_tangent() - Vec2::new(1.0, 0.0)).length() < 1e-5);
        assert!((straight.end_tangent() - Vec2::new(1.0, 0.0)).length() < 1e-5);

        let down = route(
            EdgeRouting::Straight,
            Attachment::new(Vec2::ZERO, Side::Bottom),
            Attachment::new(Vec2::new(0.0, 100.0), Side::Top),
            &options(),
        );
        assert!((down.end_tangent() - Vec2::new(0.0, 1.0)).length() < 1e-5);
    }

    /// A degenerate route — both ends in the same place — must produce finite
    /// tangents rather than a `NaN` that poisons every arrow marker built from
    /// it.
    #[test]
    fn a_zero_length_route_still_has_finite_tangents() {
        for routing in [
            EdgeRouting::Straight,
            EdgeRouting::Bezier,
            EdgeRouting::SimpleBezier,
            EdgeRouting::Step,
            EdgeRouting::SmoothStep,
        ] {
            let route = route(
                routing,
                Attachment::new(Vec2::new(10.0, 10.0), Side::Right),
                Attachment::new(Vec2::new(10.0, 10.0), Side::Left),
                &options(),
            );

            assert!(route.start_tangent().is_finite(), "{routing:?}");
            assert!(route.end_tangent().is_finite(), "{routing:?}");
            assert!(route.bounds().is_finite(), "{routing:?}");
        }
    }

    /// The bounds contain the control points, not only the ends — a Bézier
    /// bowing backwards leaves the box its endpoints span.
    #[test]
    fn the_bounds_contain_the_control_points() {
        let route = route(
            EdgeRouting::Bezier,
            from(0.0, 0.0),
            to(-100.0, 0.0),
            &options(),
        );

        assert!(
            route.bounds().width() > 100.0,
            "a backwards bezier is wider than its endpoints: {:?}",
            route.bounds()
        );
    }

    /// The rebuild path a drag takes: refilling a route allocates nothing and
    /// produces exactly what a fresh one would.
    #[test]
    fn rebuilding_in_place_matches_a_fresh_route_and_reuses_the_buffer() {
        let mut reused = route(
            EdgeRouting::SmoothStep,
            from(0.0, 0.0),
            to(300.0, 200.0),
            &options(),
        );
        let capacity = reused.segments.capacity();

        for step in 1..20 {
            route_into(
                &mut reused,
                EdgeRouting::SmoothStep,
                from(0.0, 0.0),
                to(300.0 + step as f32, 200.0),
                &options(),
            );
        }

        let fresh = route(
            EdgeRouting::SmoothStep,
            from(0.0, 0.0),
            to(319.0, 200.0),
            &options(),
        );

        assert_eq!(reused, fresh);
        assert_eq!(
            reused.segments.capacity(),
            capacity,
            "a drag must not reallocate per frame"
        );
    }

    #[test]
    fn a_floating_attachment_faces_the_side_it_is_approached_from() {
        let node = Rect::new(Vec2::new(0.0, 0.0), Vec2::new(200.0, 40.0));

        assert_eq!(Side::facing(node, Vec2::new(500.0, 20.0)), Side::Right);
        assert_eq!(Side::facing(node, Vec2::new(-500.0, 20.0)), Side::Left);
        assert_eq!(Side::facing(node, Vec2::new(100.0, -500.0)), Side::Top);
        assert_eq!(Side::facing(node, Vec2::new(100.0, 500.0)), Side::Bottom);
    }

    /// A wide node approached from just above must be entered from the top.
    /// Comparing raw dx against dy would enter it from the side, which is the
    /// bug the half-extent normalisation exists to prevent.
    #[test]
    fn a_wide_node_approached_from_above_is_entered_from_the_top() {
        let wide = Rect::new(Vec2::new(0.0, 0.0), Vec2::new(400.0, 40.0));

        assert_eq!(Side::facing(wide, Vec2::new(120.0, -60.0)), Side::Top);
    }

    #[test]
    fn the_sides_know_their_own_geometry() {
        assert_eq!(Side::Top.outward(), Vec2::new(0.0, -1.0));
        assert_eq!(Side::Left.opposite(), Side::Right);
        assert!(Side::Right.is_horizontal());
        assert!(!Side::Bottom.is_horizontal());
    }
}
