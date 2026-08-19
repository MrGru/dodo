//! **Turning the snapshot into primitives** — the one place §16's rule is
//! spent, and the reason it can be asserted without a window.
//!
//! ```text
//! GraphWorld ─> query_visible ─> VisibleSet ─> RenderSnapshot ─> plan_scene ─> PaintPlan
//!  100,000 nodes    2.3 µs         ~36 nodes     LOD + registry     canvas half
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
//! # What Phase 5 changed: this plans the snapshot, not the visible set
//!
//! [`RenderSnapshot`] sits between, and it has already decided three things
//! this file used to decide badly or not at all:
//!
//! - **which nodes are elements** rather than primitives, so this loop skips
//!   them instead of drawing a quad under a `div`;
//! - **which edges are drawn and at what rung**, which is the only thing that
//!   bounds a hairball (see [`crate::render::lod`]);
//! - **whether a label is worth shaping**, which below readable zoom is `None`
//!   and costs nothing at all.
//!
//! What is left here is the translation from those decisions into primitives,
//! plus one thing the snapshot deliberately does not carry: **colour**. A
//! snapshot with resolved colours in it would go stale when the theme changed,
//! and dodo applies a theme change live — so [`SceneInk`] is resolved once per
//! frame by the view and passed down, which is also what keeps a theme lookup
//! out of the loop over every visible shape.
//!
//! # Two culling phases, and why both
//!
//! The [`VisibleSet`](crate::spatial::VisibleSet) is the broad phase plus a
//! world-space narrow phase, at cell granularity in the first and rectangle
//! granularity in the second. This file adds nothing to it — the third
//! rejection is [`PaintPlan::push_path`]'s, against the pane in *screen* space,
//! and it is there rather than here so that no painter, present or future, can
//! skip it.
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

use std::sync::Arc;

use crate::{
    budgets::DetailLevel,
    geometry::{Rect, Vec2, Viewport},
    models::SketchStyle,
    models::{Color, NodeIndex, RenderQuality},
    render::{
        GridLevel, GridLimits, GridSettings, PaintPlan,
        cache::{CLEAN, GeometryKey, GeometryPart, TextKey},
        edges,
        lod::HandleDetail,
        plan::{DashSpec, PathPrimitive, QuadPrimitive, TextPrimitive},
        shapes, sketch,
        snapshot::{CanvasNode, RenderSnapshot},
    },
    runtime::{GraphWorld, NodeShape},
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

/// The inset a canvas-drawn label keeps from its node's edges, in screen
/// pixels. Constant on screen for the same reason the handle radius is.
pub const LABEL_PADDING_PIXELS: f32 = 6.0;

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
    /// Canvas-drawn labels.
    pub text: Color,
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
    /// Nodes that produced at least one primitive. **Excludes rich nodes** —
    /// those are elements, and the canvas draws nothing for them.
    pub nodes: u32,
    /// Nodes the snapshot handed to `views/` as GPUI elements (§16).
    pub rich_nodes: u32,
    pub edges: u32,
    pub handles: u32,
    /// Canvas-drawn labels that reached the plan. Zero below readable zoom, by
    /// construction rather than by check — see [`crate::render::lod`].
    pub labels: u32,
    /// Visible edges the LOD ladder's count cap skipped. Non-zero only on a
    /// scene culling cannot bound.
    pub skipped_edges: u32,
    /// Visible nodes skipped because their kind has no representation yet —
    /// text, images, frames. **Not** a culling number; a not-implemented one.
    pub unsupported_nodes: u32,
    /// Node bodies drawn by §13's hand this frame, rich ones included.
    ///
    /// Reported rather than inferred from the style, because the ladder may
    /// have degraded sketch to clean — see [`crate::render::lod::LodPlan::sketch`]
    /// — and "did this frame actually draw a hand?" is the question a benchmark
    /// and the launcher's overlay ask.
    pub sketched_bodies: u32,
}

impl SceneStats {
    /// **Every visible node that was drawn**, by either half of the hybrid
    /// renderer.
    ///
    /// [`nodes`](SceneStats::nodes) alone is the *canvas* half and it is
    /// legitimately zero at full zoom, where every visible node is an element.
    /// A caller asking "did the frame draw the nodes?" wants this; a caller
    /// asking "what did the painter do?" wants the field.
    pub fn drawn_nodes(&self) -> u32 {
        self.nodes + self.rich_nodes
    }
}

/// **Plans one frame from the snapshot.**
///
/// Clears `plan` against the pane, so the clip and the extraction cannot
/// disagree about which frame they belong to, then plans the grid, the edges
/// and the nodes in that order — edges under nodes, and the paint order is
/// [`PaintPlan`]'s regardless.
///
/// Nothing here iterates the document. `snapshot` is the whole input, which is
/// what §40 rule 1 asks for and what makes the cost of this function
/// proportional to the screen rather than to the file.
pub fn plan_scene(
    plan: &mut PaintPlan,
    world: &GraphWorld,
    snapshot: &RenderSnapshot,
    viewport: &Viewport,
    ink: SceneInk,
    options: &SceneOptions,
) -> SceneStats {
    let pane = Rect::new(Vec2::ZERO, viewport.size());
    plan.clear(pane);

    let counts = snapshot.counts();
    let mut stats = SceneStats {
        grid: grid::generate(&options.grid, viewport, &options.grid_limits, plan),
        rich_nodes: counts.rich_nodes,
        skipped_edges: counts.skipped_edges,
        unsupported_nodes: counts.unsupported_nodes,
        ..SceneStats::default()
    };

    plan_edges(plan, world, snapshot, viewport, ink, &mut stats);
    plan_nodes(plan, world, snapshot, viewport, ink, &mut stats);
    plan_handles(plan, world, snapshot, viewport, ink, &mut stats);
    plan_labels(plan, world, snapshot, ink, &mut stats);
    stats
}

use crate::render::grid;

/// Every edge the snapshot kept, from its **derived** route at the frame's LOD
/// rung.
///
/// The routes were brought up to date once, at the top of the frame, by
/// [`GraphWorld::rebuild_dirty_geometry`] — so this loop rebuilds nothing and a
/// pure pan reroutes nothing (§40 rule 6).
fn plan_edges(
    plan: &mut PaintPlan,
    world: &GraphWorld,
    snapshot: &RenderSnapshot,
    viewport: &Viewport,
    ink: SceneInk,
    stats: &mut SceneStats,
) {
    let Some(lod) = snapshot.lod() else {
        return;
    };
    let quality = world.settings().render_quality;

    for planned in snapshot.edges() {
        let Some(route) = world.route(planned.edge) else {
            continue;
        };

        let style = world.edges().style(planned.edge);
        let color = if planned.selected {
            ink.accent
        } else {
            fade(style.stroke.color.unwrap_or(ink.edge), style.opacity)
        };

        let paint = edges::EdgePaint {
            color,
            width: style.stroke.width,
            // A dashed edge is the expensive kind, so it is only ever asked
            // for when the document says so — and the ladder drops it at the
            // first rung down. See `render::plan::PathPaint`.
            dash: style
                .stroke
                .dash
                .spec()
                .map(|(on, off)| DashSpec::new(on, off)),
            start_marker: style.start_marker,
            end_marker: style.end_marker,
            quality,
            detail: lod.edges,
            owner: Some((planned.edge, planned.version)),
            // The seed is the edge's own id, so an edge wobbles the same way
            // for the life of the document and differently from its neighbour
            // — see `render::sketch::element_seed`.
            sketch: lod.sketch.map(|style| {
                edges::SketchHand::new(
                    style,
                    sketch::element_seed(&style, world.edges().id(planned.edge), SKETCH_EDGE_PART),
                )
            }),
        };

        edges::plan_edge(plan, route, &paint, viewport);
        stats.edges += 1;
    }
}

/// Every canvas node, as the cheapest primitive that can draw it.
///
/// The loop reads the snapshot's compact rows: an index, a screen rectangle, a
/// one-byte body and two flags. It touches
/// [`ElementKind`](crate::models::ElementKind) — which carries a `String` — not
/// at all; the snapshot already asked the registry whatever needed asking.
/// That is what §17's cold/hot split is for and what §40 rule 9 asks.
fn plan_nodes(
    plan: &mut PaintPlan,
    world: &GraphWorld,
    snapshot: &RenderSnapshot,
    viewport: &Viewport,
    ink: SceneInk,
    stats: &mut SceneStats,
) {
    let Some(lod) = snapshot.lod() else {
        return;
    };
    let nodes = world.nodes();
    let quality = world.settings().render_quality;

    for canvas in snapshot.canvas() {
        let style = nodes.style(canvas.node);
        let screen = canvas.screen;

        // A graph node's body has a radius of its own so it reads as a node
        // rather than as a drawn rectangle; a shape uses what its style says.
        let world_radius = if canvas.body == NodeShape::GraphNode && style.corner_radius <= 0.0 {
            GRAPH_NODE_RADIUS
        } else {
            style.corner_radius
        };
        let radius = viewport.world_to_screen_length(world_radius);

        // `None` means "the theme decides", which is the whole reason
        // `models::style` stores colours as `Option<Color>`: a document must
        // not carry a palette, or it would look wrong in the other theme.
        let fill = fade(style.fill.unwrap_or(ink.fill), style.opacity);
        let stroke_color = if canvas.selected {
            ink.accent
        } else {
            fade(style.stroke.color.unwrap_or(ink.stroke), style.opacity)
        };
        let stroke_width =
            viewport
                .world_to_screen_length(style.stroke.width)
                .max(if canvas.selected {
                    SELECTED_STROKE_PIXELS
                } else {
                    0.0
                });
        // §15's "merge/simplify visual details": a node a few pixels across is
        // a filled box, because its border is the whole box at that size and
        // painting one costs a second primitive to draw nothing.
        let has_stroke = canvas.detailed
            && (!style.stroke.is_invisible() || canvas.selected)
            && stroke_width > 0.0;

        // **§13's hand, if the ladder kept it.** A node too small to be worth a
        // border is too small to be worth a wobble either, so `detailed` gates
        // it here rather than in the ladder — that is a per-node question and
        // the ladder answers per frame.
        if let Some(sketch_style) = lod.sketch.filter(|_| canvas.detailed)
            && plan_sketched_body(
                plan,
                SketchedBody {
                    node: canvas.node,
                    body: canvas.body,
                    version: canvas.version,
                    screen,
                    radius,
                    fill,
                    stroke_color,
                    stroke_width,
                    has_stroke,
                },
                &sketch_style,
                world,
                quality,
            )
        {
            stats.nodes += 1;
            stats.sketched_bodies += 1;
            continue;
        }

        // §15 and Phase 0 §3 correction 7: an ellipse is 337 vertices against a
        // rectangle's 24, so below `curve_to_quad_zoom` a curved body is
        // painted as its bounding quad rather than tessellated.
        let as_quad = shapes::node_prefers_quad(canvas.body)
            || (lod.degrade_curves && canvas.body != NodeShape::Diamond)
            || !canvas.detailed;

        if as_quad {
            // Phase 0's measurement, honoured: 20,000 quads hold 60 fps where
            // the same count of rectangular paths drop to 30 — and a quad
            // carries its corner radius and its border for free, so the border
            // costs no second primitive at all.
            let mut quad = QuadPrimitive::filled(screen, fill).with_corner_radius(radius);
            if has_stroke {
                quad = quad.with_border(stroke_width, stroke_color);
            }
            plan.push_quad(quad);
        } else if let Some(outline) = shapes::outline_for_node(canvas.body, screen, radius) {
            plan.push_path(PathPrimitive::fill(outline.clone(), fill, quality).keyed(
                GeometryKey::node(
                    canvas.node,
                    GeometryPart::Fill,
                    canvas.version,
                    quality,
                    CLEAN,
                ),
            ));

            if has_stroke {
                plan.push_path(
                    PathPrimitive::stroke(outline, stroke_color, stroke_width, quality).keyed(
                        GeometryKey::node(
                            canvas.node,
                            GeometryPart::Stroke,
                            canvas.version,
                            quality,
                            CLEAN,
                        ),
                    ),
                );
            }
        }

        stats.nodes += 1;
    }

    plan_sketched_rich_bodies(plan, world, snapshot, viewport, ink, stats);
}

/// Everything one node body needs from either half of the renderer, so the
/// sketch painter can be called from the canvas loop and from the rich one
/// without either of them growing a copy of it.
struct SketchedBody {
    node: NodeIndex,
    body: NodeShape,
    version: u32,
    screen: Rect,
    radius: f32,
    fill: Color,
    stroke_color: Color,
    stroke_width: f32,
    has_stroke: bool,
}

/// **One node body, drawn by hand** (§13). `false` if this shape has no outline
/// to perturb, in which case the caller falls back to the clean painter.
///
/// Three decisions live here and each is a cost one:
///
/// - **The fill of a quad-shaped body stays a quad**, with no border. A wobbly
///   stroke over a crisp fill is what a marker on a whiteboard looks like, and
///   it halves what a sketched node costs — see [`crate::render::sketch`].
/// - **Each stroke pass is its own path and its own cache entry**, because each
///   is a separate tessellation. [`GeometryPart::SketchStroke`] carries the
///   pass index so two passes cannot collide.
/// - **The tolerance is [`SketchStyle::quality`]'s**, not the document's: a
///   deliberately imprecise line does not need a quarter-pixel bow, and this is
///   worth about half the vertices.
fn plan_sketched_body(
    plan: &mut PaintPlan,
    body: SketchedBody,
    style: &SketchStyle,
    world: &GraphWorld,
    quality: crate::models::RenderQuality,
) -> bool {
    let Some(outline) = shapes::outline_for_node(body.body, body.screen, body.radius) else {
        return false;
    };

    let id = world.nodes().id(body.node);
    let sketch_key = style.cache_key();
    let sketch_quality = style.quality(quality);

    if shapes::node_prefers_quad(body.body) {
        plan.push_quad(
            QuadPrimitive::filled(body.screen, body.fill).with_corner_radius(body.radius),
        );
    } else {
        let seed = sketch::element_seed(style, id, SKETCH_FILL_PART);
        plan.push_path(
            PathPrimitive::fill(
                sketch::fill(&outline, style, seed),
                body.fill,
                sketch_quality,
            )
            .keyed(GeometryKey::node(
                body.node,
                GeometryPart::SketchFill,
                body.version,
                sketch_quality,
                sketch_key,
            )),
        );
    }

    if body.has_stroke {
        let seed = sketch::element_seed(style, id, SKETCH_STROKE_PART);
        for (pass, stroke) in sketch::strokes(&outline, style, seed)
            .into_iter()
            .enumerate()
        {
            plan.push_path(
                PathPrimitive::stroke(stroke, body.stroke_color, body.stroke_width, sketch_quality)
                    .keyed(GeometryKey::node(
                        body.node,
                        GeometryPart::SketchStroke(pass as u8),
                        body.version,
                        sketch_quality,
                        sketch_key,
                    )),
            );
        }
    }

    true
}

/// The part salts that keep one element's paths from wobbling in lockstep. See
/// [`crate::render::sketch::element_seed`].
const SKETCH_FILL_PART: u64 = 1;
const SKETCH_STROKE_PART: u64 = 2;
const SKETCH_EDGE_PART: u64 = 3;

/// **The rich half's bodies, when a hand is drawing them.**
///
/// A rich node is a GPUI `div`, and a `div`'s border is a rectangle — there is
/// no hand-drawn form of it. So in sketch mode the element drops its background
/// and its border ([`crate::views::nodes`] reads the same flag) and the canvas
/// paints the body underneath it, exactly as it does for every other node. The
/// element keeps what it is for: focus, hover, a cursor, editable text.
///
/// This is the one place where a rich node reaches the painter at all, and it
/// is bounded by [`RenderBudgets::max_rich_elements`](crate::budgets::RenderBudgets::max_rich_elements)
/// like the rest of the rich set.
fn plan_sketched_rich_bodies(
    plan: &mut PaintPlan,
    world: &GraphWorld,
    snapshot: &RenderSnapshot,
    viewport: &Viewport,
    ink: SceneInk,
    stats: &mut SceneStats,
) {
    let Some(lod) = snapshot.lod() else {
        return;
    };
    let Some(sketch_style) = lod.sketch else {
        return;
    };
    let quality = world.settings().render_quality;
    let nodes = world.nodes();

    for rich in snapshot.rich() {
        let style = nodes.style(rich.node);
        let world_radius = if style.corner_radius <= 0.0 {
            GRAPH_NODE_RADIUS
        } else {
            style.corner_radius
        };
        let stroke_color = if rich.selected {
            ink.accent
        } else {
            fade(style.stroke.color.unwrap_or(ink.stroke), style.opacity)
        };

        if plan_sketched_body(
            plan,
            SketchedBody {
                node: rich.node,
                body: rich.visual.body,
                version: rich.version,
                screen: rich.screen,
                radius: viewport.world_to_screen_length(world_radius),
                fill: fade(style.fill.unwrap_or(ink.fill), style.opacity),
                stroke_color,
                stroke_width: viewport
                    .world_to_screen_length(style.stroke.width)
                    .max(SELECTED_STROKE_PIXELS),
                has_stroke: true,
            },
            &sketch_style,
            world,
            quality,
        ) {
            stats.sketched_bodies += 1;
        }
    }
}

/// Handle dots, as quads (§4, §15).
///
/// **Only the nodes that do not have interactive handle elements.** §44's rule
/// is that controls belong to the selected, hovered or editing element and
/// never to every inactive one — so the active node's handles are elements in
/// `views/` and everything else's are these dots. Drawing both would draw the
/// active node's handles twice.
///
/// A circle is a quad with a corner radius of half its side, so a hundred
/// thousand of them would still be the cheap primitive.
fn plan_handles(
    plan: &mut PaintPlan,
    world: &GraphWorld,
    snapshot: &RenderSnapshot,
    viewport: &Viewport,
    ink: SceneInk,
    stats: &mut SceneStats,
) {
    let Some(lod) = snapshot.lod() else {
        return;
    };
    if lod.handles == HandleDetail::Hidden {
        // §15, exactly as written: do not create handles unless interaction
        // needs them. At overview zoom a handle is smaller than a pixel.
        return;
    }

    let elements_belong_to = snapshot
        .interactive_handles()
        .first()
        .map(|handle| handle.node);

    let radius = HANDLE_SCREEN_RADIUS;
    let mut painted = 0;

    let canvas_nodes = snapshot
        .canvas()
        .iter()
        .filter(|node| node.detailed)
        .map(|node| node.node);
    let rich_nodes = snapshot.rich().iter().map(|node| node.node);

    for node in canvas_nodes.chain(rich_nodes) {
        if Some(node) == elements_belong_to {
            continue;
        }

        for handle in world.nodes().handles(node) {
            if world.handles().is_hidden(handle) {
                // §4: hidden handles stay connectable. Only the paint is
                // skipped — routing and hit-testing never read this flag.
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
    }

    stats.handles = painted;
}

/// Canvas-drawn labels (§9), for the nodes that are not elements.
///
/// A rich node's label lives inside its element, where it can be selected and
/// edited; this is the compact representation for everything else. The font
/// size is **already quantised** onto the LOD ladder by the snapshot — see
/// [`crate::render::lod`] — and `None` means the label is not shaped at all,
/// which is §15's "do not lay out rich text that cannot be read" costing
/// exactly nothing.
fn plan_labels(
    plan: &mut PaintPlan,
    world: &GraphWorld,
    snapshot: &RenderSnapshot,
    ink: SceneInk,
    stats: &mut SceneStats,
) {
    let Some(lod) = snapshot.lod() else {
        return;
    };
    if lod.detail == DetailLevel::Overview {
        return;
    }

    for canvas in snapshot.canvas() {
        let Some(font_size) = canvas.label_font_size else {
            continue;
        };
        let Some(label) = world.nodes().cold(canvas.node).label.as_ref() else {
            continue;
        };

        let inner = canvas.screen.inflate(-LABEL_PADDING_PIXELS);
        if inner.size.x <= 0.0 || inner.size.y <= 0.0 {
            continue;
        }

        plan.push_text(TextPrimitive {
            // Vertically centred on the body, which is where a node's label
            // belongs; the painter subtracts the line's own height.
            origin: Vec2::new(inner.origin.x, canvas.screen.center().y - font_size * 0.5),
            // An `Arc` clone: a refcount bump, not a `String` allocation per
            // label per frame. See `NodeCold::label`.
            text: Arc::clone(label),
            font_size,
            color: ink.text,
            key: TextKey::new(canvas.node, canvas.version, font_size),
            max_width: inner.size.x,
        });
        stats.labels += 1;
    }
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

/// A canvas node's on-screen rectangle, for a caller that has one row and wants
/// its geometry without re-deriving it from the world.
pub fn canvas_node_bounds(node: &CanvasNode) -> Rect {
    node.screen
}

/// The node a plan's interactive handles belong to, if any. Exposed so a view
/// and this file cannot disagree about which node is skipping its dots.
pub fn active_handle_node(snapshot: &RenderSnapshot) -> Option<NodeIndex> {
    snapshot
        .interactive_handles()
        .first()
        .map(|handle| handle.node)
}

/// The quality a node's body is tessellated at. One place, so the plan and any
/// cache lookup cannot disagree.
pub fn node_quality(world: &GraphWorld) -> RenderQuality {
    world.settings().render_quality
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        budgets::{RenderBackend, for_backend},
        models::{ElementKind, GraphNodeKind},
        render::GridStyle,
        runtime::{ConnectionRules, EdgeEnd},
        spatial::{SpatialIndex, VisibleSet},
    };

    fn ink() -> SceneInk {
        SceneInk {
            fill: Color::rgb(0.2, 0.2, 0.2),
            stroke: Color::rgb(0.9, 0.9, 0.9),
            edge: Color::rgb(0.7, 0.7, 0.7),
            handle: Color::rgb(0.3, 0.6, 1.0),
            accent: Color::rgb(1.0, 0.6, 0.2),
            text: Color::rgb(0.95, 0.95, 0.95),
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

    /// A snapshot for one already-queried visible set.
    fn snapshot_of(
        world: &GraphWorld,
        visible: &VisibleSet,
        viewport: &Viewport,
    ) -> RenderSnapshot {
        let mut snapshot = RenderSnapshot::new();
        snapshot.extract(
            world,
            visible,
            viewport,
            &for_backend(RenderBackend::Metal),
            &crate::render::registry::NodeRendererRegistry::with_generic_kinds(),
            None,
            Rect::new(Vec2::ZERO, viewport.size()),
        );
        snapshot
    }

    /// One frame, end to end: query, extract, plan. The snapshot is built here
    /// rather than passed in because these tests are about what reaches the
    /// *plan*, and the snapshot is now part of how it gets there.
    fn frame(world: &GraphWorld, viewport: &Viewport) -> (PaintPlan, SceneStats) {
        let index = SpatialIndex::for_world(world);
        let mut visible = crate::spatial::VisibleSet::new();
        index.query_visible(world, viewport, &mut visible);

        let mut snapshot = RenderSnapshot::new();
        snapshot.extract(
            world,
            &visible,
            viewport,
            &for_backend(RenderBackend::Metal),
            &crate::render::registry::NodeRendererRegistry::with_generic_kinds(),
            None,
            Rect::new(Vec2::ZERO, viewport.size()),
        );

        let mut plan = PaintPlan::new();
        let stats = plan_scene(&mut plan, world, &snapshot, viewport, ink(), &options());
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

        assert_eq!(small_stats.drawn_nodes(), large_stats.drawn_nodes());
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
            stats.drawn_nodes() < 100,
            "{} of 40,000 nodes were planned at 1:1",
            stats.drawn_nodes()
        );
        assert!(stats.drawn_nodes() > 0);
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
            stats.drawn_nodes() > 1_000,
            "the dense scene should make thousands visible, not {}",
            stats.drawn_nodes()
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

        assert_eq!(with_hidden.drawn_nodes() + 1, without.drawn_nodes());
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
        let snapshot = snapshot_of(&world, &visible, &viewport);
        let mut plan = PaintPlan::new();
        plan_scene(&mut plan, &world, &snapshot, &viewport, ink(), &options);

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

            let snapshot = snapshot_of(&world, &visible, &viewport);
            let mut plan = PaintPlan::new();
            plan_scene(&mut plan, &world, &snapshot, &viewport, ink(), &options());

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
            let snapshot = snapshot_of(&world, &visible, &viewport);
            plan_scene(&mut plan, &world, &snapshot, &viewport, ink(), &options());
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

    // ---- §13: the sketch renderer ---------------------------------------

    /// A world drawn by hand, from the same document as `document`.
    fn sketched(columns: u32, rows: u32) -> GraphWorld {
        let mut world = document(columns, rows);
        world.settings_mut().render_style = crate::models::RenderStyle::Sketch;
        world
    }

    fn viewport() -> Viewport {
        Viewport::new(Vec2::new(-200.0, -120.0), 1.0, Vec2::new(1_440.0, 900.0))
    }

    /// **§13's central claim, as a test**: the same document, the same visible
    /// set, the same elements — a different *drawing*.
    ///
    /// Not "more paths": the same nodes and the same edges, drawn by a hand.
    /// A sketch mode that quietly dropped or added elements would be a second
    /// document, which is exactly what §13 says it must not be.
    #[test]
    fn sketch_draws_the_same_scene_by_hand() {
        let clean = frame(&document(6, 4), &viewport());
        let sketch = frame(&sketched(6, 4), &viewport());

        assert_eq!(clean.1.drawn_nodes(), sketch.1.drawn_nodes());
        assert_eq!(clean.1.edges, sketch.1.edges);
        assert_eq!(clean.1.skipped_edges, sketch.1.skipped_edges);

        assert!(sketch.1.sketched_bodies > 0, "no body was drawn by hand");
        assert_eq!(clean.1.sketched_bodies, 0);
        assert!(
            sketch.0.paths().len() > clean.0.paths().len(),
            "a hand-drawn scene is more paths than a clean one: {} vs {}",
            sketch.0.paths().len(),
            clean.0.paths().len()
        );
    }

    /// Every node body a hand drew is `stroke_count` paths, and every one of
    /// them carries a cache key that says which pass it is. Two passes filed
    /// under one key would paint the same squiggle twice.
    #[test]
    fn every_sketch_pass_is_its_own_cache_entry() {
        let world = sketched(3, 2);
        let (plan, _) = frame(&world, &viewport());

        let keys: Vec<_> = plan.paths().iter().filter_map(|path| path.key).collect();
        let sketch_keys: Vec<_> = keys
            .iter()
            .filter(|key| matches!(key.part, GeometryPart::SketchStroke(_)))
            .collect();

        assert!(
            !sketch_keys.is_empty(),
            "nothing was keyed as a sketch pass"
        );
        let mut unique = sketch_keys.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(
            unique.len(),
            sketch_keys.len(),
            "two sketch passes shared a cache key"
        );
        assert!(
            sketch_keys.iter().all(|key| key.sketch != CLEAN),
            "a sketched path was keyed as a clean one, so a toggle would serve it"
        );
    }

    /// **§13's other half, and §40 rule 5: a repaint regenerates nothing.**
    ///
    /// Two identical frames through a real geometry cache: everything the
    /// second frame asks for is already there, so not one sketch outline is
    /// tessellated twice. The stand-in geometry is a vertex buffer, which is
    /// all the cache's policy operates on — see `render::cache`.
    #[test]
    fn a_repaint_regenerates_no_sketch_geometry() {
        let world = sketched(6, 4);
        let view = viewport();
        let budgets = for_backend(RenderBackend::Metal);
        let mut cache: crate::render::cache::GeometryCache<FakeGeometry> =
            crate::render::cache::GeometryCache::new(&budgets);

        let mut tessellations = Vec::new();
        for _ in 0..3 {
            let (plan, _) = frame(&world, &view);
            cache.begin_frame(crate::render::cache::ScreenAnchor::of(&view), false);

            let mut built = 0;
            for path in plan.paths() {
                let Some(key) = path.key else {
                    continue;
                };
                if cache.get(&key).is_none() {
                    built += 1;
                    cache.insert(key, FakeGeometry(path.outline.flattened_points(0.25)));
                }
            }
            cache.end_frame();
            tessellations.push(built);
        }

        assert!(tessellations[0] > 0, "the first frame must build something");
        assert_eq!(
            &tessellations[1..],
            &[0, 0],
            "a repaint rebuilt sketch geometry that had not changed: {tessellations:?}"
        );
    }

    /// The determinism property where it actually matters: **through the whole
    /// planning path**, not only through the generator. Two frames of an
    /// unchanged scene must plan bit-identical outlines, or the cache above is
    /// hitting on a key whose geometry moved.
    #[test]
    fn two_frames_of_an_unchanged_scene_plan_identical_geometry() {
        let world = sketched(6, 4);
        let view = viewport();

        let first = frame(&world, &view).0;
        let second = frame(&world, &view).0;

        assert_eq!(first.paths().len(), second.paths().len());
        for (a, b) in first.paths().iter().zip(second.paths()) {
            assert_eq!(
                a.outline.commands(),
                b.outline.commands(),
                "a repaint drew a different squiggle"
            );
        }
    }

    /// Moving a node redraws *that* node's hand and nobody else's — the sketch
    /// version of §19's rule, and what keeps a drag from re-tessellating the
    /// whole screen.
    #[test]
    fn moving_one_node_redraws_only_its_own_hand() {
        let mut world = sketched(6, 4);
        let view = viewport();
        let before: Vec<_> = frame(&world, &view)
            .0
            .paths()
            .iter()
            .filter_map(|path| path.key)
            .collect();

        // A node the camera can actually see — the visible set at this
        // viewport starts at index 7.
        world.move_node(NodeIndex::new(8), Vec2::new(12.0, 7.0));
        world.rebuild_dirty_geometry();
        world.clear_spatial_updates();

        let after: Vec<_> = frame(&world, &view)
            .0
            .paths()
            .iter()
            .filter_map(|path| path.key)
            .collect();

        let changed = after.iter().filter(|key| !before.contains(key)).count();
        assert!(changed > 0, "the moved node must get a new key");
        assert!(
            changed < after.len() / 2,
            "moving one node invalidated {changed} of {} keys",
            after.len()
        );
    }

    /// **The zoom rule, end to end**: a sketch stroke at overview zoom is
    /// invisible detail bought at several times the price, so the ladder draws
    /// clean and the plan says so.
    #[test]
    fn the_ladder_degrades_sketch_to_clean_when_zoomed_out() {
        let world = sketched(6, 4);

        for zoom in [1.0, 0.6, 0.4] {
            let view = Viewport::new(Vec2::ZERO, zoom, Vec2::new(1_440.0, 900.0));
            assert!(
                frame(&world, &view).1.sketched_bodies > 0,
                "sketch should survive at zoom {zoom}"
            );
        }

        for zoom in [0.3, 0.15, 0.05] {
            let view = Viewport::new(Vec2::ZERO, zoom, Vec2::new(1_440.0, 900.0));
            let (plan, stats) = frame(&world, &view);
            assert_eq!(
                stats.sketched_bodies, 0,
                "sketch should be degraded to clean at zoom {zoom}"
            );
            // And it must be genuinely cheaper, not merely renamed.
            let clean = frame(&document(6, 4), &view).0;
            assert_eq!(
                plan.estimated_path_vertices(),
                clean.estimated_path_vertices()
            );
        }
    }

    /// A vertex buffer and nothing else — the same stand-in `render::cache`'s
    /// own tests use, because the cache's policy never looks at geometry.
    #[derive(Debug, Clone, PartialEq)]
    struct FakeGeometry(u32);

    impl crate::render::cache::CachedGeometry for FakeGeometry {
        fn vertex_count(&self) -> u32 {
            self.0
        }

        fn transform(&mut self, _scale: f32, _offset: Vec2) {}
    }
}
