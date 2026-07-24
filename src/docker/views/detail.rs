//! The read-only detail surface the four pages share: the Inspect panel and the
//! container Logs viewer.
//!
//! # One panel, four pages
//!
//! Inspect exists for all four resource types and Logs for containers, so the
//! panel is a plain struct a page *owns* (like its search input) rather than a
//! view of its own. It renders as an overlay inside the page's element tree,
//! which is what makes it re-paint on the page's own `cx.notify()` — a
//! `window.open_dialog` layer would not, since nothing there observes the page
//! entity.
//!
//! Because it is owned rather than an entity, its background fetch has to get
//! back to `&mut Self` through the *page's* `Context<V>`. That is the `access`
//! function pointer every open/reload takes: the page passes something like
//! `ContainersView::detail_mut`, and the task's `update_in` uses it to find the
//! panel again. One implementation, four call sites, no duplication.
//!
//! # Read-only
//!
//! Nothing here writes to the engine. The raw-JSON pane is the same
//! [`Input`] code editor the API Explorer renders a response body in — the
//! buffer is editable in the widget (that is how the editor works) but is
//! rebuilt from the engine on every open and refresh, so an edit is discarded
//! and never travels anywhere.
//!
//! # Where the rest plugs in
//!
//! An Exec/terminal session is the same shape as Logs — an open panel over a
//! stream — but needs a *writable* bidirectional stream and a PTY, which is why
//! it is still a stub. Live log following would arrive as a second mode on this
//! panel's task: keep the stream open and push each frame through
//! [`lines_from_frames`](crate::docker::models::logs::lines_from_frames) instead
//! of collecting it.

use std::sync::Arc;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, AppContext as _, ClickEvent, Context, Entity, InteractiveElement as _,
    IntoElement, MouseButton, ParentElement as _, SharedString,
    StatefulInteractiveElement as _, Styled as _, Task, Window, div, px,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Input, InputState};
use gpui_component::{
    ActiveTheme as _, Icon, Sizable as _, StyledExt as _, h_flex, v_flex,
};

use crate::app_icon::AppIcon;
use crate::docker::components::skeleton::loading_skeleton;
use crate::docker::components::states::{empty_state, error_state};
use crate::docker::models::inspect::{FieldValue, InspectDetail, InspectKind};
use crate::docker::models::logs::{LOG_TAIL_LIMIT, LogLine, LogStream};
use crate::docker::services::DockerEngine;
use crate::docker::state::detail::DetailStatus;
use crate::i18n::{Str, t};

/// The overlay card's size. It is a panel over the list, not a full-screen mode:
/// wide enough for a JSON line, capped so the table stays visible around it.
const PANEL_W: gpui::Pixels = px(760.);
const PANEL_H: gpui::Pixels = px(560.);
/// The width of the field-label column in the Details list.
const LABEL_W: gpui::Pixels = px(150.);

/// Which detail surface is open.
#[derive(Clone, Copy, PartialEq, Eq)]
enum DetailMode {
    Inspect(InspectKind),
    Logs,
}

/// What a loaded panel holds.
enum DetailContent {
    Inspect(Box<InspectDetail>),
    Logs(Vec<LogLine>),
}

/// The resource a panel is open on, and how its fetch is going.
struct OpenDetail {
    mode: DetailMode,
    /// The id (or name, for a volume) the fetch and every reload target.
    id: String,
    /// The row's own name, shown beside the title. An engine-reported name
    /// replaces it once the detail arrives.
    title: String,
    status: DetailStatus<DetailContent>,
}

/// A page's read-only detail overlay: closed, or open on one resource.
pub struct DetailPanel {
    open: Option<OpenDetail>,
    /// The raw-JSON pane, a JSON code editor so the response is highlighted the
    /// same way the API Explorer highlights a JSON body.
    json: Entity<InputState>,
    /// The in-flight fetch; held so opening something else replaces (cancels) it.
    task: Option<Task<()>>,
}

impl DetailPanel {
    pub fn new(window: &mut Window, cx: &mut App) -> Self {
        let json = cx.new(|cx| {
            InputState::new(window, cx)
                .code_editor("json")
                .soft_wrap(false)
        });
        Self {
            open: None,
            json,
            task: None,
        }
    }

    /// Whether the overlay is showing. Pages use it to suppress row navigation
    /// while the panel has the screen.
    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    pub fn close(&mut self) {
        self.open = None;
        self.task = None;
    }

    /// Opens the Inspect panel on one resource and starts its fetch.
    pub fn open_inspect<V: 'static>(
        &mut self,
        engine: Arc<dyn DockerEngine>,
        kind: InspectKind,
        id: String,
        title: String,
        access: fn(&mut V) -> &mut Self,
        window: &mut Window,
        cx: &mut Context<V>,
    ) {
        self.open = Some(OpenDetail {
            mode: DetailMode::Inspect(kind),
            id,
            title,
            status: DetailStatus::Loading,
        });
        self.load(engine, access, window, cx);
    }

    /// Opens the Logs viewer on one container and starts its fetch.
    pub fn open_logs<V: 'static>(
        &mut self,
        engine: Arc<dyn DockerEngine>,
        id: String,
        title: String,
        access: fn(&mut V) -> &mut Self,
        window: &mut Window,
        cx: &mut Context<V>,
    ) {
        self.open = Some(OpenDetail {
            mode: DetailMode::Logs,
            id,
            title,
            status: DetailStatus::Loading,
        });
        self.load(engine, access, window, cx);
    }

    /// Re-fetches whatever is open — the panel's Refresh, and its Retry after a
    /// failure. A no-op when nothing is open.
    pub fn reload<V: 'static>(
        &mut self,
        engine: Arc<dyn DockerEngine>,
        access: fn(&mut V) -> &mut Self,
        window: &mut Window,
        cx: &mut Context<V>,
    ) {
        let Some(open) = self.open.as_mut() else {
            return;
        };
        open.status = DetailStatus::Loading;
        self.load(engine, access, window, cx);
    }

    /// The one fetch path: the engine call runs on the background executor, and
    /// the result lands back on the UI thread through the page's entity. The
    /// panel can have been closed, or pointed at something else, by then — the
    /// target is re-checked before anything is installed.
    fn load<V: 'static>(
        &mut self,
        engine: Arc<dyn DockerEngine>,
        access: fn(&mut V) -> &mut Self,
        window: &mut Window,
        cx: &mut Context<V>,
    ) {
        let Some(open) = self.open.as_ref() else {
            return;
        };
        let mode = open.mode;
        let id = open.id.clone();
        let target = id.clone();

        cx.notify();
        self.task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    match mode {
                        DetailMode::Inspect(InspectKind::Container) => engine
                            .inspect_container(&id)
                            .map(|detail| DetailContent::Inspect(Box::new(detail))),
                        DetailMode::Inspect(InspectKind::Image) => engine
                            .inspect_image(&id)
                            .map(|detail| DetailContent::Inspect(Box::new(detail))),
                        DetailMode::Inspect(InspectKind::Volume) => engine
                            .inspect_volume(&id)
                            .map(|detail| DetailContent::Inspect(Box::new(detail))),
                        DetailMode::Inspect(InspectKind::Network) => engine
                            .inspect_network(&id)
                            .map(|detail| DetailContent::Inspect(Box::new(detail))),
                        DetailMode::Logs => engine
                            .container_logs(&id, LOG_TAIL_LIMIT)
                            .map(DetailContent::Logs),
                    }
                })
                .await;

            let _ = this.update_in(cx, |view, window, cx| {
                let panel = access(view);
                // Closed, or re-opened on another row, while this was in flight.
                let still_wanted = panel
                    .open
                    .as_ref()
                    .is_some_and(|open| open.id == target && open.mode == mode);
                if !still_wanted {
                    return;
                }
                match result {
                    Ok(content) => panel.install(content, window, cx),
                    Err(error) => {
                        if let Some(open) = panel.open.as_mut() {
                            open.status = DetailStatus::Failed(error.message());
                        }
                    }
                }
                cx.notify();
            });
        }));
    }

    /// Installs a loaded detail: the engine's name replaces the row's where it
    /// has one, and an inspect's JSON is pushed into the code editor.
    fn install<V: 'static>(
        &mut self,
        content: DetailContent,
        window: &mut Window,
        cx: &mut Context<V>,
    ) {
        if let DetailContent::Inspect(detail) = &content {
            let json = detail.json.clone();
            self.json.update(cx, |state, cx| {
                state.set_value(json, window, cx);
            });
            if let (Some(open), false) = (self.open.as_mut(), detail.title.is_empty()) {
                open.title = detail.title.clone();
            }
        }
        if let Some(open) = self.open.as_mut() {
            open.status = DetailStatus::Ready(content);
        }
    }

    // ---- Rendering -----------------------------------------------------------

    /// The overlay, or `None` when nothing is open. The page passes its own
    /// `cx.listener`s for the two controls, so the panel never needs to know
    /// which page it belongs to.
    pub fn render(
        &self,
        on_close: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
        on_refresh: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
        cx: &App,
    ) -> Option<AnyElement> {
        let open = self.open.as_ref()?;
        let title = match open.mode {
            DetailMode::Inspect(_) => t(Str::DockerInspect, cx),
            DetailMode::Logs => t(Str::DockerViewLogs, cx),
        };

        Some(
            // The scrim: it dims the table and swallows clicks meant for it, so
            // the row underneath cannot be acted on while the panel is up.
            div()
                .id("docker-detail-scrim")
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .p_4()
                .bg(gpui::black().opacity(0.45))
                .on_mouse_down(MouseButton::Left, |_, _, _| {})
                .child(
                    v_flex()
                        .w(PANEL_W)
                        .max_w_full()
                        .h(PANEL_H)
                        .max_h_full()
                        .overflow_hidden()
                        .rounded(cx.theme().radius)
                        .border_1()
                        .border_color(cx.theme().border)
                        .bg(cx.theme().background)
                        .shadow_lg()
                        .child(self.render_header(title, open, on_close, on_refresh, cx))
                        .child(
                            div()
                                .w_full()
                                .flex_1()
                                .min_h_0()
                                .child(self.render_body(open, cx)),
                        ),
                )
                .into_any_element(),
        )
    }

    fn render_header(
        &self,
        title: SharedString,
        open: &OpenDetail,
        on_close: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
        on_refresh: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
        cx: &App,
    ) -> impl IntoElement {
        let icon = match open.mode {
            DetailMode::Inspect(_) => AppIcon::Eye,
            DetailMode::Logs => AppIcon::File,
        };

        h_flex()
            .w_full()
            .flex_shrink_0()
            .items_center()
            .gap_2()
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().muted.opacity(0.3))
            .child(
                Icon::new(icon)
                    .size(px(14.))
                    .text_color(cx.theme().muted_foreground),
            )
            .child(div().text_sm().font_medium().child(title))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_xs()
                    .font_family(cx.theme().mono_font_family.clone())
                    .text_color(cx.theme().muted_foreground)
                    .child(SharedString::from(open.title.clone())),
            )
            .child(
                Button::new("docker-detail-refresh")
                    .xsmall()
                    .ghost()
                    .icon(AppIcon::Refresh)
                    .tooltip(t(Str::DockerRefresh, cx))
                    .on_click(on_refresh),
            )
            .child(
                Button::new("docker-detail-close")
                    .xsmall()
                    .ghost()
                    .icon(AppIcon::Close)
                    .tooltip(t(Str::DockerClose, cx))
                    .on_click(on_close),
            )
    }

    fn render_body(&self, open: &OpenDetail, cx: &App) -> AnyElement {
        if open.status.is_loading() {
            return loading_skeleton(6, cx).into_any_element();
        }
        if let Some(error) = open.status.error() {
            return error_state(
                t(Str::DockerDetailErrorTitle, cx),
                t(error.clone(), cx),
                cx,
            )
            .into_any_element();
        }
        match open.status.ready() {
            Some(DetailContent::Inspect(detail)) => self.render_inspect(detail, cx),
            Some(DetailContent::Logs(lines)) => self.render_logs(lines, cx),
            None => div().into_any_element(),
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
                            .when(missing, |this| {
                                this.text_color(cx.theme().muted_foreground)
                            })
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

/// A small heading over one section of the panel.
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
