//! The read-only detail surface the four pages share: one **modal dialog** with
//! an Inspect tab for every resource type and a Logs tab for containers.
//!
//! # One dialog, opened by clicking a row's name
//!
//! There is no per-row eye or logs icon any more. The row's identifying cell —
//! Name on Containers, Networks and Volumes, Repository on Images — is the click
//! target (see [`widgets::name_cell`](super::widgets::name_cell)), the row's
//! activate key (`enter`) opens the same thing, and the right-click menus still
//! offer Inspect and View Logs. All of those routes land here.
//!
//! # Why this is a `window.open_dialog`, and not an overlay in the page
//!
//! It used to be a plain struct a page *owned*, rendering a scrim
//! `div().absolute().inset_0()` into the page's own element tree. That could not
//! be modal, for two independent reasons:
//!
//! 1. **A scrim is not a barrier.** gpui dispatches a mouse event to every
//!    hitbox under the cursor, topmost first, and keeps going unless a listener
//!    calls `cx.stop_propagation()` or the element sets
//!    `HitboxBehavior::BlockMouse` (`.occlude()`). The old scrim did neither — it
//!    carried an empty `on_mouse_down` closure, which registers a listener and
//!    swallows nothing — so clicks and hovers reached the row buttons behind it,
//!    Delete included.
//! 2. **`inset_0` is only the page.** The scrim was positioned against the
//!    page's root, so it never covered the in-page tab rail or the app sidebar
//!    whatever its hit behaviour.
//!
//! `gpui_component`'s [`Dialog`](gpui_component::dialog::Dialog) already solves
//! all of it, and `settings::open` is the in-repo precedent: a window-sized
//! `anchored().snap_to_window()` layer that is `.occlude()`d, a
//! `cx.stop_propagation()` on the backdrop that also closes on a left click,
//! `escape` bound to `CancelDialog` in the `Dialog` key context, a `focus_trap`
//! around the card, and `Root::close_dialog` restoring the previously focused
//! handle — which is the page's list handle, so the triggering row gets focus
//! back. Following it means dodo has one modality mechanism rather than two.
//!
//! The trade the old comment here worried about — that a dialog layer does not
//! re-paint on the page's `cx.notify()` — is why [`DetailView`] is an entity
//! rather than a struct: the dialog's body is the entity, so *its* `cx.notify()`
//! paints it. That also makes background polling harmless, since a poll notifies
//! the page and never touches this.
//!
//! # Two tabs, fetched on demand
//!
//! [`DetailTabs`] holds one slot per tab, `None` until that tab is first shown.
//! Switching tabs fetches only an unfetched slot, so inspecting a container does
//! not pull its logs and flicking between tabs hits the engine once each. The
//! rules are plain data and unit tested in
//! [`state::detail`](crate::docker::state::detail); this module is the tasks and
//! the pixels.
//!
//! # Read-only
//!
//! Nothing here writes to the engine. The raw-JSON pane is the same [`Input`]
//! code editor the API Explorer renders a response body in — the buffer is
//! editable in the widget (that is how the editor works) but is rebuilt from the
//! engine on every open and refresh, so an edit is discarded and never travels
//! anywhere.
//!
//! # Where the rest plugs in
//!
//! An Exec/terminal session is the same shape as Logs — a surface over a stream —
//! but needs a *writable* bidirectional stream and a PTY, which is why it is
//! still a stub. It would arrive as a third [`DetailTab`]. Live log following
//! would arrive as a second mode on this view's task: keep the stream open and
//! push each frame through
//! [`lines_from_frames`](crate::docker::models::logs::lines_from_frames) instead
//! of collecting it.

use std::sync::Arc;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, FocusHandle, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, SharedString, StatefulInteractiveElement as _,
    Styled as _, Task, Window, div, px,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Input, InputState};
use gpui_component::tab::{Tab, TabBar};
use gpui_component::{
    ActiveTheme as _, Icon, Sizable as _, StyledExt as _, WindowExt as _, h_flex, v_flex,
};

use crate::app_icon::AppIcon;
use crate::docker::components::skeleton::loading_skeleton;
use crate::docker::components::states::{empty_state, error_state};
use crate::docker::models::inspect::{FieldValue, InspectDetail, InspectKind};
use crate::docker::models::logs::{LOG_TAIL_LIMIT, LogLine, LogStream};
use crate::docker::services::DockerEngine;
use crate::docker::state::detail::{DetailStatus, DetailTab, DetailTabs};
use crate::i18n::{Str, t};

/// The dialog card's preferred width, and the preferred height of the body under
/// the title. The dialog sizes to its content, so the body's height is what keeps
/// the card stable while a tab loads: wide enough for a JSON line, tall enough
/// that a log tail is worth reading, and not so tall that it fills a short window.
///
/// Both are *preferred*: [`card_size`] shrinks them to fit a narrow or short
/// window. [`Dialog`] centres the card by computing `left` from the width it was
/// given, so an over-wide card is not merely clipped — it is pushed off-centre and
/// off both edges. Sizing has to happen before the dialog is built, not by
/// clamping afterwards.
///
/// [`Dialog`]: gpui_component::dialog::Dialog
const PANEL_W: gpui::Pixels = px(760.);
const PANEL_H: gpui::Pixels = px(520.);
/// Margin left around the card at a window too small for the preferred size.
const PANEL_MARGIN: gpui::Pixels = px(24.);
/// [`Dialog`]'s own left and right padding (`Edges::all(16)`, which this dialog
/// does not override), subtracted to get the body's width from the card's.
///
/// The body's width is *stated* rather than `w_full` on purpose. A percentage
/// width only resolves against an ancestor with a definite width, and inside the
/// dialog's nested wrappers it resolved to `auto` — which content-sized the
/// section headings, the field rules and the JSON editor to the widest field value
/// instead of the card.
///
/// [`Dialog`]: gpui_component::dialog::Dialog
const DIALOG_PADDING_X: gpui::Pixels = px(32.);
/// The width of the field-label column in the Details list.
const LABEL_W: gpui::Pixels = px(150.);
/// Right padding on the title row, clearing the [`Dialog`]'s own close button —
/// which is absolutely positioned in the card's top-right corner.
///
/// [`Dialog`]: gpui_component::dialog::Dialog
const TITLE_CLEARANCE: gpui::Pixels = px(24.);

/// Which resource a detail dialog opens on, and which tab it starts on.
///
/// Bundled rather than passed as five loose arguments to [`open`]: the four
/// pages all build one of these, and the two constructors are the only places
/// that decide a starting tab.
pub struct DetailRequest {
    kind: InspectKind,
    /// The id the fetch and every reload target — a name, for a volume.
    id: String,
    /// The row's own name, shown in the title. An engine-reported name replaces
    /// it once the inspect arrives.
    title: String,
    tab: DetailTab,
}

impl DetailRequest {
    /// A dialog opening on Inspect: clicking any row's name, and the Inspect
    /// item in every context menu.
    pub fn inspect(kind: InspectKind, id: String, title: String) -> Self {
        Self {
            kind,
            id,
            title,
            tab: DetailTab::DEFAULT,
        }
    }

    /// A container dialog opening straight on Logs: the View Logs context-menu
    /// item. The same dialog as [`DetailRequest::inspect`], one tab over.
    pub fn logs(id: String, title: String) -> Self {
        Self {
            kind: InspectKind::Container,
            id,
            title,
            tab: DetailTab::Logs,
        }
    }
}

/// Opens the detail dialog on one resource and starts its first tab's fetch.
///
/// `restore` is the page's list focus handle. `Root::close_dialog` already
/// re-focuses whatever was focused when the dialog opened, which is that same
/// handle; passing it explicitly makes the return deterministic for the close
/// paths that run their `on_close` after the restore, so the keyboard-highlighted
/// row is live again the moment the dialog goes away.
pub fn open(
    engine: Arc<dyn DockerEngine>,
    request: DetailRequest,
    restore: FocusHandle,
    window: &mut Window,
    cx: &mut App,
) {
    let view = cx.new(|cx| DetailView::new(engine, request, window, cx));

    window.open_dialog(cx, move |dialog, window, cx| {
        let view = view.clone();
        let restore = restore.clone();
        let (card_w, body_h) = card_size(window);
        dialog
            .w(card_w)
            .title(render_title(&view, cx))
            .on_close(move |_, window, cx| restore.focus(window, cx))
            // `content`, not `child`: `Dialog`'s plain children are wrapped in an
            // `overflow_y_scrollbar` box, and a scroll container takes its width
            // from its content — which left every `w_full` inside the body (the
            // section headings, the field rules, the JSON editor) collapsed to the
            // widest field value. `DialogContent` is a plain `w_full().flex_1()`.
            .content(move |content, _, _| {
                content.child(
                    div()
                        .w(card_w - DIALOG_PADDING_X)
                        .h(body_h)
                        .child(view.clone()),
                )
            })
    });
}

/// The card width and body height to use in this window: the preferred
/// [`PANEL_W`] / [`PANEL_H`], shrunk to leave [`PANEL_MARGIN`] around the card
/// when the window is smaller than that.
///
/// The height allowance is generous because `Dialog` places the card a tenth of
/// the viewport down and adds its own vertical padding and title row around the
/// body; `PANEL_MARGIN * 4` keeps the bottom edge inside a short window.
fn card_size(window: &Window) -> (gpui::Pixels, gpui::Pixels) {
    let viewport = window.viewport_size();
    let width = PANEL_W.min(viewport.width - PANEL_MARGIN * 2.);
    let height = PANEL_H.min(viewport.height - PANEL_MARGIN * 4.);
    (width, height)
}

/// The dialog's title row: the resource's kind icon, its name, and Refresh.
///
/// The name alone titles the dialog for all four kinds. Naming the *surface*
/// here as well would either duplicate the tab strip (a container's "Inspect"
/// title over its Logs tab) or need a conditional; the tab strip says it for a
/// container and the body's own "Details" heading says it for the other three.
fn render_title(view: &Entity<DetailView>, cx: &App) -> AnyElement {
    let detail = view.read(cx);
    let icon = kind_icon(detail.kind);
    let name = SharedString::from(detail.title.clone());
    let refresh = view.clone();

    h_flex()
        .w_full()
        .items_center()
        .gap_2()
        // Leave the card's absolutely-positioned close button its corner.
        .pr(TITLE_CLEARANCE)
        .child(
            Icon::new(icon)
                .size(px(14.))
                .text_color(cx.theme().muted_foreground),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_sm()
                .font_family(cx.theme().mono_font_family.clone())
                .child(name),
        )
        .child(
            Button::new("docker-detail-refresh")
                .xsmall()
                .ghost()
                .icon(AppIcon::Refresh)
                .tooltip(t(Str::DockerRefresh, cx))
                .on_click(move |_, window, cx| {
                    refresh.update(cx, |this, cx| this.reload(window, cx));
                }),
        )
        .into_any_element()
}

/// The icon that stands for a resource kind. The same four the Docker tab rail
/// uses for its pages, so a dialog is recognisably about a container, an image,
/// a volume or a network.
fn kind_icon(kind: InspectKind) -> AppIcon {
    match kind {
        InspectKind::Container => AppIcon::Container,
        InspectKind::Image => AppIcon::Layers,
        InspectKind::Volume => AppIcon::HardDrive,
        InspectKind::Network => AppIcon::Network,
    }
}

/// The dialog's body: the tab strip (containers only) over the active tab.
pub struct DetailView {
    engine: Arc<dyn DockerEngine>,
    kind: InspectKind,
    id: String,
    title: String,
    tabs: DetailTabs<Box<InspectDetail>, Vec<LogLine>>,
    /// The raw-JSON pane, a JSON code editor so the response is highlighted the
    /// same way the API Explorer highlights a JSON body.
    json: Entity<InputState>,
    /// One in-flight fetch per tab, so switching away from a loading tab does not
    /// cancel it — a single slot would drop the Inspect task on the way to Logs
    /// and leave Inspect stuck on its skeleton, since its slot is already filled
    /// and so never refetched.
    inspect_task: Option<Task<()>>,
    logs_task: Option<Task<()>>,
}

impl DetailView {
    fn new(
        engine: Arc<dyn DockerEngine>,
        request: DetailRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let json = cx.new(|cx| {
            InputState::new(window, cx)
                .code_editor("json")
                .soft_wrap(false)
        });
        let mut this = Self {
            engine,
            kind: request.kind,
            id: request.id,
            title: request.title,
            tabs: DetailTabs::new(request.tab),
            json,
            inspect_task: None,
            logs_task: None,
        };
        this.load(request.tab, window, cx);
        this
    }

    /// Shows `tab`, fetching it only if it has never been fetched.
    fn set_tab(&mut self, tab: DetailTab, window: &mut Window, cx: &mut Context<Self>) {
        if self.tabs.activate(tab) {
            self.load(tab, window, cx);
        }
        cx.notify();
    }

    /// Re-fetches the tab on screen — the title row's Refresh, and the Retry a
    /// failed tab offers. Only the active tab, so refreshing Inspect never
    /// discards logs already read.
    fn reload(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.load(self.tabs.active(), window, cx);
    }

    /// The one fetch path: the engine call runs on the background executor and
    /// the result lands back on the UI thread through this entity. Nothing has to
    /// be re-checked on arrival the way the old page-owned panel did — a dialog
    /// closed while its fetch was in flight drops the entity, and with it the
    /// task, so a late result has nowhere to go.
    fn load(&mut self, tab: DetailTab, window: &mut Window, cx: &mut Context<Self>) {
        self.tabs.begin_load(tab);
        cx.notify();

        let engine = self.engine.clone();
        let kind = self.kind;
        let id = self.id.clone();

        let task = cx.spawn_in(window, async move |this, cx| match tab {
            DetailTab::Inspect => {
                let result = cx
                    .background_executor()
                    .spawn(async move {
                        match kind {
                            InspectKind::Container => engine.inspect_container(&id),
                            InspectKind::Image => engine.inspect_image(&id),
                            InspectKind::Volume => engine.inspect_volume(&id),
                            InspectKind::Network => engine.inspect_network(&id),
                        }
                    })
                    .await;
                let _ = this.update_in(cx, |view, window, cx| {
                    match result {
                        Ok(detail) => view.install_inspect(Box::new(detail), window, cx),
                        Err(error) => view.tabs.set_inspect(DetailStatus::Failed(error.message())),
                    }
                    cx.notify();
                });
            }
            DetailTab::Logs => {
                let result = cx
                    .background_executor()
                    .spawn(async move { engine.container_logs(&id, LOG_TAIL_LIMIT) })
                    .await;
                let _ = this.update(cx, |view, cx| {
                    match result {
                        Ok(lines) => view.tabs.set_logs(DetailStatus::Ready(lines)),
                        Err(error) => view.tabs.set_logs(DetailStatus::Failed(error.message())),
                    }
                    cx.notify();
                });
            }
        });

        match tab {
            DetailTab::Inspect => self.inspect_task = Some(task),
            DetailTab::Logs => self.logs_task = Some(task),
        }
    }

    /// Installs a loaded inspect: the engine's name replaces the row's where it
    /// has one, and the JSON is pushed into the code editor.
    fn install_inspect(
        &mut self,
        detail: Box<InspectDetail>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let json = detail.json.clone();
        self.json.update(cx, |state, cx| {
            state.set_value(json, window, cx);
        });
        if !detail.title.is_empty() {
            self.title = detail.title.clone();
        }
        self.tabs.set_inspect(DetailStatus::Ready(detail));
    }

    // ---- Rendering -----------------------------------------------------------

    /// The tab strip, or `None` for a resource with only one surface: a one-tab
    /// strip reads as a mistake, so Images, Volumes and Networks get none.
    fn render_tabs(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let tabs = DetailTab::all_for(self.kind);
        if tabs.len() < 2 {
            return None;
        }
        let active = self.tabs.active();
        let selected = tabs.iter().position(|tab| *tab == active).unwrap_or(0);

        Some(
            TabBar::new("docker-detail-tabs")
                .underline()
                .small()
                .selected_index(selected)
                .children(tabs.iter().map(|tab| Tab::new().label(t(tab.label(), cx))))
                .on_click(cx.listener(move |this, ix: &usize, window, cx| {
                    if let Some(tab) = DetailTab::all_for(this.kind).get(*ix).copied() {
                        this.set_tab(tab, window, cx);
                    }
                }))
                .into_any_element(),
        )
    }

    fn render_body(&self, cx: &App) -> AnyElement {
        match self.tabs.active() {
            DetailTab::Inspect => match self.tabs.inspect() {
                Some(DetailStatus::Ready(detail)) => self.render_inspect(detail, cx),
                Some(DetailStatus::Failed(error)) => detail_error(error.clone(), cx),
                // `None` cannot survive a render — `load` fills the slot before
                // notifying — but it is the same "nothing yet" as Loading.
                Some(DetailStatus::Loading) | None => loading_skeleton(6, cx).into_any_element(),
            },
            DetailTab::Logs => match self.tabs.logs() {
                Some(DetailStatus::Ready(lines)) => self.render_logs(lines, cx),
                Some(DetailStatus::Failed(error)) => detail_error(error.clone(), cx),
                Some(DetailStatus::Loading) | None => loading_skeleton(6, cx).into_any_element(),
            },
        }
    }

    /// The key fields, then the engine's whole response in the JSON editor.
    fn render_inspect(&self, detail: &InspectDetail, cx: &App) -> AnyElement {
        let mut rows: Vec<AnyElement> = Vec::new();
        for field in &detail.fields {
            let value = match &field.value {
                FieldValue::Text(text) => SharedString::from(text.clone()),
                FieldValue::Flag(true) => t(Str::DockerYes, cx),
                FieldValue::Flag(false) => t(Str::DockerNo, cx),
                FieldValue::Missing => t(Str::DockerNotAvailable, cx),
            };
            let missing = matches!(field.value, FieldValue::Missing);
            rows.push(
                h_flex()
                    .w_full()
                    .items_start()
                    .gap_3()
                    .px_3()
                    .py_1p5()
                    .border_b_1()
                    .border_color(cx.theme().border.opacity(0.5))
                    .text_xs()
                    .child(
                        div()
                            .w(LABEL_W)
                            .flex_shrink_0()
                            .text_color(cx.theme().muted_foreground)
                            .child(t(field.label.clone(), cx)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .font_family(cx.theme().mono_font_family.clone())
                            .when(missing, |this| this.text_color(cx.theme().muted_foreground))
                            .child(value),
                    )
                    .into_any_element(),
            );
        }

        v_flex()
            .size_full()
            .child(section_title(t(Str::DockerDetails, cx), cx))
            .child(
                // `w_full` on every scroll box: a scroll container sizes to its
                // content otherwise, which leaves the field rows and the section
                // rules stopping short of the card's edge.
                div()
                    .id("docker-detail-fields")
                    .w_full()
                    .max_h(px(240.))
                    .flex_shrink_0()
                    .overflow_y_scroll()
                    .child(v_flex().w_full().children(rows)),
            )
            .child(section_title(t(Str::DockerRawJson, cx), cx))
            .child(
                div().w_full().flex_1().min_h_0().child(
                    Input::new(&self.json)
                        .font_family(cx.theme().mono_font_family.clone())
                        .text_size(cx.theme().mono_font_size)
                        .size_full(),
                ),
            )
            .into_any_element()
    }

    /// The bounded tail, monospaced, stderr in the danger tone. Plain elements
    /// rather than an editor: it is output to read, not text to edit, and the
    /// per-stream colouring needs one element per line anyway.
    fn render_logs(&self, lines: &[LogLine], cx: &App) -> AnyElement {
        if lines.is_empty() {
            return empty_state(
                AppIcon::File,
                t(Str::DockerNoLogs, cx),
                Some(t(Str::DockerNoLogsHint, cx)),
                cx,
            )
            .into_any_element();
        }

        let rendered: Vec<AnyElement> = lines
            .iter()
            .map(|line| {
                let color = match line.stream {
                    LogStream::Stdout => cx.theme().foreground,
                    LogStream::Stderr => cx.theme().danger,
                };
                div()
                    .w_full()
                    .px_3()
                    .py(px(1.))
                    .text_color(color)
                    // A log line is not prose: it keeps its own spacing, and a
                    // long one scrolls rather than reflowing.
                    .whitespace_nowrap()
                    .child(SharedString::from(line.text.clone()))
                    .into_any_element()
            })
            .collect();

        v_flex()
            .size_full()
            .child(
                div()
                    .id("docker-logs-scroll")
                    .w_full()
                    .flex_1()
                    .min_h_0()
                    .overflow_scroll()
                    .py_2()
                    .text_xs()
                    .font_family(cx.theme().mono_font_family.clone())
                    .child(v_flex().w_full().children(rendered)),
            )
            .child(
                // The window is stated, so a bounded view is never a silent one.
                h_flex()
                    .w_full()
                    .flex_shrink_0()
                    .px_3()
                    .py_1()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(t(Str::DockerLogsTail(LOG_TAIL_LIMIT), cx)),
            )
            .into_any_element()
    }
}

impl Render for DetailView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tabs = self.render_tabs(cx);
        v_flex()
            .size_full()
            .overflow_hidden()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(cx.theme().border)
            .children(tabs)
            .child(
                div()
                    .w_full()
                    .flex_1()
                    .min_h_0()
                    .child(self.render_body(cx)),
            )
    }
}

/// A failed tab: the frame plus the engine's own message, with nothing stale
/// behind it.
fn detail_error(error: Str, cx: &App) -> AnyElement {
    error_state(t(Str::DockerDetailErrorTitle, cx), t(error, cx), cx).into_any_element()
}

/// A small heading over one section of the body.
fn section_title(label: SharedString, cx: &App) -> impl IntoElement {
    div()
        .w_full()
        .flex_shrink_0()
        .px_3()
        .py_1p5()
        .bg(cx.theme().muted.opacity(0.3))
        .border_b_1()
        .border_color(cx.theme().border)
        .text_xs()
        .font_medium()
        .text_color(cx.theme().muted_foreground)
        .child(label)
}

#[cfg(test)]
mod tests {
    // Deliberately not `use super::*`: that pulls in `use gpui::…`, whose `test`
    // re-export shadows the standard attribute. See the dodo-build-validate skill.
    use super::{DetailRequest, kind_icon};
    use crate::docker::models::inspect::InspectKind;
    use crate::docker::state::detail::DetailTab;

    #[test]
    fn a_name_click_opens_inspect_and_the_logs_item_opens_logs() {
        // The default tab is not a rendering detail: it is what every route
        // except the View Logs menu item asks for.
        for kind in [
            InspectKind::Container,
            InspectKind::Image,
            InspectKind::Volume,
            InspectKind::Network,
        ] {
            let request = DetailRequest::inspect(kind, "id".into(), "name".into());
            assert_eq!(request.tab, DetailTab::Inspect);
            assert_eq!(request.kind, kind);
        }

        let request = DetailRequest::logs("id".into(), "name".into());
        assert_eq!(request.tab, DetailTab::Logs);
        // Only a container has logs, so the request hard-codes the kind rather
        // than trusting a caller with it.
        assert_eq!(request.kind, InspectKind::Container);
    }

    #[test]
    fn every_kind_has_its_own_icon() {
        let mut paths: Vec<String> = [
            InspectKind::Container,
            InspectKind::Image,
            InspectKind::Volume,
            InspectKind::Network,
        ]
        .iter()
        .map(|kind| gpui_component::IconNamed::path(kind_icon(*kind)).to_string())
        .collect();
        paths.sort_unstable();
        paths.dedup();
        assert_eq!(paths.len(), 4);
    }
}
