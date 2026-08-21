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
    models::{Color, NodeIndex, RenderQuality},
    models::{SketchStyle, Sloppiness},
    render::{
        GridLevel, GridLimits, GridSettings, PaintPlan,
        cache::{CLEAN, GeometryKey, GeometryPart, TextKey},
        edges, hatch,
        lod::{HandleDetail, LodPlan},
        plan::{DashSpec, ImagePrimitive, PathPrimitive, QuadPrimitive, TextPrimitive},
        shapes, sketch,
        snapshot::{CanvasNode, PlannedEdge, RenderSnapshot, RichNode},
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

/// The width a hatch line is stroked at, in screen pixels.
///
/// Thinner than a body's border on purpose: a hatch reads as shading, and at a
/// border's weight it reads as a second shape drawn inside the first.
pub const HATCH_STROKE_PIXELS: f32 = 1.0;

/// The outline width of a selected element, in screen pixels. Constant on
/// screen rather than in world units, so selection stays visible at any zoom.
pub const SELECTED_STROKE_PIXELS: f32 = 2.0;

/// The narrowest an **open** shape's stroke may be drawn, in screen pixels.
///
/// A closed body whose stroke is dropped still reads as itself — §15 drops it
/// on purpose. A line whose stroke is dropped is gone, so this is the floor
/// that keeps a hairline visible at any zoom and under any style. On screen
/// rather than in world units for the same reason
/// [`SELECTED_STROKE_PIXELS`] is.
pub const MIN_OPEN_STROKE_PIXELS: f32 = 1.0;

/// The inset a canvas-drawn label keeps from its node's edges, in screen
/// pixels. Constant on screen for the same reason the handle radius is.
pub const LABEL_PADDING_PIXELS: f32 = 6.0;

/// The widest an **edge** label is laid out into, in screen pixels.
///
/// An edge has no rectangle, so unlike a node's label there is nothing to
/// inherit a width from. A number rather than "as wide as it likes" because
/// `ShapedLine::paint` needs a wrap width and because an unbounded label on a
/// short edge would run across half the diagram. On screen rather than in world
/// units, like every other text measurement here: what matters is how much of
/// the *view* one label may cover.
pub const EDGE_LABEL_MAX_PIXELS: f32 = 220.0;

/// The **height** of that invented box, in screen pixels — and the whole of
/// what vertical alignment means to a carrier with no rectangle of its own.
///
/// An edge and a straight connector are lines: there is no box, so a label gets
/// one made for it, centred on the midpoint. [`EDGE_LABEL_MAX_PIXELS`] is how
/// wide that box is and this is how tall, which makes `Top` and `Bottom` "above
/// the line" and "below the line" — the reading a person expects of an arrow's
/// label — while `Middle` puts the block exactly on the midpoint, where every
/// edge label has been drawn since §9.
///
/// Four lines of a default label fit inside it, so a wrapped label is centred
/// on the line rather than clamped to the top of the band. Past that it
/// overflows downwards, like every other block that outgrows its box.
pub const EDGE_LABEL_BAND_PIXELS: f32 = 96.0;

/// **The box a label gets when its carrier has no rectangle** — an edge, or a
/// straight connector whose derived box collapses to a line.
///
/// One function rather than the same two lines in `plan_labels` and
/// `plan_edge_labels`, because those two are the same rule seen from either
/// side of the node/edge split and the inline editor
/// (`views::flow`'s `text_edit_bounds`) is a third statement of it. A label that
/// is laid out into one box and edited over another is what the previous slice
/// spent a fix on.
pub fn boxless_label_box(center: Vec2) -> Rect {
    let size = Vec2::new(EDGE_LABEL_MAX_PIXELS, EDGE_LABEL_BAND_PIXELS);
    Rect::new(center - size * 0.5, size)
}

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
    /// frames, freehand, an unregistered custom kind. **Not** a culling number;
    /// a not-implemented one.
    pub unsupported_nodes: u32,
    /// §10's pictures that reached the plan this frame. They cost no path
    /// vertices and no path batch — see [`render::plan`](crate::render::plan)
    /// for what they do cost.
    pub images: u32,
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

    plan_bodies(plan, world, snapshot, viewport, ink, &mut stats);
    plan.set_images_after_paths(images_belong_above_paths(world, snapshot));
    plan_handles(plan, world, snapshot, viewport, ink, &mut stats);
    plan_labels(plan, world, snapshot, viewport, ink, &mut stats);
    plan_edge_labels(plan, world, snapshot, viewport, ink, &mut stats);
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
        plan_one_edge(plan, world, planned, viewport, ink, &lod, quality, stats);
    }
}

/// One edge, from its derived route.
#[allow(clippy::too_many_arguments)]
fn plan_one_edge(
    plan: &mut PaintPlan,
    world: &GraphWorld,
    planned: &PlannedEdge,
    viewport: &Viewport,
    ink: SceneInk,
    lod: &LodPlan,
    quality: crate::models::RenderQuality,
    stats: &mut SceneStats,
) {
    {
        let Some(route) = world.route(planned.edge) else {
            return;
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
            // The edge presses as hard as *it* asks, exactly as a node does —
            // see `pressed`. An edge and the node it joins can sit at two
            // sloppinesses and each is drawn at its own.
            sketch: lod.sketch.map(|hand| {
                let style = pressed(hand, style.sloppiness);
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
    let quality = world.settings().render_quality;

    let boxed = snapshot.overlay().map(|overlay| overlay.node);

    for canvas in snapshot.canvas() {
        plan_one_node(
            plan,
            world,
            NodeBody::of_canvas(canvas, boxed),
            viewport,
            ink,
            &lod,
            quality,
            stats,
        );
    }

    plan_rich_bodies(plan, world, snapshot, viewport, ink, stats);
}

/// **One body to paint, from whichever half of the hybrid renderer owns the
/// element.**
///
/// The two halves used to disagree about what a body *is*. A canvas node was
/// painted from its [`ElementStyle`](crate::models::ElementStyle) — its stroke
/// colour, its fill, its width, its opacity, its hatch — and a rich node was a
/// `div` painted from the *theme*, so Phase 11's whole property panel reached
/// one of them and not the other. In Clean mode a rectangle at working zoom is
/// a rich node, which is exactly the element a person selects and restyles, and
/// nothing changed on screen. In Sketch mode the same edit worked, because a
/// hand-drawn border has no `div` form and the canvas had to paint it — so the
/// asymmetry read as "properties only apply in Sketch mode".
///
/// This type is the fix stated structurally: **there is one body painter and
/// both halves call it.** The rich half keeps what an element is *for* — focus,
/// hover, a cursor, a label that can be edited — and gives up the two
/// properties a `div` can only express in the theme's terms.
#[derive(Debug, Clone, Copy)]
struct NodeBody {
    node: NodeIndex,
    /// The body to paint — from the registry when it overrode one.
    body: NodeShape,
    /// What an unset fill means for this kind — see [`CanvasNode::filled`].
    filled: bool,
    /// The geometry version, for §23's cache key.
    version: u32,
    screen: Rect,
    selected: bool,
    /// **§44's hover, and it only ever comes from the rich half.** A canvas
    /// node has no element to hover, and the snapshot answers the question for
    /// one node at most.
    hovered: bool,
    /// **Whether §44's bounding box is drawn around this element this frame** —
    /// which is [`RenderSnapshot::overlay`]'s node and nothing else. See
    /// [`NodeBody::accented`] for the one question it answers.
    boxed: bool,
    detailed: bool,
    /// Which half of the renderer this body belongs to. It changes nothing
    /// about the painting — see [`NodeBody::count`] for the one thing it
    /// decides.
    rich: bool,
}

impl NodeBody {
    fn of_canvas(canvas: &CanvasNode, boxed: Option<NodeIndex>) -> NodeBody {
        NodeBody {
            node: canvas.node,
            body: canvas.body,
            filled: canvas.filled,
            version: canvas.version,
            screen: canvas.screen,
            selected: canvas.selected,
            hovered: false,
            boxed: boxed == Some(canvas.node),
            detailed: canvas.detailed,
            rich: false,
        }
    }

    fn of_rich(rich: &RichNode, boxed: Option<NodeIndex>) -> NodeBody {
        NodeBody {
            node: rich.node,
            body: rich.visual.body,
            filled: rich.visual.filled,
            version: rich.version,
            screen: rich.screen,
            selected: rich.selected,
            hovered: rich.hovered,
            boxed: boxed == Some(rich.node),
            // A node is only `rich_capable` when it is already detailed — see
            // `RenderSnapshot::extract_nodes` — so this is a restatement rather
            // than an assumption.
            detailed: true,
            rich: true,
        }
    }

    /// **Whether this body paints the accent instead of its own stroke** — a
    /// colour, a two-pixel floor, a border where the style asked for none, and
    /// no dash.
    ///
    /// It used to be "selected or hovered", and for the selected element that
    /// was a real defect: recolouring a border is the one thing that hides the
    /// colour a person is choosing, and the colour they are choosing is
    /// *always* the selected element's, because that is what the property panel
    /// is a view of. The bounding box says "selected" on its own — that is what
    /// §44 built it for — so an element that has one does not need its border
    /// repurposed as a second, lossier way of saying the same thing.
    ///
    /// **The two cases that keep it, and why neither is an oversight.**
    ///
    /// A body that is selected and has *no* box: §44 fills the overlay for the
    /// single selected node only, so a rubber band over five shapes draws no
    /// box at all. There the accent is not a redundant signal, it is the only
    /// one — dropping it makes a multiple selection invisible, and the
    /// bounding box is not this fix's to widen.
    ///
    /// A hovered body: hover has no box either, and its other affordance is
    /// §44's handle elements — which a *drawn shape has none of*, by
    /// `commands::gesture`'s `handles_for`. Removing the accent from hover
    /// therefore leaves a hovered rectangle with nothing but a cursor change.
    /// Hover is also transient and follows the pointer, so it cannot sit over
    /// the colour a person is picking the way a selection does.
    fn accented(&self) -> bool {
        self.hovered || (self.selected && !self.boxed)
    }

    /// Records this body against the half it came from.
    ///
    /// [`SceneStats::nodes`] is the **canvas** half's count and
    /// [`SceneStats::rich_nodes`] is the snapshot's, and
    /// [`SceneStats::drawn_nodes`] adds them — so a rich body, which now
    /// reaches the painter in both render styles, must not be counted here as
    /// well or every rich node is drawn twice as far as any caller can tell.
    fn count(&self, stats: &mut SceneStats) {
        if !self.rich {
            stats.nodes += 1;
        }
    }
}

/// One canvas node, as the cheapest primitive that **the frame's depth order
/// allows**.
///
/// The second half of that sentence is this phase's, and
/// [`promotes_to_path`](crate::render::scene::promotes_to_path) is where it is
/// decided.
#[allow(clippy::too_many_arguments)]
fn plan_one_node(
    plan: &mut PaintPlan,
    world: &GraphWorld,
    canvas: NodeBody,
    viewport: &Viewport,
    ink: SceneInk,
    lod: &LodPlan,
    quality: crate::models::RenderQuality,
    stats: &mut SceneStats,
) {
    let nodes = world.nodes();
    {
        // **A text element is its glyphs and nothing else** (§9). Skipped here
        // rather than allowed to fall through, because the fall-through is not
        // harmless: at low zoom or at a small size `as_quad` below is true for
        // *every* body, so a text element would paint as a solid rectangle the
        // moment it stopped being detailed. `plan_labels` is what draws it.
        if canvas.body == NodeShape::Text {
            return;
        }

        // **A picture is its pixels**, and it is skipped here for a stronger
        // version of the same reason: the fall-through would paint a solid box
        // over the photograph at the first rung down. `plan_image` is what
        // draws it.
        if canvas.body == NodeShape::Image {
            plan_image(plan, world, &canvas, viewport, ink, quality, stats);
            return;
        }

        let style = nodes.style(canvas.node);
        let screen = canvas.screen;
        let connector = nodes.connector(canvas.node).map(|connector| {
            [
                viewport.world_to_screen(connector.start.point),
                viewport.world_to_screen(connector.end.point),
            ]
        });

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
        // **The document wins, and the registry says what "unset" means.**
        // `None` is "the theme decides", which is the whole reason
        // `models::style` stores colours as `Option<Color>` — and for a kind
        // whose body is an outline holding other nodes, what the theme decides
        // is *nothing*, or the group swallows its children.
        let fill = fade(
            style.fill.unwrap_or(if canvas.filled {
                ink.fill
            } else {
                Color::TRANSPARENT
            }),
            style.opacity,
        );
        // **The accent, for the affordances that have nothing else.** §44's
        // hover ring used to be the rich element's own border colour; the body
        // is painted here for both halves now, so the feedback is painted here
        // too — otherwise moving the pointer over a node would say nothing.
        //
        // **Not for the element the bounding box is around**, which is the
        // element a person is styling: see [`NodeBody::accented`] for the whole
        // rule and for the two cases that still take the accent.
        let stroke_color = if canvas.accented() {
            ink.accent
        } else {
            fade(style.stroke.color.unwrap_or(ink.stroke), style.opacity)
        };
        // **An open shape is nothing but its stroke** — see
        // [`shapes::is_open`] for the three decisions that follow from it, and
        // why each is wrong by default.
        let open = shapes::is_open(canvas.body);
        let stroke_width = viewport
            .world_to_screen_length(style.stroke.width)
            .max(if open {
                MIN_OPEN_STROKE_PIXELS
            } else if canvas.accented() {
                SELECTED_STROKE_PIXELS
            } else {
                0.0
            });
        // §15's "merge/simplify visual details": a node a few pixels across is
        // a filled box, because its border is the whole box at that size and
        // painting one costs a second primitive to draw nothing. A line has no
        // box to fall back to, so it keeps its stroke at every rung.
        let has_stroke = open
            || (canvas.detailed
                && (!style.stroke.is_invisible() || canvas.accented())
                && stroke_width > 0.0);

        // **A body's dash** (§32's Stroke style row). It was stored, undoable,
        // read back by the panel and painted by nothing — the same defect
        // Phase 11 found for `fill_style`, on the row beside it, and the
        // edge painter had had it since Phase 3.
        //
        // Dropped below `detailed` and while the accent is showing, both for
        // the reason `render::edges` drops it at the first rung down: a dash
        // costs ~63x the vertices of the same line solid, and it says nothing
        // at a size where the border *is* the box. A selection ring that
        // flickered dashed would also be reporting the element's style where it
        // is supposed to be reporting the selection.
        let dash = style
            .stroke
            .dash
            .spec()
            .filter(|_| has_stroke && canvas.detailed && !canvas.accented())
            .map(|(on, off)| {
                DashSpec::new(
                    viewport.world_to_screen_length(on),
                    viewport.world_to_screen_length(off),
                )
            });

        // **§13's hand, if the ladder kept it.** A node too small to be worth a
        // border is too small to be worth a wobble either, so `detailed` gates
        // it here rather than in the ladder — that is a per-node question and
        // the ladder answers per frame.
        // **The element's own hand, not the frame's** (Phase 11). The document
        // decides what a hand looks like and each element says how hard it
        // presses, so the Sloppiness row multiplies the roughness rather than
        // replacing the style. `Artist` is `1.0` and therefore the identity,
        // which is what lets this be added to a format that already has
        // documents in it — and the scaled style is what
        // `SketchStyle::cache_key` is taken from, so two elements at two
        // sloppinesses cannot serve each other's squiggle.
        let hand = lod.sketch.map(|sketch| pressed(sketch, style.sloppiness));

        if let Some(sketch_style) = hand.filter(|_| canvas.detailed)
            && plan_sketched_body(
                plan,
                SketchedBody {
                    node: canvas.node,
                    body: canvas.body,
                    version: canvas.version,
                    screen,
                    connector,
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
            canvas.count(stats);
            stats.sketched_bodies += 1;
            return;
        }

        // §15 and Phase 0 §3 correction 7: an ellipse is 337 vertices against a
        // rectangle's 24, so below `curve_to_quad_zoom` a curved body is
        // painted as its bounding quad rather than tessellated.
        let as_quad = !open
            && (shapes::node_prefers_quad(canvas.body)
                || (lod.degrade_curves && canvas.body != NodeShape::Diamond)
                || !canvas.detailed)
            && !promotes_to_path(world, &canvas, lod);

        // **A hatched interior is a line set, not a fill** (Phase 11's Fill
        // row). The body still draws its border; what changes is that the
        // inside is drawn *with* the fill colour rather than flooded in it. So
        // the solid fill is suppressed and one stroked path takes its place —
        // see `render::hatch` for why it is one path and not one per line.
        //
        // `detailed` gates it for the same reason it gates the hand: an eight
        // pixel body has no interior to hatch, and the honest simplification at
        // that size is the solid box §15 already draws.
        let hatched = !open && canvas.detailed && style.fill_style.is_hatched();

        if as_quad {
            // Phase 0's measurement, honoured: 20,000 quads hold 60 fps where
            // the same count of rectangular paths drop to 30 — and a quad
            // carries its corner radius and its border for free, so the border
            // costs no second primitive at all.
            //
            // **Except a dashed one**, which a quad cannot carry: the body
            // keeps its quad *fill* and its border becomes one path beside it,
            // the same split a sketched body already makes. One path per dashed
            // body, and only for a body the document asked to be dashed.
            let mut quad =
                QuadPrimitive::filled(screen, if hatched { Color::TRANSPARENT } else { fill })
                    .with_corner_radius(radius);
            if has_stroke && dash.is_none() {
                quad = quad.with_border(stroke_width, stroke_color);
            }
            plan.push_quad(quad);
            if hatched {
                plan_hatch(
                    plan,
                    &canvas,
                    connector,
                    radius,
                    fill,
                    style.fill_style,
                    quality,
                );
            }
            if let Some(dash) = dash
                && let Some(outline) = outline_for_body(&canvas, connector, radius)
            {
                plan.push_path(
                    PathPrimitive::dashed_stroke(
                        outline,
                        stroke_color,
                        stroke_width,
                        dash,
                        quality,
                    )
                    .keyed(GeometryKey::node(
                        canvas.node,
                        GeometryPart::Stroke,
                        canvas.version,
                        quality,
                        CLEAN,
                    )),
                );
            }
        } else if let Some(outline) = outline_for_body(&canvas, connector, radius) {
            if !open && !hatched {
                plan.push_path(PathPrimitive::fill(outline.clone(), fill, quality).keyed(
                    GeometryKey::node(
                        canvas.node,
                        GeometryPart::Fill,
                        canvas.version,
                        quality,
                        CLEAN,
                    ),
                ));
            }
            if hatched {
                plan_hatch(
                    plan,
                    &canvas,
                    connector,
                    radius,
                    fill,
                    style.fill_style,
                    quality,
                );
            }

            if has_stroke {
                let stroke = match dash {
                    Some(dash) => PathPrimitive::dashed_stroke(
                        outline,
                        stroke_color,
                        stroke_width,
                        dash,
                        quality,
                    ),
                    None => PathPrimitive::stroke(outline, stroke_color, stroke_width, quality),
                };
                plan.push_path(stroke.keyed(GeometryKey::node(
                    canvas.node,
                    GeometryPart::Stroke,
                    canvas.version,
                    quality,
                    CLEAN,
                )));
            }
        }

        canvas.count(stats);
    }
}

/// **§10's picture, and the ring that has to be drawn over it.**
///
/// Three decisions, and the second and third are the ones that would be got
/// wrong by writing the obvious thing:
///
/// 1. **No `detailed` gate.** [`min_detailed_node_px`](crate::budgets::LodThresholds::min_detailed_node_px)
///    asks whether a body has room for a border and a line of label *inside*
///    it. A picture's legibility question is not that one — a thumbnail is
///    still a thumbnail — so the gate does not apply, exactly as Phase 10 found
///    for a standalone text element. It is the same threshold silently
///    excluding a kind that did not exist when it was written, and the crate
///    doc predicted this one.
/// 2. **The selection ring is a stroked path, not the body's border.** Every
///    other kind shows selection by painting its own outline in the accent
///    colour; a picture has no outline, and a *quad* border would be painted
///    before the image run and therefore underneath the photograph. A path is
///    the one primitive that is emitted after the pictures.
/// 3. **A missing resource is an empty frame, not nothing.** An element whose
///    handle names no resource — a hand-edited file, a merge that took the
///    nodes and left the pictures — still occupies its rectangle and is still
///    selectable and deletable. Drawing nothing would leave something that can
///    be clicked and cannot be seen.
#[allow(clippy::too_many_arguments)]
fn plan_image(
    plan: &mut PaintPlan,
    world: &GraphWorld,
    canvas: &NodeBody,
    viewport: &Viewport,
    ink: SceneInk,
    quality: crate::models::RenderQuality,
    stats: &mut SceneStats,
) {
    let nodes = world.nodes();
    let style = nodes.style(canvas.node);
    let screen = canvas.screen;
    let radius = viewport.world_to_screen_length(style.corner_radius);

    match nodes.cold(canvas.node).image {
        Some(image) => {
            plan.push_image(ImagePrimitive {
                bounds: screen,
                node: canvas.node,
                image,
                opacity: style.opacity.clamp(0.0, 1.0),
                corner_radius: radius,
            });
            stats.images += 1;
        }
        None => {
            plan.push_quad(
                QuadPrimitive::filled(screen, Color::TRANSPARENT)
                    .with_corner_radius(radius)
                    .with_border(1.0, fade(ink.stroke, style.opacity)),
            );
        }
    }

    if canvas.selected
        && let Some(outline) = shapes::outline_for_node(
            NodeShape::RoundedRectangle,
            screen.inflate(SELECTED_STROKE_PIXELS),
            radius,
        )
    {
        plan.push_path(
            PathPrimitive::stroke(outline, ink.accent, SELECTED_STROKE_PIXELS, quality).keyed(
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

    canvas.count(stats);
}

/// **The document's hand, pressed as hard as one element asks.**
///
/// A free function so the node loop and the edge loop cannot disagree about
/// what a [`Sloppiness`] step means — the two are far apart in this file and a
/// multiplication written twice is a multiplication that stops matching.
fn pressed(sketch: SketchStyle, sloppiness: Sloppiness) -> SketchStyle {
    SketchStyle {
        roughness: sketch.roughness * sloppiness.roughness_scale(),
        ..sketch
    }
}

/// One shape's hatched interior, as a single cached path.
///
/// The spacing is in **screen** pixels and does not scale with the zoom, for
/// the reason [`hatch::DEFAULT_SPACING`](crate::render::hatch::DEFAULT_SPACING)
/// gives — which also means the geometry is only valid at the zoom it was built
/// at, so the cache key carries the node's version and the same anchor every
/// other cached path does.
fn outline_for_body(
    body: &NodeBody,
    connector: Option<[Vec2; 2]>,
    radius: f32,
) -> Option<shapes::Outline> {
    match connector {
        Some([start, end]) => shapes::outline_for_connector(body.body, start, end),
        None => shapes::outline_for_node(body.body, body.screen, radius),
    }
}

fn plan_hatch(
    plan: &mut PaintPlan,
    canvas: &NodeBody,
    connector: Option<[Vec2; 2]>,
    radius: f32,
    color: Color,
    style: crate::models::FillStyle,
    quality: crate::models::RenderQuality,
) {
    let Some(outline) = outline_for_body(canvas, connector, radius) else {
        return;
    };
    let lines = hatch::hatch(&outline, style, hatch::DEFAULT_SPACING);
    if lines.is_empty() {
        return;
    }

    plan.push_path(
        PathPrimitive::stroke(lines, color, HATCH_STROKE_PIXELS, quality).keyed(GeometryKey::node(
            canvas.node,
            GeometryPart::Hatch,
            canvas.version,
            quality,
            CLEAN,
        )),
    );
}

/// **Which side of the path run this frame's pictures belong on** (§10).
///
/// A picture has no second form — a quad-bodied body that a depth needs above a
/// path is promoted into the path run, and there is nothing to promote a
/// bitmap into — so instead of moving one element, the whole image run moves.
/// See [`render::plan`](crate::render::plan)'s module doc for the rule and for
/// the case it cannot satisfy.
///
/// The answer is read off the *snapshot*, which is where the depths already
/// are, and it is one walk of the visible set rather than of the document. An
/// unlayered document answers `false` in one `bool` read, exactly as
/// [`promotes_to_path`] does — a canvas nobody has reordered pays nothing for
/// a mechanism it is not using.
fn images_belong_above_paths(world: &GraphWorld, snapshot: &RenderSnapshot) -> bool {
    if !world.is_layered() {
        return false;
    }

    let mut highest_image: Option<i32> = None;
    let mut highest_path: Option<i32> = None;

    for canvas in snapshot.canvas() {
        if canvas.body == NodeShape::Image {
            highest_image = Some(highest_image.map_or(canvas.z, |it: i32| it.max(canvas.z)));
        } else if !shapes::node_prefers_quad(canvas.body) && canvas.body != NodeShape::Text {
            // Only the bodies that really are painted as paths count. A
            // rectangle is a quad and is *already* below the pictures, so
            // letting one vote here would move the run for a body the run was
            // never above.
            highest_path = Some(highest_path.map_or(canvas.z, |it: i32| it.max(canvas.z)));
        }
    }

    // Every edge is a path, and an edge passing over a picture is the case a
    // user notices first.
    for edge in snapshot.edges() {
        highest_path = Some(highest_path.map_or(edge.z, |it: i32| it.max(edge.z)));
    }

    match (highest_image, highest_path) {
        (Some(image), Some(path)) => image > path,
        _ => false,
    }
}

/// **Where the per-element order and the per-kind one actually collide, and
/// what is done about it.**
///
/// [`PaintPlan`](crate::render::PaintPlan) emits every quad, then every path,
/// then every text, one contiguous run each — a *correctness* contract, not a
/// preference: interleaving costs a full-viewport render pass per batch, and
/// 192 of them halve the frame rate. So a quad can never be painted above a
/// path, however deep the path is, and the axis-aligned rectangles Phase 0
/// measured into quads are exactly the bodies a user is most likely to send
/// behind an ellipse.
///
/// The composition rule is therefore: **depth is exact within a run, the run
/// order dominates between them, and a body that a depth order needs on the
/// other side of that line is promoted out of the quad run into the path run.**
/// Promotion is what closes the only gap between quads and paths that a user
/// can reach.
///
/// Three conditions, and each is load-bearing:
///
/// - **The document must be layered.** With every element at one depth nobody
///   has expressed an order, so nothing is violated by the one the batching
///   prefers — and the dense scene keeps its 1,584 free rectangles.
/// - **The node must be `detailed`.** Below that rung the quad *is* the
///   simplification (§15): the border has become the whole box. Promoting
///   there would spend the path budget to fix an ordering error a few pixels
///   across, on the frames that can least afford it.
/// - **The frame must not be degrading curves.** Same argument, from the other
///   threshold: `degrade_curves` means this frame is already trading fidelity
///   for vertices.
///
/// What is left over is recorded in the crate doc rather than hidden here:
/// **text is a run of its own and always the last**, so a text element cannot
/// be sent behind a shape's fill. There is no promotion available for it — a
/// glyph run has no outline form — and the batching contract is worth more than
/// the ordering.
fn promotes_to_path(world: &GraphWorld, canvas: &NodeBody, lod: &LodPlan) -> bool {
    world.is_layered() && canvas.detailed && !lod.degrade_curves
}

/// **Every body in the frame, in the order the document asks for.**
///
/// Two shapes, and the fast one is the default. With no depth expressed, the
/// edges are planned and then the nodes, exactly as they were before z-order
/// existed. Once a depth *has* been expressed, the two already-sorted lists are
/// merged so that one walk visits every body in depth order — which is what
/// gives each of the plan's runs a depth-sorted run of its own, since a run
/// keeps its insertion order.
///
/// At equal depth an **edge is planned before a node**, which keeps a
/// connection behind the boxes it joins. That is the order the two loops
/// already had; it is stated here rather than left to which loop ran first.
fn plan_bodies(
    plan: &mut PaintPlan,
    world: &GraphWorld,
    snapshot: &RenderSnapshot,
    viewport: &Viewport,
    ink: SceneInk,
    stats: &mut SceneStats,
) {
    if !world.is_layered() {
        plan_edges(plan, world, snapshot, viewport, ink, stats);
        plan_nodes(plan, world, snapshot, viewport, ink, stats);
        return;
    }

    let Some(lod) = snapshot.lod() else {
        return;
    };
    let quality = world.settings().render_quality;
    let boxed = snapshot.overlay().map(|overlay| overlay.node);
    let (edges, canvas) = (snapshot.edges(), snapshot.canvas());
    let (mut next_edge, mut next_node) = (0, 0);

    while next_edge < edges.len() || next_node < canvas.len() {
        let take_edge = match (edges.get(next_edge), canvas.get(next_node)) {
            (Some(edge), Some(node)) => edge.z <= node.z,
            (Some(_), None) => true,
            _ => false,
        };

        if take_edge {
            plan_one_edge(
                plan,
                world,
                &edges[next_edge],
                viewport,
                ink,
                &lod,
                quality,
                stats,
            );
            next_edge += 1;
        } else {
            plan_one_node(
                plan,
                world,
                NodeBody::of_canvas(&canvas[next_node], boxed),
                viewport,
                ink,
                &lod,
                quality,
                stats,
            );
            next_node += 1;
        }
    }

    plan_rich_bodies(plan, world, snapshot, viewport, ink, stats);
}

/// Everything one node body needs from either half of the renderer, so the
/// sketch painter can be called from the canvas loop and from the rich one
/// without either of them growing a copy of it.
struct SketchedBody {
    node: NodeIndex,
    body: NodeShape,
    version: u32,
    screen: Rect,
    connector: Option<[Vec2; 2]>,
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
    let Some(outline) = (match body.connector {
        Some([start, end]) => shapes::outline_for_connector(body.body, start, end),
        None => shapes::outline_for_node(body.body, body.screen, body.radius),
    }) else {
        return false;
    };

    let id = world.nodes().id(body.node);
    let sketch_key = style.cache_key();
    let sketch_quality = style.quality(quality);

    if shapes::node_prefers_quad(body.body) {
        plan.push_quad(
            QuadPrimitive::filled(body.screen, body.fill).with_corner_radius(body.radius),
        );
    } else if shapes::is_open(body.body) {
        // Nothing to fill: a hand-drawn line is its strokes and no more. The
        // caller already forced `has_stroke`, so the passes below are the whole
        // shape.
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

/// **The rich half's bodies, in both render styles.**
///
/// A rich node is a GPUI `div`, and a `div` can express a body only in the
/// theme's terms: a border colour it is handed, a background it is handed, no
/// dash, no hatch, no opacity and no hand. So the element gives the body up
/// entirely ([`crate::views::nodes`] draws no border and no fill) and the
/// canvas paints it here, through the same [`plan_one_node`] every other node
/// goes through.
///
/// **That "both render styles" is the whole of Phase 12.5's second bug.** This
/// loop ran only when the ladder had kept a hand, because a hand-drawn border
/// is the case a `div` obviously cannot do. In Clean mode the element painted
/// its own body from `theme.border` and `theme.secondary` — so a stroke colour,
/// a fill, a stroke width, an opacity, a dash and a hatch were all stored,
/// undoable, read back by the panel and painted by nothing, for every
/// rectangle at working zoom. It is Phase 11's `fill_style` again, one layer
/// up: the model reached one renderer and not the other, and every test passed
/// because no test asked what a rich node hands the painter.
///
/// It is bounded by
/// [`RenderBudgets::max_rich_elements`](crate::budgets::RenderBudgets::max_rich_elements)
/// like the rest of the rich set, and a rich body is always rectangular
/// (`RenderSnapshot::extract_nodes`), so the clean path it takes is the cheap
/// one — one quad carrying its own border and corner radius, and no path
/// vertices at all unless the document is layered or the fill is hatched.
fn plan_rich_bodies(
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

    let boxed = snapshot.overlay().map(|overlay| overlay.node);

    for rich in snapshot.rich() {
        plan_one_node(
            plan,
            world,
            NodeBody::of_rich(rich, boxed),
            viewport,
            ink,
            &lod,
            quality,
            stats,
        );
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
    viewport: &Viewport,
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

        // **A text element has no border to keep clear of**, so it uses its
        // whole rectangle; every other body insets, or its label sits on its
        // own outline. The padding is the only difference between the two, and
        // it is the difference between text that starts where the user placed
        // it and text that starts six pixels in.
        //
        // **A straight connector has no rectangle to lay text into.** Its
        // bounding box is derived from the segment and collapses to a line for
        // any axis-aligned one, so insetting it produced a negative height and
        // the `continue` below silently dropped every label on a horizontal
        // arrow. It gets the same treatment an edge label does: a box of its
        // own, centred on the *actual* segment midpoint (§9), so the label sits
        // where the caret was and where `views::flow`'s editor was drawn.
        let inner = match world.nodes().connector(canvas.node) {
            Some(connector) => boxless_label_box(viewport.world_to_screen(connector.midpoint())),
            None if canvas.body == NodeShape::Text => canvas.screen,
            None => canvas.screen.inflate(-LABEL_PADDING_PIXELS),
        };
        if inner.size.x <= 0.0 || inner.size.y <= 0.0 {
            continue;
        }

        let style = world.nodes().style(canvas.node);
        let font = &style.font;
        // **Node text wraps to its host's width** (§9), which is the inner
        // rectangle above: a text element uses its whole box and every other
        // body insets by its padding, so the rule is the same sentence for both
        // and the padding is the only difference. Quantised here rather than in
        // the painter so the primitive and its cache key carry one number.
        let wrap_width = TextKey::quantize_wrap_width(inner.size.x);
        plan.push_text(TextPrimitive {
            // **Placed inside the box by the two alignment rows**, and by
            // nothing else — a label has no position of its own. The vertical
            // half is written as `offset` rather than as "centre it" because
            // `Middle` *is* the centre and is the default: a label nobody has
            // moved lands on exactly the pixel §9 put it on, and the painter
            // then subtracts the block's own extra lines.
            origin: Vec2::new(
                inner.origin.x,
                inner.origin.y + font.vertical_align.offset(inner.size.y, font_size),
            ),
            // An `Arc` clone: a refcount bump, not a `String` allocation per
            // label per frame. See `NodeCold::label`.
            text: Arc::clone(label),
            font_size,
            // **A label is drawn in its element's ink.** The panel gives a node,
            // an edge and a text element a Stroke row and no separate text
            // colour, so that is the only colour control any of them has —
            // `ElementStyle::text_color` is the one answer, and it is what makes
            // a stroke change move the label with it.
            color: fade(style.text_color().unwrap_or(ink.text), style.opacity),
            key: TextKey::node(canvas.node, canvas.text_version, font_size, wrap_width),
            max_width: inner.size.x,
            wrap_width,
            family: font.family,
            align: font.align,
            max_height: inner.size.y,
            vertical_align: font.vertical_align,
        });
        stats.labels += 1;
    }
}

/// **Edge labels (§9), on the route rather than beside it.**
///
/// The position is derived from the route *this frame*, which is what makes
/// requirement 5 fall out rather than need machinery: Phase 3's dirty
/// propagation rebuilds an edge's geometry when either endpoint moves, so the
/// midpoint this reads has already moved and the label is drawn in the right
/// place with nothing here knowing a node was dragged.
///
/// Nothing is cached on position. The shaped-line cache is keyed on the edge,
/// its geometry version and the quantised size — never on where it is — so a
/// pure pan moves every label on screen and re-shapes none of them (§40 rule 7).
fn plan_edge_labels(
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
    if lod.detail == DetailLevel::Overview {
        return;
    }

    // One screen pixel in world units — the same precision
    // [`BoxQuery::at_zoom`](crate::runtime::BoxQuery::at_zoom) flattens at, and
    // for the same reason: finer than a pixel is precision nobody can see, and
    // this walk runs per labelled visible edge per frame.
    let flatten = viewport.screen_to_world_length(1.0).max(f32::MIN_POSITIVE);

    for planned in snapshot.edges() {
        let Some(font_size) = planned.label_font_size else {
            continue;
        };
        let Some(label) = world.edges().label(planned.edge) else {
            continue;
        };
        let Some(route) = world.route(planned.edge) else {
            continue;
        };

        let center = viewport.world_to_screen(route.midpoint(flatten));
        // The box is the label's own, centred on the route: an edge has no
        // rectangle of its own to lay text into, so one is made the size of the
        // space a label is allowed to take. Wider than a node's would be, on
        // purpose — an edge label that is truncated to nothing tells the reader
        // less than one that overhangs its route. The same box a connector's
        // label gets, from the same function.
        let inner = boxless_label_box(center);
        let style = world.edges().style(planned.edge);
        let font = &style.font;
        plan.push_text(TextPrimitive {
            // **The alignment rows are honoured here too**, and they had to be:
            // this phase offers both of them for an edge, and a row the panel
            // draws and no painter reads is the failure `properties`' module doc
            // lists five costumes of. The default pair — `Center` and `Middle` —
            // is the exact midpoint of the route, which is where an edge label
            // has been drawn since §9 and what the format's version-5 rung
            // writes into every older file.
            origin: Vec2::new(
                inner.origin.x,
                inner.origin.y + font.vertical_align.offset(inner.size.y, font_size),
            ),
            text: Arc::clone(label),
            font_size,
            // An edge's label takes the edge's ink, exactly as a node's does.
            color: fade(style.text_color().unwrap_or(ink.text), style.opacity),
            key: TextKey::edge(
                planned.edge,
                planned.version,
                font_size,
                EDGE_LABEL_MAX_PIXELS,
            ),
            max_width: inner.size.x,
            // **An edge label wraps to a constant screen width**, not to
            // anything about its route. An edge has no rectangle to lay text
            // into, so the box below is invented at the size a label is allowed
            // to take — and being a *screen* constant it needs no quantisation
            // and never re-wraps on a zoom, which is the one place edge labels
            // are cheaper than node labels rather than dearer.
            wrap_width: EDGE_LABEL_MAX_PIXELS,
            family: font.family,
            align: font.align,
            max_height: inner.size.y,
            vertical_align: font.vertical_align,
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

        fn image(&mut self, _image: &crate::render::plan::ImagePrimitive) -> u32 {
            1
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

    /// **What the painter is handed, in the order it is handed it.**
    ///
    /// Depth order is only observable as a sequence, so this is the sink the
    /// z-order tests read: one row per primitive, tagged with its kind and —
    /// for a path — with the element it belongs to. Everything else in this
    /// file asks *what* reached the plan; these ask *when*.
    #[derive(Default)]
    struct OrderSink {
        rows: Vec<(&'static str, Option<crate::render::cache::GeometryOwner>)>,
        /// Every cached path's whole key, for the tests that ask *which part*
        /// of an element was painted and *which hand* drew it.
        keys: Vec<crate::render::cache::GeometryKey>,
    }

    impl crate::render::PrimitiveSink for OrderSink {
        fn quad(&mut self, _quad: &crate::render::QuadPrimitive) {
            self.rows.push(("quad", None));
        }

        fn path(&mut self, path: &crate::render::PathPrimitive) -> u32 {
            self.rows
                .push(("path", path.key.as_ref().map(|key| key.owner)));
            self.keys.extend(path.key);
            1
        }

        fn text(&mut self, _text: &crate::render::plan::TextPrimitive) -> u32 {
            self.rows.push(("text", None));
            0
        }

        fn image(&mut self, image: &crate::render::plan::ImagePrimitive) -> u32 {
            self.rows.push((
                "image",
                Some(crate::render::cache::GeometryOwner::Node(image.node)),
            ));
            1
        }
    }

    /// Two overlapping shapes with different bodies: a rectangle (a quad when
    /// nothing has been reordered) and an ellipse (always a path). That pairing
    /// is the whole point — it is the case where a per-element order and a
    /// per-kind one disagree.
    fn overlapping_pair() -> GraphWorld {
        let mut world = GraphWorld::new();
        world.create_node(
            ElementKind::Shape(crate::models::ShapeKind::Rectangle),
            Vec2::new(100.0, 100.0),
            Vec2::new(200.0, 150.0),
        );
        world.create_node(
            ElementKind::Shape(crate::models::ShapeKind::Ellipse),
            Vec2::new(150.0, 130.0),
            Vec2::new(200.0, 150.0),
        );
        world.rebuild_all_geometry();
        world.clear_spatial_updates();
        world
    }

    fn painted_rows(
        world: &GraphWorld,
        viewport: &Viewport,
    ) -> Vec<(&'static str, Option<crate::render::cache::GeometryOwner>)> {
        let (plan, _) = frame_without_grid(world, viewport);
        let mut sink = OrderSink::default();
        plan.paint_into(&mut sink);
        sink.rows
    }

    /// A world holding one picture and one shape that overlaps it, both
    /// visible, with the picture behind by default.
    ///
    /// The bytes are nonsense — nothing in this file decodes anything, and the
    /// *plan* is what is under test — but they are real bytes, because the
    /// handle is their content hash.
    fn picture_and_shape() -> (GraphWorld, NodeIndex, NodeIndex) {
        use crate::models::{ImageFormat, ImageResource, NodeImage};

        let mut world = GraphWorld::new();
        let handle = world.insert_image(ImageResource::new(
            ImageFormat::Png,
            400,
            200,
            vec![9u8; 32],
        ));

        let picture = world.create_node(
            ElementKind::Image,
            Vec2::new(100.0, 100.0),
            Vec2::new(400.0, 200.0),
        );
        world.set_node_image(picture, Some(NodeImage::new(handle)));

        let shape = world.create_node(
            ElementKind::Shape(crate::models::ShapeKind::Ellipse),
            Vec2::new(200.0, 150.0),
            Vec2::new(200.0, 150.0),
        );

        world.rebuild_all_geometry();
        world.clear_spatial_updates();
        (world, picture, shape)
    }

    /// **A picture reaches the plan as one image primitive and no path
    /// vertices at all.**
    ///
    /// The second half is the budget claim: an image is closer to a quad than
    /// to a path, so a screenful of them cannot walk into
    /// `enforce_vertex_ceiling`. It is asserted rather than assumed because the
    /// crate has been wrong about a cost model before — see the crate doc's
    /// Phase 6 correction.
    #[test]
    fn a_picture_costs_one_image_and_no_path_vertices() {
        let viewport = Viewport::new(Vec2::ZERO, 1.0, Vec2::new(900.0, 600.0));
        let (world, picture, _) = picture_and_shape();

        let (plan, stats) = frame_without_grid(&world, &viewport);

        assert_eq!(stats.images, 1);
        assert_eq!(plan.image_count(), 1);
        let planned = plan.planned_images();
        assert_eq!(planned[0].node, picture);
        assert!(planned[0].image.crop.is_full());
        assert!((planned[0].opacity - 1.0).abs() < 1e-6);

        // The picture itself tessellates nothing. The one path in the frame is
        // the ellipse's fill and stroke; the picture adds none of it.
        let (plain, _) = {
            let mut bare = GraphWorld::new();
            bare.create_node(
                ElementKind::Shape(crate::models::ShapeKind::Ellipse),
                Vec2::new(200.0, 150.0),
                Vec2::new(200.0, 150.0),
            );
            bare.rebuild_all_geometry();
            bare.clear_spatial_updates();
            frame_without_grid(&bare, &viewport)
        };
        assert_eq!(
            plan.estimated_path_vertices(),
            plain.estimated_path_vertices(),
            "a picture charged the frame's vertex budget"
        );
    }

    /// **A selected picture gains one path and it is painted over the
    /// picture.**
    ///
    /// The ring cannot be a quad: quads are painted before the image run, so a
    /// bordered quad would be a selection ring hidden underneath the very thing
    /// it is around. This is that decision as a sequence.
    #[test]
    fn a_selected_picture_s_ring_is_painted_after_it() {
        let viewport = Viewport::new(Vec2::ZERO, 1.0, Vec2::new(900.0, 600.0));
        let (mut world, picture, _) = picture_and_shape();
        world.set_node_selected(picture, true);

        let rows = painted_rows(&world, &viewport);
        let image_at = rows.iter().position(|(kind, _)| *kind == "image");
        let ring_at = rows.iter().rposition(|(kind, owner)| {
            *kind == "path" && *owner == Some(crate::render::cache::GeometryOwner::Node(picture))
        });

        assert!(image_at.is_some(), "{rows:?}");
        assert!(
            ring_at.is_some(),
            "the selected picture drew no ring: {rows:?}"
        );
        assert!(
            image_at < ring_at,
            "the ring was painted under the picture: {rows:?}"
        );
    }

    /// **A picture takes its place in the depth order**, in the direction that
    /// needed a mechanism: brought in front of a path-bodied body.
    ///
    /// Behind is the default and needs nothing — every path is emitted after
    /// the image run, so an ellipse is over a screenshot the moment both exist,
    /// which is what annotating one looks like. **In front** is the case with
    /// no promotion available: a bitmap has no outline form, so the run itself
    /// moves, and this is that rule as the sequence a painter is handed.
    #[test]
    fn a_picture_can_be_brought_in_front_of_a_path_bodied_shape() {
        let viewport = Viewport::new(Vec2::ZERO, 1.0, Vec2::new(900.0, 600.0));
        let (mut world, picture, shape) = picture_and_shape();

        // Default: the ellipse is a path, so it is over the picture already.
        let rows = painted_rows(&world, &viewport);
        let image_at = rows.iter().position(|(kind, _)| *kind == "image");
        let ellipse_at = rows.iter().position(|(kind, owner)| {
            *kind == "path" && *owner == Some(crate::render::cache::GeometryOwner::Node(shape))
        });
        assert!(image_at.is_some() && ellipse_at.is_some(), "{rows:?}");
        assert!(
            image_at < ellipse_at,
            "a picture is behind by default: {rows:?}"
        );

        // Bring the picture to the front. The ellipse cannot move — it is a
        // path either way — so the image run does.
        world.set_node_z(picture, 5);
        assert!(world.is_layered());

        let rows = painted_rows(&world, &viewport);
        let image_at = rows
            .iter()
            .position(|(kind, _)| *kind == "image")
            .expect("the picture is still drawn");
        let ellipse_at = rows
            .iter()
            .position(|(kind, owner)| {
                *kind == "path" && *owner == Some(crate::render::cache::GeometryOwner::Node(shape))
            })
            .expect("the ellipse is still drawn");

        assert!(
            ellipse_at < image_at,
            "the picture did not reach the front: {rows:?}"
        );

        // And the run is still one run: the contract is not what moved.
        let images: Vec<usize> = rows
            .iter()
            .enumerate()
            .filter(|(_, (kind, _))| *kind == "image")
            .map(|(index, _)| index)
            .collect();
        assert!(
            images.windows(2).all(|pair| pair[1] == pair[0] + 1),
            "the image run was broken up: {rows:?}"
        );
    }

    /// **An unlayered document pays one `bool` read**, and its pictures are
    /// where they always were. The same property Phase 11 asserted for the
    /// depth-ordered planning walk, for the mechanism this phase added beside
    /// it.
    #[test]
    fn a_document_nobody_has_reordered_leaves_the_image_run_alone() {
        let viewport = Viewport::new(Vec2::ZERO, 1.0, Vec2::new(900.0, 600.0));
        let (world, _, _) = picture_and_shape();
        assert!(!world.is_layered());

        let (plan, _) = frame_without_grid(&world, &viewport);

        assert!(
            !plan.images_after_paths(),
            "an unreordered document moved its image run"
        );
    }

    /// **What a screenful of pictures costs**, beside the numbers this module
    /// already records for bodies and for a layered frame.
    ///
    /// Twenty-four pictures filling a 1440×900 pane: **zero path vertices, zero
    /// path batches from the pictures themselves, and one image primitive
    /// each.** Their batching cost is GPUI's, one sprite batch per *atlas
    /// texture* rather than one per picture, and a sprite batch is a draw call
    /// with a texture bind — not the full-viewport intermediate pass with a
    /// clear that a path batch costs. See `render::plan`'s module doc.
    #[test]
    fn a_screenful_of_pictures_costs_no_path_vertices() {
        use crate::models::{ImageFormat, ImageResource, NodeImage};

        let viewport = Viewport::new(Vec2::ZERO, 1.0, Vec2::new(1440.0, 900.0));
        let mut world = GraphWorld::new();
        let handle =
            world.insert_image(ImageResource::new(ImageFormat::Png, 240, 180, vec![1u8; 8]));

        for row in 0..4 {
            for column in 0..6 {
                let node = world.create_node(
                    ElementKind::Image,
                    Vec2::new(column as f32 * 240.0, row as f32 * 225.0),
                    Vec2::new(230.0, 215.0),
                );
                world.set_node_image(node, Some(NodeImage::new(handle)));
            }
        }
        world.rebuild_all_geometry();
        world.clear_spatial_updates();

        let (plan, stats) = frame_without_grid(&world, &viewport);

        assert_eq!(stats.images, 24, "not every picture was planned");
        assert_eq!(plan.image_count(), 24);
        assert_eq!(
            plan.estimated_path_vertices(),
            0,
            "pictures charged the vertex budget"
        );
        assert_eq!(plan.path_count(), 0, "pictures cost a path batch");
        // And one resource behind all of them, which is §10's rule holding at
        // the twenty-fourth element as well as at the second.
        assert_eq!(world.image_count(), 1);
    }

    /// **Phase 2's contract, with a depth order applied over it.**
    ///
    /// Every quad, then every path, then every text — one contiguous run each.
    /// This is the assertion the whole z-order design had to fit inside: a
    /// frame that interleaved to satisfy a depth would cost a full-viewport
    /// render pass per switch, and 192 of those halve the frame rate.
    #[test]
    fn depth_order_does_not_interleave_the_paint_runs() {
        let viewport = Viewport::new(Vec2::ZERO, 1.0, Vec2::new(900.0, 600.0));
        let mut world = overlapping_pair();
        world.set_node_z(NodeIndex::new(1), -5);
        world.set_node_z(NodeIndex::new(0), 3);
        assert!(world.is_layered());

        let kinds: Vec<&str> = painted_rows(&world, &viewport)
            .iter()
            .map(|(kind, _)| *kind)
            .collect();

        let mut seen: Vec<&str> = Vec::new();
        for kind in kinds {
            if seen.last() != Some(&kind) {
                assert!(
                    !seen.contains(&kind),
                    "the {kind} run was broken and resumed: {seen:?}"
                );
                seen.push(kind);
            }
        }
        // The runs may be empty, but they may never be out of order.
        let order = ["quad", "path", "text"];
        let mut expected = order.iter().filter(|kind| seen.contains(kind));
        for kind in &seen {
            assert_eq!(Some(kind), expected.next(), "runs out of order: {seen:?}");
        }
    }

    /// **The gap the promotion closes**, which took a test to find the shape of.
    ///
    /// The obvious case — a rectangle brought to the front of an ellipse —
    /// needs no promotion at all: a rectangle at full detail is a GPUI element,
    /// and the element layer already paints above the canvas. The case that
    /// genuinely breaks is a quad-bodied body in the **middle**: an ellipse
    /// below it, an ellipse above it. The one above demotes it out of the
    /// element layer (see `place_in_depth_order`), and once it is on the canvas
    /// a quad is painted before *every* path — including the ellipse it is
    /// supposed to cover. Promoting it into the path run is what puts it back
    /// where the depths say.
    #[test]
    fn a_quad_bodied_shape_between_two_paths_is_promoted_into_the_path_run() {
        use crate::models::ShapeKind;
        use crate::render::cache::GeometryOwner;

        let viewport = Viewport::new(Vec2::ZERO, 1.0, Vec2::new(900.0, 600.0));
        let mut world = GraphWorld::new();
        let below = world.create_node(
            ElementKind::Shape(ShapeKind::Ellipse),
            Vec2::new(100.0, 100.0),
            Vec2::new(200.0, 150.0),
        );
        let middle = world.create_node(
            ElementKind::Shape(ShapeKind::Rectangle),
            Vec2::new(130.0, 120.0),
            Vec2::new(200.0, 150.0),
        );
        let above = world.create_node(
            ElementKind::Shape(ShapeKind::Ellipse),
            Vec2::new(160.0, 140.0),
            Vec2::new(200.0, 150.0),
        );
        world.rebuild_all_geometry();
        world.clear_spatial_updates();

        world.set_node_z(below, 1);
        world.set_node_z(middle, 2);
        world.set_node_z(above, 3);

        let rows = painted_rows(&world, &viewport);
        let position = |node| {
            rows.iter()
                .position(|(_, owner)| *owner == Some(GeometryOwner::Node(node)))
                .unwrap_or_else(|| panic!("{node:?} never reached the painter as a path"))
        };

        assert!(
            position(below) < position(middle) && position(middle) < position(above),
            "the three depths must be the three paint positions: {rows:?}"
        );
    }

    /// The other direction, and the one that needs the *edges* to be in the
    /// walk rather than planned as a block before it.
    #[test]
    fn an_edge_sent_to_the_front_is_painted_over_the_body_it_crosses() {
        use crate::models::ShapeKind;
        use crate::render::cache::GeometryOwner;

        let viewport = Viewport::new(Vec2::ZERO, 1.0, Vec2::new(900.0, 600.0));
        let mut world = GraphWorld::new();
        world.set_rules(ConnectionRules::PERMISSIVE);
        // Ellipses, so both bodies are canvas paths and the assertion is about
        // the walk rather than about which half of the renderer took them.
        for column in 0..2 {
            world.create_node(
                ElementKind::Shape(ShapeKind::Ellipse),
                Vec2::new(column as f32 * 260.0, 100.0),
                Vec2::new(160.0, 120.0),
            );
        }
        world
            .connect(
                EdgeEnd::node(NodeIndex::new(0)),
                EdgeEnd::node(NodeIndex::new(1)),
            )
            .expect("permissive rules accept it");
        let (edge, node) = (crate::models::EdgeIndex::new(0), NodeIndex::new(1));
        world.set_edge_z(edge, 5);
        world.rebuild_all_geometry();
        world.clear_spatial_updates();

        let rows = painted_rows(&world, &viewport);
        let node_at = rows
            .iter()
            .position(|(_, owner)| *owner == Some(GeometryOwner::Node(node)));
        let edge_at = rows
            .iter()
            .position(|(_, owner)| *owner == Some(GeometryOwner::Edge(edge)));

        assert!(
            matches!((node_at, edge_at), (Some(node), Some(edge)) if edge > node),
            "the edge is above the node in depth and must be painted after it: \
             node at {node_at:?}, edge at {edge_at:?}"
        );
    }

    // ---- Phase 11: the two style rows that have to reach the painter ----

    /// **A hatched fill is drawn, and it is drawn instead of the solid one.**
    ///
    /// The phase brief forbids a control that silently ignores the user by
    /// name, and this is the row that would have done it: `fill_style` is
    /// stored, undoable and read back by the panel whether or not anything
    /// paints it, so every other test in this crate would have passed with the
    /// Fill row inert.
    #[test]
    fn a_hatched_shape_paints_lines_instead_of_a_flooded_interior() {
        use crate::models::{FillStyle, ShapeKind};
        use crate::render::cache::{GeometryOwner, GeometryPart};

        let viewport = Viewport::new(Vec2::ZERO, 1.0, Vec2::new(900.0, 600.0));
        let mut world = GraphWorld::new();
        let node = world.create_node(
            ElementKind::Shape(ShapeKind::Ellipse),
            Vec2::new(100.0, 100.0),
            Vec2::new(220.0, 160.0),
        );
        world.rebuild_all_geometry();
        world.clear_spatial_updates();

        let parts = |world: &GraphWorld| -> Vec<GeometryPart> {
            let (plan, _) = frame_without_grid(world, &viewport);
            let mut sink = OrderSink::default();
            plan.paint_into(&mut sink);
            sink.keys
                .iter()
                .filter(|key| key.owner == GeometryOwner::Node(node))
                .map(|key| key.part)
                .collect()
        };

        assert!(parts(&world).contains(&GeometryPart::Fill));
        assert!(!parts(&world).contains(&GeometryPart::Hatch));

        let mut style = world.nodes().style(node).clone();
        style.fill_style = FillStyle::CrossHatch;
        world.set_node_style(node, style);

        let after = parts(&world);
        assert!(
            after.contains(&GeometryPart::Hatch),
            "the Fill row was set to cross-hatch and nothing hatched: {after:?}"
        );
        assert!(
            !after.contains(&GeometryPart::Fill),
            "a hatched interior must replace the flooded one, not sit under it"
        );
    }

    /// **Sloppiness reaches the hand, and two elements at two steps are drawn
    /// differently.**
    ///
    /// Asserted through the *cache key* rather than by comparing squiggles: the
    /// key carries `SketchStyle::cache_key`, so two steps producing one key
    /// would mean the second element being served the first one's geometry —
    /// which is both "the control did nothing" and a cache bug, in one
    /// assertion.
    #[test]
    fn two_sloppinesses_are_two_hands_and_two_cache_keys() {
        use crate::models::{RenderStyle, ShapeKind};
        use crate::render::cache::GeometryOwner;

        let viewport = Viewport::new(Vec2::ZERO, 1.0, Vec2::new(900.0, 600.0));
        let mut world = GraphWorld::new();
        world.settings_mut().render_style = RenderStyle::Sketch;
        let node = world.create_node(
            ElementKind::Shape(ShapeKind::Ellipse),
            Vec2::new(100.0, 100.0),
            Vec2::new(220.0, 160.0),
        );
        world.rebuild_all_geometry();
        world.clear_spatial_updates();

        let sketch_keys = |world: &GraphWorld| -> Vec<u32> {
            let (plan, _) = frame_without_grid(world, &viewport);
            let mut sink = OrderSink::default();
            plan.paint_into(&mut sink);
            sink.keys
                .iter()
                .filter(|key| key.owner == GeometryOwner::Node(node))
                .map(|key| key.sketch)
                .collect()
        };

        let artist = sketch_keys(&world);
        assert!(!artist.is_empty(), "a sketched ellipse paints something");

        let mut style = world.nodes().style(node).clone();
        style.sloppiness = crate::models::Sloppiness::Cartoonist;
        world.set_node_style(node, style);
        let cartoonist = sketch_keys(&world);

        assert_ne!(
            artist, cartoonist,
            "the Sloppiness row was moved and the hand did not change"
        );
    }

    /// The identity has to be the identity. `Artist` multiplies by `1.0`, so a
    /// document written before this field existed must draw byte for byte what
    /// it drew — which is the whole reason the field could be added to a live
    /// format at all.
    #[test]
    fn the_middle_sloppiness_leaves_the_document_s_hand_alone() {
        let hand = crate::models::SketchStyle::DEFAULT;
        assert_eq!(pressed(hand, Sloppiness::Artist), hand);
        assert!(pressed(hand, Sloppiness::Architect).roughness < hand.roughness);
        assert!(pressed(hand, Sloppiness::Cartoonist).roughness > hand.roughness);
    }

    /// **What a layered document actually costs**, as a bound rather than a
    /// hope.
    ///
    /// Measured on this machine, 1440×900 at 100 % zoom, 2026-08-20:
    ///
    /// | | quads | paths | estimated vertices |
    /// |---|---:|---:|---:|
    /// | 54 detailed rectangles + one ellipse, flat | 0 | 2 | 1,158 |
    /// | the same, one element layered | 0 | **110** | **3,804** |
    ///
    /// The 54 rectangles are GPUI elements while nothing is layered, so they
    /// reach the plan not at all; layered, they are demoted out of the element
    /// layer and promoted into the path run at two paths each. **That is the
    /// whole price of the feature** — 108 paths against
    /// `target_paths_per_frame`'s 3,000, and 3,804 vertices against a 2.4 M
    /// ceiling — and it is paid only by bodies the ladder still calls
    /// `detailed`, which is what keeps it proportional to the screen.
    ///
    /// **On every benchmark scene it is exactly zero**, and the two reasons are
    /// the two conditions: the sparse scenes are entirely rich elements with
    /// nothing canvas-drawn to demote them, and the dense scene's bodies are 18
    /// world units tall and therefore not `detailed`, so promotion declines on
    /// the frame that could least afford it.
    #[test]
    fn a_layered_frame_costs_paths_in_proportion_to_the_detailed_bodies_on_screen() {
        use crate::models::ShapeKind;

        let viewport = Viewport::new(Vec2::ZERO, 1.0, Vec2::new(1440.0, 900.0));

        // The benchmark scenes pay nothing at all.
        //
        // **`large` is deliberately not in this loop**, and that is not a
        // corner cut for the sake of a fast suite: what varies between the
        // scenes for *this* question is only whether a body is a GPUI element
        // and whether the ladder still calls it detailed. `small`, `medium` and
        // `large` give the same answer to both — every visible body is rich —
        // so the third costs 0.9 s of `build` to re-assert what the second
        // already said. `dense` is the one that differs and it is here.
        //
        // **One spatial index serves both frames**, and that is not only for
        // the sake of a fast test: a depth change marks `STYLE` and never
        // `SPATIAL`, because nothing about where an element *is* changed. If
        // that ever stops being true this loop culls against a stale index and
        // the counts stop matching, which is the right way round.
        for spec in crate::scenes::SceneSpec::ALL
            .iter()
            .filter(|spec| spec.name != "large")
        {
            let mut world = crate::scenes::build(spec);
            world.rebuild_all_geometry();
            world.clear_spatial_updates();

            let index = SpatialIndex::for_world(&world);
            let mut visible = VisibleSet::new();
            index.query_visible(&world, &viewport, &mut visible);

            let planned = |world: &GraphWorld, visible: &VisibleSet| {
                let snapshot = snapshot_of(world, visible, &viewport);
                let mut plan = PaintPlan::new();
                let stats = plan_scene(&mut plan, world, &snapshot, &viewport, ink(), &options());
                (stats.nodes, stats.edges)
            };

            let before = planned(&world, &visible);
            let first = world
                .nodes()
                .live_indices()
                .next()
                .expect("a scene has nodes");
            world.set_node_z(first, 1);

            assert_eq!(
                before,
                planned(&world, &visible),
                "{} changed what it plans when one element was layered",
                spec.name
            );
        }

        // And the case that does pay: detailed bodies with a path above them.
        let mut world = GraphWorld::new();
        for row in 0..6 {
            for column in 0..9 {
                world.create_node(
                    ElementKind::Shape(ShapeKind::Rectangle),
                    Vec2::new(column as f32 * 155.0, row as f32 * 145.0),
                    Vec2::new(140.0, 130.0),
                );
            }
        }
        let top = world.create_node(
            ElementKind::Shape(ShapeKind::Ellipse),
            Vec2::new(200.0, 200.0),
            Vec2::new(300.0, 300.0),
        );
        world.rebuild_all_geometry();
        world.clear_spatial_updates();
        world.set_node_z(top, 1);

        let (plan, _) = frame_without_grid(&world, &viewport);
        let budgets = for_backend(RenderBackend::Metal);
        assert!(
            plan.paths().len() <= budgets.target_paths_per_frame as usize,
            "a layered screenful of 54 bodies planned {} paths",
            plan.paths().len()
        );
        assert!(
            plan.estimated_path_vertices() < budgets.safe_path_vertex_ceiling / 100,
            "and {} estimated vertices",
            plan.estimated_path_vertices()
        );
    }

    /// **A document nobody has reordered pays nothing.**
    ///
    /// The whole design rests on this: with one depth shared by everything, the
    /// frame is byte-for-byte the frame the engine produced before z-order
    /// existed — same primitives, same order, same quads.
    #[test]
    fn an_unlayered_document_plans_exactly_what_it_always_did() {
        let viewport = Viewport::new(Vec2::ZERO, 1.0, Vec2::new(900.0, 600.0));
        let mut world = document(4, 3);
        assert!(!world.is_layered());
        let before = painted_rows(&world, &viewport);

        // Reordering and putting it back must also come home, because the
        // counter that decides the fast path is symmetric rather than sticky.
        world.set_node_z(NodeIndex::new(0), 4);
        assert!(world.is_layered());
        world.set_node_z(NodeIndex::new(0), 0);
        assert!(!world.is_layered());

        assert_eq!(before, painted_rows(&world, &viewport));
    }

    /// The rich layer is a layer of GPUI elements above the canvas, so a node
    /// that must sit under a canvas-drawn body cannot stay in it. This is that
    /// rule, and it is the reason the split happens after the sort rather than
    /// during the walk.
    #[test]
    fn a_node_that_must_sit_below_a_path_leaves_the_element_layer() {
        let viewport = Viewport::new(Vec2::ZERO, 1.0, Vec2::new(900.0, 600.0));
        let mut world = overlapping_pair();
        let (rectangle, ellipse) = (NodeIndex::new(0), NodeIndex::new(1));

        let index = SpatialIndex::for_world(&world);
        let mut visible = VisibleSet::new();
        index.query_visible(&world, &viewport, &mut visible);
        let snapshot = snapshot_of(&world, &visible, &viewport);
        assert!(
            snapshot.rich().iter().any(|it| it.node == rectangle),
            "a detailed rectangle is a GPUI element at full zoom"
        );

        world.set_node_z(ellipse, 2);
        let index = SpatialIndex::for_world(&world);
        let mut visible = VisibleSet::new();
        index.query_visible(&world, &viewport, &mut visible);
        let snapshot = snapshot_of(&world, &visible, &viewport);

        assert!(
            !snapshot.rich().iter().any(|it| it.node == rectangle),
            "the ellipse is above the rectangle, so the rectangle cannot stay in \
             the layer that paints above the ellipse"
        );
        assert!(snapshot.canvas().iter().any(|it| it.node == rectangle));
    }

    /// The same frame with **no background grid**, so a test can count the
    /// primitives one element produced. The grid is thousands of quads and
    /// swamps any assertion about a single node.
    fn frame_without_grid(world: &GraphWorld, viewport: &Viewport) -> (PaintPlan, SceneStats) {
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

        let options = SceneOptions::new(
            GridSettings {
                style: GridStyle::None,
                ..GridSettings::default()
            },
            GridLimits::from_budgets(&for_backend(RenderBackend::Metal)),
        );

        let mut plan = PaintPlan::new();
        let stats = plan_scene(&mut plan, world, &snapshot, viewport, ink(), &options);
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

    // ---- §9's text ------------------------------------------------------

    /// Every label a frame planned, in paint order: where it was drawn and
    /// what the shaped-line cache would file it under.
    ///
    /// A sink rather than an accessor on [`PaintPlan`], for the same reason
    /// [`RecordingSink`] is one: what matters is what actually reaches a
    /// painter, and `paint_into` is the only thing that decides that.
    #[derive(Default)]
    struct TextSink {
        labels: Vec<(crate::render::cache::TextKey, Vec2, std::sync::Arc<str>)>,
    }

    impl crate::render::PrimitiveSink for TextSink {
        fn quad(&mut self, _quad: &crate::render::QuadPrimitive) {}
        fn path(&mut self, _path: &crate::render::PathPrimitive) -> u32 {
            0
        }
        fn text(&mut self, text: &crate::render::plan::TextPrimitive) -> u32 {
            self.labels
                .push((text.key, text.origin, std::sync::Arc::clone(&text.text)));
            1
        }

        fn image(&mut self, _image: &crate::render::plan::ImagePrimitive) -> u32 {
            1
        }
    }

    fn labels_of(
        plan: &PaintPlan,
    ) -> Vec<(crate::render::cache::TextKey, Vec2, std::sync::Arc<str>)> {
        let mut sink = TextSink::default();
        plan.paint_into(&mut sink);
        sink.labels
    }

    /// Every label's text, the box it is aligned in and the width it wraps
    /// into (Phase 10.5).
    ///
    /// A second collector rather than a wider tuple on [`labels_of`], so the
    /// tests that ask about keys and positions read exactly as they did.
    fn wraps_of(plan: &PaintPlan) -> Vec<(std::sync::Arc<str>, f32, f32)> {
        #[derive(Default)]
        struct WrapSink {
            widths: Vec<(std::sync::Arc<str>, f32, f32)>,
        }

        impl crate::render::PrimitiveSink for WrapSink {
            fn quad(&mut self, _quad: &crate::render::QuadPrimitive) {}
            fn path(&mut self, _path: &crate::render::PathPrimitive) -> u32 {
                0
            }
            fn text(&mut self, text: &crate::render::plan::TextPrimitive) -> u32 {
                self.widths.push((
                    std::sync::Arc::clone(&text.text),
                    text.max_width,
                    text.wrap_width,
                ));
                1
            }

            fn image(&mut self, _image: &crate::render::plan::ImagePrimitive) -> u32 {
                1
            }
        }

        let mut sink = WrapSink::default();
        plan.paint_into(&mut sink);
        sink.widths
    }

    /// Two labelled nodes joined by a labelled edge — the smallest document
    /// that exercises all three of §9's text carriers.
    fn labelled_pair() -> GraphWorld {
        let mut world = GraphWorld::new();
        world.set_rules(ConnectionRules::PERMISSIVE);
        let a = world.create_node(
            ElementKind::GraphNode(GraphNodeKind::Default),
            Vec2::new(60.0, 60.0),
            Vec2::new(160.0, 60.0),
        );
        let b = world.create_node(
            ElementKind::GraphNode(GraphNodeKind::Default),
            Vec2::new(420.0, 60.0),
            Vec2::new(160.0, 60.0),
        );
        world.set_node_label(a, Some("source".into()));
        world.set_node_label(b, Some("sink".into()));
        let edge = world
            .connect(EdgeEnd::node(a), EdgeEnd::node(b))
            .expect("permissive rules accept it");
        world.set_edge_label(edge, Some("carries".into()));
        world.rebuild_all_geometry();
        world.clear_spatial_updates();
        world
    }

    /// A camera on the **Compact** rung.
    ///
    /// Deliberately not 100 %: at full detail a graph node becomes a rich GPUI
    /// element and its label is drawn *inside* that element, where only a
    /// window can see it. The canvas half is what these tests can assert with
    /// no window, and it is the half every large document actually uses.
    fn pane() -> Viewport {
        Viewport::new(Vec2::ZERO, 0.5, Vec2::new(800.0, 600.0))
    }

    /// **A node's text moves with the node, and is not re-shaped for it.**
    ///
    /// The two halves are one requirement: the phase brief asks for text that
    /// stays correctly positioned when a node moves, *and* for the engine's
    /// shaped-line cache to keep paying. A label drawn in the right place by
    /// re-shaping it every frame satisfies the first and defeats the second,
    /// and nothing on screen tells the two apart.
    #[test]
    fn a_node_label_follows_its_node_without_being_reshaped() {
        let mut world = labelled_pair();
        let viewport = pane();
        let node = NodeIndex::new(0);

        let (before, _) = frame_without_grid(&world, &viewport);
        let before = labels_of(&before);
        let (key, origin, text) = before
            .iter()
            .find(|(_, _, text)| text.as_ref() == "source")
            .cloned()
            .expect("the node's label reached the plan");

        world.move_node(node, Vec2::new(0.0, 120.0));
        world.rebuild_dirty_geometry();

        let (after, _) = frame_without_grid(&world, &viewport);
        let after = labels_of(&after);
        let (moved_key, moved_origin, moved_text) = after
            .iter()
            .find(|(_, _, text)| text.as_ref() == "source")
            .cloned()
            .expect("it is still drawn");

        assert_eq!(moved_text, text);
        assert_eq!(
            moved_origin - origin,
            Vec2::new(0.0, 60.0),
            "the label moved exactly as far as the node did, in screen pixels"
        );
        assert_eq!(
            moved_key, key,
            "a move must not change the shaped-line key — the position is \
             deliberately not part of it"
        );
    }

    /// **An edge's label follows its route**, which is the half of requirement
    /// 5 that needed no machinery: Phase 3's propagation rebuilds the route
    /// when an endpoint moves, and the midpoint is read from the route rather
    /// than remembered.
    ///
    /// The key *does* change here, and that is the safe direction: an edge's
    /// geometry version moves when its route does, so a rerouted edge pays one
    /// re-shape rather than risking a label shaped for a route it has left.
    #[test]
    fn an_edge_label_follows_a_rerouted_edge() {
        let mut world = labelled_pair();
        let viewport = pane();

        let (before, _) = frame_without_grid(&world, &viewport);
        let (key, origin, _) = labels_of(&before)
            .into_iter()
            .find(|(_, _, text)| text.as_ref() == "carries")
            .expect("the edge's label reached the plan");

        // Move the *target*, so the route changes without the label's own
        // element being touched at all.
        world.move_node(NodeIndex::new(1), Vec2::new(0.0, 200.0));
        let rebuilt = world.rebuild_dirty_geometry();
        assert_eq!(rebuilt, 1, "§19: exactly the one incident edge");

        let (after, _) = frame_without_grid(&world, &viewport);
        let (moved_key, moved_origin, _) = labels_of(&after)
            .into_iter()
            .find(|(_, _, text)| text.as_ref() == "carries")
            .expect("it is still drawn");

        assert_ne!(
            moved_origin, origin,
            "the label sits on the route, so a reroute has to move it"
        );
        assert!(
            moved_origin.y > origin.y,
            "the target went down, so the midpoint did"
        );
        assert_ne!(
            moved_key, key,
            "a rerouted edge is a new geometry version, and therefore a new \
             shaped line — the safe direction"
        );
    }

    // ---- Phase 10.5: what each kind of text wraps into -------------------

    /// **The wrap rule, stated for all three carriers at once.**
    ///
    /// - Node text wraps to its **host's inner width** — the box minus its
    ///   label padding, because the run sits inside a border it must not touch.
    /// - A standalone text element wraps to its **whole box**: it has no border
    ///   to keep clear of, and inset text would start six pixels from where the
    ///   user put it.
    /// - An edge label wraps to a **constant screen width**, because an edge
    ///   has no rectangle to lay text into — see [`EDGE_LABEL_MAX_PIXELS`].
    ///
    /// Leaving that implicit is how a later phase discovers that two of the
    /// three wrap somewhere nobody chose.
    #[test]
    fn each_kind_of_text_wraps_to_the_width_its_own_rule_names() {
        let mut world = labelled_pair();
        let text = world.create_node(
            ElementKind::Text,
            Vec2::new(60.0, 400.0),
            Vec2::new(240.0, 22.0),
        );
        world.set_node_label(text, Some("standalone".into()));
        world.rebuild_all_geometry();
        world.clear_spatial_updates();

        let viewport = pane();
        let zoom = viewport.zoom();
        let (plan, _) = frame_without_grid(&world, &viewport);
        let widths = wraps_of(&plan);

        let width_of = |label: &str| -> (f32, f32) {
            widths
                .iter()
                .find(|(text, _, _)| text.as_ref() == label)
                .map(|(_, max, wrap)| (*max, *wrap))
                .unwrap_or_else(|| panic!("{label} did not reach the plan"))
        };

        // A 160-unit node at zoom 0.5 is 80 screen pixels wide, less the
        // padding on both sides.
        let (node_max, node_wrap) = width_of("source");
        assert_eq!(node_max, 160.0 * zoom - LABEL_PADDING_PIXELS * 2.0);
        assert_eq!(node_wrap, TextKey::quantize_wrap_width(node_max));

        // The text element uses its whole rectangle: no padding subtracted.
        let (text_max, text_wrap) = width_of("standalone");
        assert_eq!(text_max, 240.0 * zoom);
        assert_eq!(text_wrap, TextKey::quantize_wrap_width(text_max));

        // And the edge's box is invented at a constant screen width, so it is
        // its own quantum already and never re-wraps on a zoom.
        let (edge_max, edge_wrap) = width_of("carries");
        assert_eq!(edge_max, EDGE_LABEL_MAX_PIXELS);
        assert_eq!(edge_wrap, EDGE_LABEL_MAX_PIXELS);
    }

    /// **A wrapped label follows a moved node without being re-wrapped**, and a
    /// *resized* one is re-wrapped exactly once.
    ///
    /// The pair is the requirement. Phase 10 found a moved node re-shaping its
    /// label sixty times a second, and wrapping multiplies whatever that costs:
    /// a paragraph is several shaped lines, not one. A resize is the case that
    /// genuinely must miss, because the width a run wraps into is part of the
    /// laid-out result — and `text_version` already bumps for it, which is why
    /// this needed no new plumbing.
    #[test]
    fn a_moved_node_keeps_its_wrap_and_a_resized_one_earns_a_new_one() {
        let mut world = labelled_pair();
        let viewport = pane();
        let node = NodeIndex::new(0);

        let key_and_wrap = |world: &GraphWorld| {
            let (plan, _) = frame_without_grid(world, &viewport);
            let key = labels_of(&plan)
                .into_iter()
                .find(|(_, _, text)| text.as_ref() == "source")
                .map(|(key, _, _)| key)
                .expect("the label reached the plan");
            let wrap = wraps_of(&plan)
                .into_iter()
                .find(|(text, _, _)| text.as_ref() == "source")
                .map(|(_, _, wrap)| wrap)
                .expect("the label reached the plan");
            (key, wrap)
        };

        let (key, wrap) = key_and_wrap(&world);

        world.move_node(node, Vec2::new(0.0, 120.0));
        world.rebuild_dirty_geometry();
        let (moved_key, moved_wrap) = key_and_wrap(&world);

        assert_eq!(moved_key, key, "a move re-wrapped the label");
        assert_eq!(moved_wrap, wrap);

        world.set_node_size(node, Vec2::new(320.0, 60.0));
        world.rebuild_dirty_geometry();
        let (resized_key, resized_wrap) = key_and_wrap(&world);

        assert!(
            resized_wrap > wrap,
            "a wider node did not give its text more room: {resized_wrap} vs {wrap}"
        );
        assert_ne!(
            resized_key, key,
            "a resized node served its old paragraph, laid out for a box it \
             has left"
        );
        assert_eq!(
            resized_key.wrap_width,
            (resized_wrap * 10.0).round() as u32,
            "the key and the primitive disagree about the wrap width"
        );
    }

    /// **§40 rule 7, extended to text**: a pure pan re-shapes nothing.
    ///
    /// Phase 4 asserted this for edge routes and Phase 5 for tessellations;
    /// this is the same question for the third cache. It drives a real
    /// [`ShapedLineCache`](crate::render::cache::ShapedLineCache) over sixty
    /// panned frames and counts its misses, rather than comparing keys by eye —
    /// the cache's retention window is part of the answer and only the cache
    /// knows it.
    ///
    /// It covers the wrap width for free, because Phase 10.5 put the wrap width
    /// **in the key**: a pan changes no element's screen size, so a wrapped
    /// paragraph is laid out once here too.
    #[test]
    fn a_pure_pan_shapes_every_label_once_and_never_again() {
        use crate::render::cache::ShapedLineCache;

        let world = labelled_pair();
        let mut cache: ShapedLineCache<u32> =
            ShapedLineCache::new(&for_backend(RenderBackend::Metal));

        for step in 0..60 {
            let viewport = Viewport::new(
                Vec2::new(step as f32 * 3.0, 0.0),
                0.5,
                Vec2::new(800.0, 600.0),
            );
            let (plan, _) = frame_without_grid(&world, &viewport);
            let labels = labels_of(&plan);
            assert_eq!(labels.len(), 3, "two node labels and one edge label");

            cache.begin_frame();
            for (key, _, _) in labels {
                if cache.get(&key).is_none() {
                    cache.insert(key, 1);
                }
            }
            cache.end_frame();
        }

        let stats = cache.stats();
        assert_eq!(
            stats.misses, 3,
            "sixty panned frames must shape each label exactly once"
        );
        assert_eq!(stats.reused, 60 * 3 - 3);
        assert_eq!(stats.evictions, 0, "nothing left the viewport");
    }

    /// **A text element is its glyphs and nothing else.**
    ///
    /// The negative half is the one that matters: `plan_nodes` skips
    /// `NodeShape::Text` outright, because the fall-through would have painted
    /// it as a filled quad the moment it stopped being detailed — a box around
    /// every piece of standalone text, appearing only when zoomed out.
    #[test]
    fn a_text_element_draws_its_text_and_no_body() {
        let mut world = GraphWorld::new();
        let node = world.create_node(
            ElementKind::Text,
            Vec2::new(100.0, 100.0),
            Vec2::new(200.0, 22.0),
        );
        world.set_node_label(node, Some("standalone".into()));
        world.rebuild_all_geometry();
        world.clear_spatial_updates();

        let viewport = pane();
        let (plan, stats) = frame_without_grid(&world, &viewport);

        assert_eq!(plan.text_count(), 1, "the glyphs are the whole element");
        assert_eq!(plan.quad_count(), 0, "no box");
        assert_eq!(plan.path_count(), 0, "no outline");
        assert_eq!(
            stats.unsupported_nodes, 0,
            "text is painted now, not counted as missing"
        );

        // And it is still a real element: it has bounds, so it is culled,
        // hit-tested and selected exactly like everything else.
        let labels = labels_of(&plan);
        assert_eq!(labels[0].2.as_ref(), "standalone");
    }

    /// The same element zoomed far out: §15's first bullet, on the kind that
    /// has nothing *but* text. Nothing is drawn — not a box, not a smudge —
    /// because there is nothing else it could be reduced to.
    #[test]
    fn a_text_element_below_readable_zoom_costs_nothing_at_all() {
        let mut world = GraphWorld::new();
        let node = world.create_node(
            ElementKind::Text,
            Vec2::new(0.0, 0.0),
            Vec2::new(200.0, 22.0),
        );
        world.set_node_label(node, Some("standalone".into()));
        world.rebuild_all_geometry();
        world.clear_spatial_updates();

        let viewport = Viewport::new(Vec2::new(-2_000.0, -2_000.0), 0.1, Vec2::new(800.0, 600.0));
        let (plan, _) = frame_without_grid(&world, &viewport);

        assert_eq!(
            plan.text_count(),
            0,
            "§15: not laid out, not merely unpainted"
        );
        assert_eq!(plan.quad_count(), 0);
        assert_eq!(plan.path_count(), 0);
    }

    /// **Each element is quantised against its own authored step**, so `S` text
    /// stops being laid out at a zoom `XL` text survives. The alternative — one
    /// size per frame — is what the ladder did before §9's four steps existed,
    /// and it would draw a heading and a footnote at the same size.
    #[test]
    fn two_sizes_of_text_disappear_at_two_different_zooms() {
        use crate::models::FontSize;

        let mut world = GraphWorld::new();
        for (index, size) in [FontSize::Small, FontSize::ExtraLarge]
            .into_iter()
            .enumerate()
        {
            let node = world.create_node(
                ElementKind::Text,
                Vec2::new(0.0, index as f32 * 60.0),
                Vec2::new(200.0, 40.0),
            );
            world.set_node_label(node, Some(size.name().to_owned()));
            let mut style = world.nodes().style(node).clone();
            style.font.size = size;
            world.set_node_style(node, style);
        }
        world.rebuild_all_geometry();
        world.clear_spatial_updates();

        let close = Viewport::new(Vec2::new(-20.0, -20.0), 1.0, Vec2::new(800.0, 600.0));
        let (plan, _) = frame_without_grid(&world, &close);
        assert_eq!(plan.text_count(), 2, "both read at 100 %");

        // 12 × 0.3 = 3.6 px, under the readable floor; 28 × 0.3 = 8.4 px, over.
        let far = Viewport::new(Vec2::new(-20.0, -20.0), 0.3, Vec2::new(800.0, 600.0));
        let (plan, _) = frame_without_grid(&world, &far);
        let drawn = labels_of(&plan);
        assert_eq!(drawn.len(), 1, "only the large one is still worth shaping");
        assert_eq!(drawn[0].2.as_ref(), "xl");
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

    // ---- the rich half's body, in both render styles --------------------

    /// One rectangle, big enough at 100 % zoom to be a rich node, wearing a
    /// stroke colour and a fill nothing else in the frame uses.
    fn styled_rich_rectangle(style: crate::models::RenderStyle) -> (GraphWorld, NodeIndex) {
        let mut world = GraphWorld::new();
        world.settings_mut().render_style = style;

        let node = world.create_node(
            ElementKind::Shape(crate::models::ShapeKind::Rectangle),
            Vec2::new(120.0, 120.0),
            Vec2::new(240.0, 140.0),
        );
        world.set_node_style(
            node,
            crate::models::ElementStyle {
                stroke: crate::models::StrokeStyle {
                    color: Some(RICH_STROKE),
                    width: 3.0,
                    ..crate::models::StrokeStyle::default()
                },
                fill: Some(RICH_FILL),
                ..crate::models::ElementStyle::default()
            },
        );

        world.rebuild_all_geometry();
        world.clear_spatial_updates();
        (world, node)
    }

    /// Two colours no theme ink and no other element in these tests uses, so
    /// finding either in the plan means the *document's* style reached the
    /// painter and not a fallback that happened to look similar.
    const RICH_STROKE: Color = Color::rgba(0.91, 0.13, 0.42, 1.0);
    const RICH_FILL: Color = Color::rgba(0.07, 0.63, 0.51, 1.0);

    fn plan_holds_color(plan: &PaintPlan, wanted: Color) -> bool {
        let close = |color: Color| {
            (color.r - wanted.r).abs() < 1e-3
                && (color.g - wanted.g).abs() < 1e-3
                && (color.b - wanted.b).abs() < 1e-3
        };

        plan.quads().iter().any(|quad| {
            close(quad.background) || (quad.border_width > 0.0 && close(quad.border_color))
        }) || plan.paths().iter().any(|path| close(path.paint.color()))
    }

    /// **A restyled node must look restyled in both render styles.**
    ///
    /// This is Phase 12.5's second bug, and it is Phase 11's `fill_style` one
    /// layer up. A rectangle at working zoom is a *rich* node — a GPUI element
    /// — and the element used to paint its own body from `theme.border` and
    /// `theme.secondary`. So the whole property panel wrote colours that
    /// nothing on screen read, unless the document happened to be in Sketch
    /// mode, where a hand-drawn border has no `div` form and the canvas had to
    /// paint the body. The captain saw that as "properties only apply in Sketch
    /// mode".
    ///
    /// The test asserts **what reaches the painter**, for the reason Phase 11
    /// recorded: a test on the model passes either way, and did.
    #[test]
    fn a_restyled_rich_node_reaches_the_painter_in_both_render_styles() {
        use crate::models::RenderStyle;

        for style in [RenderStyle::Clean, RenderStyle::Sketch] {
            let (world, node) = styled_rich_rectangle(style);
            let viewport = Viewport::new(Vec2::ZERO, 1.0, Vec2::new(900.0, 600.0));

            // Without this the test could pass vacuously on a frame that drew
            // the node through the canvas half, which was never broken.
            let snapshot = extracted(&world, &viewport);
            assert_eq!(
                snapshot.rich().iter().map(|it| it.node).collect::<Vec<_>>(),
                vec![node],
                "{style:?}: this node has to be a rich element for the test to \
                 mean anything"
            );

            let (plan, _) = frame_without_grid(&world, &viewport);
            assert!(
                plan_holds_color(&plan, RICH_STROKE),
                "{style:?}: the node's own stroke colour never reached the painter"
            );
            assert!(
                plan_holds_color(&plan, RICH_FILL),
                "{style:?}: the node's own fill never reached the painter"
            );
        }
    }

    /// **Both halves of the renderer must agree about a body's opacity, its
    /// hatch and its width too** — the properties a `div` could never have
    /// expressed, which is why fixing the two it *could* would not have been a
    /// smaller version of the same repair.
    ///
    /// Asserted as a hatch reaching the plan as its own cached part, exactly as
    /// Phase 11 asserts it for a canvas node.
    #[test]
    fn a_hatched_rich_node_is_hatched_in_clean_mode() {
        use crate::{
            models::{FillStyle, RenderStyle},
            render::cache::GeometryPart,
        };

        let (mut world, node) = styled_rich_rectangle(RenderStyle::Clean);
        let mut style = world.nodes().style(node).clone();
        style.fill_style = FillStyle::Hachure;
        world.set_node_style(node, style);
        world.rebuild_all_geometry();

        let viewport = Viewport::new(Vec2::ZERO, 1.0, Vec2::new(900.0, 600.0));
        let (plan, _) = frame_without_grid(&world, &viewport);

        assert!(
            plan.paths()
                .iter()
                .filter_map(|path| path.key)
                .any(|key| key.part == GeometryPart::Hatch),
            "a rich node's hachure never reached the painter"
        );
    }

    /// **A body's dash reaches the painter as a dashed stroke.**
    ///
    /// §32's Stroke style row is offered for a node and for an edge alike, and
    /// only the edge painter ever read it: a node's dash was stored, undoable,
    /// read back by the panel and painted by nothing, in *both* render styles.
    /// Third of the four "written and not drawn" controls this crate has
    /// shipped, and asserted the way Phase 11 learnt to assert them — on the
    /// primitive, not on the model.
    ///
    /// A dashed rectangle also has to stop being a quad's border, because a
    /// quad carries a solid one and nothing else; the fill stays a quad, so the
    /// dash costs exactly one path.
    #[test]
    fn a_dashed_node_border_reaches_the_painter_dashed() {
        use crate::{
            models::{DashPattern, RenderStyle},
            render::plan::PathPaint,
        };

        let (mut world, node) = styled_rich_rectangle(RenderStyle::Clean);
        let viewport = Viewport::new(Vec2::ZERO, 1.0, Vec2::new(900.0, 600.0));

        let (solid, _) = frame_without_grid(&world, &viewport);
        assert!(
            !solid
                .paths()
                .iter()
                .any(|path| matches!(path.paint, PathPaint::DashedStroke { .. })),
            "nothing was dashed before the document asked for it"
        );

        let mut style = world.nodes().style(node).clone();
        style.stroke.dash = DashPattern::new([8.0, 4.0]);
        world.set_node_style(node, style);
        world.rebuild_all_geometry();

        let (dashed, _) = frame_without_grid(&world, &viewport);
        let stroke = dashed
            .paths()
            .iter()
            .find(|path| matches!(path.paint, PathPaint::DashedStroke { .. }))
            .expect("the node's dash never reached the painter");

        let PathPaint::DashedStroke { dash, color, .. } = stroke.paint else {
            unreachable!("filtered above");
        };
        assert!((dash.on - 8.0).abs() < 1e-3);
        assert!((dash.off - 4.0).abs() < 1e-3);
        assert!(
            (color.r - RICH_STROKE.r).abs() < 1e-3,
            "and in its own colour"
        );

        // The body kept its quad, so the dash cost one path and no second
        // batch of anything.
        assert_eq!(dashed.quad_count(), solid.quad_count());
        assert_eq!(dashed.path_count(), solid.path_count() + 1);
    }

    /// **A text element's Stroke row is its colour, and it has to reach the
    /// glyphs.**
    ///
    /// The panel gives a text selection exactly one colour control
    /// (`properties`' table gives it Stroke and no Background), the control
    /// writes `stroke.color`, and every text painter read `font.color` — which
    /// nothing writes. So the one thing a person can change about a text
    /// element's appearance beyond its font did nothing at all, in both render
    /// styles. Fourth of the same kind.
    #[test]
    fn a_text_element_is_drawn_in_the_colour_its_stroke_row_writes() {
        let mut world = GraphWorld::new();
        let node = world.create_node(
            ElementKind::Text,
            Vec2::new(80.0, 80.0),
            Vec2::new(220.0, 24.0),
        );
        world.set_node_label(node, Some("a sentence".into()));

        let mut style = world.nodes().style(node).clone();
        style.stroke.color = Some(RICH_STROKE);
        world.set_node_style(node, style);
        world.rebuild_all_geometry();
        world.clear_spatial_updates();

        let viewport = Viewport::new(Vec2::ZERO, 1.0, Vec2::new(900.0, 600.0));
        let (plan, stats) = frame_without_grid(&world, &viewport);

        assert_eq!(stats.labels, 1, "the text element was not laid out at all");
        let text = plan.texts().first().expect("one run");
        assert!(
            (text.color.r - RICH_STROKE.r).abs() < 1e-3
                && (text.color.g - RICH_STROKE.g).abs() < 1e-3
                && (text.color.b - RICH_STROKE.b).abs() < 1e-3,
            "the glyphs were painted in {:?} rather than the colour the panel wrote",
            text.color
        );
    }

    /// **§9's caret opens on a straight connector, so its label has to come
    /// back out** — and on the segment, not on a rectangle it does not have.
    ///
    /// Two failures met here at once, and both looked identical from the
    /// canvas — the words vanished on commit. `render::registry` answered
    /// `shows_label: false` for every `Linear` kind, so a committed label was
    /// written to the document and read by no painter; and the generic path
    /// insets the node's box by `LABEL_PADDING_PIXELS`, which for an
    /// axis-aligned connector is a box of zero height and a `continue`.
    #[test]
    fn a_connector_label_is_drawn_on_its_true_segment_midpoint() {
        // Right-to-left and bottom-to-top, so a normalised rectangle would
        // disagree with the ordered segment about everything but the midpoint.
        for (start, end) in [
            (Vec2::new(420.0, 360.0), Vec2::new(120.0, 100.0)),
            // Horizontal: the zero-height box the inset used to throw away.
            (Vec2::new(420.0, 240.0), Vec2::new(120.0, 240.0)),
        ] {
            let mut world = GraphWorld::new();
            let node = world.create_node(
                ElementKind::Linear(crate::models::LinearKind::Arrow),
                start,
                end - start,
            );
            world.set_node_connector(node, crate::models::Connector::new(start, end));
            world.set_node_label(node, Some("weighs 3".into()));
            world.rebuild_all_geometry();
            world.clear_spatial_updates();

            let viewport = Viewport::new(Vec2::ZERO, 1.0, Vec2::new(900.0, 600.0));
            let (plan, stats) = frame_without_grid(&world, &viewport);

            assert_eq!(stats.labels, 1, "{start:?} -> {end:?} lost its label");
            let text = plan.texts().first().expect("one run");
            let midpoint = viewport.world_to_screen((start + end) * 0.5);
            assert!(
                (text.origin.x + text.max_width * 0.5 - midpoint.x).abs() < 1e-3,
                "{start:?} -> {end:?}: {text:?} is not centred on {midpoint:?}",
            );
            assert!(
                (text.origin.y + text.font_size * 0.5 - midpoint.y).abs() < 1e-3,
                "{start:?} -> {end:?}: {text:?} is not centred on {midpoint:?}",
            );
        }
    }

    /// The same registry row cost a *drawn shape* its label, and a drawn shape
    /// is what most people type into. Asserted on an ellipse so the answer
    /// cannot come from the rich half — only `NodeShape::Rectangle` and friends
    /// become elements.
    #[test]
    fn a_drawn_shapes_label_reaches_the_canvas() {
        let mut world = GraphWorld::new();
        let node = world.create_node(
            ElementKind::Shape(crate::models::ShapeKind::Ellipse),
            Vec2::new(120.0, 120.0),
            Vec2::new(220.0, 120.0),
        );
        world.set_node_label(node, Some("decision".into()));
        world.rebuild_all_geometry();
        world.clear_spatial_updates();

        let viewport = Viewport::new(Vec2::ZERO, 1.0, Vec2::new(900.0, 600.0));
        let (plan, stats) = frame_without_grid(&world, &viewport);

        assert_eq!(stats.labels, 1, "a drawn shape's label was never laid out");
        assert_eq!(plan.texts().len(), 1);
    }

    /// **Where a label's block actually lands**, which is not
    /// `TextPrimitive::origin`: the painter adds both alignment offsets once it
    /// knows how wide and how tall the shaped text turned out.
    ///
    /// So a test that asks "is this label centred?" has to do the same
    /// arithmetic, with a `shaped` width standing in for the text system's
    /// answer. Reading `origin` alone is exactly how a label that is aligned
    /// against the left edge of a centred box passes for centred.
    fn block_centre(
        text: &crate::render::plan::TextPrimitive,
        shaped_width: f32,
        lines: u32,
    ) -> Vec2 {
        let left = text.origin.x + text.align.offset(text.max_width, shaped_width);
        let top = text.origin.y + text.vertical_offset(lines);
        let height = text.font_size + (lines.saturating_sub(1) as f32) * text.line_height();
        Vec2::new(left + shaped_width * 0.5, top + height * 0.5)
    }

    /// A style with a label centred on its element, which is what
    /// `FlowEditor::commit_text` leaves behind and what the format's version-5
    /// rung writes into every older file.
    fn centred_label_style() -> crate::models::ElementStyle {
        let mut style = crate::models::ElementStyle::default();
        style.font.centre_on_element();
        style
    }

    /// **Requirement 1, as the frame the painter is handed**: a label sits in
    /// the middle of the element it belongs to, for a node and for both kinds
    /// of line.
    ///
    /// Asserted on the *block*, not on the primitive's origin. Before this
    /// phase a connector's and a node's label were laid into a centred box and
    /// then aligned against its **left edge**, so the box was centred and the
    /// words were not — a bug `origin`-only assertions cannot see, and the
    /// reason `block_centre` exists.
    #[test]
    fn a_label_is_centred_on_the_element_it_belongs_to() {
        let shaped = 84.0;
        let viewport = Viewport::new(Vec2::ZERO, 1.0, Vec2::new(900.0, 600.0));

        // A drawn shape: an ellipse, so the answer cannot come from the rich
        // half — only rectangles become elements.
        let mut world = GraphWorld::new();
        let body = Rect::new(Vec2::new(120.0, 120.0), Vec2::new(240.0, 140.0));
        let node = world.create_node(
            ElementKind::Shape(crate::models::ShapeKind::Ellipse),
            body.origin,
            body.size,
        );
        world.set_node_label(node, Some("decision".into()));
        world.set_node_style(node, centred_label_style());
        world.rebuild_all_geometry();
        world.clear_spatial_updates();

        let (plan, _) = frame_without_grid(&world, &viewport);
        let text = plan.texts().first().expect("the shape's label");
        let want = viewport.world_to_screen(body.center());
        let got = block_centre(text, shaped, 1);
        assert!(
            (got - want).length() < 1e-3,
            "a shape's label landed at {got:?} rather than on its centre {want:?}"
        );

        // A straight connector, drawn right-to-left so a normalised rectangle
        // would disagree with the ordered segment about everything but this.
        let (start, end) = (Vec2::new(520.0, 380.0), Vec2::new(160.0, 120.0));
        let mut world = GraphWorld::new();
        let connector = world.create_node(
            ElementKind::Linear(crate::models::LinearKind::Arrow),
            start,
            end - start,
        );
        world.set_node_connector(connector, crate::models::Connector::new(start, end));
        world.set_node_label(connector, Some("weighs 3".into()));
        world.set_node_style(connector, centred_label_style());
        world.rebuild_all_geometry();
        world.clear_spatial_updates();

        let (plan, _) = frame_without_grid(&world, &viewport);
        let text = plan.texts().first().expect("the connector's label");
        let want = viewport.world_to_screen((start + end) * 0.5);
        let got = block_centre(text, shaped, 1);
        assert!(
            (got - want).length() < 1e-3,
            "a connector's label landed at {got:?} rather than on its midpoint {want:?}"
        );

        // And a graph edge, on its route's arc-length midpoint.
        let mut world = labelled_pair();
        let edge = world.edges().indices().next().expect("one edge");
        world.set_edge_style(edge, centred_label_style());
        world.rebuild_all_geometry();
        world.clear_spatial_updates();

        let viewport = pane();
        let (plan, _) = frame_without_grid(&world, &viewport);
        let text = plan
            .texts()
            .iter()
            .find(|text| text.text.as_ref() == "carries")
            .expect("the edge's label");
        let flatten = viewport.screen_to_world_length(1.0);
        let want = viewport.world_to_screen(world.route(edge).unwrap().midpoint(flatten));
        let got = block_centre(text, shaped, 1);
        assert!(
            (got - want).length() < 1e-3,
            "an edge's label landed at {got:?} rather than on its route {want:?}"
        );
    }

    /// **Requirement 2, as the colour in the frame**: a label is drawn in its
    /// element's stroke colour, and a stroke change moves it with no second
    /// press.
    ///
    /// This is the sixth costume of `properties`' rule — a row the panel writes
    /// and no painter reads — so it is asserted on what reaches the painter
    /// rather than on the model. The Stroke row is the only colour control a
    /// node or an edge has; before this phase it moved the outline and left the
    /// words in the theme's foreground.
    #[test]
    fn a_label_is_drawn_in_its_elements_stroke_colour_and_follows_a_change() {
        let mut world = labelled_pair();
        let node = world.nodes().indices().next().expect("a node");
        let edge = world.edges().indices().next().expect("an edge");
        let viewport = pane();

        let theme_ink = ink().text;
        let colours = |world: &GraphWorld| {
            let (plan, _) = frame_without_grid(world, &viewport);
            plan.texts()
                .iter()
                .map(|text| (std::sync::Arc::clone(&text.text), text.color))
                .collect::<Vec<_>>()
        };

        for (label, color) in colours(&world) {
            assert_eq!(color, theme_ink, "{label} did not start on the theme's ink");
        }

        let red = Color::rgb(0.878, 0.192, 0.192);
        let mut style = centred_label_style();
        style.stroke.color = Some(red);
        world.set_node_style(node, style.clone());
        world.set_edge_style(edge, style);
        world.rebuild_all_geometry();
        world.clear_spatial_updates();

        let after = colours(&world);
        assert_eq!(after.len(), 3, "three labels: two nodes and the edge");
        for (label, color) in after {
            if label.as_ref() == "sink" {
                assert_eq!(color, theme_ink, "the untouched node's label moved");
            } else {
                assert_eq!(color, red, "{label} did not follow its stroke");
            }
        }
    }

    /// **A selected element keeps its own border**, and that is the whole of
    /// requirement 3.
    ///
    /// Selection used to repaint the body's stroke in the accent, floor it at
    /// [`SELECTED_STROKE_PIXELS`], force a border on to a style that asked for
    /// none, and drop the dash. That hides the one thing a person needs while
    /// they are choosing a border colour — the border colour — and the element
    /// whose colour they are choosing is *always* the selected one, because the
    /// property panel is a view of the selection. §44's bounding box already
    /// says "selected"; see [`NodeBody::accented`].
    ///
    /// Asserted on the quad the painter is handed, four properties at a time,
    /// against the same node unselected. A body that is one path rather than
    /// one quad takes the same `stroke_color`, so the quad is the cheapest
    /// place to read the decision rather than a special case of it.
    #[test]
    fn a_selected_element_draws_its_own_border_rather_than_the_accent() {
        let viewport = Viewport::new(Vec2::ZERO, 1.0, Vec2::new(900.0, 600.0));
        let teal = Color::rgb(0.13, 0.55, 0.55);

        let build = || {
            let mut world = GraphWorld::new();
            let node = world.create_node(
                ElementKind::Shape(crate::models::ShapeKind::Rectangle),
                Vec2::new(120.0, 120.0),
                Vec2::new(240.0, 140.0),
            );
            let mut style = crate::models::ElementStyle::default();
            style.stroke.color = Some(teal);
            style.stroke.width = 1.0;
            world.set_node_style(node, style);
            world.rebuild_all_geometry();
            world.clear_spatial_updates();
            (world, node)
        };

        // The body's quad: the only one whose bounds are the node's box, so a
        // grid dot or a handle dot cannot be mistaken for it.
        let body_quad = |world: &GraphWorld| {
            let (plan, _) = frame_without_grid(world, &viewport);
            let screen = viewport.world_rect_to_screen(
                world
                    .nodes()
                    .bounds(world.nodes().indices().next().expect("a node")),
            );
            *plan
                .quads()
                .iter()
                .find(|quad| (quad.bounds.origin - screen.origin).length() < 1e-3)
                .expect("the node's body reached the painter as a quad")
        };

        let (world, _) = build();
        let unselected = body_quad(&world);
        assert_eq!(unselected.border_color, fade(teal, 1.0));

        let (mut world, node) = build();
        world.set_node_selected(node, true);
        let selected = body_quad(&world);
        assert_eq!(
            selected.border_color, unselected.border_color,
            "selecting the node repainted its border in the accent, so the colour \
             the property panel is editing is the one thing that cannot be seen"
        );
        assert_eq!(
            selected.border_width, unselected.border_width,
            "selecting the node thickened its border"
        );
    }

    /// **The two cases that still take the accent, stated so a change to either
    /// is deliberate.**
    ///
    /// Both are elements §44 draws no bounding box around, so for both the
    /// accent is the only thing on screen saying anything at all — see
    /// [`NodeBody::accented`]. A multiple selection is the case the fix above
    /// would otherwise have made invisible; hover has no box either, and a
    /// *drawn shape* has no handle elements to fall back on
    /// (`commands::gesture`'s `handles_for`).
    #[test]
    fn an_element_with_no_bounding_box_still_takes_the_accent() {
        let viewport = Viewport::new(Vec2::ZERO, 1.0, Vec2::new(900.0, 600.0));

        let mut world = GraphWorld::new();
        let mut style = crate::models::ElementStyle::default();
        style.stroke.color = Some(Color::rgb(0.13, 0.55, 0.55));
        style.stroke.width = 1.0;
        let nodes: Vec<_> = [120.0f32, 480.0]
            .into_iter()
            .map(|x| {
                let node = world.create_node(
                    ElementKind::Shape(crate::models::ShapeKind::Rectangle),
                    Vec2::new(x, 120.0),
                    Vec2::new(240.0, 140.0),
                );
                world.set_node_style(node, style.clone());
                node
            })
            .collect();
        world.rebuild_all_geometry();
        world.clear_spatial_updates();

        let border_of = |world: &GraphWorld, hovered: Option<NodeIndex>, node: NodeIndex| {
            let index = SpatialIndex::for_world(world);
            let mut visible = crate::spatial::VisibleSet::new();
            index.query_visible(world, &viewport, &mut visible);

            let mut snapshot = RenderSnapshot::new();
            snapshot.extract(
                world,
                &visible,
                &viewport,
                &for_backend(RenderBackend::Metal),
                &crate::render::registry::NodeRendererRegistry::with_generic_kinds(),
                hovered,
                Rect::new(Vec2::ZERO, viewport.size()),
            );

            let options = SceneOptions::new(
                GridSettings {
                    style: GridStyle::None,
                    ..GridSettings::default()
                },
                GridLimits::from_budgets(&for_backend(RenderBackend::Metal)),
            );
            let mut plan = PaintPlan::new();
            plan_scene(&mut plan, world, &snapshot, &viewport, ink(), &options);

            let screen = viewport.world_rect_to_screen(world.nodes().bounds(node));
            plan.quads()
                .iter()
                .find(|quad| (quad.bounds.origin - screen.origin).length() < 1e-3)
                .expect("the node's body reached the painter as a quad")
                .border_color
        };

        // Two selected: no overlay, so the accent is the only signal.
        world.set_node_selected(nodes[0], true);
        world.set_node_selected(nodes[1], true);
        assert!(world.selection().single_node().is_none());
        assert_eq!(border_of(&world, None, nodes[0]), ink().accent);

        // One selected: the bounding box says it, so the border is its own.
        world.set_node_selected(nodes[1], false);
        assert_eq!(world.selection().single_node(), Some(nodes[0]));
        assert_ne!(border_of(&world, None, nodes[0]), ink().accent);

        // Hovered: no box, and a drawn shape has no handles either.
        world.set_node_selected(nodes[0], false);
        assert_eq!(border_of(&world, Some(nodes[0]), nodes[0]), ink().accent);
    }

    /// **Requirement 3, as the frame again**: each of the two alignment rows
    /// moves the label it is drawn over.
    ///
    /// Three distinct, ordered placements per axis, all of them inside the
    /// element — a row that wrote a field no painter read would give three
    /// identical answers, which is the failure this crate has now met six
    /// times.
    #[test]
    fn the_alignment_rows_move_a_label_inside_its_element() {
        let shaped = 60.0;
        let viewport = Viewport::new(Vec2::ZERO, 1.0, Vec2::new(900.0, 600.0));
        let body = Rect::new(Vec2::new(120.0, 120.0), Vec2::new(240.0, 140.0));

        let placed = |align: crate::models::TextAlign, vertical: crate::models::VerticalAlign| {
            let mut world = GraphWorld::new();
            let node = world.create_node(
                ElementKind::Shape(crate::models::ShapeKind::Ellipse),
                body.origin,
                body.size,
            );
            world.set_node_label(node, Some("decision".into()));
            let mut style = centred_label_style();
            style.font.align = align;
            style.font.vertical_align = vertical;
            world.set_node_style(node, style);
            world.rebuild_all_geometry();
            world.clear_spatial_updates();

            let (plan, _) = frame_without_grid(&world, &viewport);
            block_centre(plan.texts().first().expect("a label"), shaped, 1)
        };

        use crate::models::{TextAlign, VerticalAlign};
        let middle = viewport.world_rect_to_screen(body).center();

        let left = placed(TextAlign::Left, VerticalAlign::Middle);
        let centre = placed(TextAlign::Center, VerticalAlign::Middle);
        let right = placed(TextAlign::Right, VerticalAlign::Middle);
        assert!(
            left.x < centre.x && centre.x < right.x,
            "the text-align row did not move the label: {left:?} {centre:?} {right:?}"
        );
        assert!((centre.x - middle.x).abs() < 1e-3);
        assert!((left.y - centre.y).abs() < 1e-3 && (right.y - centre.y).abs() < 1e-3);

        let top = placed(TextAlign::Center, VerticalAlign::Top);
        let bottom = placed(TextAlign::Center, VerticalAlign::Bottom);
        assert!(
            top.y < centre.y && centre.y < bottom.y,
            "the vertical-align row did not move the label: {top:?} {centre:?} {bottom:?}"
        );
        assert!((top.x - centre.x).abs() < 1e-3 && (bottom.x - centre.x).abs() < 1e-3);

        // Inside the element, padding included: a label pushed to an edge must
        // not sit on the border it is drawn against.
        let inner = viewport
            .world_rect_to_screen(body)
            .inflate(-LABEL_PADDING_PIXELS);
        for point in [top, bottom, left, right] {
            assert!(
                point.y >= inner.origin.y && point.y <= inner.max().y,
                "{point:?} left the element's inner box {inner:?}"
            );
        }
    }

    /// The other two text rows, on the same principle: the family and the
    /// authored size both have to arrive at the painter, because a row that
    /// stops at the document is a control that does nothing.
    #[test]
    fn the_font_family_and_size_rows_reach_the_painter() {
        let viewport = Viewport::new(Vec2::ZERO, 1.0, Vec2::new(900.0, 600.0));
        let mut world = GraphWorld::new();
        let node = world.create_node(
            ElementKind::Shape(crate::models::ShapeKind::Ellipse),
            Vec2::new(120.0, 120.0),
            Vec2::new(240.0, 140.0),
        );
        world.set_node_label(node, Some("decision".into()));
        let mut style = centred_label_style();
        style.font.family = crate::models::FontFamily::Code;
        style.font.size = crate::models::FontSize::ExtraLarge;
        world.set_node_style(node, style);
        world.rebuild_all_geometry();
        world.clear_spatial_updates();

        let (plan, _) = frame_without_grid(&world, &viewport);
        let text = plan.texts().first().expect("a label");
        assert_eq!(text.family, crate::models::FontFamily::Code);
        assert_eq!(
            text.font_size,
            crate::models::FontSize::ExtraLarge.world_size(),
            "at 100 % zoom the LOD quantiser is the identity"
        );
    }

    /// The snapshot a frame is planned from, for the tests that need to know
    /// which half of the renderer took a node.
    fn extracted(world: &GraphWorld, viewport: &Viewport) -> RenderSnapshot {
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
        snapshot
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

    // ---- §7's free linear elements, in a real frame ----------------------

    /// **A drawn line reaches the painter as one stroked path and no quad.**
    ///
    /// The three things that would each silently break it: a fill pass on a
    /// zero-area outline, `has_stroke` dropping the only pass the shape has,
    /// and the curve/detail degradation turning a diagonal into the solid box
    /// it spans. Asserted on what a real frame plans, at three zooms, because
    /// two of the three only happen at some of them.
    #[test]
    fn a_free_line_is_stroked_once_and_never_becomes_a_quad() {
        for kind in [
            ElementKind::Linear(crate::models::LinearKind::Line),
            ElementKind::Linear(crate::models::LinearKind::Arrow),
        ] {
            for zoom in [1.0, 0.4, 0.08] {
                let mut world = GraphWorld::new();
                world.create_node(
                    kind.clone(),
                    Vec2::new(-100.0, -50.0),
                    Vec2::new(200.0, 100.0),
                );
                world.rebuild_all_geometry();
                world.clear_spatial_updates();

                let viewport = Viewport::new(Vec2::ZERO, zoom, Vec2::new(1_440.0, 900.0));
                let (plan, stats) = frame_without_grid(&world, &viewport);

                assert_eq!(stats.nodes, 1, "{kind:?} at {zoom} was not planned");
                assert_eq!(
                    plan.quads().len(),
                    0,
                    "{kind:?} at {zoom} was painted as a quad"
                );
                assert_eq!(
                    plan.paths().len(),
                    1,
                    "{kind:?} at {zoom} should be exactly one stroked path"
                );
                assert!(
                    plan.paths()[0].paint.width().is_some(),
                    "{kind:?} at {zoom} was filled rather than stroked"
                );
                assert!(
                    plan.paths()[0].paint.width().unwrap() >= MIN_OPEN_STROKE_PIXELS,
                    "{kind:?} at {zoom} was stroked below the visible floor"
                );
            }
        }
    }

    /// A hand-drawn line is still only strokes: §13's fill pass has nothing to
    /// fill, and a sketched fill of an open outline is a shape lyon invents.
    #[test]
    fn a_sketched_line_adds_strokes_and_no_fill() {
        let mut world = GraphWorld::new();
        world.create_node(
            ElementKind::Linear(crate::models::LinearKind::Line),
            Vec2::new(-100.0, -50.0),
            Vec2::new(200.0, 100.0),
        );
        world.settings_mut().render_style = crate::models::RenderStyle::Sketch;
        world.rebuild_all_geometry();
        world.clear_spatial_updates();

        let viewport = Viewport::new(Vec2::ZERO, 1.0, Vec2::new(1_440.0, 900.0));
        let (plan, stats) = frame_without_grid(&world, &viewport);

        assert_eq!(stats.sketched_bodies, 1);
        assert_eq!(plan.quads().len(), 0);
        assert!(!plan.paths().is_empty());
        for path in plan.paths() {
            assert!(
                path.paint.width().is_some(),
                "a sketched line must be strokes only"
            );
        }
    }
}
