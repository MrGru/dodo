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
    geometry::{Vec2, Viewport},
    interaction::{
        InputModifiers, InteractionEffect, InteractionEvent, InteractionMachine, PointerButton,
    },
    models::{Color, ElementKind, FlowDocument},
    render::{
        GridLevel, GridLimits, GridSettings, PaintPlan, PaintStats, WindowPainter, grid,
        plan::{PathPrimitive, QuadPrimitive},
        shapes,
    },
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
struct ShapeInk {
    fill: Color,
    stroke: Color,
}

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
    document: FlowDocument,
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
}

impl FlowView {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> FlowView {
        // Resolved once, here, rather than read per frame: it is a compile-time
        // property of the build. Held on the view rather than reached for
        // globally so a benchmark or a test can mount a view against another
        // platform's budgets.
        let budgets = crate::budgets::current();

        FlowView {
            document: FlowDocument::new(),
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
        }
    }

    pub fn document(&self) -> &FlowDocument {
        &self.document
    }

    pub fn document_mut(&mut self) -> &mut FlowDocument {
        &mut self.document
    }

    /// Replaces the document. The viewport is left alone: opening a document
    /// does not move the camera, because session restore decides where the
    /// camera was and it is not this method's business.
    pub fn set_document(&mut self, document: FlowDocument) {
        self.document = document;
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

    /// Frames the whole document, or resets to 1:1 if it is empty.
    pub fn zoom_to_fit(&mut self) {
        match self.document.content_bounds() {
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
    fn sync_theme(&mut self, cx: &App) -> ShapeInk {
        let theme = cx.theme();
        let border = from_hsla(theme.border);

        self.grid.minor.color = border.with_alpha(border.a * 0.55);
        self.grid.major.color = border;

        ShapeInk {
            fill: from_hsla(theme.secondary),
            stroke: from_hsla(theme.foreground).with_alpha(0.7),
        }
    }

    /// Every shape in the document, as the cheapest primitive that can draw it.
    ///
    /// **No culling and no spatial query**, and that is a deliberate hole
    /// rather than an oversight: §40 rule 1 forbids scanning every element to
    /// find the visible ones, and the uniform grid that answers it properly is
    /// Phase 4's. Writing a linear scan here would satisfy this phase and then
    /// be the thing everyone forgot to delete. The vertex ceiling in
    /// [`PaintPlan::enforce_vertex_ceiling`] is what stands in for culling
    /// until then — it keeps the window from going black, it does not keep the
    /// frame fast.
    fn plan_shapes(&mut self, ink: ShapeInk) {
        let quality = self.document.settings.render_quality;

        for node in &self.document.nodes {
            if node.hidden {
                continue;
            }

            let ElementKind::Shape(kind) = &node.kind else {
                // Graph nodes, text, images and frames are later phases'. A
                // `_ =>` here would silently draw them as rectangles and hide
                // the fact that they are not implemented.
                continue;
            };

            let screen = self.viewport.world_rect_to_screen(node.bounds());
            let radius = self
                .viewport
                .world_to_screen_length(node.style.corner_radius);
            // `None` means "the theme decides", which is the whole reason
            // `models::style` stores colours as `Option<Color>`: a document must
            // not carry a palette, or it would look wrong in the other theme.
            let fill = fade(node.style.fill.unwrap_or(ink.fill), node.style.opacity);
            let stroke_color = fade(
                node.style.stroke.color.unwrap_or(ink.stroke),
                node.style.opacity,
            );
            let stroke_width = self
                .viewport
                .world_to_screen_length(node.style.stroke.width);
            let has_stroke = !node.style.stroke.is_invisible() && stroke_width > 0.0;

            if shapes::prefers_quad(kind.clone(), 0.0) {
                // Phase 0's measurement, honoured: 20,000 quads hold 60 fps
                // where the same count of rectangular paths drop to 30 — and a
                // quad carries its corner radius and its border for free, so
                // the border costs no second primitive at all.
                let mut quad = QuadPrimitive::filled(screen, fill).with_corner_radius(radius);
                if has_stroke {
                    quad = quad.with_border(stroke_width, stroke_color);
                }
                self.plan.push_quad(quad);
                continue;
            }

            let outline = shapes::outline_for(kind, screen, radius);
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
    }

    /// The box-selection rectangle: **a quad**, so it adds no path batch on top
    /// of the frame it is drawn over.
    fn plan_selection_rect(&mut self, cx: &App) {
        let Some(world) = self.interaction.selection_rect() else {
            return;
        };

        let accent = from_hsla(cx.theme().selection);
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

        self.plan.clear();
        self.last_grid = grid::generate(
            &self.grid,
            &self.viewport,
            &self.grid_limits,
            &mut self.plan,
        );
        self.plan_shapes(ink);
        self.plan_selection_rect(cx);

        self.dropped_paths = self.plan.enforce_vertex_ceiling(&self.budgets);

        let mut painter = WindowPainter::new(window, bounds);
        self.last_paint = self.plan.paint_into(&mut painter);

        self.install_input(bounds, hitbox, window, cx);
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

        if let InteractionEffect::PanBy(delta) = effect {
            self.viewport.pan_by(delta);
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
        InteractionEvent::PointerDown {
            screen,
            world: self.viewport.screen_to_world(screen),
            button,
            modifiers: InputModifiers {
                shift: modifiers.shift,
                control: modifiers.control,
                alt: modifiers.alt,
                command: modifiers.platform,
            },
            pan_key_held: self.pan_key_held,
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
                    let effect = this
                        .interaction
                        .handle(InteractionEvent::PointerUp { button });
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
