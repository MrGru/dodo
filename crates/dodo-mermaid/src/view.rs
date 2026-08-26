//! The Mermaid workspace view: a tab bar, an editor and a live SVG preview —
//! GPUI's half of this crate. [`crate::render`]'s module doc is the other half
//! of the boundary; nothing here calls `mermaid_rs_renderer` directly, and
//! nothing outside this file names a GPUI type.
//!
//! # The live-rendering pipeline
//!
//! An edit never renders inline. [`MermaidView::schedule_render`] is the whole
//! pipeline: it debounces, then renders on the background executor, then
//! discards the result if a later edit has already moved the tab's
//! [`MermaidTab::render_generation`] on — the same "stamp a revision, compare
//! before redoing the work" shape root `AGENTS.md` asks for, applied to a
//! background task rather than a `render` body. A [`gpui::Task`] stored on the
//! tab is dropped — and therefore cancelled — the moment a newer edit replaces
//! it, so the common case (typing) never even reaches the renderer for
//! anything but the keystroke that pauses.
//!
//! **The last successful preview is never cleared by a failed one.** A render
//! error updates [`MermaidTab::render_error`] and leaves
//! [`MermaidTab::rendered_image`] exactly as it was — the workspace plan calls
//! this out by name, because the alternative is a preview that blanks on every
//! unbalanced bracket while the user is still typing the other half of it.
//! That is also why [`MermaidView::render_preview_status`] exists: with the
//! error nowhere on screen, a preserved preview silently stops matching what
//! the user typed, which is strictly worse than blanking it.
//!
//! # Controls live in the pane they act on
//!
//! There is no status bar. A full-width row across the bottom held the word
//! "Mermaid" — which the window title already says — beside controls that act
//! on the preview and signals that describe it, none of which belong to a
//! strip of chrome at the far end of the window. Each now floats inside its
//! own pane:
//!
//! | Control | Pane | Corner |
//! |---|---|---|
//! | [`MermaidView::render_preview_status`] — spinner, render error | preview | top-left |
//! | [`MermaidView::render_zoom_controls`] — `-` / Fit / `+` | preview | bottom-right |
//! | [`MermaidView::render_template_menu`] — insert a template | editor | top-right |
//!
//! Each is a *child element of its pane*, which is what makes
//! [`crate::workspace::WorkspaceMode`]'s two predicates the only thing
//! deciding whether it exists: a control cannot be stranded in a pane that is
//! not drawn, because it is not drawn either. The one piece of state that
//! could outlive its pane is the template menu's open flag, and
//! [`MermaidView::set_mode`] closes it for that reason.
//!
//! # Why the preview is a rasterised image, not the `svg()` element
//!
//! gpui's own `svg()` element paints its file as an **alpha mask** tinted with
//! one colour (`Window::paint_svg`) — right for a monochrome icon, wrong for a
//! diagram with its own fills, strokes and text colours. `cx.svg_renderer()`
//! is the same `SvgRenderer` the sidebar's icons rasterise through
//! (`src/tray/icon.rs`), and `render_single_frame` already rasterises in full
//! colour into an `Arc<RenderImage>` that `img()` accepts directly
//! (`ImageSource::Render`) — so the preview reuses gpui's existing resvg/usvg
//! pipeline rather than adding a second copy of either crate, exactly what the
//! workspace plan's dependency priority asks for.
//!
//! The image is rasterised once per successful render, at a fixed scale
//! ([`PREVIEW_SCALE`]) independent of the user's zoom — zooming and panning
//! never touch [`crate::render`] or re-rasterise; they only change the bounds
//! the already-rasterised bitmap is painted into. See
//! [`MermaidView::render_preview`]'s doc for how.
//!
//! # Zoom is relative to fit, not absolute pixels
//!
//! [`MermaidTab::zoom`] is a multiplier over "fit the container", not over the
//! image's raw pixel size: `1.0` always means exactly fit, whatever the
//! preview pane's current size is. That is what makes "auto-fit on first
//! render, preserve zoom across later edits, Cmd-0 resets to fit" — the
//! workspace plan's phase-4 rule — three lines instead of a stored fit
//! computation: a brand-new tab's `zoom` defaults to `1.0` and is never
//! touched by [`MermaidView::schedule_render`], so it reads as "fit" until the
//! user explicitly changes it, and resetting is just setting it back to `1.0`.
//!
//! The arithmetic itself is [`crate::zoom`]'s, not this file's — including for
//! the wheel, whose modifier rule is `dodo-flow`'s canvas rule character for
//! character (see [`install_preview_input`]).
//!
//! # No `#[gpui::test]` here, on purpose
//!
//! `dodo-flow`'s `views/flow.rs` — the other view in dodo built on a
//! `canvas()` plus raw `window.on_mouse_event` listeners — has none either,
//! and this file does not add the first: at this pinned `gpui` revision,
//! adding *any* `#[cfg(test)] mod tests { #[gpui::test] fn … }` to this file,
//! however trivial the test body, makes `cargo test -p dodo-mermaid` either
//! crash (`SIGBUS`, a `syn` parser stack overflow inside `gpui_macros`) or
//! demand an ever-larger `#![recursion_limit]` that never converges. Isolated
//! by bisection: a single three-line `#[gpui::test]` fn already triggers it,
//! and `dodo-json-formatter` and `dodo-flow` — which have no `#[gpui::test]`
//! either — are the closest working comparisons. [`crate::render`]'s,
//! [`crate::workspace`]'s, [`crate::templates`]'s and [`crate::zoom`]'s plain
//! `#[test]`s and the standalone `examples/mermaid.rs` launcher are this
//! crate's evidence instead, which is the reason every rule this view obeys
//! is stated in one of those four modules rather than here; re-attempt a
//! GPUI-level test here only after confirming on a newer `gpui` revision that
//! the crash is gone.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use dodo_app_icon::AppIcon;

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::button::{Button, ButtonGroup, ButtonVariants};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::popover::Popover;
use gpui_component::{ActiveTheme, Selectable, Sizable, h_flex, v_flex};

use crate::i18n::{Language, LanguageExt, Str, mermaid, t};
use crate::render::{DefaultMermaidRenderer, MermaidRenderer, MermaidTheme};
use crate::templates::{self, MermaidTemplate};
use crate::workspace::{self, CloseOutcome, WorkspaceMode};
use crate::zoom;

/// How long an edit waits before it is rendered. Within the workspace plan's
/// 100–200ms guidance and short enough that typing does not feel like it is
/// waiting on anything — `dodo-mermaid`'s own benchmark puts a real render at
/// well under a millisecond, so almost the whole delay here is debounce, not
/// work.
const DEBOUNCE: Duration = Duration::from_millis(150);

/// How long a render must still be running before the "Rendering…" status
/// appears. Root `AGENTS.md`'s perceptual-threshold rule: a render this crate
/// measures in microseconds must never flash a spinner on every keystroke.
const SPINNER_THRESHOLD: Duration = Duration::from_millis(150);

/// The multiplier over the diagram's own declared size the preview is
/// rasterised at. `render_single_frame` already doubles this
/// (`gpui::SMOOTH_SVG_SCALE_FACTOR`) for antialiasing, so the effective
/// density is 4x — enough headroom that a moderate zoom stays crisp without a
/// re-rasterise.
const PREVIEW_SCALE: f32 = 2.0;

/// How far a floating control is inset from its pane's right edge when the
/// pane is the editor.
///
/// Not the `2` (8px) the preview's overlays use: `gpui-component` paints the
/// code editor's scrollbar as a 16px-wide overlay *inside* the input's own
/// bounds (`scroll::Scrollbar`'s `WIDTH`, `4·2 + 8`), so a control any closer
/// to the edge sits on top of the track and takes the drag that was meant for
/// it. The preview has no scrollbar and needs no such clearance.
const EDITOR_OVERLAY_RIGHT: Pixels = px(16.);

/// The key-binding context the workspace establishes on its root. Scoped the
/// same way `dodo_docker`'s `KEY_CONTEXT` is: bindings registered against it
/// in [`init`] fire only while a Mermaid tab holds focus.
pub const KEY_CONTEXT: &str = "MermaidWorkspace";

actions!(dodo, [MermaidZoomIn, MermaidZoomOut, MermaidZoomReset]);

/// Registers the preview's zoom shortcuts, scoped to [`KEY_CONTEXT`]:
/// `cmd-=` / `cmd--` step the zoom, `cmd-0` resets to fit. Must run after
/// `gpui_component::init`, the same ordering rule every other tool's `init`
/// follows.
pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-=", MermaidZoomIn, Some(KEY_CONTEXT)),
        KeyBinding::new("cmd--", MermaidZoomOut, Some(KEY_CONTEXT)),
        KeyBinding::new("cmd-0", MermaidZoomReset, Some(KEY_CONTEXT)),
    ]);
}

/// One Mermaid document: its editor, its most recent render, and enough
/// bookkeeping to debounce and to reject a stale result.
///
/// [`Self::render_error`] is a raw `String`, not a [`Str`] — the renderer's
/// message is third-party text and stays verbatim, exactly like
/// `json_formatter::Text::InvalidJson`'s `detail` field; [`MermaidView`]
/// builds the translated frame around it at render time.
struct MermaidTab {
    id: u64,
    title: Str,
    editor: Entity<InputState>,
    render_task: Option<Task<()>>,
    render_generation: u64,
    last_rendered_hash: Option<u64>,
    rendered_image: Option<Arc<RenderImage>>,
    /// The last successful render's raw SVG text — kept alongside the
    /// rasterised [`Self::rendered_image`] purely for Copy SVG / Save SVG
    /// (workspace plan phase 6); the preview itself never reads this field.
    rendered_svg: Option<String>,
    render_error: Option<String>,
    rendering: bool,
    show_spinner: bool,
    /// Multiplier over "fit the preview pane"; `1.0` is fit. See this module's
    /// doc for why that makes fit-on-first-render and preserve-on-edit free.
    zoom: f32,
    /// The manual pan offset from centred, in screen pixels at `zoom`.
    pan: Point<Pixels>,
}

/// The Mermaid workspace: one or more [`MermaidTab`]s, a tab bar, and the
/// editor/preview split.
pub struct MermaidView {
    tabs: Vec<MermaidTab>,
    active: usize,
    next_id: u64,
    mode: WorkspaceMode,
    /// The language the editor placeholder was built for; see
    /// [`Self::sync_language`].
    language: Language,
    focus_handle: FocusHandle,
    /// The screen position of the pointer at the last drag event, while a
    /// preview pan is in progress. One field rather than one per tab: only
    /// the active tab's preview is ever visible, so only one drag can be live
    /// at a time.
    panning_from: Option<Point<Pixels>>,
    /// Whether the editor's floating template button has its menu open.
    template_menu_open: bool,
}

impl MermaidView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let language = Language::current(cx);
        let mut view = Self {
            tabs: Vec::new(),
            active: 0,
            next_id: 0,
            mode: WorkspaceMode::Split,
            language,
            focus_handle: cx.focus_handle(),
            panning_from: None,
            template_menu_open: false,
        };
        view.open_blank_tab(window, cx);
        view
    }

    /// Opens a new tab with `source` already in the editor, and selects it.
    /// Used by [`Self::open_blank_tab`] and by a recognised paste (the
    /// workspace's `Route::Mermaid` entry point).
    pub fn open_tab(&mut self, source: String, window: &mut Window, cx: &mut Context<Self>) {
        let id = self.next_id;
        self.next_id += 1;
        let title = mermaid::Text::UntitledTab((id + 1) as usize).into();

        let placeholder = t(mermaid::Text::EditorPlaceholder, cx);
        let editor = cx.new(|cx| {
            InputState::new(window, cx)
                .code_editor("mermaid")
                .multi_line(true)
                .line_number(true)
                .soft_wrap(true)
                .placeholder(placeholder)
        });
        if !source.is_empty() {
            editor.update(cx, |state, cx| {
                state.set_value(source, window, cx);
            });
        }

        cx.subscribe(&editor, move |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.schedule_render(id, cx);
            }
        })
        .detach();

        self.tabs.push(MermaidTab {
            id,
            title,
            editor,
            render_task: None,
            render_generation: 0,
            last_rendered_hash: None,
            rendered_image: None,
            rendered_svg: None,
            render_error: None,
            rendering: false,
            show_spinner: false,
            zoom: 1.0,
            pan: Point::default(),
        });
        self.active = self.tabs.len() - 1;
        // A pasted diagram (`open_tab`'s other caller) should be on screen
        // already rendered, not waiting out the debounce.
        self.schedule_render(id, cx);
        cx.notify();
    }

    /// The one way a blank tab is made.
    ///
    /// The tab bar's "+" and the last tab being closed are the same event as
    /// far as the workspace is concerned — both must leave the user looking at
    /// an empty editor that is active — so they are deliberately one call and
    /// not two similar ones that can drift.
    fn open_blank_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_tab(String::new(), window, cx);
    }

    /// Closes the tab with `id`. What that leaves behind is
    /// [`workspace::close_outcome`]'s decision, not this method's — every case
    /// of it is asserted there, which is the only place in this crate a test
    /// can reach (see this module's doc).
    fn close_tab(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self.tabs.iter().position(|tab| tab.id == id) else {
            return;
        };
        match workspace::close_outcome(self.tabs.len(), self.active, index) {
            CloseOutcome::ReplaceWithBlank => {
                self.tabs.remove(index);
                // `open_blank_tab` sets `active` and notifies; a workspace
                // with no tabs must not survive even until the end of this
                // method, because `render` cannot draw one.
                self.open_blank_tab(window, cx);
            }
            CloseOutcome::RemoveThenActivate(active) => {
                self.tabs.remove(index);
                self.active = active;
                cx.notify();
            }
        }
    }

    fn tab_mut(&mut self, id: u64) -> Option<&mut MermaidTab> {
        self.tabs.iter_mut().find(|tab| tab.id == id)
    }

    /// Debounces, renders on the background executor, and discards the result
    /// if `id`'s tab has moved on to a newer generation or closed entirely by
    /// the time it finishes. See this module's doc for the shape.
    fn schedule_render(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(tab) = self.tab_mut(id) else {
            return;
        };

        tab.render_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(DEBOUNCE).await;

            let started = this.update(cx, |view, cx| {
                let tab = view.tab_mut(id)?;
                let source = tab.editor.read(cx).value().to_string();
                if source.trim().is_empty() {
                    // Nothing to render, and nothing to call an error either —
                    // a brand-new blank tab is not invalid Mermaid, it is just
                    // empty. Any previous preview stays exactly as it was.
                    tab.rendering = false;
                    return None;
                }
                let hash = hash_source(&source);
                if tab.last_rendered_hash == Some(hash) {
                    return None;
                }
                tab.render_generation += 1;
                tab.rendering = true;
                // Read now, not from the background task: `cx.theme()` needs
                // the window, and dodo's `Dodo`/`System` appearance already
                // resolves to light or dark by the time anything reaches here
                // — there is no separate Mermaid theme setting to keep in
                // sync with it.
                let theme = if cx.theme().is_dark() {
                    MermaidTheme::Dark
                } else {
                    MermaidTheme::Light
                };
                cx.notify();
                Some((source, hash, tab.render_generation, theme))
            });
            let Ok(Some((source, hash, generation, theme))) = started else {
                return;
            };

            // The delayed "Rendering…" indicator: only surfaces if this
            // render is still in flight after `SPINNER_THRESHOLD`.
            let spinner_watch = this.clone();
            cx.spawn(async move |cx| {
                cx.background_executor().timer(SPINNER_THRESHOLD).await;
                let _ = spinner_watch.update(cx, |view, cx| {
                    if let Some(tab) = view.tab_mut(id)
                        && tab.render_generation == generation
                        && tab.rendering
                    {
                        tab.show_spinner = true;
                        cx.notify();
                    }
                });
            })
            .detach();

            let output = cx
                .background_executor()
                .spawn(async move { DefaultMermaidRenderer.render(&source, theme) })
                .await;

            let _ = this.update(cx, |view, cx| {
                let Some(tab) = view.tab_mut(id) else {
                    return;
                };
                if tab.render_generation != generation {
                    return; // A later edit already superseded this render.
                }
                tab.rendering = false;
                tab.show_spinner = false;
                tab.last_rendered_hash = Some(hash);
                match output {
                    Ok(rendered) => {
                        tab.render_error = None;
                        tab.rendered_image = cx
                            .svg_renderer()
                            .render_single_frame(rendered.svg.as_bytes(), PREVIEW_SCALE)
                            .ok();
                        tab.rendered_svg = Some(rendered.svg);
                    }
                    Err(error) => {
                        // The last-good `rendered_image`/`rendered_svg` is
                        // left untouched.
                        tab.render_error = Some(error.to_string());
                    }
                }
                cx.notify();
            });
        }));
    }

    /// Re-pushes the localized placeholder text the editor holds internally.
    /// Cheap and idempotent, following `json_formatter::JsonFormatter`'s
    /// pattern for the same problem.
    fn sync_language(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let language = Language::current(cx);
        if language == self.language {
            return;
        }
        self.language = language;

        let placeholder = t(mermaid::Text::EditorPlaceholder, cx);
        for tab in &self.tabs {
            tab.editor.update(cx, |state, cx| {
                state.set_placeholder(placeholder.clone(), window, cx);
            });
        }
    }

    fn active_tab_mut(&mut self) -> Option<&mut MermaidTab> {
        self.tabs.get_mut(self.active)
    }

    /// Switches layout, and closes the template menu on the way.
    ///
    /// The menu's open flag is the one piece of floating-control state that
    /// can outlive the pane holding it: leaving Split for Preview stops
    /// drawing the editor and its popover, but the flag stays set, so coming
    /// back would pop a menu open that nobody asked for.
    fn set_mode(&mut self, mode: WorkspaceMode, cx: &mut Context<Self>) {
        self.mode = mode;
        self.template_menu_open = false;
        cx.notify();
    }

    fn zoom_in(&mut self, cx: &mut Context<Self>) {
        if let Some(tab) = self.active_tab_mut() {
            tab.zoom = zoom::stepped_in(tab.zoom);
            cx.notify();
        }
    }

    fn zoom_out(&mut self, cx: &mut Context<Self>) {
        if let Some(tab) = self.active_tab_mut() {
            tab.zoom = zoom::stepped_out(tab.zoom);
            cx.notify();
        }
    }

    /// Cmd-0, and also what a new tab starts at: `1.0` reads as "fit" by
    /// construction (see this module's doc), and centring is just zeroing the
    /// pan.
    fn zoom_reset(&mut self, cx: &mut Context<Self>) {
        if let Some(tab) = self.active_tab_mut() {
            tab.zoom = 1.0;
            tab.pan = Point::default();
            cx.notify();
        }
    }

    fn on_zoom_in(&mut self, _: &MermaidZoomIn, _: &mut Window, cx: &mut Context<Self>) {
        self.zoom_in(cx);
    }

    fn on_zoom_out(&mut self, _: &MermaidZoomOut, _: &mut Window, cx: &mut Context<Self>) {
        self.zoom_out(cx);
    }

    fn on_zoom_reset(&mut self, _: &MermaidZoomReset, _: &mut Window, cx: &mut Context<Self>) {
        self.zoom_reset(cx);
    }

    /// Appends `template`'s source to the active tab's editor, leaving the
    /// caret at the end of what was inserted.
    ///
    /// **Appends, in this tab.** The templates used to open a new tab each;
    /// they are just as often a reminder of a syntax halfway through a
    /// document, and neither reading survives a button that throws the buffer
    /// away. Where the blank line between old and new goes is
    /// [`templates::appended`]'s rule, not this method's.
    ///
    /// Select-all-then-`replace` rather than `InputState::set_value`, for two
    /// reasons that both bite silently: `set_value` clears the undo history,
    /// so an accidental click could not be taken back, and it suppresses
    /// `InputEvent::Change`, so the insertion would never reach the
    /// subscription that calls [`Self::schedule_render`] and the preview would
    /// sit there showing the diagram from before.
    ///
    /// The caret then lands at the end of the appended block, focused: what a
    /// person does next is type into the diagram they just inserted. `replace`
    /// leaves the selection there already but does not scroll to it, so the
    /// position is re-set to the one it already holds — that round trip is
    /// what runs `InputState::move_to`'s scroll-into-view and takes focus off
    /// the popover.
    fn append_template(
        &mut self,
        template: MermaidTemplate,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.tabs.get(self.active) else {
            return;
        };
        let editor = tab.editor.clone();
        editor.update(cx, |state, cx| {
            let existing = state.value().to_string();
            let appended = templates::appended(&existing, template.source());
            state.set_selected_range(0..existing.len(), cx);
            state.replace(appended, window, cx);
            let caret = state.cursor_position();
            state.set_cursor_position(caret, window, cx);
        });
        cx.notify();
    }

    /// Copies the active tab's editor contents verbatim — not the last
    /// rendered source, so an edit made since the last render is included.
    fn copy_source(&self, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(self.active) else {
            return;
        };
        let source = tab.editor.read(cx).value().to_string();
        cx.write_to_clipboard(ClipboardItem::new_string(source));
    }

    /// Copies the active tab's last successful render. Nothing to copy before
    /// the first successful render, or while only an error is on offer — the
    /// button simply does nothing rather than copying stale or empty text.
    fn copy_svg(&self, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(self.active) else {
            return;
        };
        if let Some(svg) = tab.rendered_svg.clone() {
            cx.write_to_clipboard(ClipboardItem::new_string(svg));
        }
    }

    /// Saves the active tab's editor contents as `.mmd`, via the platform's
    /// own save dialog. The write happens on the background executor, never
    /// on the UI thread — the same shape `dodo-api-explorer`'s response export
    /// uses (`services::file_export::write_file`).
    fn save_source(&self, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(self.active) else {
            return;
        };
        let source = tab.editor.read(cx).value().to_string();
        let suggested = format!("{}.mmd", t(tab.title.clone(), cx));
        self.save_to_chosen_path(source.into_bytes(), &suggested, cx);
    }

    /// Saves the active tab's last successful render as `.svg`. Same
    /// no-render-yet guard as [`Self::copy_svg`].
    fn save_svg(&self, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(self.active) else {
            return;
        };
        let Some(svg) = tab.rendered_svg.clone() else {
            return;
        };
        let suggested = format!("{}.svg", t(tab.title.clone(), cx));
        self.save_to_chosen_path(svg.into_bytes(), &suggested, cx);
    }

    fn save_to_chosen_path(&self, bytes: Vec<u8>, suggested_name: &str, cx: &mut Context<Self>) {
        let directory = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let receiver = cx.prompt_for_new_path(&directory, Some(suggested_name));
        cx.spawn(async move |_, cx| {
            let Ok(Ok(Some(path))) = receiver.await else {
                return;
            };
            cx.background_executor()
                .spawn(async move {
                    let _ = std::fs::write(&path, &bytes);
                })
                .await;
        })
        .detach();
    }

    /// The tab strip, ending in a "+" that opens a blank tab.
    ///
    /// **One click, one blank tab.** "+" used to drop a template menu, which
    /// made the commonest gesture in the workspace — start a new diagram —
    /// cost a click and a read; the templates are now the editor's own
    /// floating button ([`Self::render_template_menu`]) where they append into
    /// the document instead of demanding a tab of their own.
    fn render_tab_bar(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        h_flex()
            .items_center()
            .gap_1()
            .children(self.tabs.iter().enumerate().map(|(index, tab)| {
                let id = tab.id;
                let active = index == self.active;
                h_flex()
                    .id(("mermaid-tab", id))
                    .items_center()
                    .gap_2()
                    .px_2()
                    .py_1()
                    .rounded(cx.theme().radius)
                    .when(active, |this| this.bg(cx.theme().secondary))
                    .child(div().text_sm().child(t(tab.title.clone(), cx)))
                    .child(
                        Button::new(("mermaid-tab-close", id))
                            .ghost()
                            .xsmall()
                            .icon(AppIcon::Close)
                            .tooltip(t(mermaid::Text::CloseTabTooltip, cx))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.close_tab(id, window, cx);
                            })),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(position) = this.tabs.iter().position(|tab| tab.id == id) {
                            this.active = position;
                            cx.notify();
                        }
                    }))
            }))
            .child(
                Button::new("mermaid-new-tab")
                    .ghost()
                    .xsmall()
                    .icon(AppIcon::Plus)
                    .tooltip(t(mermaid::Text::NewTabTooltip, cx))
                    .on_click(cx.listener(|this, _, window, cx| this.open_blank_tab(window, cx))),
            )
    }

    /// The editor's floating template button: [`MermaidTemplate::ALL`], each a
    /// plain row that appends its source into the buffer
    /// ([`Self::append_template`]) and closes the menu. A hand-rolled list
    /// inside a `Popover`, the same shape `dodo-api-explorer`'s per-row node
    /// menu uses — this library revision has no separate "popup menu" type
    /// worth reaching for over it.
    ///
    /// **Top-right, not the preview's bottom-right.** The two floating
    /// clusters sit in adjacent panes in Split mode; matching corners would
    /// read as one row of controls straddling the divider. Anchoring at the
    /// top also means the popover opens *downwards* over the editor rather
    /// than off the bottom of the window, and the right edge keeps it out of
    /// the way of the caret, which starts at the left and travels down.
    /// [`EDITOR_OVERLAY_RIGHT`] is what keeps it off the code editor's own
    /// overlay scrollbar.
    fn render_template_menu(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let mut menu = v_flex().gap_0p5().p_1();
        for template in MermaidTemplate::ALL {
            menu = menu.child(
                Button::new(("mermaid-template", template as usize))
                    .ghost()
                    .xsmall()
                    .w_full()
                    .justify_start()
                    .label(t(template.label(), cx))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.template_menu_open = false;
                        this.append_template(template, window, cx);
                    })),
            );
        }

        overlay_ground(cx)
            .absolute()
            .top_2()
            .right(EDITOR_OVERLAY_RIGHT)
            .child(
                Popover::new("mermaid-template-menu")
                    .open(self.template_menu_open)
                    .on_open_change(cx.listener(|this, open, _, cx| {
                        this.template_menu_open = *open;
                        cx.notify();
                    }))
                    .trigger(
                        Button::new("mermaid-templates")
                            .ghost()
                            .xsmall()
                            .icon(AppIcon::LayoutDashboard)
                            .tooltip(t(mermaid::Text::TemplatesTooltip, cx)),
                    )
                    .w(px(140.))
                    .child(menu),
            )
    }

    /// The Editor / Split / Preview switch, as one segmented control.
    ///
    /// `ButtonGroup` rather than `ToggleGroup`: the two look alike from the
    /// outside and are not. `ToggleGroup` is a set of *independent* toggles —
    /// its `on_click` hands back one `bool` per item and it is happy for two
    /// to be on at once, so exclusivity would have to be re-derived here from
    /// which flag moved. `ButtonGroup` with its default `multiple(false)`
    /// already **is** single-select: it clears the selection and reports the
    /// one index that was clicked, which is exactly what a mode is.
    ///
    /// Icons rather than labels, so tooltips are not optional. Both come from
    /// [`WorkspaceMode::label`], the same three strings the buttons used to
    /// draw — a tooltip that reworded them would be a second name for one
    /// thing.
    fn render_mode_toggle(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        ButtonGroup::new("mermaid-mode")
            .outline()
            .compact()
            .small()
            .children(WorkspaceMode::ALL.map(|mode| {
                let selected = self.mode == mode;
                let button = Button::new(mode_button_id(mode))
                    .icon(mode_icon(mode))
                    .tooltip(t(mode.label(), cx))
                    .selected(selected);
                // The group applies its own variant to every child only when
                // it has one, so leaving it unset is what lets the chosen
                // segment carry `primary` while the rest stay default —
                // outline's own selected tint is a few per cent of opacity
                // apart from its normal one, which is not a selection anybody
                // reads at a glance.
                if selected { button.primary() } else { button }
            }))
            .on_click(cx.listener(|this, selected: &Vec<usize>, _, cx| {
                let Some(index) = selected.first().copied() else {
                    return;
                };
                let Some(mode) = WorkspaceMode::ALL.get(index).copied() else {
                    return;
                };
                this.set_mode(mode, cx);
            }))
    }

    /// The editor pane, with the template button floating in it.
    ///
    /// `relative()` is what the overlay is positioned against; without it the
    /// button anchors to whichever ancestor happens to be positioned, which is
    /// the workspace root.
    fn render_editor(&self, tab: &MermaidTab, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        div()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .relative()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(cx.theme().border)
            .child(
                Input::new(&tab.editor)
                    .font_family(cx.theme().mono_font_family.clone())
                    .text_size(cx.theme().mono_font_size)
                    .size_full(),
            )
            .child(self.render_template_menu(cx))
    }

    /// Paints the active tab's rasterised diagram at "fit × zoom", offset by
    /// `pan`, installs the pan/zoom mouse handlers over it, and floats the
    /// zoom controls and the render status in its corners.
    ///
    /// A `canvas()` rather than `img()`, because centring "fit times a zoom
    /// the user controls" needs the container's actual pixel bounds at paint
    /// time — `img()`'s own `ObjectFit::Contain` only ever fits to `1.0`, with
    /// no way to ask it for a different multiple. Nothing here parses or lays
    /// out Mermaid: [`paint_preview_image`] only ever repaints the bitmap
    /// [`MermaidView::schedule_render`] already produced, at whatever bounds
    /// this frame's zoom and pan say — the requirement the workspace plan's
    /// phase 4 states by name.
    ///
    /// The body is an `AnyElement` because its two arms are different element
    /// types and the overlays have to be added to the frame once, after
    /// either: a second copy of the overlay chain per arm is how one of them
    /// ends up a version behind.
    fn render_preview(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let frame = div()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .relative()
            .overflow_hidden();

        let Some(tab) = self.tabs.get(self.active) else {
            return frame;
        };

        let body = match tab.rendered_image.clone() {
            Some(image) => {
                let zoom = tab.zoom;
                let pan = tab.pan;
                let view = cx.entity();
                canvas(
                    |bounds, window, _cx| window.insert_hitbox(bounds, HitboxBehavior::Normal),
                    move |bounds, hitbox, window, _cx| {
                        paint_preview_image(&image, zoom, pan, bounds, window);
                        install_preview_input(view.clone(), &hitbox, bounds, window);
                    },
                )
                .absolute()
                .size_full()
                .into_any_element()
            }
            None => div()
                .absolute()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(t(mermaid::Text::EmptyPreviewHint, cx)),
                )
                .into_any_element(),
        };

        frame
            .child(body)
            .child(self.render_preview_status(tab, cx))
            .child(self.render_zoom_controls(cx))
    }

    /// The rendering spinner and the render error, floating in the preview's
    /// top-left corner.
    ///
    /// **This is where the deleted status bar's two real signals went.** The
    /// bar was a full-width row holding the word "Mermaid" plus these two, and
    /// the label was the only part of it with no job. The signals could not go
    /// with it: the preview deliberately keeps the last good render when the
    /// source stops parsing (this module's doc says why), so with the error
    /// nowhere on screen the preview quietly stops matching the text beside
    /// it.
    ///
    /// Top-left because the diagram is centred and the zoom cluster owns the
    /// bottom-right: a message here crosses a wide diagram's corner rather
    /// than its middle, and never the controls. Both signals are chips with a
    /// ground of their own — a rendered diagram can put any colour at all
    /// behind them — and both can be up at once, because a render that starts
    /// does not clear the previous error until it succeeds.
    fn render_preview_status(
        &self,
        tab: &MermaidTab,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        v_flex()
            .absolute()
            .top_2()
            .left_2()
            // `v_flex()` sets no `align_items`, so it defaults to stretch and
            // a one-word spinner chip would be dragged out to the width of a
            // paragraph-long error beside it. Each chip sizes to its own text.
            .items_start()
            // A renderer message is third-party text of no known length; left
            // to itself it would run the width of the pane and out of it.
            .max_w(relative(0.75))
            .gap_1()
            .text_xs()
            .when(tab.show_spinner, |this| {
                this.child(
                    overlay_ground(cx)
                        .px_2()
                        .py_0p5()
                        .text_color(cx.theme().muted_foreground)
                        .child(t(mermaid::Text::Rendering, cx)),
                )
            })
            .when_some(tab.render_error.clone(), |this, detail| {
                this.child(
                    overlay_ground(cx)
                        .px_2()
                        .py_0p5()
                        .text_color(cx.theme().danger)
                        .child(t(mermaid::Text::RenderError { detail }, cx)),
                )
            })
    }

    /// `-` / Fit / `+`, floating in the preview's bottom-right corner.
    ///
    /// In the pane they act on, and therefore drawn only when that pane is
    /// (see this module's doc). Bottom-right rather than top-right: the status
    /// chips have the top-left, a diagram is centred, and the corner furthest
    /// from both is the one a control can sit in without covering anything a
    /// person is reading.
    fn render_zoom_controls(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        overlay_ground(cx)
            .absolute()
            .bottom_2()
            .right_2()
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .p_0p5()
            .child(
                Button::new("mermaid-zoom-out")
                    .ghost()
                    .xsmall()
                    .label(t(mermaid::Text::ZoomOutLabel, cx))
                    .on_click(cx.listener(|this, _, _, cx| this.zoom_out(cx))),
            )
            .child(
                Button::new("mermaid-zoom-fit")
                    .ghost()
                    .xsmall()
                    .label(t(mermaid::Text::FitLabel, cx))
                    .on_click(cx.listener(|this, _, _, cx| this.zoom_reset(cx))),
            )
            .child(
                Button::new("mermaid-zoom-in")
                    .ghost()
                    .xsmall()
                    .label(t(mermaid::Text::ZoomInLabel, cx))
                    .on_click(cx.listener(|this, _, _, cx| this.zoom_in(cx))),
            )
    }

    /// Copy source / copy SVG / save `.mmd` / save SVG — workspace plan
    /// phase 6's "Required" scope, and deliberately nothing past it (no
    /// PNG/PDF, no export dialog beyond the platform's own save prompt).
    ///
    /// **Four buttons, four glyphs.** These are two verbs over two payloads,
    /// and the payload is the half a person actually has to read: copying the
    /// source and copying the render are one keystroke apart and not
    /// undoable-by-eye. So the payload is carried by a *mark* —
    /// [`AppIcon::CopyImage`] and [`AppIcon::ImageDown`] both draw a picture,
    /// [`AppIcon::Copy`] and [`AppIcon::Save`] both draw none — and the verb
    /// by the motif around it. Giving both copies `Copy` and both saves `Save`
    /// left the tooltip as the only thing telling them apart, which a toolbar
    /// used by sight cannot rely on.
    fn render_file_actions(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        h_flex()
            .items_center()
            .gap_1()
            .child(
                Button::new("mermaid-copy-source")
                    .ghost()
                    .xsmall()
                    .icon(AppIcon::Copy)
                    .tooltip(t(mermaid::Text::CopySourceTooltip, cx))
                    .on_click(cx.listener(|this, _, _, cx| this.copy_source(cx))),
            )
            .child(
                Button::new("mermaid-copy-svg")
                    .ghost()
                    .xsmall()
                    .icon(AppIcon::CopyImage)
                    .tooltip(t(mermaid::Text::CopySvgTooltip, cx))
                    .on_click(cx.listener(|this, _, _, cx| this.copy_svg(cx))),
            )
            .child(
                Button::new("mermaid-save-source")
                    .ghost()
                    .xsmall()
                    .icon(AppIcon::Save)
                    .tooltip(t(mermaid::Text::SaveSourceTooltip, cx))
                    .on_click(cx.listener(|this, _, _, cx| this.save_source(cx))),
            )
            .child(
                Button::new("mermaid-save-svg")
                    .ghost()
                    .xsmall()
                    .icon(AppIcon::ImageDown)
                    .tooltip(t(mermaid::Text::SaveSvgTooltip, cx))
                    .on_click(cx.listener(|this, _, _, cx| this.save_svg(cx))),
            )
    }
}

impl Focusable for MermaidView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for MermaidView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_language(window, cx);

        let root = v_flex()
            .id("mermaid-workspace")
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus_handle)
            .size_full()
            .gap_2()
            .on_action(cx.listener(Self::on_zoom_in))
            .on_action(cx.listener(Self::on_zoom_out))
            .on_action(cx.listener(Self::on_zoom_reset));

        let Some(active) = self.tabs.get(self.active) else {
            // Unreachable by construction — `close_tab` never leaves the
            // workspace tabless — and deliberately still handled, because the
            // alternative is a panic and because this early return is *why*
            // an empty workspace was unrecoverable: no tab bar renders here,
            // so there is nothing left to open a tab from.
            return root;
        };

        root.child(
            h_flex()
                .items_center()
                .justify_between()
                .child(self.render_tab_bar(cx))
                .child(
                    h_flex()
                        .items_center()
                        .gap_3()
                        .child(self.render_file_actions(cx))
                        .child(self.render_mode_toggle(cx)),
                ),
        )
        .child(
            h_flex()
                // The one line that makes this row a *split* rather than a
                // toolbar. `h_flex()` is `flex_row()` **plus** `items_center()`
                // — right for a row of controls, wrong here: in a row the
                // cross axis is height, so the inherited `items_center` sized
                // each pane to its own content and centred it, and the panes'
                // `flex_1()` did not object because `flex_1` sizes along the
                // *main* axis, which is width. The result was an editor and a
                // preview one line tall floating in the middle of an empty
                // window. `dodo-encoder-decoder`'s `PaneLayout::Horizontal`
                // carries `items_stretch()` for exactly this reason; every
                // other `h_flex()` in this file really is a toolbar and must
                // keep the centring it inherits.
                .items_stretch()
                .flex_1()
                .min_h_0()
                .gap_2()
                // Both panes — and therefore the floating controls inside
                // each — are gated on `WorkspaceMode`'s own predicates rather
                // than on a `matches!` written out here, so the three
                // mode-dependent surfaces cannot disagree about what Split
                // means.
                .when(self.mode.shows_editor(), |this| {
                    this.child(self.render_editor(active, cx))
                })
                .when(self.mode.shows_preview(), |this| {
                    this.child(self.render_preview(cx))
                }),
        )
    }
}

fn hash_source(source: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    source.hash(&mut hasher);
    hasher.finish()
}

/// The glyph a mode is drawn as, now that the switch carries no text.
///
/// Three marks that have to be told apart at 16px and mean *source only*,
/// *both side by side* and *result only*. [`AppIcon::SquareCode`] is a pane
/// with `< >` in it, [`AppIcon::Columns`] is a pane divided into side-by-side
/// bands, and [`AppIcon::Eye`] is the one glyph in the set that means "look at
/// it" rather than naming a thing. All three are already in dodo's vocabulary,
/// so this adds no SVG — and none of them is the editor's own template button
/// ([`AppIcon::LayoutDashboard`]), which is the collision that would actually
/// cost a person time, since that button sits inside the pane the first of
/// these turns on.
fn mode_icon(mode: WorkspaceMode) -> AppIcon {
    match mode {
        WorkspaceMode::Editor => AppIcon::SquareCode,
        WorkspaceMode::Split => AppIcon::Columns,
        WorkspaceMode::Preview => AppIcon::Eye,
    }
}

fn mode_button_id(mode: WorkspaceMode) -> &'static str {
    match mode {
        WorkspaceMode::Editor => "mermaid-mode-editor",
        WorkspaceMode::Split => "mermaid-mode-split",
        WorkspaceMode::Preview => "mermaid-mode-preview",
    }
}

/// The ground a control or a message needs where it floats over a pane's
/// content.
///
/// One helper rather than three copies: the workspace's floating clusters have
/// to read as the same kind of object, and both panes can put anything at all
/// behind them — a rendered diagram is arbitrary colour, and syntax-highlighted
/// code is arbitrary text.
fn overlay_ground(cx: &App) -> Div {
    div()
        .rounded(cx.theme().radius)
        .bg(cx.theme().popover)
        .border_1()
        .border_color(cx.theme().border)
}

/// Paints `image` centred in `bounds` at "fit × `zoom`", offset by `pan`. Pure
/// arithmetic over bounds gpui already computed this frame — no parse, no
/// layout, no re-rasterisation, whatever `zoom` and `pan` are.
fn paint_preview_image(
    image: &Arc<RenderImage>,
    zoom: f32,
    pan: Point<Pixels>,
    bounds: Bounds<Pixels>,
    window: &mut Window,
) {
    let natural = image.size(0);
    let natural_width = natural.width.0 as f32;
    let natural_height = natural.height.0 as f32;
    if natural_width <= 0.0 || natural_height <= 0.0 {
        return;
    }

    let fit = (bounds.size.width.as_f32() / natural_width)
        .min(bounds.size.height.as_f32() / natural_height);
    let scale = (fit * zoom).max(0.001);
    let width = px(natural_width * scale);
    let height = px(natural_height * scale);

    let origin = point(
        bounds.origin.x + (bounds.size.width - width).half() + pan.x,
        bounds.origin.y + (bounds.size.height - height).half() + pan.y,
    );

    window
        .paint_image(
            Bounds::new(origin, size(width, height)),
            Corners::default(),
            image.clone(),
            0,
            false,
        )
        .ok();
}

/// Registers this frame's pan and zoom listeners over the preview's hitbox.
///
/// Mirrors `dodo-flow`'s canvas input pattern (`views/flow.rs`): listeners are
/// registered from inside the paint closure and last exactly one frame, and
/// `capture_pointer` is what keeps a drag alive once the pointer leaves the
/// preview pane — there is nothing to release, it clears on mouse up.
///
/// **The wheel rule is `dodo-flow`'s, deliberately unchanged.** Cmd (or Ctrl)
/// plus the wheel zooms; a bare wheel or a two-finger trackpad swipe pans.
/// dodo has exactly two zoomable surfaces and a person who has learnt one has
/// learnt the other, so the modifier is copied rather than chosen again —
/// including `should_handle_scroll` over `is_hovered`, which is gpui's own
/// advice for scroll events, and `stop_propagation`, without which every wheel
/// notch here would also scroll the main pane this tool sits in.
///
/// The zoom is cursor-anchored through [`zoom::anchored_pan`], fed the factor
/// that was *applied* rather than the one asked for — see that function's doc
/// for why the difference is visible at the ends of the range.
fn install_preview_input(
    view: Entity<MermaidView>,
    hitbox: &Hitbox,
    bounds: Bounds<Pixels>,
    window: &mut Window,
) {
    {
        let (hitbox, view) = (hitbox.clone(), view.clone());
        window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble
                || !hitbox.is_hovered(window)
                || event.button != MouseButton::Left
            {
                return;
            }
            window.capture_pointer(hitbox.id);
            view.update(cx, |this, _| {
                this.panning_from = Some(event.position);
            });
        });
    }
    {
        let (hitbox, view) = (hitbox.clone(), view.clone());
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble || !hitbox.is_hovered(window) {
                return;
            }
            view.update(cx, |this, cx| {
                let Some(from) = this.panning_from else {
                    return;
                };
                this.panning_from = Some(event.position);
                if let Some(tab) = this.active_tab_mut() {
                    tab.pan.x += event.position.x - from.x;
                    tab.pan.y += event.position.y - from.y;
                    cx.notify();
                }
            });
        });
    }
    {
        let view = view.clone();
        window.on_mouse_event(move |_: &MouseUpEvent, phase, _window, cx| {
            if phase != DispatchPhase::Bubble {
                return;
            }
            // Not gated on hover: a drag that ends outside the preview must
            // still stop, or the next move over it would resume panning with
            // a stale anchor.
            view.update(cx, |this, _| this.panning_from = None);
        });
    }
    {
        let (hitbox, view) = (hitbox.clone(), view.clone());
        window.on_mouse_event(move |event: &ScrollWheelEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble || !hitbox.should_handle_scroll(window) {
                return;
            }
            let pixels = event.delta.pixel_delta(px(zoom::SCROLL_LINE_HEIGHT));
            let zooming = event.modifiers.platform || event.modifiers.control;
            let centre = bounds.center();

            view.update(cx, |this, cx| {
                let Some(tab) = this.active_tab_mut() else {
                    return;
                };
                if zooming {
                    let before = tab.zoom;
                    let after = zoom::scaled(before, zoom::wheel_factor(pixels.y.as_f32()));
                    tab.zoom = after;
                    let applied = after / before;
                    tab.pan.x = px(zoom::anchored_pan(
                        tab.pan.x.as_f32(),
                        (event.position.x - centre.x).as_f32(),
                        applied,
                    ));
                    tab.pan.y = px(zoom::anchored_pan(
                        tab.pan.y.as_f32(),
                        (event.position.y - centre.y).as_f32(),
                        applied,
                    ));
                } else {
                    tab.pan.x += pixels.x;
                    tab.pan.y += pixels.y;
                }
                cx.notify();
            });
            cx.stop_propagation();
        });
    }
}
