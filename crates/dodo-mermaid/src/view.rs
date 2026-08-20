//! The Mermaid workspace view: a tab bar, an editor, a live SVG preview and a
//! status line — GPUI's half of this crate. [`crate::render`]'s module doc is
//! the other half of the boundary; nothing here calls `mermaid_rs_renderer`
//! directly.
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
//! either — are the closest working comparisons. [`crate::render`]'s 12 tests
//! and the standalone `examples/mermaid.rs` launcher are this crate's
//! evidence instead; re-attempt a GPUI-level test here only after confirming
//! on a newer `gpui` revision that the crash is gone.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use dodo_app_icon::AppIcon;

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::popover::Popover;
use gpui_component::{ActiveTheme, Sizable, h_flex, v_flex};

use crate::i18n::{Language, LanguageExt, Str, mermaid, t};
use crate::render::{DefaultMermaidRenderer, MermaidRenderer, MermaidTheme};

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

/// How far one `+`/`-` step or keystroke moves the zoom. Multiplicative, so
/// repeated steps feel even whether zooming in or out.
const ZOOM_STEP: f32 = 1.25;

/// The zoom range, as a multiplier over "fit". `1.0` is fit; this is generous
/// enough for a close read of a dense diagram or a wide view of a small one,
/// without letting a stray scroll send it somewhere the user has to hunt for
/// the reset button to escape.
const MIN_ZOOM: f32 = 0.1;
const MAX_ZOOM: f32 = 8.0;

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

/// The three ways to lay out the editor and the preview.
#[derive(Clone, Copy, PartialEq, Eq)]
enum WorkspaceMode {
    Editor,
    Split,
    Preview,
}

/// The "+" button's fixed template set (workspace plan phase 6). Small and
/// deliberately not extensible from the UI — the plan's own words are
/// "discoverability, not a giant template marketplace" — so this is a plain
/// enum over a `const` example each, not a registry.
#[derive(Clone, Copy, PartialEq, Eq)]
enum MermaidTemplate {
    Blank,
    Flowchart,
    Sequence,
    Class,
    State,
    Er,
    Architecture,
}

impl MermaidTemplate {
    const ALL: [MermaidTemplate; 7] = [
        MermaidTemplate::Blank,
        MermaidTemplate::Flowchart,
        MermaidTemplate::Sequence,
        MermaidTemplate::Class,
        MermaidTemplate::State,
        MermaidTemplate::Er,
        MermaidTemplate::Architecture,
    ];

    fn label(self) -> mermaid::Text {
        match self {
            MermaidTemplate::Blank => mermaid::Text::TemplateBlank,
            MermaidTemplate::Flowchart => mermaid::Text::TemplateFlowchart,
            MermaidTemplate::Sequence => mermaid::Text::TemplateSequence,
            MermaidTemplate::Class => mermaid::Text::TemplateClass,
            MermaidTemplate::State => mermaid::Text::TemplateState,
            MermaidTemplate::Er => mermaid::Text::TemplateEr,
            MermaidTemplate::Architecture => mermaid::Text::TemplateArchitecture,
        }
    }

    /// The example source the template inserts. Each was checked against the
    /// real renderer during development (`mermaid-rs-renderer` 0.3.1) —
    /// opening a template that immediately shows a syntax error would be
    /// worse than no templates at all.
    fn source(self) -> &'static str {
        match self {
            MermaidTemplate::Blank => "",
            MermaidTemplate::Flowchart => {
                "flowchart LR\n  A[Start] --> B{Decision}\n  B -->|Yes| C[Do it]\n  B -->|No| D[Skip]\n"
            }
            MermaidTemplate::Sequence => {
                "sequenceDiagram\n  participant Alice\n  participant Bob\n  Alice->>Bob: Hello Bob\n  Bob-->>Alice: Hi Alice\n"
            }
            MermaidTemplate::Class => {
                "classDiagram\n  Animal <|-- Duck\n  Animal : +String name\n  Animal : +makeSound()\n"
            }
            MermaidTemplate::State => {
                "stateDiagram-v2\n  [*] --> Idle\n  Idle --> Running : start\n  Running --> Idle : stop\n"
            }
            MermaidTemplate::Er => {
                "erDiagram\n  CUSTOMER ||--o{ ORDER : places\n  ORDER ||--|{ LINE_ITEM : contains\n"
            }
            MermaidTemplate::Architecture => {
                "architecture-beta\n  group api(cloud)[API]\n  service db(database)[Database] in api\n  service server(server)[Server] in api\n  server:R -- L:db\n"
            }
        }
    }
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
    /// Whether the "+" button's template menu is open.
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
        view.open_tab(String::new(), window, cx);
        view
    }

    /// Opens a new tab with `source` already in the editor, and selects it.
    /// Used both by the "+" button and — from the day the clipboard detector
    /// lands (workspace plan phase 5) — by a recognised paste.
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

    fn close_tab(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(index) = self.tabs.iter().position(|tab| tab.id == id) else {
            return;
        };
        // At least one tab always exists — closing the last one replaces it
        // with a fresh blank tab rather than leaving an empty workspace with
        // no editor to type into.
        if self.tabs.len() == 1 {
            self.tabs.remove(index);
            cx.notify();
            return;
        }
        self.tabs.remove(index);
        if self.active >= self.tabs.len() {
            self.active = self.tabs.len() - 1;
        } else if self.active > index {
            self.active -= 1;
        }
        cx.notify();
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

    fn zoom_in(&mut self, cx: &mut Context<Self>) {
        if let Some(tab) = self.active_tab_mut() {
            tab.zoom = (tab.zoom * ZOOM_STEP).min(MAX_ZOOM);
            cx.notify();
        }
    }

    fn zoom_out(&mut self, cx: &mut Context<Self>) {
        if let Some(tab) = self.active_tab_mut() {
            tab.zoom = (tab.zoom / ZOOM_STEP).max(MIN_ZOOM);
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
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.close_tab(id, cx);
                            })),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(position) = this.tabs.iter().position(|tab| tab.id == id) {
                            this.active = position;
                            cx.notify();
                        }
                    }))
            }))
            .child(self.render_template_menu(cx))
    }

    /// The "+" button's template menu: [`MermaidTemplate::ALL`], each a plain
    /// row that opens a new tab with that template's source and closes the
    /// menu. A hand-rolled list inside a `Popover`, the same shape
    /// `dodo-api-explorer`'s per-row node menu uses — this library revision
    /// has no separate "popup menu" type worth reaching for over it.
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
                        this.open_tab(template.source().to_owned(), window, cx);
                    })),
            );
        }

        Popover::new("mermaid-new-tab-menu")
            .open(self.template_menu_open)
            .on_open_change(cx.listener(|this, open, _, cx| {
                this.template_menu_open = *open;
                cx.notify();
            }))
            .trigger(
                Button::new("mermaid-new-tab")
                    .ghost()
                    .xsmall()
                    .icon(AppIcon::Plus)
                    .tooltip(t(mermaid::Text::NewTabTooltip, cx)),
            )
            .w(px(140.))
            .child(menu)
    }

    fn render_mode_toggle(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let editor = self.mode_button(WorkspaceMode::Editor, mermaid::Text::ModeEditor, cx);
        let split = self.mode_button(WorkspaceMode::Split, mermaid::Text::ModeSplit, cx);
        let preview = self.mode_button(WorkspaceMode::Preview, mermaid::Text::ModePreview, cx);
        h_flex()
            .items_center()
            .gap_1()
            .child(editor)
            .child(split)
            .child(preview)
    }

    fn mode_button(
        &self,
        mode: WorkspaceMode,
        label: mermaid::Text,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let id = match mode {
            WorkspaceMode::Editor => "mermaid-mode-editor",
            WorkspaceMode::Split => "mermaid-mode-split",
            WorkspaceMode::Preview => "mermaid-mode-preview",
        };
        let button = Button::new(id).small().label(t(label, cx));
        let button = if self.mode == mode {
            button.primary()
        } else {
            button.ghost()
        };
        button.on_click(cx.listener(move |this, _, _, cx| {
            this.mode = mode;
            cx.notify();
        }))
    }

    fn render_editor(&self, tab: &MermaidTab, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        div()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(cx.theme().border)
            .child(
                Input::new(&tab.editor)
                    .font_family(cx.theme().mono_font_family.clone())
                    .text_size(cx.theme().mono_font_size)
                    .size_full(),
            )
    }

    /// Paints the active tab's rasterised diagram at "fit × zoom", offset by
    /// `pan`, and installs the drag-to-pan mouse handlers over it.
    ///
    /// A `canvas()` rather than `img()`, because centring "fit times a zoom
    /// the user controls" needs the container's actual pixel bounds at paint
    /// time — `img()`'s own `ObjectFit::Contain` only ever fits to `1.0`, with
    /// no way to ask it for a different multiple. Nothing here parses or lays
    /// out Mermaid: [`paint_preview_image`] only ever repaints the bitmap
    /// [`MermaidView::schedule_render`] already produced, at whatever bounds
    /// this frame's zoom and pan say — the requirement the workspace plan's
    /// phase 4 states by name.
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

        match tab.rendered_image.clone() {
            Some(image) => {
                let zoom = tab.zoom;
                let pan = tab.pan;
                let view = cx.entity();
                frame.child(
                    canvas(
                        |bounds, window, _cx| window.insert_hitbox(bounds, HitboxBehavior::Normal),
                        move |bounds, hitbox, window, _cx| {
                            paint_preview_image(&image, zoom, pan, bounds, window);
                            install_preview_drag(view.clone(), &hitbox, window);
                        },
                    )
                    .absolute()
                    .size_full(),
                )
            }
            None => frame.flex().items_center().justify_center().child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(t(mermaid::Text::EmptyPreviewHint, cx)),
            ),
        }
    }

    fn render_status_bar(
        &self,
        tab: &MermaidTab,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        h_flex()
            .items_center()
            .justify_between()
            .text_sm()
            .child(
                h_flex()
                    .items_center()
                    .gap_2()
                    .child(t(mermaid::Text::StatusLabel, cx))
                    .when(tab.show_spinner, |this| {
                        this.child(
                            div()
                                .text_color(cx.theme().muted_foreground)
                                .child(t(mermaid::Text::Rendering, cx)),
                        )
                    })
                    .when_some(tab.render_error.clone(), |this, detail| {
                        this.child(
                            div()
                                .text_color(cx.theme().danger)
                                .child(t(mermaid::Text::RenderError { detail }, cx)),
                        )
                    }),
            )
            .child(self.render_zoom_controls(cx))
    }

    fn render_zoom_controls(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        h_flex()
            .items_center()
            .gap_1()
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
                    .icon(AppIcon::Copy)
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
                    .icon(AppIcon::Save)
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
                .flex_1()
                .min_h_0()
                .gap_2()
                .when(
                    matches!(self.mode, WorkspaceMode::Editor | WorkspaceMode::Split),
                    |this| this.child(self.render_editor(active, cx)),
                )
                .when(
                    matches!(self.mode, WorkspaceMode::Split | WorkspaceMode::Preview),
                    |this| this.child(self.render_preview(cx)),
                ),
        )
        .child(self.render_status_bar(active, cx))
    }
}

fn hash_source(source: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    source.hash(&mut hasher);
    hasher.finish()
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

/// Registers this frame's drag-to-pan listeners over the preview's hitbox.
/// Mirrors `dodo-flow`'s canvas input pattern (`views/flow.rs`): listeners are
/// registered from inside the paint closure and last exactly one frame, and
/// `capture_pointer` is what keeps the drag alive once the pointer leaves the
/// preview pane — there is nothing to release, it clears on mouse up.
fn install_preview_drag(view: Entity<MermaidView>, hitbox: &Hitbox, window: &mut Window) {
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
}
