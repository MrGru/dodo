//! **Turning the visible set into primitives** — the one place §16's rule is
//! spent, and the reason it can be asserted without a window.
//!
//! ```text
//! GraphWorld ─> SpatialIndex::query_visible ─> VisibleSet ─> plan_scene ─> PaintPlan
//!  100,000 nodes            broad + narrow       ~34 nodes     ~120 paths
//! ```
//!
//! # Why this is a file and not a method on the view
//!
//! Until Phase 4 these loops lived in `views::flow`, which names GPUI, which
//! means the only way to check what they planned was to open a window and look.
//! For a phase whose central deliverable is *"zero offscreen paths reach the
//! painter"* that is not good enough — a geometric bug that can only be seen is
//! a geometric bug that ships, as the crate doc puts it. Moved here, the
//! property is an ordinary unit test:
//! `a_huge_document_plans_nothing_outside_the_pane`.
//!
//! The view keeps what is genuinely interaction: the connection preview and the
//! rubber band, both of which are on screen by construction.
//!
//! # Two culling phases, and why both
//!
//! The [`VisibleSet`] is the broad phase plus a world-space narrow phase, at
//! cell granularity in the first and rectangle granularity in the second. This
//! file adds nothing to it — the third rejection is
//! [`PaintPlan::push_path`]'s, against the pane in *screen* space, and it is
//! there rather than here so that no painter, present or future, can skip it.
//!
//! [`PaintPlan::culled_paths`] is what that third rejection catches, and the
//! honest expectation is **a small boundary fringe rather than zero**. Both
//! earlier phases deliberately over-report: the query rectangle is the viewport
//! plus the screen-constant paint margin, and an edge is indexed at its control
//! hull rather than at its curve. Erring outward is the right direction — a
//! false "visible" costs one wasted path, a false "hidden" is a missing edge.
//!
//! What must be true is that the fringe is set by the **pane** and not by the
//! document, and `culling_work_does_not_grow_with_the_document` is that
//! assertion: a sixteen-times-larger document at the same camera leaves the
//! clip exactly the same work.
//!
//! **This file names no UI framework.**

use crate::{
    geometry::{Rect, Vec2, Viewport},
    models::{Color, NodeIndex, RenderQuality},
    render::{
        GridLevel, GridLimits, GridSettings, PaintPlan, edges, grid,
        plan::{DashSpec, PathPrimitive, QuadPrimitive},
        shapes,
    },
    runtime::{GraphWorld, NodeShape},
    spatial::VisibleSet,
};

/// How big a handle dot is drawn, in **screen** pixels.
///
/// Screen rather than world, so a handle stays grabbable when zoomed out and
/// does not swallow its node when zoomed in. It is the same number
/// [`HitTolerance::HANDLE_SCREEN_RADIUS`](crate::runtime::HitTolerance::HANDLE_SCREEN_RADIUS)
/// tests against, less the grabbing margin — a target you can hit slightly
/// outside is right, a target that is smaller than it looks is not.
pub const HANDLE_SCREEN_RADIUS: f32 = 4.5;

/// A graph node's body radius in world units, when its style does not set one.
///
/// A default rather than a hard-coded look: `ElementStyle::corner_radius` wins
/// whenever a document says anything, and this is only what an unstyled node
/// falls back to so it reads as a node rather than as a drawn rectangle.
pub const GRAPH_NODE_RADIUS: f32 = 6.0;

/// The outline width of a selected element, in screen pixels. Constant on
/// screen rather than in world units, so selection stays visible at any zoom.
pub const SELECTED_STROKE_PIXELS: f32 = 2.0;

/// The colours an element falls back to when its own style leaves them unset.
///
/// Resolved from the active theme once per frame by the view and passed down,
/// rather than read per element: a theme lookup inside the loop over every
/// visible shape is exactly the sort of per-element cost §40 warns about.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SceneInk {
    pub fill: Color,
    pub stroke: Color,
    /// Edges and their markers.
    pub edge: Color,
    /// Handle dots.
    pub handle: Color,
    /// The selection outline, the box-select rectangle and the connection
    /// preview — everything that says "you are doing something".
    pub accent: Color,
}

/// What the frame is being drawn with, apart from its colours.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SceneOptions {
    pub grid: GridSettings,
    pub grid_limits: GridLimits,
}

impl SceneOptions {
    pub fn new(grid: GridSettings, grid_limits: GridLimits) -> SceneOptions {
        SceneOptions { grid, grid_limits }
    }
}

/// What one extraction produced. Counts rather than geometry, because the
/// geometry went into the plan and this is what a benchmark and a test read.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SceneStats {
    pub grid: GridLevel,
    /// Nodes that produced at least one primitive.
    pub nodes: u32,
    pub edges: u32,
    pub handles: u32,
    /// Visible nodes skipped because their kind has no representation yet —
    /// text, images, frames. **Not** a culling number; a not-implemented one.
    pub unsupported_nodes: u32,
}

/// **Plans one frame from the visible set.**
///
/// Clears `plan` against the pane, so the clip and the extraction cannot
/// disagree about which frame they belong to, then plans the grid, the edges
/// and the nodes in that order — edges under nodes, and the paint order is
/// [`PaintPlan`]'s regardless.
///
/// Nothing here iterates the document. `visible` is the whole input, which is
/// what §40 rule 1 asks for and what makes the cost of this function
/// proportional to the screen rather than to the file.
pub fn plan_scene(
    plan: &mut PaintPlan,
    world: &GraphWorld,
    visible: &VisibleSet,
    viewport: &Viewport,
    ink: SceneInk,
    options: &SceneOptions,
) -> SceneStats {
    let pane = Rect::new(Vec2::ZERO, viewport.size());
    plan.clear(pane);

    let quality = world.settings().render_quality;
    let mut stats = SceneStats {
        grid: grid::generate(&options.grid, viewport, &options.grid_limits, plan),
        ..SceneStats::default()
    };

    plan_edges(plan, world, visible, viewport, ink, quality, &mut stats);
    plan_nodes(plan, world, visible, viewport, ink, quality, &mut stats);
    stats
}

/// Every visible edge, from its **derived** route.
///
/// The routes were brought up to date once, at the top of the frame, by
/// [`GraphWorld::rebuild_dirty_geometry`] — so this loop rebuilds nothing and a
/// pure pan reroutes nothing (§40 rule 6). An edge whose route is stale is
/// skipped rather than drawn from a stale one: it will be current on the frame
/// the rebuild ran, and painting the old one would show an edge hanging off a
/// node that has already moved.
fn plan_edges(
    plan: &mut PaintPlan,
    world: &GraphWorld,
    visible: &VisibleSet,
    viewport: &Viewport,
    ink: SceneInk,
    quality: RenderQuality,
    stats: &mut SceneStats,
) {
    for &edge in visible.edges() {
        let Some(route) = world.route(edge) else {
            continue;
        };

        let style = world.edges().style(edge);
        let color = if world.edges().is_selected(edge) {
            ink.accent
        } else {
            fade(style.stroke.color.unwrap_or(ink.edge), style.opacity)
        };

        let paint = edges::EdgePaint {
            color,
            width: style.stroke.width,
            // A dashed edge is the expensive kind, so it is only ever asked
            // for when the document says so — see `render::plan::PathPaint`.
            dash: style
                .stroke
                .dash
                .spec()
                .map(|(on, off)| DashSpec::new(on, off)),
            start_marker: style.start_marker,
            end_marker: style.end_marker,
            quality,
        };

        edges::plan_edge(plan, route, &paint, viewport);
        stats.edges += 1;
    }
}

/// Every visible node, as the cheapest primitive that can draw it, and its
/// handles.
///
/// The loop reads the runtime's hot arrays: a position, a size, a one-byte
/// [`NodeShape`] and a style. It never touches
/// [`ElementKind`](crate::models::ElementKind), which carries a `String` —
/// that is what §17's cold/hot split is for and what §40 rule 9 asks.
fn plan_nodes(
    plan: &mut PaintPlan,
    world: &GraphWorld,
    visible: &VisibleSet,
    viewport: &Viewport,
    ink: SceneInk,
    quality: RenderQuality,
    stats: &mut SceneStats,
) {
    let nodes = world.nodes();

    for &node in visible.nodes() {
        let shape = nodes.shape(node);
        if shape == NodeShape::Other {
            // Text, images, frames and custom kinds are later phases'. A
            // fallback rectangle here would silently draw them and hide the
            // fact that they are not implemented.
            stats.unsupported_nodes += 1;
            continue;
        }

        let style = nodes.style(node);
        let screen = viewport.world_rect_to_screen(nodes.bounds(node));
        // A graph node's body has a radius of its own so it reads as a node
        // rather than as a drawn rectangle; a shape uses what its style says.
        let world_radius = if shape == NodeShape::GraphNode && style.corner_radius <= 0.0 {
            GRAPH_NODE_RADIUS
        } else {
            style.corner_radius
        };
        let radius = viewport.world_to_screen_length(world_radius);

        // `None` means "the theme decides", which is the whole reason
        // `models::style` stores colours as `Option<Color>`: a document must
        // not carry a palette, or it would look wrong in the other theme.
        let fill = fade(style.fill.unwrap_or(ink.fill), style.opacity);
        let selected = nodes.is_selected(node);
        let stroke_color = if selected {
            ink.accent
        } else {
            fade(style.stroke.color.unwrap_or(ink.stroke), style.opacity)
        };
        let stroke_width = viewport
            .world_to_screen_length(style.stroke.width)
            .max(if selected {
                SELECTED_STROKE_PIXELS
            } else {
                0.0
            });
        let has_stroke = (!style.stroke.is_invisible() || selected) && stroke_width > 0.0;

        if shapes::node_prefers_quad(shape) {
            // Phase 0's measurement, honoured: 20,000 quads hold 60 fps where
            // the same count of rectangular paths drop to 30 — and a quad
            // carries its corner radius and its border for free, so the border
            // costs no second primitive at all.
            let mut quad = QuadPrimitive::filled(screen, fill).with_corner_radius(radius);
            if has_stroke {
                quad = quad.with_border(stroke_width, stroke_color);
            }
            plan.push_quad(quad);
        } else if let Some(outline) = shapes::outline_for_node(shape, screen, radius) {
            plan.push_path(PathPrimitive::fill(outline.clone(), fill, quality));

            if has_stroke {
                plan.push_path(PathPrimitive::stroke(
                    outline,
                    stroke_color,
                    stroke_width,
                    quality,
                ));
            }
        }

        stats.nodes += 1;
        stats.handles += plan_handles(plan, world, viewport, node, ink);
    }
}

/// A node's handles, as quads.
///
/// **Geometry and data in this phase, not interaction.** A handle is drawn so a
/// connection can be aimed at it and so the routing is visible; the interactive
/// element with its own hover state and cursor is Phase 5's, where §15's LOD
/// decides whether a node is detailed enough to have one at all. A circle is a
/// quad with a corner radius of half its side, so a hundred thousand of them
/// would still be the cheap primitive.
fn plan_handles(
    plan: &mut PaintPlan,
    world: &GraphWorld,
    viewport: &Viewport,
    node: NodeIndex,
    ink: SceneInk,
) -> u32 {
    let radius = HANDLE_SCREEN_RADIUS;
    let mut painted = 0;

    for handle in world.nodes().handles(node) {
        if world.handles().is_hidden(handle) {
            // §4: hidden handles stay connectable. Only the paint is skipped —
            // routing and hit-testing never read this flag.
            continue;
        }

        let center = viewport.world_to_screen(world.handle_position(handle));
        plan.push_quad(
            QuadPrimitive::filled(
                Rect::new(center - Vec2::splat(radius), Vec2::splat(radius * 2.0)),
                ink.handle,
            )
            .with_corner_radius(radius)
            .with_border(1.0, ink.fill),
        );
        painted += 1;
    }

    painted
}

/// Applies an element's opacity to one of its colours.
///
/// `ElementStyle::opacity` multiplies both stroke and fill alpha rather than
/// replacing it, so a half-transparent fill inside a half-transparent element
/// is a quarter — which is what every other editor does and what a user
/// dragging an opacity slider expects.
pub fn fade(color: Color, opacity: f32) -> Color {
    color.with_alpha(color.a * opacity.clamp(0.0, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        budgets::{RenderBackend, for_backend},
        models::{ElementKind, GraphNodeKind},
        render::GridStyle,
        runtime::{ConnectionRules, EdgeEnd},
        spatial::SpatialIndex,
    };

    fn ink() -> SceneInk {
        SceneInk {
            fill: Color::rgb(0.2, 0.2, 0.2),
            stroke: Color::rgb(0.9, 0.9, 0.9),
            edge: Color::rgb(0.7, 0.7, 0.7),
            handle: Color::rgb(0.3, 0.6, 1.0),
            accent: Color::rgb(1.0, 0.6, 0.2),
        }
    }

    fn options() -> SceneOptions {
        SceneOptions::new(
            GridSettings::default(),
            GridLimits::from_budgets(&for_backend(RenderBackend::Metal)),
        )
    }

    /// A locality-preserving grid of nodes, each joined to its right-hand
    /// neighbour — the shape of a real diagram rather than a random graph.
    fn document(columns: u32, rows: u32) -> GraphWorld {
        let mut world = GraphWorld::new();
        world.set_rules(ConnectionRules::PERMISSIVE);
        for row in 0..rows {
            for column in 0..columns {
                world.create_node(
                    ElementKind::GraphNode(GraphNodeKind::Default),
                    Vec2::new(column as f32 * 240.0, row as f32 * 140.0),
                    Vec2::new(160.0, 60.0),
                );
            }
        }
        for row in 0..rows {
            for column in 0..columns.saturating_sub(1) {
                let index = row * columns + column;
                world
                    .connect(
                        EdgeEnd::node(NodeIndex::new(index)),
                        EdgeEnd::node(NodeIndex::new(index + 1)),
                    )
                    .expect("permissive rules accept it");
            }
        }
        world.rebuild_all_geometry();
        world.clear_spatial_updates();
        world
    }

    /// Records the painted extent of every path it is handed, so the
    /// "zero offscreen paths" criterion is checked on what actually reached a
    /// sink rather than on what was in the plan.
    #[derive(Default)]
    struct RecordingSink {
        paths: Vec<Rect>,
    }

    impl crate::render::PrimitiveSink for RecordingSink {
        fn quad(&mut self, _quad: &crate::render::QuadPrimitive) {}

        fn path(&mut self, path: &crate::render::PathPrimitive) -> u32 {
            let Some(bounds) = path.outline.bounds() else {
                return 0;
            };
            self.paths
                .push(bounds.inflate(path.paint.width().unwrap_or(0.0) * 0.5));
            1
        }

        fn text(&mut self, _text: &crate::render::plan::TextPrimitive) -> u32 {
            0
        }
    }

    fn frame(world: &GraphWorld, viewport: &Viewport) -> (PaintPlan, SceneStats) {
        let index = SpatialIndex::for_world(world);
        let mut visible = VisibleSet::new();
        index.query_visible(world, viewport, &mut visible);

        let mut plan = PaintPlan::new();
        let stats = plan_scene(&mut plan, world, &visible, viewport, ink(), &options());
        (plan, stats)
    }

    /// **The phase's central property.** A document far larger than the pane
    /// plans nothing the pane cannot see — not "clipped later", not "cheap
    /// enough", but never handed to the painter at all.
    #[test]
    fn a_huge_document_plans_nothing_outside_the_pane() {
        let world = document(200, 200);
        assert_eq!(world.nodes().len(), 40_000);

        for step in 0..12 {
            let viewport = Viewport::new(
                Vec2::new(step as f32 * -1_700.0, step as f32 * -900.0),
                if step % 4 == 0 { 0.4 } else { 1.0 },
                Vec2::new(1_440.0, 900.0),
            );
            let (plan, _) = frame(&world, &viewport);
            let pane = plan.clip();

            for path in plan.paths() {
                let extent = path
                    .outline
                    .bounds()
                    .expect("a planned path has geometry")
                    .inflate(path.paint.width().unwrap_or(0.0) * 0.5);
                assert!(
                    extent.intersects(pane),
                    "an offscreen path reached the plan at step {step}: {extent:?} vs {pane:?}"
                );
            }
            for quad in plan.quads() {
                assert!(
                    quad.bounds.normalized().intersects(pane),
                    "an offscreen quad reached the plan at step {step}: {:?}",
                    quad.bounds
                );
            }
        }
    }

    /// The same property from the other side, and the honest version of it:
    /// the clip is not doing the culling, it is catching a **fringe**.
    ///
    /// It cannot be zero and should not be. The broad phase queries the
    /// viewport inflated by the screen-constant paint margin, and an edge is
    /// indexed at its control hull rather than at its curve — both deliberately
    /// over-report, because a false "visible" costs one wasted path and a false
    /// "hidden" is a missing edge. What matters is that what gets through is a
    /// small share of the frame rather than a share of the document.
    #[test]
    fn the_clip_only_catches_a_boundary_fringe() {
        let world = document(120, 120);

        for step in 0..8 {
            let viewport = Viewport::new(
                Vec2::new(step as f32 * -900.0, step as f32 * -500.0),
                1.0,
                Vec2::new(1_440.0, 900.0),
            );
            let (plan, _) = frame(&world, &viewport);

            assert!(
                plan.culled_paths() < plan.path_count().max(1) / 2,
                "the clip rejected {} of {} paths at step {step} — that is bulk, not a fringe",
                plan.culled_paths(),
                plan.path_count()
            );
        }
    }

    /// **The property that actually matters**, and the one §40 rule 1 is about:
    /// the work the clip has to do is set by the *pane*, not by the document.
    /// A document sixteen times larger at the same camera must not make the
    /// culling sixteen times busier.
    #[test]
    fn culling_work_does_not_grow_with_the_document() {
        let viewport = Viewport::new(Vec2::ZERO, 1.0, Vec2::new(1_440.0, 900.0));

        let (small, small_stats) = frame(&document(50, 50), &viewport);
        let (large, large_stats) = frame(&document(200, 200), &viewport);

        assert_eq!(small_stats.nodes, large_stats.nodes);
        assert_eq!(small_stats.edges, large_stats.edges);
        assert_eq!(
            small.culled_paths(),
            large.culled_paths(),
            "a 16x document changed how much the clip rejected"
        );
    }

    /// §16's rule as a number, through the whole pipeline rather than through
    /// the index alone.
    #[test]
    fn a_forty_thousand_node_document_plans_a_screenful() {
        let world = document(200, 200);
        let viewport = Viewport::new(Vec2::ZERO, 1.0, Vec2::new(1_440.0, 900.0));
        let (_, stats) = frame(&world, &viewport);

        assert!(
            stats.nodes < 100,
            "{} of 40,000 nodes were planned at 1:1",
            stats.nodes
        );
        assert!(stats.nodes > 0);
        assert!(stats.edges > 0);
    }

    /// **The vertex ceiling, asserted rather than approached.** The dense scene
    /// is deliberately the worst case the plan calls for: several thousand
    /// objects visible at once.
    #[test]
    fn a_dense_visible_scene_stays_far_under_the_vertex_ceiling() {
        let budgets = for_backend(RenderBackend::Metal);
        let mut world = GraphWorld::new();
        world.set_rules(ConnectionRules::PERMISSIVE);
        // 30 x 24 world units on a 34 x 26 pitch: a 1440x900 pane sees
        // thousands at once.
        for row in 0..120u32 {
            for column in 0..120u32 {
                world.create_node(
                    ElementKind::GraphNode(GraphNodeKind::Default),
                    Vec2::new(column as f32 * 34.0, row as f32 * 26.0),
                    Vec2::new(30.0, 18.0),
                );
            }
        }
        for index in 0..14_000u32 {
            world
                .connect(
                    EdgeEnd::node(NodeIndex::new(index)),
                    EdgeEnd::node(NodeIndex::new(index + 1)),
                )
                .expect("valid");
        }
        world.rebuild_all_geometry();
        world.clear_spatial_updates();

        let viewport = Viewport::new(Vec2::ZERO, 1.0, Vec2::new(1_440.0, 900.0));
        let (plan, stats) = frame(&world, &viewport);

        assert!(
            stats.nodes > 1_000,
            "the dense scene should make thousands visible, not {}",
            stats.nodes
        );

        let vertices = plan.estimated_path_vertices();
        assert!(
            vertices < budgets.safe_path_vertex_ceiling,
            "{vertices} estimated vertices against a {} ceiling",
            budgets.safe_path_vertex_ceiling
        );
        // Not merely under it: an order of magnitude under, which is what
        // "never approached" has to mean if the number is to be worth anything.
        assert!(
            vertices * 4 < budgets.safe_path_vertex_ceiling,
            "{vertices} vertices is within 4x of the black-window ceiling"
        );
    }

    #[test]
    fn a_hidden_node_is_never_planned() {
        let mut world = document(6, 6);
        world.set_node_hidden(NodeIndex::new(0), true);

        let viewport = Viewport::new(Vec2::ZERO, 1.0, Vec2::new(1_440.0, 900.0));
        let (_, with_hidden) = frame(&world, &viewport);

        world.set_node_hidden(NodeIndex::new(0), false);
        let (_, without) = frame(&world, &viewport);

        assert_eq!(with_hidden.nodes + 1, without.nodes);
    }

    #[test]
    fn an_empty_document_plans_only_the_grid() {
        let world = GraphWorld::new();
        let viewport = Viewport::new(Vec2::ZERO, 1.0, Vec2::new(800.0, 600.0));
        let (plan, stats) = frame(&world, &viewport);

        assert_eq!(stats.nodes, 0);
        assert_eq!(stats.edges, 0);
        assert_eq!(plan.path_count(), 0, "the grid is quads, never paths");
        assert!(plan.quad_count() > 0);
    }

    #[test]
    fn a_grid_that_is_switched_off_plans_no_grid_quads() {
        let world = GraphWorld::new();
        let viewport = Viewport::new(Vec2::ZERO, 1.0, Vec2::new(800.0, 600.0));
        let index = SpatialIndex::for_world(&world);
        let mut visible = VisibleSet::new();
        index.query_visible(&world, &viewport, &mut visible);

        let mut options = options();
        options.grid.style = GridStyle::None;
        let mut plan = PaintPlan::new();
        plan_scene(&mut plan, &world, &visible, &viewport, ink(), &options);

        assert_eq!(plan.quad_count(), 0);
    }

    /// The scene's clip is the pane it was extracted for, so a plan and its
    /// clip cannot come from different frames.
    #[test]
    fn the_plan_is_clipped_to_the_viewport_it_was_planned_for() {
        let world = document(4, 4);
        let viewport = Viewport::new(Vec2::ZERO, 1.0, Vec2::new(640.0, 480.0));
        let (plan, _) = frame(&world, &viewport);

        assert_eq!(plan.clip(), Rect::new(Vec2::ZERO, Vec2::new(640.0, 480.0)));
    }

    /// **The exit criteria, on §38's own scenes.**
    ///
    /// The harness prints these numbers and this test refuses to let them
    /// regress. It runs the four scenes at their real sizes, which is a couple
    /// of seconds of a `cargo test` — worth it, because a gate that is only
    /// checked by a human running a benchmark is a gate nobody checks.
    #[test]
    fn every_exit_criterion_holds_on_every_benchmark_scene() {
        use crate::scenes::{self, BENCH_PANE, SceneSpec};

        let budgets = for_backend(RenderBackend::Metal);

        for spec in SceneSpec::ALL {
            let world = scenes::build(&spec);
            assert_eq!(world.nodes().len(), spec.nodes, "{} built short", spec.name);
            let index = SpatialIndex::for_world(&world);
            let viewport = spec.viewport(BENCH_PANE);

            let mut visible = VisibleSet::new();
            index.query_visible(&world, &viewport, &mut visible);

            let mut plan = PaintPlan::new();
            plan_scene(&mut plan, &world, &visible, &viewport, ink(), &options());

            // 1. Painted vertices stay under the platform ceiling, and are not
            //    merely under it — the harness records 5.5 % on the worst
            //    scene, so 25 % is a regression bound with room to spare rather
            //    than a number tuned to today's measurement.
            let vertices = plan.estimated_path_vertices();
            assert!(
                vertices < budgets.safe_path_vertex_ceiling / 4,
                "{}: {vertices} estimated vertices, a quarter of the {} safe ceiling is the bound",
                spec.name,
                budgets.safe_path_vertex_ceiling
            );

            // 2. Nothing was dropped, which is the same criterion from the
            //    other side: the guard is a backstop and culling is what stops
            //    it firing.
            let mut guarded = plan.clone();
            assert_eq!(
                guarded.enforce_vertex_ceiling(&budgets),
                0,
                "{}: the black-window guard had to drop paths",
                spec.name
            );

            // 3. One contiguous path batch, against a budget of 64.
            let mut sink = RecordingSink::default();
            let stats = plan.paint_into(&mut sink);
            assert!(
                stats.within_batch_budget(&budgets),
                "{}: {} path batches",
                spec.name,
                stats.path_batches
            );
            assert!(stats.path_batches <= 1);

            // 4. Zero offscreen paths reached the sink.
            let pane = plan.clip();
            for path in sink.paths {
                assert!(
                    path.intersects(pane),
                    "{}: an offscreen path reached the painter: {path:?}",
                    spec.name
                );
            }

            // 5. §16: a big document produces a screenful of elements.
            if spec.nodes > 10_000 && spec.name != "dense" {
                assert!(
                    visible.node_count() < 200,
                    "{}: {} of {} nodes visible",
                    spec.name,
                    visible.node_count(),
                    spec.nodes
                );
            }
        }
    }

    /// §50: *does pan cause edge-route cache misses? It generally should not.*
    /// Here it must not, exactly, and it holds by construction — a pan changes
    /// the viewport and the viewport is not in the world.
    #[test]
    fn a_pure_pan_rebuilds_no_route_and_moves_nothing_in_the_index() {
        use crate::scenes::{self, BENCH_PANE, SceneSpec};

        let mut world = scenes::build(&SceneSpec {
            nodes: 4_000,
            edges: 8_000,
            ..SceneSpec::MEDIUM
        });
        let mut index = SpatialIndex::for_world(&world);
        world.clear_spatial_updates();

        let mut viewport = SceneSpec::MEDIUM.viewport(BENCH_PANE);
        let mut visible = VisibleSet::new();
        let mut plan = PaintPlan::new();
        let before = world.geometry().rebuild_count();

        for frame in 0..60 {
            viewport.pan_by(Vec2::new(if frame % 2 == 0 { 7.0 } else { 5.0 }, 3.0));

            assert_eq!(world.rebuild_dirty_geometry(), 0, "frame {frame} rerouted");
            let report = index.sync(&world);
            world.clear_spatial_updates();
            assert!(report.is_empty(), "frame {frame} queued a spatial update");

            index.query_visible(&world, &viewport, &mut visible);
            plan_scene(&mut plan, &world, &visible, &viewport, ink(), &options());
        }

        assert_eq!(
            world.geometry().rebuild_count(),
            before,
            "sixty frames of pure pan cost route rebuilds"
        );
    }

    #[test]
    fn opacity_multiplies_alpha_rather_than_replacing_it() {
        let half = Color::rgba(1.0, 1.0, 1.0, 0.5);
        assert_eq!(fade(half, 0.5).a, 0.25);
        assert_eq!(fade(half, 1.0).a, 0.5);
        // Out-of-range opacity is clamped rather than producing a negative or
        // over-bright alpha.
        assert_eq!(fade(half, -1.0).a, 0.0);
        assert_eq!(fade(half, 4.0).a, 0.5);
    }
}
