//! The Containers page: the round-1 deliverable.
//!
//! Owns the engine handle, the [`ContainersState`] store and the search input,
//! and is the only place that starts a Docker call — always on the background
//! executor, never on the UI thread. The list load fills six of the seven
//! columns at once; the CPU column trickles in per running row afterwards
//! ([`ContainersView::start_cpu_sweep`]), which is the seam round 2's live
//! polling replaces without touching the table's render.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, Pixels, Render, SharedString, StatefulInteractiveElement as _, Styled as _,
    Task, Window, div, px,
};
use gpui_component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_component::checkbox::Checkbox;
use gpui_component::dialog::DialogButtonProps;
use gpui_component::input::{InputEvent, InputState};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Sizable as _, StyledExt as _, WindowExt as _, h_flex,
    v_flex,
};

use crate::app_icon::AppIcon;
use crate::docker::components::search_bar::search_bar;
use crate::docker::components::skeleton::loading_skeleton;
use crate::docker::components::states::{empty_state, error_state};
use crate::docker::components::status_badge::status_badge;
use crate::docker::components::toolbar::toolbar;
use crate::docker::models::container::Container;
use crate::docker::models::port::format_ports;
use crate::docker::models::time::RelativeTime;
use crate::docker::services::{DockerEngine, default_engine};
use crate::docker::state::containers::{ContainersState, LoadStatus};
use crate::i18n::{Language, Str, t};

/// Fixed column widths shared by the header and every row so they line up. Name,
/// Image and Ports take the remaining width as flex columns and truncate.
const SELECT_W: Pixels = px(36.);
const STATUS_W: Pixels = px(116.);
const CPU_W: Pixels = px(72.);
const STARTED_W: Pixels = px(140.);
const ACTIONS_W: Pixels = px(156.);
const SEARCH_W: Pixels = px(240.);
/// The table's minimum width. Below it the table scrolls horizontally rather
/// than crushing the flex columns (Name, Image, Ports) to nothing — so at a
/// narrow window Name stays readable and the row is scrolled to reach Actions.
const TABLE_MIN_W: Pixels = px(900.);

/// Which lifecycle call a per-row button triggers.
#[derive(Clone, Copy)]
enum Lifecycle {
    Start,
    Stop,
    Restart,
    Remove,
}

pub struct ContainersView {
    engine: Arc<dyn DockerEngine>,
    state: ContainersState,
    search: Entity<InputState>,
    /// The in-flight list load; held so a new refresh replaces (cancels) the old.
    load_task: Option<Task<()>>,
    /// The in-flight per-row CPU sweep, cancelled the same way on refresh.
    cpu_task: Option<Task<()>>,
    /// Whether the first load has been kicked off; makes [`Self::ensure_loaded`]
    /// idempotent so returning to the page preserves its rows.
    loaded_once: bool,
    /// The language the search placeholder was built for; see [`Self::sync_language`].
    language: Language,
}

impl ContainersView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let placeholder = t(Str::DockerSearchPlaceholder, cx);
        let search = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));

        // Instant, case-insensitive filtering: every keystroke updates the query.
        cx.subscribe(&search, |this, state, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.state.set_query(state.read(cx).value().to_string());
                cx.notify();
            }
        })
        .detach();

        Self {
            engine: default_engine(),
            state: ContainersState::default(),
            search,
            load_task: None,
            cpu_task: None,
            loaded_once: false,
            language: Language::current(cx),
        }
    }

    /// Loads the list the first time the page is shown, once.
    pub fn ensure_loaded(&mut self, cx: &mut Context<Self>) {
        if !self.loaded_once {
            self.loaded_once = true;
            self.refresh(cx);
        }
    }

    /// Reloads the container list on the background executor. Keeps the current
    /// rows on screen while the load runs, then swaps them in and starts the CPU
    /// sweep. A failure becomes the error state.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.state.begin_load();
        cx.notify();

        let engine = self.engine.clone();
        self.load_task = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { engine.list_containers() })
                .await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(rows) => {
                        this.state.set_rows(rows);
                        this.start_cpu_sweep(cx);
                    }
                    Err(error) => this.state.set_error(error.message()),
                }
                cx.notify();
            });
        }));
    }

    /// Measures CPU for each running container in turn, updating that single row
    /// as its reading arrives so the table fills in progressively rather than
    /// waiting on the slowest container.
    fn start_cpu_sweep(&mut self, cx: &mut Context<Self>) {
        let ids = self.state.running_ids();
        if ids.is_empty() {
            self.cpu_task = None;
            return;
        }

        let engine = self.engine.clone();
        self.cpu_task = Some(cx.spawn(async move |this, cx| {
            for id in ids {
                let engine = engine.clone();
                let fetch_id = id.clone();
                let percent = cx
                    .background_executor()
                    .spawn(async move { engine.cpu_percent(&fetch_id) })
                    .await
                    .ok()
                    .flatten();
                // Stop the sweep if the view is gone.
                if this
                    .update(cx, |this, cx| {
                        this.state.set_cpu(&id, percent);
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        }));
    }

    /// Runs a lifecycle call, then reloads so the table reflects the change.
    /// A failure surfaces as the inline action banner, keeping the rows.
    fn run_lifecycle(&mut self, id: String, action: Lifecycle, cx: &mut Context<Self>) {
        let engine = self.engine.clone();
        cx.spawn(async move |this, cx| {
            let call_engine = engine.clone();
            let call_id = id.clone();
            let result = cx
                .background_executor()
                .spawn(async move {
                    match action {
                        Lifecycle::Start => call_engine.start(&call_id),
                        Lifecycle::Stop => call_engine.stop(&call_id),
                        Lifecycle::Restart => call_engine.restart(&call_id),
                        Lifecycle::Remove => call_engine.remove(&call_id),
                    }
                })
                .await;
            let _ = this.update(cx, |this, cx| match result {
                Ok(()) => this.refresh(cx),
                Err(error) => {
                    this.state.set_action_error(error.message());
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Opens the delete confirmation. Removal is destructive, so it never fires
    /// straight from the row button.
    fn confirm_delete(
        &mut self,
        id: String,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let entity = cx.entity();
        window.open_alert_dialog(cx, move |alert, _window, cx| {
            let entity = entity.clone();
            let id = id.clone();
            alert
                .title(t(Str::DockerDeleteTitle, cx))
                .description(t(Str::DockerDeleteMessage(name.clone()), cx))
                .button_props(
                    DialogButtonProps::default()
                        .ok_text(t(Str::Delete, cx))
                        .ok_variant(ButtonVariant::Danger)
                        .cancel_text(t(Str::DockerCancel, cx))
                        .show_cancel(true),
                )
                .on_ok(move |_, _window, cx| {
                    entity.update(cx, |this, cx| {
                        this.run_lifecycle(id.clone(), Lifecycle::Remove, cx)
                    });
                    true
                })
        });
    }

    fn select_all(&mut self, checked: bool, cx: &mut Context<Self>) {
        if checked {
            self.state.selection.set_all(self.state.visible_ids());
        } else {
            self.state.selection.clear();
        }
        cx.notify();
    }

    /// Re-pushes the search placeholder when the language changes, the same sweep
    /// the API Explorer does for its widget-held strings.
    fn sync_language(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let language = Language::current(cx);
        if language == self.language {
            return;
        }
        self.language = language;
        let placeholder = t(Str::DockerSearchPlaceholder, cx);
        self.search.update(cx, |state, cx| {
            state.set_placeholder(placeholder, window, cx);
        });
    }

    // ---- Rendering -----------------------------------------------------------

    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        toolbar(cx)
            .child(search_bar(&self.search, SEARCH_W))
            .child(div().flex_1())
            .child(
                Button::new("docker-refresh")
                    .small()
                    .icon(AppIcon::Refresh)
                    .label(t(Str::DockerRefresh, cx))
                    .on_click(cx.listener(|this, _, _, cx| this.refresh(cx))),
            )
            // Filter and Create are future placeholders — present but disabled so
            // the toolbar's final shape is visible now.
            .child(
                Button::new("docker-filter")
                    .small()
                    .ghost()
                    .icon(AppIcon::Filter)
                    .label(t(Str::DockerFilter, cx))
                    .disabled(true),
            )
            .child(
                Button::new("docker-create")
                    .small()
                    .icon(AppIcon::Plus)
                    .label(t(Str::DockerCreate, cx))
                    .disabled(true),
            )
    }

    fn render_action_banner(&self, message: SharedString, cx: &App) -> impl IntoElement {
        div()
            .w_full()
            .flex_shrink_0()
            .mx_3()
            .my_2()
            .px_3()
            .py_2()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(cx.theme().danger)
            .bg(cx.theme().danger.opacity(0.1))
            .text_xs()
            .text_color(cx.theme().danger)
            .child(message)
    }

    fn render_body(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        // Error wins over everything: the list could not be loaded.
        if let LoadStatus::Failed(message) = self.state.status() {
            return self.render_error(t(message.clone(), cx), cx);
        }
        // First load, nothing yet on screen: the non-blocking skeleton.
        if matches!(self.state.status(), LoadStatus::Loading) && !self.state.has_rows() {
            return loading_skeleton(6, cx).into_any_element();
        }
        // A completed load with no containers at all: the empty state.
        if self.state.is_empty() {
            return self.render_empty(cx);
        }
        self.render_table(cx)
    }

    fn render_error(&self, message: SharedString, cx: &mut Context<Self>) -> gpui::AnyElement {
        error_state(t(Str::DockerUnreachableTitle, cx), message, cx)
            .child(
                Button::new("docker-retry")
                    .small()
                    .icon(AppIcon::Refresh)
                    .label(t(Str::DockerRetry, cx))
                    .on_click(cx.listener(|this, _, _, cx| this.refresh(cx))),
            )
            .into_any_element()
    }

    fn render_empty(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        empty_state(
            AppIcon::Inbox,
            t(Str::NoContainers, cx),
            Some(t(Str::NoContainersHint, cx)),
            cx,
        )
        .child(
            // The empty state's own Create button — a placeholder like the toolbar's.
            Button::new("docker-empty-create")
                .small()
                .icon(AppIcon::Plus)
                .label(t(Str::DockerCreate, cx))
                .disabled(true),
        )
        .into_any_element()
    }

    fn render_table(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let rows: Vec<Container> = self.state.visible().into_iter().cloned().collect();

        // Rows exist but the search hides them all: a centred empty note.
        if rows.is_empty() {
            return empty_state(AppIcon::Inbox, t(Str::NoContainers, cx), None, cx)
                .into_any_element();
        }

        let visible_ids: Vec<&str> = rows.iter().map(|row| row.id.as_str()).collect();
        let all_selected = self.state.selection.all_selected(visible_ids.into_iter());
        let now = now_unix();

        // Materialise each row before building the list: `render_row`'s return
        // borrows `cx`, so it cannot be produced from inside a `map` closure.
        let mut row_elements = Vec::with_capacity(rows.len());
        for row in rows {
            row_elements.push(self.render_row(row, now, cx).into_any_element());
        }

        // One scroll container over both axes, with the header and the rows as
        // siblings sharing the minimum width — so a narrow window scrolls the
        // whole table sideways with the columns staying aligned. (Round 2 can pin
        // the header; keeping it in the same scroll keeps the alignment simple.)
        div()
            .id("docker-table-scroll")
            .size_full()
            .overflow_scroll()
            .child(
                v_flex()
                    .min_w(TABLE_MIN_W)
                    .child(self.render_header(all_selected, cx))
                    .children(row_elements),
            )
            .into_any_element()
    }

    fn render_header(&self, all_selected: bool, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .w_full()
            .min_w(TABLE_MIN_W)
            .flex_shrink_0()
            .items_center()
            .gap_2()
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().muted.opacity(0.3))
            .text_xs()
            .font_medium()
            .text_color(cx.theme().muted_foreground)
            .child(
                div().w(SELECT_W).flex_shrink_0().child(
                    Checkbox::new("docker-select-all")
                        .checked(all_selected)
                        .tooltip(t(Str::DockerSelectAll, cx))
                        .on_click(
                            cx.listener(|this, checked: &bool, _, cx| {
                                this.select_all(*checked, cx)
                            }),
                        ),
                ),
            )
            .child(header_cell(t(Str::DockerColumnName, cx)).flex_1().min_w_0())
            .child(
                header_cell(t(Str::DockerColumnImage, cx))
                    .flex_1()
                    .min_w_0(),
            )
            .child(
                header_cell(t(Str::DockerColumnStatus, cx))
                    .w(STATUS_W)
                    .flex_shrink_0(),
            )
            .child(
                header_cell(t(Str::DockerColumnCpu, cx))
                    .w(CPU_W)
                    .flex_shrink_0(),
            )
            .child(
                header_cell(t(Str::DockerColumnPorts, cx))
                    .flex_1()
                    .min_w_0(),
            )
            .child(
                header_cell(t(Str::DockerColumnLastStarted, cx))
                    .w(STARTED_W)
                    .flex_shrink_0(),
            )
            .child(
                header_cell(t(Str::DockerColumnActions, cx))
                    .w(ACTIONS_W)
                    .flex_shrink_0(),
            )
    }

    fn render_row(&self, row: Container, now: i64, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.state.selection.is_selected(&row.id);
        let status = row.status;
        let ports = format_ports(&row.ports);
        let started = RelativeTime::since(row.started_at, now);
        let cpu = cpu_label(&row);

        h_flex()
            .w_full()
            .min_w(TABLE_MIN_W)
            .flex_shrink_0()
            .items_center()
            .gap_2()
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(cx.theme().border.opacity(0.5))
            .text_sm()
            .when(selected, |this| this.bg(cx.theme().accent.opacity(0.4)))
            .child(
                div().w(SELECT_W).flex_shrink_0().child(
                    Checkbox::new(SharedString::from(format!("sel-{}", row.id)))
                        .checked(selected)
                        .tooltip(t(Str::DockerSelectRow, cx))
                        .on_click(cx.listener({
                            let id = row.id.clone();
                            move |this, checked: &bool, _, cx| {
                                this.state.selection.toggle(&id, *checked);
                                cx.notify();
                            }
                        })),
                ),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .font_medium()
                    .truncate()
                    .child(SharedString::from(row.name.clone())),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(cx.theme().muted_foreground)
                    .child(SharedString::from(row.image.clone())),
            )
            .child(div().w(STATUS_W).flex_shrink_0().child(status_badge(
                t(status.label(), cx),
                status.color(cx),
                cx,
            )))
            .child(
                div()
                    .w(CPU_W)
                    .flex_shrink_0()
                    .text_color(cx.theme().muted_foreground)
                    .child(cpu),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(cx.theme().muted_foreground)
                    .child(if ports.is_empty() {
                        SharedString::from("—")
                    } else {
                        SharedString::from(ports)
                    }),
            )
            .child(
                div()
                    .w(STARTED_W)
                    .flex_shrink_0()
                    .text_color(cx.theme().muted_foreground)
                    .child(t(started.label(), cx)),
            )
            .child(
                div()
                    .w(ACTIONS_W)
                    .flex_shrink_0()
                    .child(self.render_actions(&row, cx)),
            )
    }

    fn render_actions(&self, row: &Container, cx: &mut Context<Self>) -> impl IntoElement {
        let status = row.status;
        h_flex()
            .gap_1()
            .child(action_button(
                SharedString::from(format!("start-{}", row.id)),
                AppIcon::Play,
                t(Str::DockerStart, cx),
                status.can_start(),
                ButtonVariant::Ghost,
                cx.listener({
                    let id = row.id.clone();
                    move |this, _, _, cx| this.run_lifecycle(id.clone(), Lifecycle::Start, cx)
                }),
            ))
            .child(action_button(
                SharedString::from(format!("stop-{}", row.id)),
                AppIcon::Stop,
                t(Str::DockerStop, cx),
                status.can_stop(),
                ButtonVariant::Ghost,
                cx.listener({
                    let id = row.id.clone();
                    move |this, _, _, cx| this.run_lifecycle(id.clone(), Lifecycle::Stop, cx)
                }),
            ))
            .child(action_button(
                SharedString::from(format!("restart-{}", row.id)),
                AppIcon::Restart,
                t(Str::DockerRestart, cx),
                status.can_restart(),
                ButtonVariant::Ghost,
                cx.listener({
                    let id = row.id.clone();
                    move |this, _, _, cx| this.run_lifecycle(id.clone(), Lifecycle::Restart, cx)
                }),
            ))
            .child(action_button(
                SharedString::from(format!("delete-{}", row.id)),
                AppIcon::Trash,
                t(Str::Delete, cx),
                true,
                ButtonVariant::Danger,
                cx.listener({
                    let id = row.id.clone();
                    let name = row.name.clone();
                    move |this, _, window, cx| {
                        this.confirm_delete(id.clone(), name.clone(), window, cx)
                    }
                }),
            ))
    }
}

impl Render for ContainersView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_language(window, cx);

        let action_error = self
            .state
            .action_error()
            .map(|message| t(message.clone(), cx));

        v_flex()
            .size_full()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(cx.theme().border)
            .overflow_hidden()
            .bg(cx.theme().background)
            .child(self.render_toolbar(cx))
            .when_some(action_error, |this, message| {
                this.child(self.render_action_banner(message, cx))
            })
            .child(div().flex_1().min_h_0().child(self.render_body(cx)))
    }
}

/// A header cell: a `div` carrying the caption, ready for width refinements.
fn header_cell(label: SharedString) -> gpui::Div {
    div().truncate().child(label)
}

/// One small, tooltip-bearing action button, disabled when the action is invalid.
fn action_button(
    id: SharedString,
    icon: AppIcon,
    tooltip: SharedString,
    enabled: bool,
    variant: ButtonVariant,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    Button::new(id)
        .xsmall()
        .with_variant(variant)
        .icon(icon)
        .tooltip(tooltip)
        .disabled(!enabled)
        .on_click(on_click)
}

/// The CPU cell text: a percentage once measured, an ellipsis while a running
/// container's reading is in flight, and a dash for anything not running. The
/// number and symbols are not language, so they are not translated.
fn cpu_label(row: &Container) -> SharedString {
    match row.cpu_percent {
        Some(percent) => SharedString::from(format!("{percent:.1}%")),
        None if row.status.is_running() => SharedString::from("…"),
        None => SharedString::from("—"),
    }
}

/// Now, in Unix seconds, for relative-time formatting. A clock before the epoch
/// is impossible in practice; `0` is a harmless fallback.
fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|delta| delta.as_secs() as i64)
        .unwrap_or(0)
}
