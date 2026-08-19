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
    MouseMoveEvent, MouseUpEvent, ParentElement, Path, PinchEvent, Pixels, Point, Render,
    ScrollWheelEvent, ShapedLine, Styled, Window, canvas, div, px,
};
use gpui_component::ActiveTheme;

use crate::{
    budgets::RenderBudgets,
    commands::{FlowEditor, gesture},
    geometry::{Attachment, EdgeRoute, Rect, RouteOptions, Vec2, Viewport, route},
    instrument::{Instruments, Probe},
    interaction::{
        BoxSelection, InputModifiers, InteractionEffect, InteractionEvent, InteractionMachine,
        PointerButton,
    },
    models::{
        Color, EdgeIndex, EdgeRouting, FlowDocument, NodeIndex, RenderQuality, RenderStyle,
        SketchStyle,
    },
    render::{
        GridLevel, GridLimits, GridSettings, PaintPlan, PaintStats, SceneInk, SceneOptions,
        SceneStats, WindowPainter,
        cache::{CacheStats, GeometryCache, ShapedLineCache},
        edges,
        lod::LodPlan,
        plan::QuadPrimitive,
        registry::NodeRendererRegistry,
        scene,
        snapshot::{RenderSnapshot, SnapshotCounts},
    },
    runtime::{BoxQuery, EdgeEnd, GraphWorld, HitTolerance, PointerTarget, SelectionSet},
    spatial::{SpatialIndex, SyncReport, VisibleSet},
    views::{
        keymap::{Redo, Undo},
        nodes,
    },
};

/// The key-binding context the canvas establishes on its root, so canvas
/// bindings fire only while it holds focus and never leak into another tool —
/// the same scoping every other dodo tool uses.
///
/// **The scoping has a precondition that is easy to miss and silent when it is
/// missed**: GPUI dispatches a key event down the *focus* path, and
/// `Window::dispatch_key_event` falls back to the dispatch tree's **root node**
/// when nothing is focused. A canvas that never takes focus therefore has its
/// context, its key handlers and its actions outside the path entirely — every
/// binding is dead and nothing says so. [`FlowView::new`] focuses on mount and
/// the mouse-down handler refocuses, which is what makes this constant mean
/// anything.
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

/// Whether to print one line after the first painted frame.
///
/// Here rather than in the launcher for one reason: everything worth reporting
/// — the LOD rung, the element count, the cache — only exists **after** a frame
/// has been extracted *and* painted, and a wrapper view's `render` runs before
/// its child's. A launcher can only see those numbers on the second frame, and
/// a window nobody is looking at may never produce one.
fn reporting() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("DODO_FLOW_REPORT").is_some())
}

/// The width the connection preview is drawn at, in screen pixels.
const PREVIEW_WIDTH: f32 = 1.5;

/// The Flow Canvas.
pub struct FlowView {
    /// **The world, and the only door to it** (§30). Held as a
    /// [`FlowEditor`] rather than as a [`GraphWorld`] on purpose: this view is
    /// where every gesture lands, so it is where a mutation that bypasses the
    /// undo history would be written, and an editor lends no `&mut` to the
    /// world it owns. `commands::editor`'s module doc has the whole argument.
    editor: FlowEditor,
    viewport: Viewport,
    budgets: RenderBudgets,
    focus_handle: FocusHandle,

    grid: GridSettings,
    grid_limits: GridLimits,
    interaction: InteractionMachine,

    /// §21's index over the world, and the visible set it answers into. Both
    /// are fields rather than locals so a pan reuses their buffers (§40 rule
    /// 14) — the visible set is refilled sixty times a second during a drag.
    spatial: SpatialIndex,
    visible: VisibleSet,
    last_sync: SyncReport,
    last_scene: SceneStats,

    /// Broad-phase scratch for a committed rubber band. Kept for the same
    /// reason, and cleared rather than reallocated.
    node_candidates: Vec<NodeIndex>,
    edge_candidates: Vec<EdgeIndex>,

    /// §39's probes. Off unless `DODO_FLOW_INSTRUMENT` is set; see
    /// [`crate::instrument`] for what that costs.
    instruments: Instruments,

    /// §24's extraction, and the two caches it feeds. All three are fields
    /// rather than locals for the same reason the plan is: a pan refills them
    /// and must not reallocate them.
    snapshot: RenderSnapshot,
    geometry_cache: GeometryCache<Path<Pixels>>,
    text_cache: ShapedLineCache<ShapedLine>,
    /// §43's registry. Held here so a launcher or an embedding app can register
    /// its own node kinds against a mounted canvas.
    registry: NodeRendererRegistry,

    /// The node the pointer is over, or `None`. §44's other half: a hovered
    /// node gets controls, and nothing else does.
    hovered: Option<NodeIndex>,

    /// The pane's size as of the last paint.
    ///
    /// **`render` runs before layout**, so this is the only size it can extract
    /// a snapshot against. It is exact on every frame but the first and on a
    /// resize, and `paint` requests one animation frame when it changes — see
    /// `FlowView::paint`.
    pane: Vec2,

    /// Whether the camera zoomed since the last frame. The geometry cache needs
    /// it to tell a live pinch (scale the cached tessellation, stay responsive)
    /// from a settled camera (re-tessellate, be correct) — see
    /// [`crate::render::cache`].
    zooming: bool,
    /// Whether [`reporting`]'s one-shot line has been printed.
    reported: bool,

    /// The text colour the shaped-line cache was filled at. A `ShapedLine`
    /// bakes its colour at shape time, and dodo applies a theme change live, so
    /// a changed ink is a cache that has to go.
    text_ink: Option<Color>,

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
}

impl FlowView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> FlowView {
        // Resolved once, here, rather than read per frame: it is a compile-time
        // property of the build. Held on the view rather than reached for
        // globally so a benchmark or a test can mount a view against another
        // platform's budgets.
        let budgets = crate::budgets::current();

        // **Focused on mount, and refocused on every press** — see
        // `KEY_CONTEXT`. Without a focus the canvas's key context is not on
        // GPUI's dispatch path at all, and every binding scoped to it is dead.
        let focus_handle = cx.focus_handle();
        window.focus(&focus_handle, cx);

        FlowView {
            editor: FlowEditor::new(),
            viewport: Viewport::default(),
            grid: GridSettings::default(),
            grid_limits: GridLimits::from_budgets(&budgets),
            budgets,
            focus_handle,
            interaction: InteractionMachine::new(),
            spatial: SpatialIndex::new(crate::spatial::DEFAULT_CELL_SIZE),
            visible: VisibleSet::new(),
            last_sync: SyncReport::default(),
            last_scene: SceneStats::default(),
            node_candidates: Vec::new(),
            edge_candidates: Vec::new(),
            instruments: Instruments::from_env(),
            snapshot: RenderSnapshot::new(),
            geometry_cache: GeometryCache::new(&budgets),
            text_cache: ShapedLineCache::new(&budgets),
            registry: NodeRendererRegistry::with_generic_kinds(),
            hovered: None,
            pane: Vec2::ZERO,
            zooming: false,
            reported: false,
            text_ink: None,
            plan: PaintPlan::new(),
            last_paint: PaintStats::default(),
            last_grid: GridLevel::empty(),
            dropped_paths: 0,
            pan_key_held: false,
            rebuilt_routes: 0,
            preview_route: EdgeRoute::default(),
        }
    }

    /// The runtime graph — the stores, the adjacency index and the dirty state.
    pub fn world(&self) -> &GraphWorld {
        self.editor.world()
    }

    /// The editor: the world plus §30's history, and every door that may change
    /// the document.
    ///
    /// **There is deliberately no `world_mut`.** It existed until Phase 7 and it
    /// was the bypass — a caller holding `&mut GraphWorld` moves a node without
    /// the history hearing, and the corruption surfaces three undos later with
    /// nothing to trace it to. Everything that went through it goes through
    /// [`FlowEditor::apply`] or one of the editor's named non-recording doors.
    pub fn editor(&self) -> &FlowEditor {
        &self.editor
    }

    pub fn editor_mut(&mut self) -> &mut FlowEditor {
        &mut self.editor
    }

    /// The world written back out as a document. Allocates; for a save or a
    /// test, never for a frame.
    pub fn to_document(&self) -> FlowDocument {
        self.editor.world().to_document()
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
    /// **The undo history goes with it.** A stored delta names runtime indices,
    /// and every index means something else in a different document.
    pub fn set_document(&mut self, document: FlowDocument) -> crate::runtime::LoadReport {
        let report = self.editor.load_document(document);
        // The index is built from the routes, so they have to exist first —
        // and the whole-document rebuild is the one place that is allowed to
        // be proportional to the file rather than to the screen.
        self.editor.rebuild_all_geometry();
        self.rebuild_spatial_index();
        report
    }

    /// Rebuilds the spatial index from the whole world, and spends the dirty
    /// queues the build filled.
    ///
    /// Document-proportional, so it belongs to loading rather than to a frame;
    /// [`SpatialIndex::sync`] is the per-frame call. Public because a caller
    /// that has replaced the world wholesale — a launcher building a scene, a
    /// benchmark — has to say so.
    pub fn rebuild_spatial_index(&mut self) {
        self.spatial = SpatialIndex::for_world(self.editor.world());
        self.editor.clear_spatial_updates();
        // Every cached tessellation and every shaped line is filed under a
        // runtime index, and a rebuilt world means those indices point at
        // something else. Keeping them would paint one document's geometry for
        // another's.
        self.geometry_cache.clear();
        self.text_cache.clear();
        self.snapshot.reset();
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

    /// **What is selected** (§28), as compact runtime ids.
    pub fn selection(&self) -> &SelectionSet {
        self.editor.world().selection()
    }

    /// The spatial index over the world.
    pub fn spatial(&self) -> &SpatialIndex {
        &self.spatial
    }

    /// **What the viewport could see on the last frame** — §16's number, from
    /// outside. A 100,000-node document must make this tens, not thousands.
    pub fn visible(&self) -> &VisibleSet {
        &self.visible
    }

    /// What the last frame's extraction produced.
    pub fn last_scene(&self) -> SceneStats {
        self.last_scene
    }

    /// **§24's snapshot from the last frame** — what the canvas and the element
    /// tree were both drawn from.
    pub fn snapshot(&self) -> &RenderSnapshot {
        &self.snapshot
    }

    /// What the last frame's snapshot decided, in counts. `rich_nodes` is §16's
    /// number.
    pub fn snapshot_counts(&self) -> SnapshotCounts {
        self.snapshot.counts()
    }

    /// **Every GPUI element the last frame created** (§16). Tens on any
    /// document, however large.
    pub fn element_count(&self) -> u32 {
        self.snapshot.element_count()
    }

    /// The LOD rung the last frame ran at (§15), or `None` before the first.
    pub fn last_lod(&self) -> Option<LodPlan> {
        self.snapshot.lod()
    }

    /// **§23's cache, from the last frame.** `hit_rate` is ~1.0 during a pure
    /// pan and `translated` is what it did about it.
    pub fn geometry_cache_stats(&self) -> CacheStats {
        self.geometry_cache.frame_stats()
    }

    /// The bytes the geometry cache is holding. Never above
    /// [`RenderBudgets::geometry_cache_max_bytes`].
    pub fn geometry_cache_bytes(&self) -> usize {
        self.geometry_cache.bytes()
    }

    /// The engine's own shaped-line cache (§9).
    pub fn text_cache_stats(&self) -> CacheStats {
        self.text_cache.stats()
    }

    /// §43's registry, so a launcher can register node kinds against a mounted
    /// canvas.
    pub fn registry_mut(&mut self) -> &mut NodeRendererRegistry {
        &mut self.registry
    }

    /// The node under the pointer, or `None`.
    pub fn hovered(&self) -> Option<NodeIndex> {
        self.hovered
    }

    /// What the last frame's spatial sync had to do. `nodes_moved` is zero for
    /// a pure pan and small for a drag; see [`SyncReport`].
    pub fn last_sync(&self) -> SyncReport {
        self.last_sync
    }

    /// §39's probes. Off unless `DODO_FLOW_INSTRUMENT` is set.
    pub fn instruments(&self) -> &Instruments {
        &self.instruments
    }

    pub fn instruments_mut(&mut self) -> &mut Instruments {
        &mut self.instruments
    }

    /// §13's render style. **A renderer strategy, not document geometry.**
    pub fn render_style(&self) -> RenderStyle {
        self.editor.world().settings().render_style
    }

    /// **Switches between clean and hand-drawn** (§13).
    ///
    /// One field and a repaint. Nothing is recreated, nothing is marked dirty,
    /// no route is rebuilt and no element is touched — which is what makes this
    /// a rendering strategy rather than a second document, and it is asserted
    /// by `runtime::world`'s
    /// `switching_render_style_touches_no_element`. The geometry cache keeps
    /// both hands' entries apart by key ([`GeometryKey::sketch`](crate::render::cache::GeometryKey::sketch)),
    /// so switching back finds the old tessellations still warm.
    pub fn set_render_style(&mut self, style: RenderStyle, cx: &mut Context<Self>) {
        if self.editor.world().settings().render_style == style {
            return;
        }
        self.editor.set_render_style(style);
        cx.notify();
    }

    pub fn toggle_render_style(&mut self, cx: &mut Context<Self>) {
        let next = match self.render_style() {
            RenderStyle::Clean => RenderStyle::Sketch,
            RenderStyle::Sketch => RenderStyle::Clean,
        };
        self.set_render_style(next, cx);
    }

    /// The hand [`RenderStyle::Sketch`] draws with (§13).
    pub fn sketch_style(&self) -> SketchStyle {
        self.editor.world().settings().sketch
    }

    /// Changes the hand. A different style is a different cache key, so the
    /// visible set re-tessellates once and then stays cached.
    pub fn set_sketch_style(&mut self, style: SketchStyle, cx: &mut Context<Self>) {
        if self.editor.world().settings().sketch == style {
            return;
        }
        self.editor.set_sketch_style(style);
        cx.notify();
    }

    /// What [`crate::render::scene`] is given each frame.
    fn scene_options(&self) -> SceneOptions {
        SceneOptions::new(self.grid, self.grid_limits)
    }

    /// Frames the whole document, or resets to 1:1 if it is empty.
    pub fn zoom_to_fit(&mut self) {
        match self.editor.world().content_bounds() {
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
    fn sync_theme(&mut self, cx: &App) -> SceneInk {
        let theme = cx.theme();
        let border = from_hsla(theme.border);

        self.grid.minor.color = border.with_alpha(border.a * 0.55);
        self.grid.major.color = border;

        SceneInk {
            fill: from_hsla(theme.secondary),
            stroke: from_hsla(theme.foreground).with_alpha(0.7),
            edge: from_hsla(theme.foreground).with_alpha(0.55),
            handle: from_hsla(theme.primary),
            accent: from_hsla(theme.selection),
            text: from_hsla(theme.foreground),
        }
    }

    /// The connection being dragged out of a handle, if there is one (§8).
    ///
    /// Routed exactly like a committed edge — same router, same options — so
    /// the preview bends the way the edge will and there is no second opinion
    /// about where it would go. The loose end faces back the way the source
    /// leaves, which is what makes the curve settle instead of kinking as the
    /// pointer crosses the node.
    fn plan_connection_preview(&mut self, ink: SceneInk) {
        let Some(pending) = self.interaction.pending_connection() else {
            return;
        };

        let source = self.editor.world().attachment(
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
    fn plan_selection_rect(&mut self, ink: SceneInk) {
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

    /// **Brings the graph, the index and the snapshot up to date**, and is the
    /// one step whose order matters.
    ///
    /// Rebuild first, so routes are current; sync the index from those routes;
    /// query; extract only what came back. Syncing before the rebuild would
    /// index an edge where it used to be, and querying before the sync would
    /// return the same.
    ///
    /// An idle frame finds both queues empty and does no work at all, which is
    /// what makes §40 rule 6 hold by construction rather than by care.
    ///
    /// **Called from `render`, not from paint.** GPUI builds the element tree
    /// in `render` and paints afterwards, so a snapshot extracted during paint
    /// would be a frame late for the elements built from it. The cost of moving
    /// it earlier is that `render` only knows the pane size the last paint
    /// measured — see [`FlowView::pane`].
    fn refresh_snapshot(&mut self) {
        let timer = self.instruments.start();
        self.rebuilt_routes = self.editor.rebuild_dirty_geometry();
        self.instruments.record(Probe::EdgeRoute, timer);

        let timer = self.instruments.start();
        self.last_sync = self.spatial.sync(self.editor.world());
        self.editor.clear_spatial_updates();
        self.instruments.record(Probe::SpatialUpdate, timer);

        let timer = self.instruments.start();
        self.spatial
            .query_visible(self.editor.world(), &self.viewport, &mut self.visible);
        self.instruments.record(Probe::VisibilityQuery, timer);

        let timer = self.instruments.start();
        self.snapshot.extract(
            self.editor.world(),
            &self.visible,
            &self.viewport,
            &self.budgets,
            &self.registry,
            self.hovered,
            Rect::new(Vec2::ZERO, self.viewport.size()),
        );
        self.instruments.record(Probe::RenderExtract, timer);
    }

    fn paint(
        &mut self,
        bounds: Bounds<Pixels>,
        hitbox: &Hitbox,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let pane = Vec2::new(bounds.size.width.as_f32(), bounds.size.height.as_f32());
        let ink = self.sync_theme(cx);

        // A `ShapedLine` bakes its colour at shape time and dodo applies a
        // theme change live, so a changed ink is a cache that has to go. One
        // comparison a frame, against a re-shape of every visible label the
        // first time somebody switches theme.
        if self.text_ink != Some(ink.text) {
            self.text_cache.clear();
            self.text_ink = Some(ink.text);
        }

        if self.pane != pane {
            // **The one case `render` cannot have got right.** It extracted
            // against the previous pane, so the snapshot is for the wrong size
            // and the frame after this one has to redo it. A `cx.notify()` here
            // would record the dirty view and schedule nothing —
            // `WindowInvalidator::invalidate_view` only acts in
            // `DrawPhase::None` — so this asks for a frame the supported way.
            self.pane = pane;
            self.viewport.set_size(pane);
            self.refresh_snapshot();
            window.request_animation_frame();
        }

        let timer = self.instruments.start();
        let options = self.scene_options();
        self.last_scene = scene::plan_scene(
            &mut self.plan,
            self.editor.world(),
            &self.snapshot,
            &self.viewport,
            ink,
            &options,
        );
        // The two overlays are the view's own, and both are on screen by
        // construction — a preview follows the pointer and a rubber band is
        // drawn where it is being dragged. Neither is cached: §23 says not to
        // cache what changes every frame.
        self.plan_connection_preview(ink);
        self.plan_selection_rect(ink);
        self.instruments.record(Probe::RenderExtract, timer);

        self.last_grid = self.last_scene.grid;
        self.dropped_paths = self.plan.enforce_vertex_ceiling(&self.budgets);

        let timer = self.instruments.start();
        let anchor = WindowPainter::anchor(&self.viewport, bounds);
        let zooming = self.zooming;
        // Split so the plan can be read while the caches are written — both are
        // fields of `self`, and `paint_into` borrows one of each.
        let FlowView {
            plan,
            geometry_cache,
            text_cache,
            ..
        } = self;
        geometry_cache.begin_frame(anchor, zooming);
        text_cache.begin_frame();
        let mut painter = WindowPainter::new(window, cx, bounds, geometry_cache, text_cache);
        self.last_paint = plan.paint_into(&mut painter);
        self.geometry_cache.end_frame();
        self.text_cache.end_frame();
        self.instruments.record(Probe::CanvasPaint, timer);

        // The gesture is over unless another zoom event arrives before the next
        // frame, which is what makes the cache re-tessellate once a pinch
        // settles rather than leaving the canvas with scaled strokes.
        self.zooming = false;

        if reporting() && !self.reported {
            self.reported = true;
            self.report_frame();
        }

        self.install_input(bounds, hitbox, window, cx);
    }

    /// One line describing what the hybrid renderer decided on this frame.
    ///
    /// Off unless `DODO_FLOW_REPORT` is set. It is the launcher's acceptance
    /// check made printable — §16's element count, §15's rung and §23's cache
    /// on real geometry, rather than on a benchmark scene.
    fn report_frame(&self) {
        let Some(lod) = self.snapshot.lod() else {
            return;
        };
        let counts = self.snapshot.counts();
        let cache = self.geometry_cache.frame_stats();

        println!(
            "first frame: zoom {:.2} -> {:?} detail, edges {:?}, labels {:?} px",
            lod.zoom, lod.detail, lod.edges, lod.label_font_size
        );
        println!(
            "  §16  {} GPUI elements ({} rich nodes, {} handles, {} toolbar) \
             from {} document nodes",
            self.snapshot.element_count(),
            counts.rich_nodes,
            counts.interactive_handles,
            u32::from(self.snapshot.overlay().is_some()),
            self.editor.world().nodes().len(),
        );
        println!(
            "  §15  {} canvas nodes, {} of {} visible edges drawn, {} skipped, {} labels",
            counts.canvas_nodes,
            counts.edges,
            self.visible.edge_count(),
            counts.skipped_edges,
            self.last_scene.labels,
        );
        println!(
            "  §13  style {:?}{} — {} bodies drawn by hand",
            self.editor.world().settings().render_style,
            match (self.editor.world().settings().sketch_request(), lod.sketch) {
                (Some(_), Some(_)) => ", hand kept",
                (Some(_), None) => ", hand degraded to clean by the ladder",
                _ => "",
            },
            self.last_scene.sketched_bodies,
        );
        println!(
            "  §23  geometry cache {} lookups / {} misses, {:.1} KB; text {} shaped",
            cache.lookups(),
            cache.misses,
            self.geometry_cache.bytes() as f64 / 1e3,
            self.text_cache.len(),
        );
        println!(
            "  paint {} quads, {} paths, {} vertices, {} batches, {} glyphs",
            self.last_paint.quads,
            self.last_paint.paths,
            self.last_paint.path_vertices,
            self.last_paint.path_batches,
            self.last_paint.glyphs,
        );
    }

    /// **Resolves a committed rubber band into a selection** (§28).
    ///
    /// Broad phase from the spatial index, narrow phase in the world — the two
    /// halves §21 and §28 both ask for, and neither of them a scan. The
    /// candidate buffers are fields, so a rubber band released sixty times over
    /// a session allocates once.
    fn commit_box_selection(&mut self, selection: BoxSelection) -> bool {
        let timer = self.instruments.start();

        self.node_candidates.clear();
        self.edge_candidates.clear();
        self.spatial
            .node_candidates(selection.rect, &mut self.node_candidates);
        self.spatial
            .edge_candidates(selection.rect, &mut self.edge_candidates);

        let query =
            BoxQuery::at_zoom(selection.rect, self.viewport.zoom()).additive(selection.additive);
        let changed = self.editor.apply_box_selection(
            query,
            self.node_candidates.iter().copied(),
            self.edge_candidates.iter().copied(),
        );

        self.instruments.record(Probe::HitTest, timer);
        // A band that selected nothing and replaced a selection still changed
        // something; `apply_box_selection` counts additions, and the clear is
        // the other half.
        changed > 0 || !selection.additive
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
            // The camera is this view's, and nothing below it knows the camera
            // exists.
            InteractionEffect::PanBy(delta) => self.viewport.pan_by(delta),

            // **The seam Phase 2 left open.** The machine hands back a world
            // rectangle and says nothing about what is in it; this is where the
            // broad phase and the narrow phase answer that (§28).
            InteractionEffect::CommitBoxSelect(selection) => {
                return self.commit_box_selection(selection) | effect.needs_repaint();
            }

            // **Everything that touches the document goes through §30's
            // commands**, and the mapping lives in `commands::gesture` rather
            // than here — see that module for why, and for the drag it lets a
            // test perform with no window. A refusal is silent on the canvas
            // and visible under `DODO_FLOW_TRACE_INPUT`: the connection tool
            // that colours a handle by its reason is a later phase's, and the
            // reason is already carried for it.
            other => {
                let report = gesture::apply_gesture(&mut self.editor, other);
                match report.connection {
                    Some(Ok(edge)) => trace(format_args!("Connect ok, edge={edge}")),
                    Some(Err(error)) => trace(format_args!("Connect refused: {error}")),
                    None => {}
                }
            }
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

    /// **What is under the pointer** — §29's two phases, both of them present.
    ///
    /// Broad phase from the spatial index, narrow phase in the world. Phase 3
    /// wrote `hit_test` to take its candidates as an argument precisely so that
    /// this line, and only this line, would change; `nodes().indices()` used to
    /// be the argument and now a grid query is.
    ///
    /// The broad phase is asked for the *tolerance*-inflated point rather than
    /// the point, because a handle sits on its node's border and a press within
    /// grabbing distance of one can land in a neighbouring cell.
    ///
    /// A **locked** node reads as empty canvas, so a press on one starts a box
    /// selection instead of a drag that would be refused — §26's behaviour, and
    /// cheaper to answer here than to explain after the fact.
    fn target_at(&self, world: Vec2) -> PointerTarget {
        let radius = self
            .viewport
            .screen_to_world_length(HitTolerance::HANDLE_SCREEN_RADIUS);

        let mut candidates = Vec::new();
        self.spatial.nodes_at(world, radius, &mut candidates);

        match self
            .editor
            .world()
            .hit_test(world, candidates, HitTolerance::new(radius))
        {
            PointerTarget::Node(node) if self.editor.world().nodes().is_locked(node) => {
                PointerTarget::Empty
            }
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
                    // Takes the focus back from whatever had it, so the
                    // canvas's bindings keep working after the pointer has been
                    // somewhere else — a toolbar button, another pane.
                    this.focus_handle.clone().focus(window, cx);

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
                    // **§44's hover half.** A hovered node gets controls, so
                    // the canvas has to know which one — but only a *change*
                    // repaints, which is what keeps a pointer moving across one
                    // node from driving a 60 fps loop.
                    if this.interaction.is_idle() {
                        let world = this
                            .viewport
                            .screen_to_world(this.local(event.position, bounds));
                        let hovered = match this.target_at(world) {
                            PointerTarget::Node(node) => Some(node),
                            PointerTarget::Handle { node, .. } => Some(node),
                            PointerTarget::Empty => None,
                        };
                        if this.hovered != hovered {
                            this.hovered = hovered;
                            cx.notify();
                        }
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
                        this.zooming = true;
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
                    this.zooming = true;
                    cx.notify();
                });
            });
        }
    }

    /// §26's undo, reached through the action bound in [`crate::init`].
    ///
    /// An open gesture is closed by [`FlowEditor::undo`] itself, so pressing
    /// undo mid-drag takes a whole step rather than folding into the drag.
    fn on_undo(&mut self, _: &Undo, _window: &mut Window, cx: &mut Context<Self>) {
        if self.editor.undo() {
            cx.notify();
        }
    }

    fn on_redo(&mut self, _: &Redo, _window: &mut Window, cx: &mut Context<Self>) {
        if self.editor.redo() {
            cx.notify();
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

        // **The one piece of work this body does, and it is bounded by the
        // screen.** Extraction has to happen here rather than in paint: GPUI
        // builds the element tree in `render` and paints afterwards, so a
        // snapshot extracted during paint would be a frame late for every
        // element built from it.
        //
        // That is safe against the trap the crate doc names — this body runs
        // whenever anything above the canvas redraws — because everything it
        // costs is proportional to the viewport: a 2.3 µs spatial query and a
        // walk of tens of visible elements. The heavy work stayed in paint, and
        // the *geometry* it would rebuild is now cached (§23) rather than
        // rebuilt per frame. What must never appear here is a copy of anything
        // document-sized.
        self.refresh_snapshot();

        let nodes = nodes::nodes(&self.snapshot, self.editor.world(), cx);
        let handles = nodes::handles(&self.snapshot, cx);
        let selection = nodes::selection_box(&self.snapshot, cx);
        let toolbar = nodes::toolbar(&self.snapshot, cx);

        div()
            .id("flow-canvas")
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus_handle)
            .size_full()
            .relative()
            .overflow_hidden()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .on_action(cx.listener(Self::on_undo))
            .on_action(cx.listener(Self::on_redo))
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
            // **The rich half.** One layer above the canvas, absolutely
            // positioned, holding tens of elements — never one per document
            // node. See `views::nodes`.
            .child(
                nodes::layer()
                    .children(nodes)
                    .children(selection)
                    .children(handles)
                    .children(toolbar),
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
