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
//!
//! **Keyboard input has a precondition the mouse does not**: it goes down the
//! *focus* path. See [`KEY_CONTEXT`] — a canvas that never takes focus has
//! every one of its bindings silently dead, which is what Phase 2's `Esc` and
//! space-to-pan turned out to be. This view focuses on mount and refocuses on
//! every press.
//!
//! # Where an edit goes
//!
//! Nothing here changes the document. The view holds a
//! [`FlowEditor`] rather than a [`GraphWorld`] and there is **no
//! `world_mut`**;
//! every gesture that means an edit goes through
//! [`apply_gesture`](crate::commands::gesture::apply_gesture),
//! which turns §25's effects into §30's commands. `commands::editor`'s module
//! doc says why that is enforced by ownership rather than by convention, and
//! `commands::gesture`'s says why the mapping is not in this file.
//!
//! Two effects stay here because they are not the document's: `PanBy` moves the
//! camera, and `CommitBoxSelect` needs the spatial broad phase to say what its
//! rectangle contains.

use std::sync::OnceLock;

use dodo_i18n::{flow, t};
use gpui::{
    App, Bounds, Context, DispatchPhase, Entity, FocusHandle, Focusable, Hitbox, HitboxBehavior,
    InteractiveElement, IntoElement, KeyDownEvent, KeyUpEvent, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, ParentElement, Path, PinchEvent, Pixels, Point, Render,
    ScrollWheelEvent, ShapedLine, SharedString, Styled, Window, canvas, div, px,
};
use gpui::{AppContext as _, Div};
use gpui_component::{
    ActiveTheme,
    input::{Input, InputState},
};

use crate::{
    budgets::RenderBudgets,
    commands::{FlowEditor, gesture},
    geometry::{Attachment, EdgeRoute, Rect, RouteOptions, Vec2, Viewport, route},
    instrument::{Instruments, Probe},
    interaction::{
        BoxSelection, CanvasTool, InputModifiers, InteractionEffect, InteractionEvent,
        InteractionMachine, PointerButton, TextTarget,
    },
    models::{
        Color, EdgeIndex, EdgeRouting, FlowDocument, FontFamily, NodeIndex, RenderQuality,
        RenderStyle, SketchStyle,
    },
    render::{
        GridLevel, GridLimits, GridSettings, PaintPlan, PaintStats, SceneInk, SceneOptions,
        SceneStats, WindowPainter,
        cache::{CacheStats, GeometryCache, ShapedLineCache},
        edges,
        lod::LodPlan,
        painter::{self, from_hsla},
        plan::{PathPrimitive, QuadPrimitive},
        registry::NodeRendererRegistry,
        scene,
        scene::GRAPH_NODE_RADIUS,
        shapes,
        snapshot::{RenderSnapshot, SnapshotCounts},
    },
    runtime::{BoxQuery, EdgeEnd, GraphWorld, HitTolerance, PointerTarget, SelectionSet},
    spatial::{SpatialIndex, SyncReport, VisibleSet},
    views::{
        keymap::{Delete, Redo, SelectTool, ToggleToolLock, Undo},
        nodes, palette,
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
///
/// dodo's `gpui-component-recipes` skill already states this — *"with nothing
/// focused, the dispatch path is the window root alone"* — and says to focus in
/// the constructor. This canvas did not, from Phase 2 until Phase 7 needed a
/// binding that anybody would notice was missing. The lesson is about the
/// failure mode rather than the fact: a dead binding produces no error, no
/// warning and no wrong behaviour, only an absence, so it survives every review
/// that is not specifically looking for it.
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

/// The height of §9's text editor, in screen pixels.
///
/// A **screen** constant rather than the edited element's height, and that is
/// deliberate: a node three pixels tall at low zoom is still a thing a user
/// double-clicked to type in, and a three-pixel text field is not an editor.
/// The text lands where the element's own layout puts it once the caret closes;
/// while it is open the field is a control, and a control is measured in
/// pixels.
const EDIT_LINE_PIXELS: f32 = 26.0;

/// The narrowest §9's text editor is drawn, in screen pixels. Same argument as
/// [`EDIT_LINE_PIXELS`], on the other axis.
const MIN_EDITOR_PIXELS: f32 = 120.0;

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

    /// §9's three faces, and the theme's two font names they were resolved
    /// from. Recomputed only when those names change — see
    /// [`FlowView::sync_fonts`], which walks every installed font.
    fonts: Option<painter::FontSet>,
    font_names: Option<(SharedString, SharedString)>,

    /// **The text editor, when a caret is out** (§9).
    ///
    /// `None` most of the time, and that is the whole footprint text editing
    /// has on an idle canvas: one `Option` and no element. What is being edited
    /// is [`InteractionMachine`]'s — see
    /// [`InteractionState::EditingText`](crate::interaction::InteractionState::EditingText)
    /// — so this holds only the widget collecting the characters, which is the
    /// half that needs a `Window` to exist.
    editing: Option<TextEditor>,

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
            fonts: None,
            font_names: None,
            editing: None,
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

    /// **§45's active tool**: what the next press on the canvas means.
    pub fn tool(&self) -> CanvasTool {
        self.interaction.tool()
    }

    /// **Picks up a tool.** The palette's click handler and the key bindings
    /// both land here.
    ///
    /// It goes through the interaction machine as an event rather than setting
    /// a field, because §45's rule is that tool activation *is* an interaction
    /// state change — and because the machine is then the only thing that knows
    /// which tool is active, so there is no second copy to drift.
    ///
    /// A tool change while a gesture is in progress is refused by the machine;
    /// see [`InteractionEvent::SelectTool`]. `Esc` is the way out, and
    /// [`FlowView::on_key_down`] sends the cancel first for exactly that
    /// reason.
    pub fn set_tool(&mut self, tool: CanvasTool, window: &mut Window, cx: &mut Context<Self>) {
        // The palette is a sibling element, so clicking it moves the focus off
        // the canvas and every binding scoped to `KEY_CONTEXT` would go dead
        // until the next press on the canvas itself — the exact failure Phase 7
        // found, arriving through a control this phase added. Taking the focus
        // back here is the whole fix.
        self.focus_handle.clone().focus(window, cx);

        let effect = self.interaction.handle(InteractionEvent::SelectTool(tool));
        if effect.needs_repaint() {
            cx.notify();
        }
    }

    /// **§45's tool lock**: whether finishing a drawing keeps the tool.
    pub fn tool_locked(&self) -> bool {
        self.interaction.tool_locked()
    }

    /// Switches the lock. The toolbar toggle and the `q` binding both land
    /// here, for the same reason [`FlowView::set_tool`] exists: one door, and
    /// the interaction machine is the only place the answer is kept.
    pub fn set_tool_locked(&mut self, locked: bool, window: &mut Window, cx: &mut Context<Self>) {
        // The palette is a sibling element; clicking it moves the focus off the
        // canvas and every binding scoped to `KEY_CONTEXT` goes dead until the
        // next press on the canvas itself. See `set_tool`.
        self.focus_handle.clone().focus(window, cx);

        let effect = self
            .interaction
            .handle(InteractionEvent::SetToolLock(locked));
        if effect.needs_repaint() {
            cx.notify();
        }
    }

    /// **Removes whatever is selected**, through §30's one applier.
    ///
    /// Both delete keys and the toolbar's own action come here, and there is
    /// nothing between this and [`FlowEditor::delete_selection`] but the focus
    /// and the repaint — which is the whole point of the phase's state model.
    /// A view that assembled the command would be a second removal path, and
    /// the first thing to drift from it would be the incident-edge cascade.
    pub fn delete_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_handle.clone().focus(window, cx);

        if self.editor.delete_selection() {
            // A deleted node may have been the hovered one, and §44's controls
            // are drawn from that field rather than from the snapshot. Left
            // set, they would hang over a node that is no longer there until
            // the pointer moved.
            self.hovered = None;
            cx.notify();
        }
    }

    /// Whether the toolbar's Delete action would do anything — §28's selection,
    /// asked as the question the button needs.
    pub fn can_delete(&self) -> bool {
        !self.editor.world().selection().is_empty()
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

    /// What the palette needs to know about the canvas.
    ///
    /// Three cheap reads, assembled here rather than in `render`'s `div` chain
    /// so the palette's inputs are one named thing — and so a fourth (Phase
    /// 11's) is added in one place.
    fn palette_state(&self) -> palette::PaletteState {
        palette::PaletteState {
            tool: self.interaction.tool(),
            tool_locked: self.interaction.tool_locked(),
            can_delete: self.can_delete(),
        }
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

    /// **§9's three faces, resolved against the theme and the installed
    /// fonts.**
    ///
    /// Cached on the view and recomputed only when the theme's own two font
    /// names change: resolving a family means asking the text system for the
    /// list of every installed font, which is not a per-frame question.
    ///
    /// The hand-drawn face is a *preference* — dodo ships no font of its own,
    /// see [`FontFamily::preferred_faces`](crate::models::FontFamily::preferred_faces)
    /// — so the first candidate the machine actually has wins and the theme's
    /// UI font is the fallback. **On a machine with none of them, choosing
    /// Hand-drawn changes nothing on screen.** That is checked here rather than
    /// hoped for: GPUI's `resolve_font` silently substitutes a fallback for a
    /// name it cannot find, so a family that is simply named would draw in
    /// something arbitrary with nothing to say so.
    fn sync_fonts(&mut self, window: &Window, cx: &App) -> painter::FontSet {
        let theme = cx.theme();
        let names = (theme.font_family.clone(), theme.mono_font_family.clone());
        if self.font_names.as_ref() == Some(&names) {
            return self
                .fonts
                .clone()
                .unwrap_or_else(|| painter::FontSet::uniform(window.text_style().font()));
        }

        let base = window.text_style().font();
        let normal = gpui::Font {
            family: names.0.clone(),
            ..base.clone()
        };
        let code = gpui::Font {
            family: names.1.clone(),
            ..base.clone()
        };

        let installed = window.text_system().all_font_names();
        let hand_drawn = FontFamily::HandDrawn
            .preferred_faces(crate::budgets::current_host())
            .iter()
            .find(|face| installed.iter().any(|name| name == *face))
            .map(|face| gpui::Font {
                family: (*face).into(),
                ..base.clone()
            })
            .unwrap_or_else(|| normal.clone());

        let fonts = painter::FontSet {
            normal,
            hand_drawn,
            code,
        };
        self.font_names = Some(names);
        self.fonts = Some(fonts.clone());
        fonts
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

    /// **The element about to be created** (§45), drawn where it will land.
    ///
    /// Painted through the same [`shapes::outline_for_node`] the committed
    /// element will use, from the same rectangle
    /// [`creation_rect`](crate::interaction::creation_rect) resolved — so the
    /// preview is not an approximation of the result, it is the result drawn
    /// early. A click's default-size box appears the moment the button goes
    /// down, which is also what tells the user a click is going to place
    /// something.
    ///
    /// Not cached, for the same reason the rubber band is not: §23 says not to
    /// cache what changes every frame.
    fn plan_creation_preview(&mut self, ink: SceneInk) {
        let Some((tool, world)) = self.interaction.creation_preview() else {
            return;
        };
        let Some(kind) = tool.element_kind() else {
            return;
        };

        let screen = self.viewport.world_rect_to_screen(world);
        let shape = crate::runtime::NodeShape::of(&kind);
        let radius = self.viewport.world_to_screen_length(GRAPH_NODE_RADIUS);
        let Some(outline) = shapes::outline_for_node(shape, screen, radius) else {
            return;
        };

        if !shapes::is_open(shape) {
            self.plan.push_path(PathPrimitive::fill(
                outline.clone(),
                ink.accent.with_alpha(0.12),
                RenderQuality::BALANCED,
            ));
        }
        self.plan.push_path(PathPrimitive::stroke(
            outline,
            ink.accent,
            PREVIEW_WIDTH,
            RenderQuality::BALANCED,
        ));
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
        self.plan_creation_preview(ink);
        self.instruments.record(Probe::RenderExtract, timer);

        self.last_grid = self.last_scene.grid;
        self.dropped_paths = self.plan.enforce_vertex_ceiling(&self.budgets);

        let timer = self.instruments.start();
        let anchor = WindowPainter::anchor(&self.viewport, bounds);
        let zooming = self.zooming;
        // §9's three faces. Resolved here rather than in the painter because
        // only this layer may name a theme, and cached on the view because
        // resolving one walks every installed font.
        let fonts = self.sync_fonts(window, cx);
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
        let mut painter =
            WindowPainter::with_fonts(window, cx, bounds, geometry_cache, text_cache, fonts);
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

    /// **§9's text effects, reachable from two places.**
    ///
    /// [`FlowView::apply`] gets them from a press, and `on_key_down` gets them
    /// from `Esc` and `Enter` — which arrive on a path that has no hitbox,
    /// because a keystroke starts no drag. Splitting it out is what stops the
    /// keyboard route quietly dropping them, which is what happened the first
    /// time: `Esc` while editing produced a `CancelTextEdit` that was handed to
    /// `apply_gesture`, which has no arm for it, and the caret stayed on screen
    /// with nothing to say why.
    ///
    /// Answers whether the effect was one of the three.
    fn apply_text_effect(
        &mut self,
        effect: InteractionEffect,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        match effect {
            InteractionEffect::BeginTextEdit(target) => {
                self.begin_text_edit(target, window, cx);
                true
            }
            InteractionEffect::CommitTextEdit(target) => {
                self.commit_text_edit(target, window, cx);
                true
            }
            InteractionEffect::CancelTextEdit => {
                self.cancel_text_edit(window, cx);
                true
            }
            _ => false,
        }
    }

    /// Applies one effect from the interaction machine, and says whether the
    /// canvas has to be repainted.
    fn apply(
        &mut self,
        effect: InteractionEffect,
        hitbox: &Hitbox,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
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

            // **§9's three text effects stay here for the same reason `PanBy`
            // does**: what they need is a widget, and a widget needs a
            // `Window`. `commands::gesture` deals in effects that become
            // commands, and none of these is one — the *commit* becomes a
            // command, through `FlowEditor::commit_text`, which is on the far
            // side of the UI line where every other edit is.
            InteractionEffect::BeginTextEdit(_)
            | InteractionEffect::CommitTextEdit(_)
            | InteractionEffect::CancelTextEdit => {
                self.apply_text_effect(effect, window, cx);
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
        let tolerance = HitTolerance::at_zoom(self.viewport.zoom());
        let radius = tolerance.handle_radius;

        let mut candidates = Vec::new();
        self.spatial.nodes_at(world, radius, &mut candidates);

        let target = match self.editor.world().hit_test(world, candidates, tolerance) {
            PointerTarget::Node(node) if self.editor.world().nodes().is_locked(node) => {
                PointerTarget::Empty
            }
            target => target,
        };
        if !target.is_empty() {
            return target;
        }

        // **Edges are asked only once nothing else was hit**, which is the
        // ranking `runtime::hit`'s doc records: an edge passing under a node is
        // still the node when you press it. The broad phase is the index's edge
        // grid over the tolerance-inflated point — never a scan (§40 rule 1).
        let mut edges = Vec::new();
        self.spatial.edge_candidates(
            Rect::new(world, Vec2::ZERO).inflate(tolerance.edge_radius),
            &mut edges,
        );
        let flatten = self
            .viewport
            .screen_to_world_length(1.0)
            .max(f32::MIN_POSITIVE);

        match self
            .editor
            .world()
            .hit_test_edge(world, edges, tolerance, flatten)
        {
            Some(edge) => PointerTarget::Edge(edge),
            None => PointerTarget::Empty,
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
                    // somewhere else — a toolbar button, another pane. **Not
                    // while a caret is out**: the field has the focus on
                    // purpose, and stealing it here would send every letter to
                    // the tool palette instead of into the text.
                    if this.editing.is_none() {
                        this.focus_handle.clone().focus(window, cx);
                    }

                    // **§9's double-click**, raised instead of a press rather
                    // than beside it — see `InteractionEvent::DoubleClick` for
                    // why the two are separate events. The count is the
                    // platform's, which is the only thing that knows this
                    // machine's double-click interval.
                    let interaction = if button == PointerButton::Left && event.click_count >= 2 {
                        let screen = this.local(event.position, bounds);
                        let world = this.viewport.screen_to_world(screen);
                        InteractionEvent::DoubleClick {
                            world,
                            target: this.target_at(world),
                        }
                    } else {
                        this.pointer_event(event.position, bounds, button, event.modifiers)
                    };
                    let effect = this.interaction.handle(interaction);
                    if this.apply(effect, &hitbox, window, cx) {
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
                        // §44's hover is a *node's* — an edge gets no controls
                        // and no ring — so an edge under the pointer is the
                        // same answer as empty canvas.
                        let hovered = this.target_at(world).node();
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
                    if this.apply(effect, &hitbox, window, cx) {
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
                    if this.apply(effect, &hitbox, window, cx) {
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

    /// §45's tool activation, reached through the binding registered in
    /// [`crate::init`] or through the palette.
    fn on_select_tool(&mut self, action: &SelectTool, window: &mut Window, cx: &mut Context<Self>) {
        self.set_tool(action.tool, window, cx);
    }

    /// `Delete` and `Backspace`, reached through the bindings registered in
    /// [`crate::init`]. The palette's button calls
    /// [`FlowView::delete_selection`] directly, so both routes are the same
    /// method rather than two that have to agree.
    fn on_delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        self.delete_selection(window, cx);
    }

    /// `q`, and the palette's toggle.
    fn on_toggle_tool_lock(
        &mut self,
        _: &ToggleToolLock,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.set_tool_locked(!self.interaction.tool_locked(), window, cx);
    }

    /// **`Esc` means two things, in order**: abandon whatever is in progress,
    /// then put the Select tool back.
    ///
    /// Two events rather than one, so the interaction machine keeps its
    /// one-effect-per-event rule — and in this order, because a tool change is
    /// refused while a gesture is running. `commands::keys` records why this
    /// keystroke is handled raw here instead of joining the binding table.
    ///
    /// Both effects go through the ordinary [`FlowView::apply`], so an
    /// abandoned creation and an abandoned drag are undone the same way they
    /// always were.
    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        // **While a caret is out, the canvas answers two keys and ignores the
        // rest** (§9). It sees them at all because the text field is a
        // descendant of this element, so a key gpui's single-line `Input`
        // declines — `enter` and `escape` both call `cx.propagate()` — bubbles
        // up here.
        //
        // The `else` branch below is the part that matters: without it, typing
        // a space into a label would set `pan_key_held`, and the *next* press
        // anywhere on the canvas would pan instead of doing what the tool said.
        if self.editing.is_some() {
            let event = match event.keystroke.key.as_str() {
                "enter" => InteractionEvent::FinishTextEdit,
                "escape" => InteractionEvent::Cancel,
                _ => return,
            };
            let effect = self.interaction.handle(event);
            self.apply_text_effect(effect, window, cx);
            return;
        }

        match event.keystroke.key.as_str() {
            PAN_KEY => self.pan_key_held = true,
            "escape" => {
                let cancelled = self.interaction.handle(InteractionEvent::Cancel);
                let mut repaint = cancelled.needs_repaint();
                if repaint {
                    let report = gesture::apply_gesture(&mut self.editor, cancelled);
                    repaint |= report.changed;
                }

                let restored = self
                    .interaction
                    .handle(InteractionEvent::SelectTool(CanvasTool::Select));
                if repaint || restored.needs_repaint() {
                    // The canvas may have lost the focus to the palette; a
                    // keystroke that reached this handler proves it has it now,
                    // and taking it again is free.
                    self.focus_handle.clone().focus(window, cx);
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

fn trace(args: std::fmt::Arguments<'_>) {
    if tracing_input() {
        eprintln!("[flow-input] {args}");
    }
}

/// **The caret, while there is one** (§9).
///
/// A `gpui-component` [`InputState`] and the target it is editing. The target
/// is duplicated from [`InteractionMachine`]'s state on purpose and it is the
/// one duplication in this file: the machine is the authority on *whether* a
/// caret is out, and this is what the commit handler reads when the machine has
/// already returned to `Idle` — the effect carries the target precisely so the
/// two can never be asked in the wrong order.
struct TextEditor {
    target: TextTarget,
    input: Entity<InputState>,
    /// Where the editor is drawn, in pane-relative screen pixels, as of the
    /// frame it was opened on.
    ///
    /// **Recomputed every frame for an existing element** — see
    /// [`FlowView::editor_bounds`] — so an editor over a node that is panned or
    /// zoomed stays on it. A pending element has no world position but its own,
    /// so it carries the world rectangle instead and is projected the same way.
    world: Rect,
}

impl FlowView {
    /// **Opens a caret** (§9), seeded with whatever text the target already has.
    ///
    /// The seeding is what makes existing text *editable* rather than merely
    /// replaceable, which the phase brief calls out by name: an editor that
    /// always opened blank would silently discard a label the moment anybody
    /// double-clicked one to read it.
    fn begin_text_edit(&mut self, target: TextTarget, window: &mut Window, cx: &mut Context<Self>) {
        let Some(world) = self.text_edit_bounds(target) else {
            // The target went away between the press and here — a document
            // swap, or an undo from another handler. Nothing to edit, and the
            // machine is told so rather than left holding a caret.
            self.interaction.handle(InteractionEvent::Cancel);
            return;
        };

        let seed = self.editor.text_of(target).unwrap_or_default().to_owned();
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(t(flow::Text::TextPlaceholder, cx))
                .default_value(seed)
        });
        // Focus goes to the field, which takes it away from the canvas — and
        // that is correct rather than a lapse: while a caret is out, letters
        // must reach the text and not the tool palette. `commit_text_edit`
        // gives it back.
        window.focus(&input.focus_handle(cx), cx);

        self.editing = Some(TextEditor {
            target,
            input,
            world,
        });
        cx.notify();
    }

    /// **Commits what is in the editor**, through §30's one applier.
    fn commit_text_edit(
        &mut self,
        target: TextTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(editor) = self.editing.take() else {
            return;
        };
        let text = editor.input.read(cx).value().to_string();
        // The effect's target wins over the editor's when they disagree, which
        // they cannot today — both come from the same machine transition — but
        // the effect is the one the state machine actually decided on.
        let _ = editor.target;

        let changed = self.editor.commit_text(target, &text);
        self.after_text_edit(changed, window, cx);
    }

    /// Abandons the editor. Nothing reaches the document, including for a
    /// pending element — which is what makes an escaped Text-tool gesture free.
    fn cancel_text_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.editing = None;
        self.after_text_edit(false, window, cx);
    }

    /// The two things both endings do: give the canvas its focus back — or
    /// every binding scoped to it is dead, which is Phase 7's whole lesson —
    /// and repaint.
    fn after_text_edit(&mut self, changed: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_handle.clone().focus(window, cx);
        if changed {
            // A new or resized element has to reach the spatial index before
            // the next frame culls against it. `sync` is the per-frame call and
            // `refresh_snapshot` runs it, so this is only the notify.
            self.editor.rebuild_dirty_geometry();
        }
        cx.notify();
    }

    /// **Where an editor for `target` belongs, in world units.**
    ///
    /// `None` when the target no longer exists, which is the honest answer for
    /// a node an undo took away between the double-click and the open.
    fn text_edit_bounds(&self, target: TextTarget) -> Option<Rect> {
        match target {
            TextTarget::New(rect) => Some(rect.normalized()),
            TextTarget::Node(node) => {
                let nodes = self.editor.world().nodes();
                (nodes.contains(node) && nodes.is_live(node)).then(|| nodes.bounds(node))
            }
            TextTarget::Edge(edge) => {
                let route = self.editor.world().route(edge)?;
                let flatten = self
                    .viewport
                    .screen_to_world_length(1.0)
                    .max(f32::MIN_POSITIVE);
                let center = route.midpoint(flatten);
                // The editor is a box centred on the route, the same width the
                // painter lays an edge label into — so what is typed is the
                // width of what will be drawn.
                let size = Vec2::new(
                    self.viewport
                        .screen_to_world_length(scene::EDGE_LABEL_MAX_PIXELS),
                    self.viewport.screen_to_world_length(EDIT_LINE_PIXELS),
                );
                Some(Rect::new(center - size * 0.5, size))
            }
        }
    }

    /// The editor's element, positioned over whatever is being edited.
    ///
    /// Projected from world units **every frame**, which is what keeps the
    /// field on its node while the canvas is panned or zoomed underneath it.
    /// The height is a screen constant rather than the element's: a text field
    /// three pixels tall is not an editor, whatever the thing it is editing
    /// looks like at that zoom.
    /// Returns a concrete `Div` rather than an `impl IntoElement`, and that is
    /// not a style choice: an opaque return type captures the `&self` borrow
    /// for as long as the element lives, and this one is built at the top of
    /// `render` and consumed at the bottom — with every other builder reading
    /// `self` in between.
    fn text_editor_element(&self, cx: &App) -> Option<Div> {
        let editing = self.editing.as_ref()?;
        // A live element may have moved since the caret opened — an undo, a
        // resize — so the world rectangle is re-read rather than remembered.
        let world = self
            .text_edit_bounds(editing.target)
            .unwrap_or(editing.world);
        let screen = self.viewport.world_rect_to_screen(world).normalized();

        Some(
            div()
                .absolute()
                .left(px(screen.origin.x))
                .top(px(screen.origin.y))
                .w(px(screen.size.x.max(MIN_EDITOR_PIXELS)))
                .h(px(EDIT_LINE_PIXELS))
                .rounded(cx.theme().radius)
                .border_1()
                .border_color(cx.theme().primary)
                .bg(cx.theme().background)
                .child(Input::new(&editing.input).size_full()),
        )
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

        // Built before the tree, because it reads the viewport and the world
        // and both are `&mut self` borrows the builder below already holds.
        let editor = self.text_editor_element(cx);

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
            .on_action(cx.listener(Self::on_select_tool))
            .on_action(cx.listener(Self::on_delete))
            .on_action(cx.listener(Self::on_toggle_tool_lock))
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
            // **§45's palette.** Chrome rather than content, so it sits above
            // the rich layer and is positioned against the pane rather than
            // against the document — a control anchored in world space would
            // pan away from the user.
            .child(
                div()
                    .absolute()
                    .top(px(12.0))
                    .left(px(12.0))
                    .child(palette::palette(cx.entity(), self.palette_state(), cx)),
            )
            // **§9's caret, above everything.** `children` rather than `child`
            // so an idle canvas builds no element at all — text editing costs
            // one `Option` when nobody is typing.
            .children(editor)
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
