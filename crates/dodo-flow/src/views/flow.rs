//! [`FlowView`] — the canvas pane: the grid, the shapes, pan, zoom and the
//! selection rectangle.
//!
//! # The shape of a frame
//!
//! `render` builds a `div` and a `canvas()` and does **no work**. Everything
//! happens in the canvas's paint closure, and that split is a contract rather
//! than a style: dodo's root `AGENTS.md` records why `render` bodies must stay
//! cheap — a dirty child marks its whole ancestor path dirty, and an ancestor
//! redraw sets `Window::refreshing`, which bypasses the element cache for every
//! descendant. So this `render` re-runs whenever anything above it in the app
//! redraws, with nothing of its own changed. A `render` that generated the grid
//! would generate it for a progress tick in another tool.
//!
//! Inside the paint closure the order is fixed and each step earns its place:
//!
//! ```text
//! measure pane ─> resolve theme ink ─> clear plan ─> grid ─> shapes ─>
//! selection rect ─> enforce vertex ceiling ─> paint_into(WindowPainter) ─>
//! install input listeners
//! ```
//!
//! The plan is a field rather than a local, so a pan frame reuses its buffers
//! instead of reallocating them (§40 rules 13 and 14), and
//! `enforce_vertex_ceiling` runs before anything is painted because the ceiling
//! it guards is a **black window**, not a slow frame — see [`crate::budgets`].
//!
//! # Repaint is driven by change, never by a clock
//!
//! §35 and §40 rule 15 forbid a permanent frame loop, and Phase 0 measured the
//! idle cost of getting this right at 2 paints in 3 seconds. Every repaint here
//! comes from a `cx.notify()` in a mouse or key handler — a real event that
//! really changed something, which is why [`InteractionEffect::needs_repaint`]
//! exists rather than an unconditional notify.
//!
//! There is a trap immediately next to this and dodo's root `AGENTS.md` names
//! it: `WindowInvalidator::invalidate_view` only marks the window dirty in
//! `DrawPhase::None`, so a `cx.notify()` from inside prepaint or paint records
//! the dirty view and **schedules nothing**. Nothing in this file notifies from
//! paint. The paint closure does mutate `self` — it measures the pane, records
//! [`PaintStats`] and stores the grid level — but every one of those is
//! *derived from the frame being painted*, so none of them needs another frame.
//!
//! # Input, and what is actually known about it
//!
//! Listeners are registered from inside paint via `Window::on_mouse_event`,
//! which is the only place GPUI accepts them. There is **no implicit hit
//! testing** — a listener registered this way receives every mouse event in the
//! window — so every handler checks the canvas hitbox itself, and positions
//! arrive in *window* coordinates and have the element's origin subtracted
//! before they reach [`Viewport`].
//!
//! Drags use `Window::capture_pointer`, which routes moves and the mouse-up to
//! this hitbox regardless of where the pointer actually is, and auto-releases
//! on mouse up. That is what lets a pan continue off the edge of the pane.
//!
//! **`DODO_FLOW_TRACE_INPUT=1` prints every event this view receives.** It is
//! here because Phase 0 could not trigger a single real input event — macOS
//! discards synthetic ones from an untrusted process — so everything known
//! about how mouse, scroll and pinch arrive was read from GPUI's source rather
//! than observed. The trace is how that gets checked on a real machine, and it
//! costs one atomic load per event when it is off.

use std::sync::OnceLock;

use gpui::{
    App, Bounds, Context, DispatchPhase, FocusHandle, Focusable, Hitbox, HitboxBehavior,
    InteractiveElement, IntoElement, KeyDownEvent, KeyUpEvent, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, ParentElement, PinchEvent, Pixels, Point, Render,
    ScrollWheelEvent, Styled, Window, canvas, div, px,
};
use gpui_component::ActiveTheme;

use crate::{
    budgets::RenderBudgets,
    geometry::{Attachment, EdgeRoute, Rect, RouteOptions, Vec2, Viewport, route},
    interaction::{
        ConnectionSource, InputModifiers, InteractionEffect, InteractionEvent, InteractionMachine,
        PointerButton,
    },
    models::{Color, EdgeRouting, FlowDocument, NodeIndex, RenderQuality},
    render::{
        GridLevel, GridLimits, GridSettings, PaintPlan, PaintStats, WindowPainter, edges, grid,
        plan::{DashSpec, PathPrimitive, QuadPrimitive},
        shapes,
    },
    runtime::{EdgeEnd, GraphWorld, HitTolerance, NodeShape, PointerTarget},
};

/// The key-binding context the canvas establishes on its root, so canvas
/// bindings fire only while it holds focus and never leak into another tool —
/// the same scoping every other dodo tool uses.
pub const KEY_CONTEXT: &str = "FlowCanvas";

/// The key that turns a left-drag into a pan, alongside the middle button.
///
/// §26 asks for configurable bindings rather than hard-coded platform keys, and
/// this is not that yet — it is one named constant in the one place that reads
/// it, which is the shape a binding table replaces without touching the state
/// machine.
const PAN_KEY: &str = "space";

/// How many screen pixels one line of wheel scroll moves the view.
///
/// A wheel reports `ScrollDelta::Lines` and a trackpad reports
/// `ScrollDelta::Pixels`; `ScrollDelta::pixel_delta` normalises them against a
/// line height, and this is the line height the canvas uses. It is not a text
/// line height — nothing here has text — so it is stated rather than borrowed
/// from a font.
const SCROLL_LINE_HEIGHT: f32 = 20.0;

/// The zoom a pinch of `delta` applies: `1 + delta`.
///
/// macOS's `NSEvent.magnification` is already a *relative* factor — 0.1 means
/// ten per cent bigger — so this is the identity, written out because the fact
/// is not obvious and the alternative reading (an absolute scale) silently
/// inverts the gesture.
fn pinch_factor(delta: f32) -> f32 {
    (1.0 + delta).clamp(0.1, 10.0)
}

/// Wheel-notch zoom, for a mouse with no pinch gesture. Cmd or Ctrl plus the
/// wheel, which is what every other canvas application binds.
fn wheel_zoom_factor(vertical_pixels: f32) -> f32 {
    Viewport::ZOOM_STEP.powf(vertical_pixels / SCROLL_LINE_HEIGHT)
}

fn tracing_input() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("DODO_FLOW_TRACE_INPUT").is_some())
}

/// The colours an element falls back to when its own style leaves them unset.
///
/// Resolved from the active theme once per frame and passed down, rather than
/// read per element: `cx.theme()` is a lookup, and this is the inner loop over
/// every shape in the document.
#[derive(Debug, Clone, Copy, PartialEq)]
struct CanvasInk {
    fill: Color,
    stroke: Color,
    /// Edges and their markers.
    edge: Color,
    /// Handle dots.
    handle: Color,
    /// The selection outline, the box-select rectangle and the connection
    /// preview — everything that says "you are doing something".
    accent: Color,
}

/// How big a handle dot is drawn, in **screen** pixels.
///
/// Screen rather than world, so a handle stays grabbable when zoomed out and
/// does not swallow its node when zoomed in. It is the same number
/// [`HitTolerance::HANDLE_SCREEN_RADIUS`] tests against, less the grabbing
/// margin — a target you can hit slightly outside is right, a target that is
/// smaller than it looks is not.
const HANDLE_SCREEN_RADIUS: f32 = 4.5;

/// The width the connection preview is drawn at, in screen pixels.
const PREVIEW_WIDTH: f32 = 1.5;

/// A graph node's body radius in world units, when its style does not set one.
///
/// A default rather than a hard-coded look: `ElementStyle::corner_radius` wins
/// whenever a document says anything, and this is only what an unstyled node
/// falls back to so it reads as a node rather than as a drawn rectangle.
const GRAPH_NODE_RADIUS: f32 = 6.0;

/// The outline width of a selected element, in screen pixels. Constant on
/// screen rather than in world units, so selection stays visible at any zoom.
const SELECTED_STROKE_PIXELS: f32 = 2.0;

/// Applies an element's opacity to one of its colours.
///
/// `ElementStyle::opacity` multiplies both stroke and fill alpha rather than
/// replacing it, so a half-transparent fill inside a half-transparent element
/// is a quarter — which is what every other editor does and what a user
/// dragging an opacity slider expects.
fn fade(color: Color, opacity: f32) -> Color {
    color.with_alpha(color.a * opacity.clamp(0.0, 1.0))
}

/// The Flow Canvas.
pub struct FlowView {
    world: GraphWorld,
    viewport: Viewport,
    budgets: RenderBudgets,
    focus_handle: FocusHandle,

    grid: GridSettings,
    grid_limits: GridLimits,
    interaction: InteractionMachine,

    /// Reused across frames. See the module doc: a pan must not allocate.
    plan: PaintPlan,
    last_paint: PaintStats,
    last_grid: GridLevel,
    /// Paths the black-window guard had to drop last frame. Non-zero means
    /// culling is not doing its job; Phase 4's harness asserts on it.
    dropped_paths: u32,

    /// Whether [`PAN_KEY`] is held. The one piece of keyboard state the
    /// interaction machine needs and cannot see for itself.
    pan_key_held: bool,

    /// Routes rebuilt on the last frame. Zero on an idle frame and on a pure
    /// pan; equal to the dragged node's degree while a node is being dragged.
    /// The §19 number, observable from outside for a benchmark or a test.
    rebuilt_routes: u32,

    /// The connection preview's route, kept so that dragging one out rebuilds
    /// into the same buffers instead of allocating per mouse move (§40 rule 14).
    preview_route: EdgeRoute,

    /// The one selected node.
    ///
    /// **Not a selection model.** §28's selection is a *set*, and resolving a
    /// box selection into one needs the spatial index's broad phase, which is
    /// Phase 4's. This is the single node a drag highlights, so that dragging
    /// has feedback; it is the field Phase 4 replaces, not a design.
    selection: Option<NodeIndex>,
}

impl FlowView {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> FlowView {
        // Resolved once, here, rather than read per frame: it is a compile-time
        // property of the build. Held on the view rather than reached for
        // globally so a benchmark or a test can mount a view against another
        // platform's budgets.
        let budgets = crate::budgets::current();

        FlowView {
            world: GraphWorld::new(),
            viewport: Viewport::default(),
            grid: GridSettings::default(),
            grid_limits: GridLimits::from_budgets(&budgets),
            budgets,
            focus_handle: cx.focus_handle(),
            interaction: InteractionMachine::new(),
            plan: PaintPlan::new(),
            last_paint: PaintStats::default(),
            last_grid: GridLevel::empty(),
            dropped_paths: 0,
            pan_key_held: false,
            rebuilt_routes: 0,
            preview_route: EdgeRoute::default(),
            selection: None,
        }
    }

    /// The runtime graph — the stores, the adjacency index and the dirty state.
    pub fn world(&self) -> &GraphWorld {
        &self.world
    }

    pub fn world_mut(&mut self) -> &mut GraphWorld {
        &mut self.world
    }

    /// The world written back out as a document. Allocates; for a save or a
    /// test, never for a frame.
    pub fn to_document(&self) -> FlowDocument {
        self.world.to_document()
    }

    /// Replaces the document, **rebuilding the runtime from it**.
    ///
    /// The viewport is left alone: opening a document does not move the camera,
    /// because session restore decides where the camera was and it is not this
    /// method's business.
    ///
    /// Anything the document held that the runtime could not represent comes
    /// back in the [`LoadReport`](crate::runtime::LoadReport) — a dangling edge
    /// is a fact about the file, and swallowing it here would be the loader
    /// deciding on the caller's behalf.
    pub fn set_document(&mut self, document: FlowDocument) -> crate::runtime::LoadReport {
        let (world, report) = GraphWorld::from_document(&document);
        self.world = world;
        self.selection = None;
        report
    }

    pub fn viewport(&self) -> &Viewport {
        &self.viewport
    }

    pub fn viewport_mut(&mut self) -> &mut Viewport {
        &mut self.viewport
    }

    /// The render ceilings this build is working under. See [`crate::budgets`];
    /// every painter asks this rather than naming a number.
    pub fn budgets(&self) -> &RenderBudgets {
        &self.budgets
    }

    pub fn grid_settings(&self) -> &GridSettings {
        &self.grid
    }

    pub fn set_grid_settings(&mut self, settings: GridSettings) {
        self.grid = settings;
    }

    pub fn interaction(&self) -> &InteractionMachine {
        &self.interaction
    }

    /// **What the last frame actually painted.** The accounting hook Phase 4's
    /// benchmark harness asserts against; see [`crate::render::plan`] for why
    /// the vertex count is a correctness signal rather than a statistic.
    pub fn last_paint_stats(&self) -> PaintStats {
        self.last_paint
    }

    /// What the grid chose last frame — its level, spacing and cost.
    pub fn last_grid_level(&self) -> GridLevel {
        self.last_grid
    }

    /// Paths dropped by the black-window guard last frame. Always zero in a
    /// healthy scene.
    pub fn dropped_paths(&self) -> u32 {
        self.dropped_paths
    }

    /// **Edge routes rebuilt on the last frame** — §19's number, from outside.
    ///
    /// Zero on an idle frame and on a pure pan; while a node is dragged it is
    /// that node's degree, and nothing else. `runtime::world`'s property test
    /// asserts the rule; this is how the launcher shows it happening.
    pub fn rebuilt_routes(&self) -> u32 {
        self.rebuilt_routes
    }

    /// The selected node, if any. See the field's own doc — a selection *set*
    /// is Phase 4's, once the box select can resolve into one.
    pub fn selection(&self) -> Option<NodeIndex> {
        self.selection
    }

    /// Frames the whole document, or resets to 1:1 if it is empty.
    pub fn zoom_to_fit(&mut self) {
        match self.world.content_bounds() {
            Some(content) => self.viewport.zoom_to_fit(content, 48.0),
            None => self.viewport.reset(),
        }
    }

    // ---- painting -------------------------------------------------------

    /// Pulls the grid's colours out of the active theme.
    ///
    /// Done per frame rather than at construction because dodo applies a theme
    /// change live — `dodo-theming-settings` is the rule — and a grid cached at
    /// startup would stay dark on a light theme until the app restarted.
    fn sync_theme(&mut self, cx: &App) -> CanvasInk {
        let theme = cx.theme();
        let border = from_hsla(theme.border);

        self.grid.minor.color = border.with_alpha(border.a * 0.55);
        self.grid.major.color = border;

        CanvasInk {
            fill: from_hsla(theme.secondary),
            stroke: from_hsla(theme.foreground).with_alpha(0.7),
            edge: from_hsla(theme.foreground).with_alpha(0.55),
            handle: from_hsla(theme.primary),
            accent: from_hsla(theme.selection),
        }
    }

    /// Every node, as the cheapest primitive that can draw it, and its handles.
    ///
    /// **No culling and no spatial query**, and that is a deliberate hole
    /// rather than an oversight: §40 rule 1 forbids scanning every element to
    /// find the visible ones, and the uniform grid that answers it properly is
    /// Phase 4's. Writing a linear scan here would satisfy this phase and then
    /// be the thing everyone forgot to delete. The vertex ceiling in
    /// [`PaintPlan::enforce_vertex_ceiling`] is what stands in for culling
    /// until then — it keeps the window from going black, it does not keep the
    /// frame fast.
    ///
    /// The loop reads the runtime's hot arrays: a position, a size, a one-byte
    /// [`NodeShape`] and a style. It never touches
    /// [`ElementKind`](crate::models::ElementKind), which carries a `String` —
    /// that is what §17's cold/hot split is for and what §40 rule 9 asks.
    fn plan_nodes(&mut self, ink: CanvasInk) {
        let quality = self.world.settings().render_quality;

        for node in self.world.nodes().indices() {
            let nodes = self.world.nodes();
            if nodes.is_hidden(node) {
                continue;
            }

            let shape = nodes.shape(node);
            if shape == NodeShape::Other {
                // Text, images, frames and custom kinds are later phases'. A
                // fallback rectangle here would silently draw them and hide the
                // fact that they are not implemented.
                continue;
            }

            let style = nodes.style(node);
            let screen = self.viewport.world_rect_to_screen(nodes.bounds(node));
            // A graph node's body has a radius of its own so it reads as a
            // node rather than as a drawn rectangle; a shape uses what its
            // style says.
            let world_radius = if shape == NodeShape::GraphNode && style.corner_radius <= 0.0 {
                GRAPH_NODE_RADIUS
            } else {
                style.corner_radius
            };
            let radius = self.viewport.world_to_screen_length(world_radius);

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
            let stroke_width = self
                .viewport
                .world_to_screen_length(style.stroke.width)
                .max(if selected {
                    SELECTED_STROKE_PIXELS
                } else {
                    0.0
                });
            let has_stroke = (!style.stroke.is_invisible() || selected) && stroke_width > 0.0;

            if shapes::node_prefers_quad(shape) {
                // Phase 0's measurement, honoured: 20,000 quads hold 60 fps
                // where the same count of rectangular paths drop to 30 — and a
                // quad carries its corner radius and its border for free, so
                // the border costs no second primitive at all.
                let mut quad = QuadPrimitive::filled(screen, fill).with_corner_radius(radius);
                if has_stroke {
                    quad = quad.with_border(stroke_width, stroke_color);
                }
                self.plan.push_quad(quad);
            } else if let Some(outline) = shapes::outline_for_node(shape, screen, radius) {
                self.plan
                    .push_path(PathPrimitive::fill(outline.clone(), fill, quality));

                if has_stroke {
                    self.plan.push_path(PathPrimitive::stroke(
                        outline,
                        stroke_color,
                        stroke_width,
                        quality,
                    ));
                }
            }

            self.plan_handles(node, ink);
        }
    }

    /// A node's handles, as quads.
    ///
    /// **Geometry and data in this phase, not interaction.** A handle is drawn
    /// so a connection can be aimed at it and so the routing is visible; the
    /// interactive element with its own hover state and cursor is Phase 5's,
    /// where §15's LOD decides whether a node is detailed enough to have one at
    /// all. A circle is a quad with a corner radius of half its side, so a
    /// hundred thousand of them would still be the cheap primitive.
    fn plan_handles(&mut self, node: NodeIndex, ink: CanvasInk) {
        let radius = HANDLE_SCREEN_RADIUS;

        for handle in self.world.nodes().handles(node) {
            if self.world.handles().is_hidden(handle) {
                // §4: hidden handles stay connectable. Only the paint is
                // skipped — routing and hit-testing never read this flag.
                continue;
            }

            let center = self
                .viewport
                .world_to_screen(self.world.handle_position(handle));
            self.plan.push_quad(
                QuadPrimitive::filled(
                    Rect::new(center - Vec2::splat(radius), Vec2::splat(radius * 2.0)),
                    ink.handle,
                )
                .with_corner_radius(radius)
                .with_border(1.0, ink.fill),
            );
        }
    }

    /// Every edge, from its **derived** route.
    ///
    /// The routes are brought up to date once, at the top of the frame, by
    /// [`GraphWorld::rebuild_dirty_geometry`] — so this loop rebuilds nothing
    /// and a pure pan reroutes nothing (§40 rule 6). An edge whose route is
    /// stale is skipped rather than drawn from a stale one: it will be current
    /// on the frame the rebuild ran, and painting the old one would show an
    /// edge hanging off a node that has already moved.
    fn plan_edges(&mut self, ink: CanvasInk) {
        let quality = self.world.settings().render_quality;

        for edge in self.world.edges().indices() {
            if self.world.edges().is_hidden(edge) {
                continue;
            }
            let Some(route) = self.world.route(edge) else {
                continue;
            };

            let style = self.world.edges().style(edge);
            let selected = self.world.edges().is_selected(edge);
            let color = if selected {
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

            edges::plan_edge(&mut self.plan, route, &paint, &self.viewport);
        }
    }

    /// The connection being dragged out of a handle, if there is one (§8).
    ///
    /// Routed exactly like a committed edge — same router, same options — so
    /// the preview bends the way the edge will and there is no second opinion
    /// about where it would go. The loose end faces back the way the source
    /// leaves, which is what makes the curve settle instead of kinking as the
    /// pointer crosses the node.
    fn plan_connection_preview(&mut self, ink: CanvasInk) {
        let Some(pending) = self.interaction.pending_connection() else {
            return;
        };

        let source = self.world.attachment(
            EdgeEnd::handle(pending.source.node, pending.source.handle),
            pending.current_world,
        );
        let target = Attachment::new(pending.current_world, source.side.opposite());

        route::route_into(
            &mut self.preview_route,
            EdgeRouting::Bezier,
            source,
            target,
            &RouteOptions::DEFAULT,
        );

        edges::plan_connection_preview(
            &mut self.plan,
            &self.preview_route,
            ink.accent,
            self.viewport.screen_to_world_length(PREVIEW_WIDTH),
            RenderQuality::BALANCED,
            &self.viewport,
        );
    }

    /// The box-selection rectangle: **a quad**, so it adds no path batch on top
    /// of the frame it is drawn over.
    fn plan_selection_rect(&mut self, ink: CanvasInk) {
        let Some(world) = self.interaction.selection_rect() else {
            return;
        };

        let accent = ink.accent;
        self.plan.push_quad(
            QuadPrimitive::filled(
                self.viewport.world_rect_to_screen(world),
                accent.with_alpha(0.16),
            )
            .with_border(1.0, accent.with_alpha(0.9)),
        );
    }

    fn paint(
        &mut self,
        bounds: Bounds<Pixels>,
        hitbox: &Hitbox,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.viewport.set_size(Vec2::new(
            bounds.size.width.as_f32(),
            bounds.size.height.as_f32(),
        ));
        let ink = self.sync_theme(cx);

        // **Before anything is planned**, and only for what actually changed:
        // an idle frame finds an empty queue and does no work at all, which is
        // what makes §40 rule 6 hold by construction rather than by care.
        self.rebuilt_routes = self.world.rebuild_dirty_geometry();

        self.plan.clear();
        self.last_grid = grid::generate(
            &self.grid,
            &self.viewport,
            &self.grid_limits,
            &mut self.plan,
        );
        // Edges under nodes, nodes under the overlays. The *paint* order is
        // `PaintPlan`'s and is by primitive kind whatever this order is; this
        // one decides what sits on top within a kind.
        self.plan_edges(ink);
        self.plan_nodes(ink);
        self.plan_connection_preview(ink);
        self.plan_selection_rect(ink);

        self.dropped_paths = self.plan.enforce_vertex_ceiling(&self.budgets);

        let mut painter = WindowPainter::new(window, bounds);
        self.last_paint = self.plan.paint_into(&mut painter);

        self.install_input(bounds, hitbox, window, cx);
    }

    /// Replaces the selection with at most one node.
    ///
    /// A set is §28's and needs the spatial index's broad phase to build from a
    /// rectangle; this is the one-node case a drag needs, and it is written as
    /// a method so Phase 4 replaces one body rather than four call sites.
    fn select_only(&mut self, node: Option<NodeIndex>) {
        if self.selection == node {
            return;
        }

        if let Some(previous) = self.selection {
            self.world.set_node_selected(previous, false);
        }
        if let Some(node) = node {
            self.world.set_node_selected(node, true);
        }
        self.selection = node;
    }

    /// Turns a dropped connection into an edge, or into nothing.
    ///
    /// **The validation is the world's** (§4) — this only says where the drop
    /// landed. A refusal is silent on the canvas and visible under
    /// `DODO_FLOW_TRACE_INPUT`: the connection tool that colours a handle by
    /// [`ConnectionError`](crate::runtime::ConnectionError) is Phase 5's, and
    /// the reason is already carried for it.
    fn commit_connection(&mut self, source: ConnectionSource, target: PointerTarget) {
        let end = match target {
            PointerTarget::Handle { node, handle } => EdgeEnd::handle(node, handle),
            // §4's whole-node connection mode: dropping on a body connects to
            // the node, and the router picks a point on its border.
            PointerTarget::Node(node) => EdgeEnd::node(node),
            PointerTarget::Empty => {
                trace(format_args!("Connect dropped on empty canvas"));
                return;
            }
        };

        match self
            .world
            .connect(EdgeEnd::handle(source.node, source.handle), end)
        {
            Ok(edge) => trace(format_args!("Connect ok, edge={edge}")),
            Err(error) => trace(format_args!("Connect refused: {error}")),
        }
    }

    // ---- input ----------------------------------------------------------

    /// Applies one effect from the interaction machine, and says whether the
    /// canvas has to be repainted.
    fn apply(&mut self, effect: InteractionEffect, hitbox: &Hitbox, window: &mut Window) -> bool {
        if effect.starts_a_drag() {
            // Keeps the drag alive once the pointer leaves the pane. Auto-
            // releases on mouse up, so there is nothing to undo.
            window.capture_pointer(hitbox.id);
        }

        match effect {
            InteractionEffect::PanBy(delta) => self.viewport.pan_by(delta),

            // **The propagation rule, entered from a gesture.** Everything the
            // move invalidates is decided by `GraphWorld::move_node`; the view
            // does not know which edges exist and must not.
            InteractionEffect::DragNodeBy { node, delta } => self.world.move_node(node, delta),
            InteractionEffect::CancelNodeDrag { node, revert } => {
                self.world.move_node(node, revert)
            }
            InteractionEffect::BeginNodeDrag(node) => self.select_only(Some(node)),
            InteractionEffect::BeginBoxSelect(_) => self.select_only(None),
            InteractionEffect::BeginConnect(source) => self.select_only(Some(source.node)),
            InteractionEffect::CommitConnect { source, target } => {
                self.commit_connection(source, target)
            }
            _ => {}
        }

        effect.needs_repaint()
    }

    /// The pane-local position of a window-space event.
    fn local(&self, position: Point<Pixels>, bounds: Bounds<Pixels>) -> Vec2 {
        Vec2::new(
            position.x.as_f32() - bounds.origin.x.as_f32(),
            position.y.as_f32() - bounds.origin.y.as_f32(),
        )
    }

    fn pointer_event(
        &self,
        position: Point<Pixels>,
        bounds: Bounds<Pixels>,
        button: PointerButton,
        modifiers: gpui::Modifiers,
    ) -> InteractionEvent {
        let screen = self.local(position, bounds);
        let world = self.viewport.screen_to_world(screen);
        InteractionEvent::PointerDown {
            screen,
            world,
            button,
            modifiers: InputModifiers {
                shift: modifiers.shift,
                control: modifiers.control,
                alt: modifiers.alt,
                command: modifiers.platform,
            },
            pan_key_held: self.pan_key_held,
            target: self.target_at(world),
        }
    }

    /// **What is under the pointer** — §29's two phases, with the broad one
    /// still standing open.
    ///
    /// `nodes().indices()` is the candidate set, and it is the whole document.
    /// That is deliberate and it is **not** the linear scan §40 rule 1 forbids:
    /// rule 1 is about scanning every element *per frame* to find the visible
    /// ones, and this runs once per pointer press, on an event a human
    /// generated. Phase 4's uniform grid replaces this one argument with a
    /// query — there is nothing else here to delete, which is why the candidate
    /// set is a parameter of `GraphWorld::hit_test` rather than something it
    /// fetches for itself.
    ///
    /// A **locked** node reads as empty canvas, so a press on one starts a box
    /// selection instead of a drag that would be refused — §26's behaviour, and
    /// cheaper to answer here than to explain after the fact.
    fn target_at(&self, world: Vec2) -> PointerTarget {
        let tolerance = HitTolerance::new(
            self.viewport
                .screen_to_world_length(HitTolerance::HANDLE_SCREEN_RADIUS),
        );

        match self
            .world
            .hit_test(world, self.world.nodes().indices(), tolerance)
        {
            PointerTarget::Node(node) if self.world.nodes().is_locked(node) => PointerTarget::Empty,
            target => target,
        }
    }

    /// Registers this frame's mouse listeners.
    ///
    /// Must be called from paint — `Window::on_mouse_event` asserts on it — and
    /// the listeners last exactly one frame, so this runs every time.
    fn install_input(
        &mut self,
        bounds: Bounds<Pixels>,
        hitbox: &Hitbox,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let view = cx.entity();

        {
            let (hitbox, view) = (hitbox.clone(), view.clone());
            window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble || !hitbox.is_hovered(window) {
                    return;
                }
                let Some(button) = to_pointer_button(event.button) else {
                    return;
                };
                trace(format_args!(
                    "MouseDown button={button:?} window_pos=({}, {}) mods={:?} clicks={}",
                    event.position.x.as_f32(),
                    event.position.y.as_f32(),
                    event.modifiers,
                    event.click_count
                ));

                view.update(cx, |this, cx| {
                    let interaction =
                        this.pointer_event(event.position, bounds, button, event.modifiers);
                    let effect = this.interaction.handle(interaction);
                    if this.apply(effect, &hitbox, window) {
                        cx.notify();
                    }
                });
            });
        }

        {
            let (hitbox, view) = (hitbox.clone(), view.clone());
            window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble || !hitbox.is_hovered(window) {
                    return;
                }

                trace(format_args!(
                    "MouseMove window_pos=({}, {}) pressed={:?} mods={:?}",
                    event.position.x.as_f32(),
                    event.position.y.as_f32(),
                    event.pressed_button,
                    event.modifiers
                ));

                view.update(cx, |this, cx| {
                    // While Idle this is `InteractionEffect::None` and notifies
                    // nothing, which is what keeps a hovering pointer from
                    // driving a 60 fps repaint loop over an idle canvas.
                    if this.interaction.is_idle() {
                        return;
                    }
                    let screen = this.local(event.position, bounds);
                    let effect = this.interaction.handle(InteractionEvent::PointerMove {
                        screen,
                        world: this.viewport.screen_to_world(screen),
                    });
                    if this.apply(effect, &hitbox, window) {
                        cx.notify();
                    }
                });
            });
        }

        {
            let (hitbox, view) = (hitbox.clone(), view.clone());
            window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble || !hitbox.is_hovered(window) {
                    return;
                }
                let Some(button) = to_pointer_button(event.button) else {
                    return;
                };
                trace(format_args!("MouseUp button={button:?}"));

                view.update(cx, |this, cx| {
                    let world = this
                        .viewport
                        .screen_to_world(this.local(event.position, bounds));
                    let effect = this.interaction.handle(InteractionEvent::PointerUp {
                        button,
                        world,
                        target: this.target_at(world),
                    });
                    // A committed box selection is where Phase 4 will resolve
                    // world rectangle into element ids, through the spatial
                    // index's broad phase (§28).
                    if this.apply(effect, &hitbox, window) {
                        cx.notify();
                    }
                });
            });
        }

        {
            let (hitbox, view) = (hitbox.clone(), view.clone());
            window.on_mouse_event(move |event: &ScrollWheelEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble || !hitbox.should_handle_scroll(window) {
                    return;
                }
                let pixels = event.delta.pixel_delta(px(SCROLL_LINE_HEIGHT));
                trace(format_args!(
                    "Scroll window_pos=({}, {}) delta={:?} px=({}, {}) mods={:?} phase={:?}",
                    event.position.x.as_f32(),
                    event.position.y.as_f32(),
                    event.delta,
                    pixels.x.as_f32(),
                    pixels.y.as_f32(),
                    event.modifiers,
                    event.touch_phase
                ));

                view.update(cx, |this, cx| {
                    let anchor = this.local(event.position, bounds);
                    // Cmd or Ctrl plus the wheel zooms; a bare wheel or a
                    // two-finger trackpad swipe pans. Both are what every other
                    // canvas application does, and the mouse-only path matters
                    // because a wheel has no pinch gesture.
                    if event.modifiers.platform || event.modifiers.control {
                        this.viewport
                            .zoom_by(anchor, wheel_zoom_factor(pixels.y.as_f32()));
                    } else {
                        this.viewport
                            .pan_by(Vec2::new(pixels.x.as_f32(), pixels.y.as_f32()));
                    }
                    cx.notify();
                });
                cx.stop_propagation();
            });
        }

        {
            let (hitbox, view) = (hitbox.clone(), view.clone());
            window.on_mouse_event(move |event: &PinchEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble || !hitbox.is_hovered(window) {
                    return;
                }
                trace(format_args!(
                    "Pinch window_pos=({}, {}) delta={} mods={:?} phase={:?}",
                    event.position.x.as_f32(),
                    event.position.y.as_f32(),
                    event.delta,
                    event.modifiers,
                    event.phase
                ));

                view.update(cx, |this, cx| {
                    // **Cursor-anchored zoom.** The world point under the pinch
                    // centre stays under it, and the arithmetic is
                    // `Viewport::zoom_by`'s alone — §22 forbids a transform
                    // formula anywhere else, and this is the call site that
                    // would otherwise have grown one.
                    let anchor = this.local(event.position, bounds);
                    this.viewport.zoom_by(anchor, pinch_factor(event.delta));
                    cx.notify();
                });
            });
        }
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        match event.keystroke.key.as_str() {
            PAN_KEY => self.pan_key_held = true,
            "escape" => {
                let effect = self.interaction.handle(InteractionEvent::Cancel);
                if effect.needs_repaint() {
                    cx.notify();
                }
            }
            _ => {}
        }
    }

    fn on_key_up(&mut self, event: &KeyUpEvent, _window: &mut Window, _cx: &mut Context<Self>) {
        if event.keystroke.key.as_str() == PAN_KEY {
            self.pan_key_held = false;
        }
    }
}

/// GPUI's button vocabulary as the machine's.
///
/// `None` for the navigation buttons: the canvas has no back/forward, and
/// mapping them onto `Left` would make a thumb button start a box selection.
fn to_pointer_button(button: MouseButton) -> Option<PointerButton> {
    match button {
        MouseButton::Left => Some(PointerButton::Left),
        MouseButton::Middle => Some(PointerButton::Middle),
        MouseButton::Right => Some(PointerButton::Right),
        MouseButton::Navigate(_) => None,
    }
}

/// GPUI's colour as the crate's pure one. The inverse of
/// [`crate::render::painter::to_hsla`], and it lives here for the same reason:
/// `models/` may not name a UI framework, so a theme colour can only be
/// converted on this side of the boundary.
fn from_hsla(color: gpui::Hsla) -> Color {
    let rgba: gpui::Rgba = color.into();
    Color::rgba(rgba.r, rgba.g, rgba.b, rgba.a)
}

fn trace(args: std::fmt::Arguments<'_>) {
    if tracing_input() {
        eprintln!("[flow-input] {args}");
    }
}

impl Focusable for FlowView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for FlowView {
    /// **Deliberately empty of work.** See the module doc: this body runs
    /// whenever anything above the canvas redraws, so everything that costs
    /// something happens in the paint closure instead.
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = cx.entity();

        div()
            .id("flow-canvas")
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus_handle)
            .size_full()
            .relative()
            .overflow_hidden()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .on_key_down(cx.listener(Self::on_key_down))
            .on_key_up(cx.listener(Self::on_key_up))
            .child(
                canvas(
                    // Prepaint: the hitbox every mouse listener gates on. It
                    // has to exist before paint, because paint is where the
                    // listeners are registered and they capture it.
                    |bounds, window, _cx| window.insert_hitbox(bounds, HitboxBehavior::Normal),
                    move |bounds, hitbox, window, cx| {
                        view.update(cx, |this, cx| this.paint(bounds, &hitbox, window, cx));
                    },
                )
                .absolute()
                .size_full(),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::painter::to_hsla;

    /// macOS reports `NSEvent.magnification` as a relative factor, so a zero
    /// delta must be the identity — a pinch that reported nothing must not
    /// move the zoom.
    #[test]
    fn a_zero_pinch_delta_changes_nothing() {
        assert_eq!(pinch_factor(0.0), 1.0);
    }

    #[test]
    fn a_pinch_apart_zooms_in_and_together_zooms_out() {
        assert!(pinch_factor(0.1) > 1.0);
        assert!(pinch_factor(-0.1) < 1.0);
    }

    /// A malformed or hostile delta must not be able to send the zoom to zero
    /// or to infinity in one event; the viewport clamps too, but a NaN would
    /// get past that.
    #[test]
    fn an_absurd_pinch_delta_is_clamped() {
        assert_eq!(pinch_factor(-5.0), 0.1);
        assert_eq!(pinch_factor(100.0), 10.0);
    }

    /// One line of wheel travel is one zoom notch, and the two directions are
    /// exact reciprocals — so zooming in and back out returns to where it
    /// started rather than drifting.
    #[test]
    fn wheel_zoom_is_geometric_and_reversible() {
        let up = wheel_zoom_factor(SCROLL_LINE_HEIGHT);
        let down = wheel_zoom_factor(-SCROLL_LINE_HEIGHT);

        assert!((up - Viewport::ZOOM_STEP).abs() < 1e-5);
        assert!((up * down - 1.0).abs() < 1e-5);
    }

    #[test]
    fn a_wheel_at_rest_does_not_zoom() {
        assert!((wheel_zoom_factor(0.0) - 1.0).abs() < 1e-6);
    }

    /// Cursor-anchored zoom, end to end through the same call the pinch handler
    /// makes: the world point under the pointer must not move. The formula
    /// lives in `Viewport` (§22) and this is the proof that the view uses it
    /// rather than its own.
    #[test]
    fn zooming_by_a_wheel_notch_keeps_the_world_under_the_cursor() {
        let mut viewport = Viewport::new(Vec2::new(37.0, -12.0), 1.0, Vec2::new(1440.0, 900.0));
        let anchor = Vec2::new(300.0, 640.0);
        let before = viewport.screen_to_world(anchor);

        viewport.zoom_by(anchor, wheel_zoom_factor(SCROLL_LINE_HEIGHT));
        let after = viewport.screen_to_world(anchor);

        assert!((after - before).length() < 1e-3, "{before:?} -> {after:?}");
    }

    #[test]
    fn only_the_three_real_buttons_reach_the_state_machine() {
        assert_eq!(
            to_pointer_button(MouseButton::Left),
            Some(PointerButton::Left)
        );
        assert_eq!(
            to_pointer_button(MouseButton::Middle),
            Some(PointerButton::Middle)
        );
        assert_eq!(
            to_pointer_button(MouseButton::Right),
            Some(PointerButton::Right)
        );
        assert_eq!(
            to_pointer_button(MouseButton::Navigate(gpui::NavigationDirection::Back)),
            None
        );
    }

    #[test]
    fn a_theme_colour_survives_the_round_trip_across_the_boundary() {
        let original = Color::rgba(0.3, 0.6, 0.9, 0.5);
        let round_tripped = from_hsla(to_hsla(original));

        assert!((round_tripped.r - original.r).abs() < 1e-3);
        assert!((round_tripped.g - original.g).abs() < 1e-3);
        assert!((round_tripped.b - original.b).abs() < 1e-3);
        assert!((round_tripped.a - original.a).abs() < 1e-3);
    }
}
