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
    Anchor, App, AppContext as _, Context, Entity, FocusHandle, Focusable, InteractiveElement as _,
    IntoElement, MouseButton, ParentElement as _, Pixels, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, Task, Window, div, px,
};
use gpui_component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_component::checkbox::Checkbox;
use gpui_component::dialog::DialogButtonProps;
use gpui_component::input::{InputEvent, InputState};
use gpui_component::menu::{ContextMenuExt as _, PopupMenu};
use gpui_component::popover::Popover;
use gpui_component::{
    ActiveTheme as _, Disableable as _, Icon, Sizable as _, StyledExt as _, WindowExt as _, h_flex,
    v_flex,
};

use crate::app_icon::AppIcon;
use crate::docker::components::search_bar::search_bar;
use crate::docker::components::skeleton::loading_skeleton;
use crate::docker::components::states::{empty_state, error_state};
use crate::docker::components::status_badge::status_badge;
use crate::docker::components::toolbar::toolbar;
use crate::docker::models::container::Container;
use crate::docker::models::inspect::InspectKind;
use crate::docker::models::port::format_ports;
use crate::docker::models::status::ContainerStatus;
use crate::docker::models::time::RelativeTime;
use crate::docker::services::{DockerEngine, default_engine};
use crate::docker::state::containers::{ContainersState, LoadStatus};
use crate::docker::state::filters::FILTERABLE_STATUSES;
use crate::docker::state::focus::{FocusMove, next_focus};
use crate::docker::state::grouping::{ContainerGroup, GroupKey, GroupStatus};
use crate::docker::views::detail::DetailPanel;
use crate::docker::views::widgets::coming_soon_button;
use crate::docker::{
    DockerCloseDetail, DockerContextDelete, DockerContextInspect, DockerContextLogs,
    DockerContextRestart, DockerContextStart, DockerContextStats, DockerContextStop,
    DockerContextTerminal, DockerMoveDown, DockerMoveUp, DockerRefreshList, DockerToggleSelect,
    KEY_CONTEXT, POLL_INTERVAL,
};
use crate::i18n::{Language, Str, t};

/// Fixed column widths shared by the header and every row so they line up. Name,
/// Image and Ports take the remaining width as flex columns and truncate.
const SELECT_W: Pixels = px(36.);
const STATUS_W: Pixels = px(116.);
const CPU_W: Pixels = px(72.);
const STARTED_W: Pixels = px(140.);
/// Six xsmall buttons: Inspect, Logs, Start, Stop, Restart, Delete.
const ACTIONS_W: Pixels = px(220.);
const SEARCH_W: Pixels = px(240.);
/// The table's minimum width. Below it the table scrolls horizontally rather
/// than crushing the flex columns (Name, Image, Ports) to nothing — so at a
/// narrow window Name stays readable and the row is scrolled to reach Actions.
const TABLE_MIN_W: Pixels = px(964.);

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
    /// The background auto-refresh loop, present only while this is the active,
    /// visible page. Dropping it (via [`Self::set_polling`]) stops polling.
    poll_task: Option<Task<()>>,
    /// The list's own focus handle: the key-binding context ([`KEY_CONTEXT`]) and
    /// the arrow/space/refresh actions hang off this, so navigation is scoped to
    /// the Docker page and does not leak into other tools.
    focus_handle: FocusHandle,
    /// The keyboard-highlighted row (a container id), moved by arrow keys and
    /// toggled by space/x. `None` until the first arrow press.
    focused: Option<String>,
    /// The row a right-click opened the context menu on; the menu's lifecycle
    /// actions read this. Set on right mouse-down, before the menu builds.
    context_target: Option<String>,
    /// The read-only Inspect / Logs overlay, closed until a row action opens it.
    detail: DetailPanel,
    /// Set when the page becomes active so `render` moves focus to the list once,
    /// letting keyboard navigation work without a click first.
    needs_focus: bool,
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
            poll_task: None,
            focus_handle: cx.focus_handle(),
            focused: None,
            context_target: None,
            detail: DetailPanel::new(window, cx),
            needs_focus: false,
            loaded_once: false,
            language: Language::current(cx),
        }
    }

    /// How the detail panel's background fetch finds its way back to itself
    /// through this view; see [`DetailPanel`].
    fn detail_mut(&mut self) -> &mut DetailPanel {
        &mut self.detail
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

    /// Starts or stops the background auto-refresh loop. [`DockerView`] drives
    /// this so exactly the active, visible page polls. Idempotent: starting an
    /// already-running loop is a no-op, so it is safe to call on every page
    /// switch.
    ///
    /// [`DockerView`]: crate::docker::views::DockerView
    pub fn set_polling(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if enabled {
            if self.poll_task.is_some() {
                return;
            }
            // Becoming the active page: pull keyboard focus to the list once so
            // arrow navigation works without a click first.
            self.needs_focus = true;
            self.start_poll_loop(cx);
            cx.notify();
        } else {
            self.poll_task = None;
        }
    }

    /// The auto-refresh loop: every [`POLL_INTERVAL`] it re-lists the containers on
    /// the background executor, merges the result incrementally (only changed rows
    /// re-render, and selection/scroll/expanded/search all survive), then sweeps
    /// live CPU for the running rows in the same task so sweeps never overlap. An
    /// unreachable engine degrades to the error state without spamming re-renders;
    /// the table returns on the next good tick. The whole cycle is sequential, so
    /// a slow engine simply slows the cadence rather than piling tasks up.
    fn start_poll_loop(&mut self, cx: &mut Context<Self>) {
        let engine = self.engine.clone();
        self.poll_task = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(POLL_INTERVAL).await;

                let list_engine = engine.clone();
                let listed = cx
                    .background_executor()
                    .spawn(async move { list_engine.list_containers() })
                    .await;

                // Merge the fresh list (or record the error). The running ids come
                // back so the CPU sweep runs in this same task, never overlapping.
                let running = this.update(cx, |this, cx| match listed {
                    Ok(rows) => {
                        if this.state.merge_rows(rows) {
                            cx.notify();
                        }
                        Some(this.state.running_ids())
                    }
                    Err(error) => {
                        if this.state.set_poll_error(error.message()) {
                            cx.notify();
                        }
                        None
                    }
                });
                let running = match running {
                    Ok(running) => running,
                    Err(_) => break, // the view is gone; stop the loop.
                };

                if let Some(ids) = running {
                    for id in ids {
                        let cpu_engine = engine.clone();
                        let fetch_id = id.clone();
                        let percent = cx
                            .background_executor()
                            .spawn(async move { cpu_engine.cpu_percent(&fetch_id) })
                            .await
                            .ok()
                            .flatten();
                        if this
                            .update(cx, |this, cx| {
                                if this.state.set_cpu(&id, percent) {
                                    cx.notify();
                                }
                            })
                            .is_err()
                        {
                            return;
                        }
                    }
                }
            }
        }));
    }

    // ---- Keyboard navigation -------------------------------------------------

    /// Moves the keyboard highlight one row, in visible (grouped) order.
    fn move_focus(&mut self, dir: FocusMove, cx: &mut Context<Self>) {
        let keys = self.state.visible_ids();
        self.focused = next_focus(&keys, self.focused.as_deref(), dir);
        cx.notify();
    }

    fn on_move_up(&mut self, _: &DockerMoveUp, _: &mut Window, cx: &mut Context<Self>) {
        self.move_focus(FocusMove::Up, cx);
    }

    fn on_move_down(&mut self, _: &DockerMoveDown, _: &mut Window, cx: &mut Context<Self>) {
        self.move_focus(FocusMove::Down, cx);
    }

    /// Toggles the highlighted row's selection (space / x). A no-op when nothing
    /// is highlighted yet.
    fn on_toggle_select(&mut self, _: &DockerToggleSelect, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(id) = self.focused.clone() {
            let selected = self.state.selection.is_selected(&id);
            self.state.selection.toggle(&id, !selected);
            cx.notify();
        }
    }

    fn on_refresh_action(&mut self, _: &DockerRefreshList, _: &mut Window, cx: &mut Context<Self>) {
        self.refresh(cx);
    }

    // ---- Right-click context menu --------------------------------------------

    fn on_context_start(&mut self, _: &DockerContextStart, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(id) = self.context_target.clone() {
            self.run_lifecycle(id, Lifecycle::Start, cx);
        }
    }

    fn on_context_stop(&mut self, _: &DockerContextStop, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(id) = self.context_target.clone() {
            self.run_lifecycle(id, Lifecycle::Stop, cx);
        }
    }

    fn on_context_restart(
        &mut self,
        _: &DockerContextRestart,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(id) = self.context_target.clone() {
            self.run_lifecycle(id, Lifecycle::Restart, cx);
        }
    }

    fn on_context_delete(
        &mut self,
        _: &DockerContextDelete,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(id) = self.context_target.clone() {
            if let Some(row) = self.state.row(&id) {
                let name = row.name.clone();
                self.confirm_delete(id, name, window, cx);
            }
        }
    }

    fn on_context_inspect(
        &mut self,
        _: &DockerContextInspect,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(id) = self.context_target.clone() {
            self.open_inspect(id, window, cx);
        }
    }

    fn on_context_logs(
        &mut self,
        _: &DockerContextLogs,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(id) = self.context_target.clone() {
            self.open_logs(id, window, cx);
        }
    }

    /// Escape closes an open panel; with nothing open it does nothing, leaving
    /// the key free for whatever else claims it.
    fn on_close_detail(&mut self, _: &DockerCloseDetail, _: &mut Window, cx: &mut Context<Self>) {
        if self.detail.is_open() {
            self.detail.close();
            cx.notify();
        }
    }

    // ---- The read-only detail panel ------------------------------------------

    /// Opens Inspect on one container. The fetch runs on the background
    /// executor; the row's name labels the panel until the engine's own does.
    fn open_inspect(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let title = self.row_name(&id);
        let engine = self.engine.clone();
        self.detail.open_inspect(
            engine,
            InspectKind::Container,
            id,
            title,
            Self::detail_mut,
            window,
            cx,
        );
    }

    /// Opens the log viewer on one container.
    fn open_logs(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let title = self.row_name(&id);
        let engine = self.engine.clone();
        self.detail
            .open_logs(engine, id, title, Self::detail_mut, window, cx);
    }

    /// The name of the row with this id, or the id itself if it has gone (a poll
    /// can remove a row between the right-click and the action).
    fn row_name(&self, id: &str) -> String {
        self.state
            .row(id)
            .map(|row| row.name.clone())
            .unwrap_or_else(|| id.to_string())
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

    /// Runs one lifecycle call across a set of containers on the background
    /// executor, then reloads. Each call is independent: one failure does not
    /// abort the rest, and the count that failed surfaces in the action banner
    /// after the refresh (which would otherwise clear it). `ids` is pre-filtered
    /// to those the action is valid for, so invalid rows are simply skipped.
    fn run_bulk(&mut self, ids: Vec<String>, action: Lifecycle, cx: &mut Context<Self>) {
        if ids.is_empty() {
            return;
        }
        let engine = self.engine.clone();
        cx.spawn(async move |this, cx| {
            let failures = cx
                .background_executor()
                .spawn(async move {
                    let mut failed = 0usize;
                    for id in ids {
                        let result = match action {
                            Lifecycle::Start => engine.start(&id),
                            Lifecycle::Stop => engine.stop(&id),
                            Lifecycle::Restart => engine.restart(&id),
                            Lifecycle::Remove => engine.remove(&id),
                        };
                        if result.is_err() {
                            failed += 1;
                        }
                    }
                    failed
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                // Refresh first (it clears any prior banner), then post the
                // partial-failure count so it survives the reload.
                this.refresh(cx);
                if failures > 0 {
                    this.state
                        .set_action_error(Str::DockerBulkFailures(failures));
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Confirms then bulk-deletes the whole selection. Destructive, so it always
    /// routes through the alert, mirroring the per-row Delete.
    fn confirm_bulk_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let ids = self.state.selected_ids();
        if ids.is_empty() {
            return;
        }
        let count = ids.len();
        let entity = cx.entity();
        window.open_alert_dialog(cx, move |alert, _window, cx| {
            let entity = entity.clone();
            let ids = ids.clone();
            alert
                .title(t(Str::DockerBulkDeleteTitle, cx))
                .description(t(Str::DockerBulkDeleteMessage(count), cx))
                .button_props(
                    DialogButtonProps::default()
                        .ok_text(t(Str::Delete, cx))
                        .ok_variant(ButtonVariant::Danger)
                        .cancel_text(t(Str::DockerCancel, cx))
                        .show_cancel(true),
                )
                .on_ok(move |_, _window, cx| {
                    entity.update(cx, |this, cx| {
                        this.run_bulk(ids.clone(), Lifecycle::Remove, cx)
                    });
                    true
                })
        });
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
            .child(self.render_filter(cx))
            // Create is still a future placeholder — present, disabled and
            // tooltipped so it reads as a planned feature (see `docker/mod.rs`).
            .child(coming_soon_button(
                "docker-create".into(),
                AppIcon::Plus,
                t(Str::DockerCreate, cx),
                cx,
            ))
    }

    /// The Filter button and its popover. The button reads "Filter" normally and
    /// "Filter (N)" in the primary tone when N filter types are active, so the
    /// toolbar shows at a glance that the list is narrowed.
    fn render_filter(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let active = self.state.filters().active_count();
        let label = if active > 0 {
            t(Str::DockerFilterWithCount(active), cx)
        } else {
            t(Str::DockerFilter, cx)
        };
        let trigger = Button::new("docker-filter")
            .small()
            .icon(AppIcon::Filter)
            .label(label)
            .when(active > 0, |button| button.primary())
            .when(active == 0, |button| button.ghost());

        Popover::new("docker-filter-popover")
            .anchor(Anchor::TopRight)
            .trigger(trigger)
            .child(self.render_filter_panel(cx))
    }

    /// The filter popover's body: a Status section, a Compose-project section and
    /// an Image section (each shown only when it has options), the Has-published-
    /// ports toggle, the Favorites placeholder, and Clear filters. Built eagerly
    /// with this view's `cx`, so each checkbox toggles the store directly.
    fn render_filter_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let filters = self.state.filters();
        let projects = self.state.available_projects();
        let images = self.state.available_images();

        v_flex()
            .w(px(240.))
            .gap_3()
            .child(
                h_flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_sm()
                            .font_medium()
                            .child(t(Str::DockerFilterTitle, cx)),
                    )
                    .when(filters.is_active(), |row| {
                        row.child(
                            Button::new("docker-filter-clear")
                                .xsmall()
                                .ghost()
                                .label(t(Str::DockerFilterClear, cx))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.state.filters_mut().clear();
                                    cx.notify();
                                })),
                        )
                    }),
            )
            // Status — always present; the five filterable lifecycle states.
            .child(filter_section_title(t(Str::DockerColumnStatus, cx), cx))
            .children(FILTERABLE_STATUSES.map(|status| {
                Checkbox::new(SharedString::from(format!("filter-status-{status:?}")))
                    .label(t(status.label(), cx))
                    .checked(filters.is_status_selected(status))
                    .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                        this.state.filters_mut().toggle_status(status, *checked);
                        cx.notify();
                    }))
            }))
            // Compose project — only when at least one project exists.
            .when(!projects.is_empty(), |panel| {
                panel
                    .child(filter_section_title(t(Str::DockerFilterProject, cx), cx))
                    .children(projects.into_iter().map(|project| {
                        let checked = filters.is_project_selected(&project);
                        Checkbox::new(SharedString::from(format!("filter-project-{project}")))
                            .label(SharedString::from(project.clone()))
                            .checked(checked)
                            .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                                this.state
                                    .filters_mut()
                                    .toggle_project(project.clone(), *checked);
                                cx.notify();
                            }))
                    }))
            })
            // Image — only when there is something to pick.
            .when(!images.is_empty(), |panel| {
                panel
                    .child(filter_section_title(t(Str::DockerColumnImage, cx), cx))
                    .children(images.into_iter().map(|image| {
                        let checked = filters.is_image_selected(&image);
                        Checkbox::new(SharedString::from(format!("filter-image-{image}")))
                            .label(SharedString::from(image.clone()))
                            .checked(checked)
                            .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                                this.state
                                    .filters_mut()
                                    .toggle_image(image.clone(), *checked);
                                cx.notify();
                            }))
                    }))
            })
            // Has published ports (boolean) and the Favorites placeholder.
            .child(filter_section_title(t(Str::DockerColumnPorts, cx), cx))
            .child(
                Checkbox::new("filter-published-ports")
                    .label(t(Str::DockerFilterPublishedPorts, cx))
                    .checked(filters.published_ports_only())
                    .on_click(cx.listener(|this, checked: &bool, _, cx| {
                        this.state.filters_mut().set_published_ports_only(*checked);
                        cx.notify();
                    })),
            )
            // Favorites is a future feature: a clearly-labelled, disabled stub.
            .child(
                Checkbox::new("filter-favorites")
                    .label(t(Str::DockerFilterFavorites, cx))
                    .checked(false)
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

    /// The bulk-action bar, shown only while something is selected. Start and
    /// Stop enable only when the selection contains a container the action is
    /// valid for; Delete enables whenever anything is selected.
    fn render_bulk_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let count = self.state.selection.count();
        let startable = self.state.bulk_startable_ids();
        let stoppable = self.state.bulk_stoppable_ids();

        h_flex()
            .w_full()
            .flex_shrink_0()
            // Wrap the bulk buttons onto a second row at a narrow width rather
            // than clipping them, the same rule the toolbar follows.
            .flex_wrap()
            .items_center()
            .gap_2()
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().accent.opacity(0.3))
            .text_sm()
            .child(
                div()
                    .font_medium()
                    .child(t(Str::DockerBulkSelected(count), cx)),
            )
            .child(div().flex_1())
            .child(
                Button::new("bulk-start")
                    .xsmall()
                    .ghost()
                    .icon(AppIcon::Play)
                    .label(t(Str::DockerBulkStart, cx))
                    .disabled(startable.is_empty())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let ids = this.state.bulk_startable_ids();
                        this.run_bulk(ids, Lifecycle::Start, cx);
                    })),
            )
            .child(
                Button::new("bulk-stop")
                    .xsmall()
                    .ghost()
                    .icon(AppIcon::Stop)
                    .label(t(Str::DockerBulkStop, cx))
                    .disabled(stoppable.is_empty())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let ids = this.state.bulk_stoppable_ids();
                        this.run_bulk(ids, Lifecycle::Stop, cx);
                    })),
            )
            .child(
                Button::new("bulk-delete")
                    .xsmall()
                    .danger()
                    .icon(AppIcon::Trash)
                    .label(t(Str::DockerBulkDelete, cx))
                    .on_click(
                        cx.listener(|this, _, window, cx| this.confirm_bulk_delete(window, cx)),
                    ),
            )
            .child(
                Button::new("bulk-clear")
                    .xsmall()
                    .ghost()
                    .label(t(Str::DockerBulkClear, cx))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.state.selection.clear();
                        cx.notify();
                    })),
            )
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
            coming_soon_button(
                "docker-empty-create".into(),
                AppIcon::Plus,
                t(Str::DockerCreate, cx),
                cx,
            ),
        )
        .into_any_element()
    }

    fn render_table(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let groups = self.state.visible_groups();

        // Rows exist but the search/filters hide them all: a centred empty note.
        if groups.is_empty() {
            return empty_state(AppIcon::Inbox, t(Str::NoContainers, cx), None, cx)
                .into_any_element();
        }

        let all_selected = self
            .state
            .selection
            .all_selected(self.state.visible_ids().iter().map(String::as_str));
        let now = now_unix();

        // Materialise each group as its header row plus, when expanded, its
        // container rows. `render_*`'s return borrows `cx`, so this cannot be a
        // `map` closure.
        let mut blocks: Vec<gpui::AnyElement> = Vec::new();
        for group in groups {
            let collapsed = self.state.is_collapsed(&group.key);
            blocks.push(
                self.render_group_header(&group, collapsed, cx)
                    .into_any_element(),
            );
            if !collapsed {
                for row in &group.containers {
                    blocks.push(self.render_row(row.clone(), now, cx).into_any_element());
                }
            }
        }

        // One scroll container over both axes, with the header, the group headers
        // and the rows all sharing the minimum width — so a narrow window scrolls
        // the whole table sideways with the columns staying aligned.
        div()
            .id("docker-table-scroll")
            .size_full()
            .overflow_scroll()
            .child(
                // `w_full` + `min_w`: on a wide window the flex columns (Name,
                // Image, Ports) grow to fill the pane, so there is no dead space
                // on the right; below `TABLE_MIN_W` the min width wins and the
                // table scrolls horizontally instead of crushing those columns.
                v_flex()
                    .w_full()
                    .min_w(TABLE_MIN_W)
                    .child(self.render_header(all_selected, cx))
                    .children(blocks),
            )
            .into_any_element()
    }

    /// A compose-group header row: a chevron that toggles the group, the project
    /// name (or "Ungrouped"), the container count and a running summary coloured
    /// by the group's rolled-up status. Clicking anywhere on it collapses or
    /// expands the group.
    fn render_group_header(
        &self,
        group: &ContainerGroup,
        collapsed: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let key = group.key.clone();
        let title = match &group.key {
            GroupKey::Project(name) => SharedString::from(name.clone()),
            GroupKey::Ungrouped => t(Str::DockerUngrouped, cx),
        };
        let chevron = if collapsed {
            AppIcon::ChevronRight
        } else {
            AppIcon::ChevronDown
        };
        let summary_color = group_status_color(group.status(), cx);

        h_flex()
            .id(SharedString::from(format!("group-{title}")))
            .w_full()
            .min_w(TABLE_MIN_W)
            .flex_shrink_0()
            .items_center()
            .gap_2()
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().muted.opacity(0.5))
            .text_sm()
            .cursor_pointer()
            .hover(|this| this.bg(cx.theme().muted.opacity(0.8)))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.state.toggle_group(key.clone());
                cx.notify();
            }))
            .child(
                Icon::new(chevron)
                    .size(px(14.))
                    .text_color(cx.theme().muted_foreground),
            )
            .child(div().font_medium().child(title))
            .child(status_badge(
                t(Str::DockerGroupContainers(group.total()), cx),
                cx.theme().muted_foreground,
                cx,
            ))
            .child(status_badge(
                t(Str::DockerGroupRunning(group.running_count()), cx),
                summary_color,
                cx,
            ))
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
        let focused = self.focused.as_deref() == Some(row.id.as_str());
        let status = row.status;
        let ports = format_ports(&row.ports);
        let started = RelativeTime::since(row.started_at, now);
        let cpu = cpu_label(&row);
        let focus_handle = self.focus_handle.clone();

        h_flex()
            .id(SharedString::from(format!("crow-{}", row.id)))
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
            // The keyboard highlight: a left accent stripe (plus a faint tint when
            // the row is not also selected) so arrow-key focus is unmistakable.
            .when(focused && !selected, |this| {
                this.bg(cx.theme().accent.opacity(0.2))
            })
            .when(focused, |this| {
                this.border_l_2().border_color(cx.theme().primary)
            })
            // Record which row a right-click targets, before the menu builds, so
            // the context actions know which container to act on.
            .on_mouse_down(
                MouseButton::Right,
                cx.listener({
                    let id = row.id.clone();
                    move |this, _, _, _| this.context_target = Some(id.clone())
                }),
            )
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
            // Right-click menu: the lifecycle four mirror the row buttons and are
            // state-aware; Inspect / View Logs / Open Terminal are disabled
            // "coming soon" placeholders a later round fills in. Actions dispatch
            // to this view's focus handle and route through the same service layer
            // off the UI thread.
            .context_menu(move |menu, _window, cx| {
                container_context_menu(menu, status, focus_handle.clone(), cx)
            })
    }

    fn render_actions(&self, row: &Container, cx: &mut Context<Self>) -> impl IntoElement {
        let status = row.status;
        h_flex()
            .gap_1()
            // The two read-only actions come first: they are always valid, for
            // any container, in any state.
            .child(action_button(
                SharedString::from(format!("inspect-{}", row.id)),
                AppIcon::Eye,
                t(Str::DockerInspect, cx),
                true,
                ButtonVariant::Ghost,
                cx.listener({
                    let id = row.id.clone();
                    move |this, _, window, cx| this.open_inspect(id.clone(), window, cx)
                }),
            ))
            .child(action_button(
                SharedString::from(format!("logs-{}", row.id)),
                AppIcon::File,
                t(Str::DockerViewLogs, cx),
                true,
                ButtonVariant::Ghost,
                cx.listener({
                    let id = row.id.clone();
                    move |this, _, window, cx| this.open_logs(id.clone(), window, cx)
                }),
            ))
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

impl Focusable for ContainersView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for ContainersView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_language(window, cx);

        // On becoming the active page, pull focus to the list once so arrow-key
        // navigation works before the user clicks anything.
        if self.needs_focus {
            self.needs_focus = false;
            self.focus_handle.focus(window, cx);
        }

        let action_error = self
            .state
            .action_error()
            .map(|message| t(message.clone(), cx));
        let has_selection = !self.state.selection.is_empty();

        let engine = self.engine.clone();
        let detail = self.detail.render(
            cx.listener(|this, _, _, cx| {
                this.detail.close();
                cx.notify();
            }),
            cx.listener(move |this, _, window, cx| {
                this.detail
                    .reload(engine.clone(), Self::detail_mut, window, cx);
            }),
            cx,
        );

        v_flex()
            .size_full()
            // `relative` so the detail overlay positions against this page.
            .relative()
            // The Docker list's key-binding scope: arrow/space/x/refresh actions
            // fire only while this page holds focus, and a focused search box
            // (a deeper context) still takes the arrows for text editing.
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::on_move_up))
            .on_action(cx.listener(Self::on_move_down))
            .on_action(cx.listener(Self::on_toggle_select))
            .on_action(cx.listener(Self::on_refresh_action))
            .on_action(cx.listener(Self::on_context_start))
            .on_action(cx.listener(Self::on_context_stop))
            .on_action(cx.listener(Self::on_context_restart))
            .on_action(cx.listener(Self::on_context_delete))
            .on_action(cx.listener(Self::on_context_inspect))
            .on_action(cx.listener(Self::on_context_logs))
            .on_action(cx.listener(Self::on_close_detail))
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(cx.theme().border)
            .overflow_hidden()
            .bg(cx.theme().background)
            .child(self.render_toolbar(cx))
            .when(has_selection, |this| this.child(self.render_bulk_bar(cx)))
            .when_some(action_error, |this, message| {
                this.child(self.render_action_banner(message, cx))
            })
            .child(div().flex_1().min_h_0().child(self.render_body(cx)))
            .children(detail)
    }
}

/// A header cell: a `div` carrying the caption, ready for width refinements.
fn header_cell(label: SharedString) -> gpui::Div {
    div().truncate().child(label)
}

/// A small section heading inside the filter popover.
fn filter_section_title(label: SharedString, cx: &App) -> impl IntoElement {
    div()
        .text_xs()
        .font_medium()
        .text_color(cx.theme().muted_foreground)
        .child(label)
}

/// The colour of a group's running summary: success when all up, muted when all
/// stopped, warning for a partial mix — the same semantic tones the per-row
/// status badge uses.
fn group_status_color(status: GroupStatus, cx: &App) -> gpui::Hsla {
    match status {
        GroupStatus::AllRunning => cx.theme().success,
        GroupStatus::PartiallyRunning => cx.theme().warning,
        GroupStatus::NoneRunning => cx.theme().muted_foreground,
    }
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

/// Builds the per-row right-click menu. Inspect and View Logs open the
/// read-only detail panel and are always available; the lifecycle three are
/// enabled by the same status predicates as the row buttons; a separator sets
/// off Delete; and a "Coming soon" label heads what the module still stubs
/// (Open Terminal, Stats), disabled so they read as future features rather than
/// broken controls. `action_context` points the actions at the view's focus
/// handle so its `on_action` handlers catch them.
fn container_context_menu(
    menu: PopupMenu,
    status: ContainerStatus,
    focus: FocusHandle,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    menu.action_context(focus)
        .menu_with_icon(t(Str::DockerInspect, cx), AppIcon::Eye, Box::new(DockerContextInspect))
        .menu_with_icon(t(Str::DockerViewLogs, cx), AppIcon::File, Box::new(DockerContextLogs))
        .separator()
        .menu_with_icon_and_disabled(
            t(Str::DockerStart, cx),
            AppIcon::Play,
            Box::new(DockerContextStart),
            !status.can_start(),
        )
        .menu_with_icon_and_disabled(
            t(Str::DockerStop, cx),
            AppIcon::Stop,
            Box::new(DockerContextStop),
            !status.can_stop(),
        )
        .menu_with_icon_and_disabled(
            t(Str::DockerRestart, cx),
            AppIcon::Restart,
            Box::new(DockerContextRestart),
            !status.can_restart(),
        )
        .separator()
        .menu_with_icon(t(Str::Delete, cx), AppIcon::Trash, Box::new(DockerContextDelete))
        .separator()
        .label(t(Str::DockerComingSoonLabel, cx))
        .menu_with_icon_and_disabled(
            t(Str::DockerOpenTerminal, cx),
            AppIcon::SquareCode,
            Box::new(DockerContextTerminal),
            true,
        )
        .menu_with_icon_and_disabled(
            t(Str::DockerStats, cx),
            AppIcon::Sliders,
            Box::new(DockerContextStats),
            true,
        )
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
