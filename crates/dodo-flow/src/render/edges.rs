//! Turning an [`EdgeRoute`] into primitives: the line, and its two markers.
//!
//! # Where the world→screen step happens, and why it is here
//!
//! A route is world-space control points; a [`PaintPlan`] is pane-relative
//! screen pixels. This file is the crossing, and it is one function deep so the
//! transform is applied in exactly one place — §22 forbids scattering the
//! formula, and an edge painted through a second copy of it would be an edge
//! that drifted from its node at high zoom.
//!
//! Tessellating in screen space is also what makes the flattening tolerance
//! mean what it says: `RenderQuality` is in *pixels of deviation on the
//! display*, so the curve is flattened after the zoom has been applied, not
//! before.
//!
//! # What is a path and what is a quad
//!
//! The line is a path — it is a genuine curve or a diagonal, which is what
//! paths are for. A **dot marker is a quad**, because a circle is a quad with a
//! corner radius of half its side and Phase 0 measured a quad at twice the
//! throughput of the equivalent path. The arrow, triangle and diamond markers
//! are paths: they are neither axis-aligned nor round.
//!
//! # This file plans, it does not paint
//!
//! Everything goes into the plan's typed buckets, so the paint-order contract
//! holds however many edges are pushed and in whatever order — see
//! [`crate::render::plan`]. In particular a dot marker being a quad does **not**
//! interleave a quad between two paths; it lands in the quad bucket and is
//! emitted with the rest of them.
//!
//! **This file names no UI framework.**

use crate::{
    geometry::{
        ArrowGeometry, EdgeRoute, RouteSegment, Vec2, Viewport,
        arrow::{self, ArrowPolygon},
    },
    models::{ArrowMarker, Color, EdgeIndex, RenderQuality},
    render::{
        Outline, PaintPlan,
        cache::{GeometryKey, GeometryPart},
        lod::EdgeDetail,
        plan::{DashSpec, PathPrimitive, QuadPrimitive},
    },
};

/// The thinnest an edge is drawn, in screen pixels.
///
/// Zoomed far out, a world-space stroke width scales below one pixel and lyon
/// tessellates a stroke that is there but invisible — paying the vertices for
/// an edge nobody can see. Clamping keeps the graph legible at overview zoom,
/// which is the zoom at which the edges are what the picture *is*.
pub const MIN_STROKE_PIXELS: f32 = 0.75;

/// Everything the painter needs to know about how one edge looks.
///
/// Widths and marker sizes are in **world** units, matching
/// [`StrokeStyle::width`](crate::models::StrokeStyle::width); this module
/// converts. Colours are already resolved against the theme, because
/// `models/` may not name one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EdgePaint {
    pub color: Color,
    /// World units.
    pub width: f32,
    /// World units. `None` is a solid line — and it is worth keeping `None`
    /// rather than a zero-length pattern, because the dashed path costs 63× the
    /// vertices (see [`crate::render::plan::PathPaint`]).
    pub dash: Option<DashSpec>,
    pub start_marker: ArrowMarker,
    pub end_marker: ArrowMarker,
    pub quality: RenderQuality,
    /// **The LOD rung this edge is drawn at** (§15). It decides whether the
    /// curves survive, whether the markers do, whether a dash does, and what
    /// tolerance the tessellation uses — see [`EdgeDetail`].
    pub detail: EdgeDetail,
    /// The edge's index and route version, for the geometry cache key. `None`
    /// for a path that is not a document edge — the connection preview.
    pub owner: Option<(EdgeIndex, u32)>,
}

impl EdgePaint {
    pub fn new(color: Color, width: f32, quality: RenderQuality) -> EdgePaint {
        EdgePaint {
            color,
            width,
            dash: None,
            start_marker: ArrowMarker::None,
            end_marker: ArrowMarker::None,
            quality,
            detail: EdgeDetail::Full,
            owner: None,
        }
    }

    /// The LOD rung. See [`EdgePaint::detail`].
    pub fn at_detail(mut self, detail: EdgeDetail) -> EdgePaint {
        self.detail = detail;
        self
    }

    /// The cache identity. See [`EdgePaint::owner`].
    pub fn owned_by(mut self, edge: EdgeIndex, version: u32) -> EdgePaint {
        self.owner = Some((edge, version));
        self
    }

    /// The tolerance this edge is actually tessellated at, once the rung has
    /// had its say. **This is what goes in the cache key**, so a rung change is
    /// a miss by construction.
    pub fn effective_quality(&self) -> RenderQuality {
        self.detail.quality(self.quality)
    }

    pub fn with_markers(mut self, start: ArrowMarker, end: ArrowMarker) -> EdgePaint {
        self.start_marker = start;
        self.end_marker = end;
        self
    }

    pub fn with_dash(mut self, dash: DashSpec) -> EdgePaint {
        self.dash = Some(dash);
        self
    }
}

/// **The route as a screen-space outline at one LOD rung** (§15).
///
/// The rungs below `Coarse` do not merely tessellate more loosely — they emit a
/// *different outline*, which is the only thing that actually removes vertices.
/// A `Polyline` keeps the route's corners and drops its curvature; a `Hairline`
/// is the chord. See [`crate::render::lod`] for why that is mandatory rather
/// than nice: Phase 4's scattered scene puts 61,104 edges genuinely in the
/// viewport, and no amount of culling can bound it.
pub fn route_outline_at(route: &EdgeRoute, viewport: &Viewport, detail: EdgeDetail) -> Outline {
    if detail.keeps_curves() {
        return route_outline(route, viewport);
    }

    if detail == EdgeDetail::Hairline {
        let mut outline = Outline::with_capacity(2);
        outline.move_to(viewport.world_to_screen(route.start()));
        outline.line_to(viewport.world_to_screen(route.end()));
        return outline;
    }

    let mut outline = Outline::with_capacity(route.segments().len() + 1);
    for (index, point) in route.corner_points().enumerate() {
        let screen = viewport.world_to_screen(point);
        if index == 0 {
            outline.move_to(screen);
        } else {
            outline.line_to(screen);
        }
    }
    outline
}

/// The route as a screen-space outline, ready to tessellate.
pub fn route_outline(route: &EdgeRoute, viewport: &Viewport) -> Outline {
    let mut outline = Outline::with_capacity(route.segments().len() + 1);
    outline.move_to(viewport.world_to_screen(route.start()));

    for segment in route.segments() {
        match *segment {
            RouteSegment::Line(to) => {
                outline.line_to(viewport.world_to_screen(to));
            }
            RouteSegment::Cubic { c1, c2, to } => {
                outline.cubic_to(
                    viewport.world_to_screen(c1),
                    viewport.world_to_screen(c2),
                    viewport.world_to_screen(to),
                );
            }
        }
    }

    outline
}

/// **Plans one edge**: its line, then its markers.
pub fn plan_edge(plan: &mut PaintPlan, route: &EdgeRoute, paint: &EdgePaint, viewport: &Viewport) {
    if route.is_empty() || paint.color.is_invisible() {
        return;
    }

    let width = viewport
        .world_to_screen_length(paint.width)
        .max(MIN_STROKE_PIXELS);
    let outline = route_outline_at(route, viewport, paint.detail);
    let quality = paint.effective_quality();

    // A dash is the expensive kind — 63x the vertices of the same line solid —
    // so the ladder drops it at the first rung down, and this is where that is
    // spent rather than in the caller.
    let dash = paint.dash.filter(|_| paint.detail.keeps_dashes());

    let mut path = match dash {
        Some(dash) => PathPrimitive::dashed_stroke(
            outline,
            paint.color,
            width,
            DashSpec::new(
                viewport.world_to_screen_length(dash.on),
                viewport.world_to_screen_length(dash.off),
            ),
            quality,
        ),
        None => PathPrimitive::stroke(outline, paint.color, width, quality),
    };

    if let Some((edge, version)) = paint.owner {
        path = path.keyed(GeometryKey::edge(
            edge,
            GeometryPart::Stroke,
            version,
            quality,
        ));
    }
    plan.push_path(path);

    if paint.detail.keeps_markers() {
        plan_markers(plan, route, paint, viewport, width);
    }
}

/// Both endpoint decorations, in screen space.
///
/// The **source** marker points back down the edge — the negated start
/// tangent — because an arrow at the source of a two-headed edge points away
/// from the node it is attached to, exactly as the target's does.
fn plan_markers(
    plan: &mut PaintPlan,
    route: &EdgeRoute,
    paint: &EdgePaint,
    viewport: &Viewport,
    screen_width: f32,
) {
    // Sized off the *screen* stroke width, so a marker keeps its proportion to
    // the line it decorates at every zoom rather than shrinking to a speck.
    let length = arrow::marker_length(screen_width);

    let markers = [
        (
            paint.start_marker,
            viewport.world_to_screen(route.start()),
            Vec2::ZERO - route.start_tangent(),
        ),
        (
            paint.end_marker,
            viewport.world_to_screen(route.end()),
            route.end_tangent(),
        ),
    ];

    for (kind, tip, direction) in markers {
        let Some(geometry) = arrow::marker(kind, tip, direction, length) else {
            continue;
        };

        match geometry {
            // A quad, not a path: see the module doc.
            ArrowGeometry::Dot { center, radius } => plan.push_quad(
                QuadPrimitive::filled(
                    crate::geometry::Rect::new(
                        center - Vec2::splat(radius),
                        Vec2::splat(radius * 2.0),
                    ),
                    paint.color,
                )
                .with_corner_radius(radius),
            ),
            ArrowGeometry::Polygon(polygon) => {
                plan.push_path(marker_path(&polygon, paint, screen_width))
            }
        }
    }
}

fn marker_path(polygon: &ArrowPolygon, paint: &EdgePaint, screen_width: f32) -> PathPrimitive {
    let mut outline = Outline::with_capacity(polygon.len() + 1);
    for (index, point) in polygon.points().iter().enumerate() {
        if index == 0 {
            outline.move_to(*point);
        } else {
            outline.line_to(*point);
        }
    }
    if polygon.closed {
        outline.close();
    }

    if polygon.filled {
        PathPrimitive::fill(outline, paint.color, paint.effective_quality())
    } else {
        // An open arrow head is stroked at the edge's own width, so it reads as
        // a continuation of the line rather than as a separate mark.
        PathPrimitive::stroke(
            outline,
            paint.color,
            screen_width,
            paint.effective_quality(),
        )
    }
}

/// **The connection preview** (§8): a line from a handle to wherever the
/// pointer is, dashed so it reads as provisional.
///
/// Takes a route like any other edge, because it *is* one — the interaction
/// builds a route between the source handle and the pointer, so the preview
/// bends the same way the committed edge will and there is no second router to
/// disagree with the first.
pub fn plan_connection_preview(
    plan: &mut PaintPlan,
    route: &EdgeRoute,
    color: Color,
    width: f32,
    quality: RenderQuality,
    viewport: &Viewport,
) {
    let paint = EdgePaint {
        color,
        width,
        dash: Some(DashSpec::new(PREVIEW_DASH_ON, PREVIEW_DASH_OFF)),
        start_marker: ArrowMarker::Dot,
        end_marker: ArrowMarker::ArrowClosed,
        quality,
        // Always full: a preview is one path following the pointer, and it is
        // the thing the user is looking at.
        detail: EdgeDetail::Full,
        // Never cached: it changes every frame by definition, which is exactly
        // what §23 says not to cache.
        owner: None,
    };

    plan_edge(plan, route, &paint, viewport);
}

/// The preview's dash, in world units at zoom 1. Long enough that a short drag
/// still shows two or three dashes.
const PREVIEW_DASH_ON: f32 = 6.0;
const PREVIEW_DASH_OFF: f32 = 4.0;

#[cfg(test)]
mod tests {
    use super::{EdgePaint, MIN_STROKE_PIXELS, plan_connection_preview, plan_edge, route_outline};
    use crate::{
        geometry::{Attachment, Side, Vec2, Viewport, route},
        models::{ArrowMarker, Color, EdgeRouting, RenderQuality},
        render::{
            PaintPlan,
            plan::{DashSpec, PathPaint},
            shapes::SubpathCommand,
        },
    };

    fn viewport(zoom: f32) -> Viewport {
        Viewport::new(Vec2::ZERO, zoom, Vec2::new(1000.0, 800.0))
    }

    fn a_route(routing: EdgeRouting) -> crate::geometry::EdgeRoute {
        route::route(
            routing,
            Attachment::new(Vec2::new(0.0, 0.0), Side::Right),
            Attachment::new(Vec2::new(200.0, 100.0), Side::Left),
            &crate::geometry::RouteOptions::DEFAULT,
        )
    }

    fn paint() -> EdgePaint {
        EdgePaint::new(Color::WHITE, 2.0, RenderQuality::BALANCED)
    }

    #[test]
    fn an_outline_follows_the_route_through_the_viewport() {
        let route = a_route(EdgeRouting::Straight);
        let viewport = viewport(2.0);

        let outline = route_outline(&route, &viewport);

        assert_eq!(
            outline.commands(),
            &[
                SubpathCommand::MoveTo(viewport.world_to_screen(Vec2::new(0.0, 0.0))),
                SubpathCommand::LineTo(viewport.world_to_screen(Vec2::new(200.0, 100.0))),
            ]
        );
    }

    #[test]
    fn a_curved_route_keeps_its_control_points_through_the_transform() {
        let route = a_route(EdgeRouting::Bezier);
        let viewport = viewport(0.5);

        let outline = route_outline(&route, &viewport);

        assert!(matches!(
            outline.commands()[1],
            SubpathCommand::CubicTo { .. }
        ));
    }

    #[test]
    fn a_plain_edge_is_one_stroked_path_and_no_quads() {
        let mut plan = PaintPlan::new();

        plan_edge(
            &mut plan,
            &a_route(EdgeRouting::Bezier),
            &paint(),
            &viewport(1.0),
        );

        assert_eq!(plan.path_count(), 1);
        assert_eq!(plan.quad_count(), 0);
    }

    /// A hairline at overview zoom must not disappear: the graph's shape *is*
    /// its edges at that zoom.
    #[test]
    fn a_stroke_never_thins_below_the_visible_minimum() {
        let mut plan = PaintPlan::new();
        let mut paint = paint();
        paint.width = 1.0;

        plan_edge(
            &mut plan,
            &a_route(EdgeRouting::Straight),
            &paint,
            &viewport(0.01),
        );

        assert_eq!(
            plan.paths().first().and_then(|path| path.paint.width()),
            Some(MIN_STROKE_PIXELS)
        );
    }

    #[test]
    fn a_dashed_edge_is_planned_as_the_expensive_kind() {
        let mut plan = PaintPlan::new();
        let paint = paint().with_dash(DashSpec::new(6.0, 4.0));

        plan_edge(
            &mut plan,
            &a_route(EdgeRouting::Straight),
            &paint,
            &viewport(2.0),
        );

        let path = plan.paths().first().expect("one path");
        let PathPaint::DashedStroke { dash, .. } = path.paint else {
            panic!("a dashed edge must not be planned as a solid stroke");
        };
        // The pattern is in world units and scales with the zoom, like the
        // stroke it decorates.
        assert_eq!(dash.on, 12.0);
        assert_eq!(dash.off, 8.0);
    }

    /// The measurement Phase 0 made, showing up where it matters: the same line
    /// dashed costs far more of the frame's vertex budget than solid.
    #[test]
    fn a_dashed_edge_costs_far_more_of_the_vertex_budget_than_a_solid_one() {
        let mut solid = PaintPlan::new();
        let mut dashed = PaintPlan::new();
        let route = a_route(EdgeRouting::Straight);

        plan_edge(&mut solid, &route, &paint(), &viewport(1.0));
        plan_edge(
            &mut dashed,
            &route,
            &paint().with_dash(DashSpec::new(6.0, 4.0)),
            &viewport(1.0),
        );

        assert!(
            dashed.estimated_path_vertices() > solid.estimated_path_vertices() * 5,
            "solid {} vs dashed {}",
            solid.estimated_path_vertices(),
            dashed.estimated_path_vertices()
        );
    }

    #[test]
    fn a_filled_marker_adds_a_path_and_a_dot_adds_a_quad() {
        let route = a_route(EdgeRouting::Straight);

        let mut filled = PaintPlan::new();
        plan_edge(
            &mut filled,
            &route,
            &paint().with_markers(ArrowMarker::None, ArrowMarker::ArrowClosed),
            &viewport(1.0),
        );
        assert_eq!(filled.path_count(), 2, "the line and the arrow head");
        assert_eq!(filled.quad_count(), 0);

        let mut dotted = PaintPlan::new();
        plan_edge(
            &mut dotted,
            &route,
            &paint().with_markers(ArrowMarker::None, ArrowMarker::Dot),
            &viewport(1.0),
        );
        assert_eq!(dotted.path_count(), 1, "a dot is not a path");
        assert_eq!(dotted.quad_count(), 1);

        // And it is a *circle*: a square quad with a corner radius of half its
        // side, straddling the end of the edge.
        let dot = dotted.quads()[0];
        assert_eq!(dot.bounds.size.x, dot.bounds.size.y);
        assert_eq!(dot.corner_radius, dot.bounds.size.x * 0.5);
        assert!(
            dot.bounds.contains_point(Vec2::new(200.0, 100.0)),
            "the dot covers the end of the edge: {:?}",
            dot.bounds
        );
    }

    #[test]
    fn both_markers_are_planned_when_both_are_asked_for() {
        let mut plan = PaintPlan::new();

        plan_edge(
            &mut plan,
            &a_route(EdgeRouting::Straight),
            &paint().with_markers(ArrowMarker::ArrowClosed, ArrowMarker::ArrowClosed),
            &viewport(1.0),
        );

        assert_eq!(plan.path_count(), 3);
    }

    /// The source marker points **away** from its own node, which means back
    /// down the edge — the negated start tangent.
    #[test]
    fn the_source_marker_points_back_down_the_edge() {
        let mut plan = PaintPlan::new();
        let route = a_route(EdgeRouting::Straight);

        plan_edge(
            &mut plan,
            &route,
            &paint().with_markers(ArrowMarker::ArrowClosed, ArrowMarker::None),
            &viewport(1.0),
        );

        let marker = &plan.paths()[1].outline;
        let tip = match marker.commands()[0] {
            SubpathCommand::MoveTo(p) => p,
            other => panic!("a marker starts with a move: {other:?}"),
        };
        let body: Vec<Vec2> = marker.commands()[1..]
            .iter()
            .filter_map(|command| match command {
                SubpathCommand::LineTo(p) => Some(*p),
                _ => None,
            })
            .collect();

        assert_eq!(tip, Vec2::ZERO, "the tip sits on the source end");
        assert!(
            body.iter().all(|p| p.x > tip.x),
            "the body is on the far side from the direction of travel: {body:?}"
        );
    }

    #[test]
    fn an_empty_or_invisible_edge_plans_nothing() {
        let mut plan = PaintPlan::new();
        let route = crate::geometry::EdgeRoute::default();

        plan_edge(&mut plan, &route, &paint(), &viewport(1.0));

        let mut invisible = paint();
        invisible.color = Color::TRANSPARENT;
        plan_edge(
            &mut plan,
            &a_route(EdgeRouting::Bezier),
            &invisible,
            &viewport(1.0),
        );

        assert!(plan.is_empty());
    }

    /// The preview reads as provisional — dashed — and shows where it would
    /// land.
    #[test]
    fn a_connection_preview_is_dashed_and_carries_an_arrow() {
        let mut plan = PaintPlan::new();

        plan_connection_preview(
            &mut plan,
            &a_route(EdgeRouting::Bezier),
            Color::WHITE,
            2.0,
            RenderQuality::BALANCED,
            &viewport(1.0),
        );

        assert!(matches!(
            plan.paths()[0].paint,
            PathPaint::DashedStroke { .. }
        ));
        assert_eq!(plan.path_count(), 2, "the line and the arrow head");
        assert_eq!(plan.quad_count(), 1, "the dot at the source handle");
    }
}
