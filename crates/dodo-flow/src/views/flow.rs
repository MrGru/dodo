//! [`FlowView`] — the canvas pane.
//!
//! **Today this paints an empty themed pane and nothing else.** That is the whole
//! deliverable here, and it is not a placeholder in the usual sense: the engine
//! underneath it is complete for this phase and unit tested with no `App` and
//! no window, so what this file proves is the *boundary* — that
//! `crate::models` and `crate::geometry` reach a GPUI view without any GPUI
//! type having leaked the other way.
//!
//! Three decisions are already visible in this file's shape and are worth
//! stating before the code that depends on them arrives:
//!
//! - **The root is a `div` with an id, a focus handle and a key context**, not
//!   a bare `canvas()`. `Canvas::id()` returns `None` in the pinned gpui, so a
//!   canvas element carries no element state: no focus, no key context, no
//!   tooltip, no cursor style. Every one of those is needed (`Esc`, `Delete`,
//!   `Cmd+Z`), so the wrapper is where they live and the canvas becomes a child
//!   of it when the canvas layer lands.
//! - **The view fills its pane and clips.** `dodo-tool-view` distinguishes a
//!   tool whose root fills the pane from one the pane scrolls; an infinite
//!   canvas is emphatically the former — it does its own scrolling, in world
//!   space, and an enclosing scroll container would fight it.
//! - **`render` stays cheap, and will have to stay cheap under pressure.**
//!   dodo's root `AGENTS.md` records why: a dirty child marks its whole
//!   ancestor path dirty and an ancestor redraw sets `Window::refreshing`,
//!   which bypasses the element cache for every descendant — so this `render`
//!   re-runs with nothing of its own changed. Once it paints, it must extract a
//!   render snapshot behind a revision stamp rather than copy the document.
//!
//! There are **no user-visible strings here, and that is deliberate**: nothing
//! reaches the sidebar yet, so nothing needs an English and a
//! Vietnamese catalogue entry yet. If a label appears in this file before then,
//! it has escaped its phase — see dodo's `dodo-i18n-text` skill for the rule it
//! would be breaking.

use gpui::{
    App, Context, FocusHandle, Focusable, InteractiveElement, IntoElement, Render, Styled, Window,
    div,
};
use gpui_component::ActiveTheme;

use crate::{budgets::RenderBudgets, geometry::Viewport, models::FlowDocument};

/// The key-binding context the canvas establishes on its root, so canvas
/// bindings fire only while it holds focus and never leak into another tool —
/// the same scoping every other dodo tool uses.
pub const KEY_CONTEXT: &str = "FlowCanvas";

/// The Flow Canvas.
pub struct FlowView {
    document: FlowDocument,
    viewport: Viewport,
    budgets: RenderBudgets,
    focus_handle: FocusHandle,
}

impl FlowView {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> FlowView {
        FlowView {
            document: FlowDocument::new(),
            viewport: Viewport::default(),
            // Resolved once, here, rather than read per frame: it is a
            // compile-time property of the build. Held on the view rather than
            // reached for globally so a benchmark or a test can mount a view
            // against another platform's budgets.
            budgets: crate::budgets::current(),
            focus_handle: cx.focus_handle(),
        }
    }

    pub fn document(&self) -> &FlowDocument {
        &self.document
    }

    pub fn document_mut(&mut self) -> &mut FlowDocument {
        &mut self.document
    }

    /// Replaces the document. The viewport is left alone: opening a document
    /// does not move the camera, because session restore decides
    /// where the camera was and it is not this method's business.
    pub fn set_document(&mut self, document: FlowDocument) {
        self.document = document;
    }

    pub fn viewport(&self) -> &Viewport {
        &self.viewport
    }

    pub fn viewport_mut(&mut self) -> &mut Viewport {
        &mut self.viewport
    }

    /// The render ceilings this build is working under. See
    /// [`crate::budgets`]; every painter asks this rather than
    /// naming a number.
    pub fn budgets(&self) -> &RenderBudgets {
        &self.budgets
    }
}

impl Focusable for FlowView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for FlowView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("flow-canvas")
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus_handle)
            .size_full()
            .relative()
            .overflow_hidden()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
    }
}
