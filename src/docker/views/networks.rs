//! The Networks page: round 3's third list page.
//!
//! The same shape as the Images and Volumes pages, over the network columns.
//! "Containers" counts the attachments derived from the container set. Delete is
//! confirmed then refused sanely while a container is still attached — and for
//! the predefined `bridge`/`host`/`none` networks it is *disabled* outright
//! ([`Network::is_predefined`]), since the engine would only reject it. Inspect
//! is a disabled placeholder for a later round.

use std::sync::Arc;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, AppContext as _, Context, Entity, FocusHandle, Focusable, InteractiveElement as _,
    IntoElement, MouseButton, ParentElement as _, Pixels, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, Task, Window, div, px,
};
use gpui_component::button::{Button, ButtonVariant};
use gpui_component::dialog::DialogButtonProps;
use gpui_component::input::{InputEvent, InputState};
use gpui_component::menu::ContextMenuExt as _;
use gpui_component::{
    ActiveTheme as _, Sizable as _, StyledExt as _, WindowExt as _, h_flex, v_flex,
};

use crate::app_icon::AppIcon;
use crate::docker::components::search_bar::search_bar;
use crate::docker::components::skeleton::loading_skeleton;
use crate::docker::components::states::{empty_state, error_state};
use crate::docker::components::toolbar::toolbar;
use crate::docker::models::network::Network;
use crate::docker::models::time::RelativeTime;
use crate::docker::services::{DockerEngine, default_engine};
use crate::docker::state::containers::LoadStatus;
use crate::docker::state::focus::{FocusMove, next_focus};
use crate::docker::state::resource::ResourceState;
use crate::docker::views::widgets::{
    action_button, count_cell, header_cell, muted_cell, now_unix, placeholder_button,
    resource_context_menu,
};
use crate::docker::{
    DockerContextDelete, DockerMoveDown, DockerMoveUp, DockerRefreshList, KEY_CONTEXT,
    POLL_INTERVAL,
};
use crate::i18n::{Language, Str, t};

/// Fixed column widths. Name takes the remaining width as the one flex column.
const DRIVER_W: Pixels = px(120.);
const SCOPE_W: Pixels = px(100.);
const CONTAINERS_W: Pixels = px(116.);
const CREATED_W: Pixels = px(132.);
const ACTIONS_W: Pixels = px(84.);
const SEARCH_W: Pixels = px(240.);
const TABLE_MIN_W: Pixels = px(760.);

pub struct NetworksView {
    engine: Arc<dyn DockerEngine>,
    state: ResourceState<Network>,
    search: Entity<InputState>,
    load_task: Option<Task<()>>,
    /// The background auto-refresh loop, present only while active and visible.
    poll_task: Option<Task<()>>,
    /// The list's focus handle; keyboard nav is scoped to it (see [`KEY_CONTEXT`]).
    focus_handle: FocusHandle,
    /// The keyboard-highlighted row (a network id). `None` until the first arrow.
    focused: Option<String>,
    /// The row a right-click opened the menu on; the Delete action reads it.
    context_target: Option<String>,
    /// Set when the page becomes active so `render` focuses the list once.
    needs_focus: bool,
    loaded_once: bool,
    language: Language,
}

impl NetworksView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let placeholder = t(Str::DockerSearchNetworks, cx);
        let search = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));

        cx.subscribe(&search, |this, state, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.state.set_query(state.read(cx).value().to_string());
                cx.notify();
            }
        })
        .detach();

        Self {
            engine: default_engine(),
            state: ResourceState::default(),
            search,
            load_task: None,
            poll_task: None,
            focus_handle: cx.focus_handle(),
            focused: None,
            context_target: None,
            needs_focus: false,
            loaded_once: false,
            language: Language::current(cx),
        }
    }

    pub fn ensure_loaded(&mut self, cx: &mut Context<Self>) {
        if !self.loaded_once {
            self.loaded_once = true;
            self.refresh(cx);
        }
    }

    /// Starts or stops the background auto-refresh loop; [`DockerView`] drives it
    /// so only the active, visible page polls. Idempotent.
    ///
    /// [`DockerView`]: crate::docker::views::DockerView
    pub fn set_polling(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if enabled {
            if self.poll_task.is_some() {
                return;
            }
            self.needs_focus = true;
            self.start_poll_loop(cx);
            cx.notify();
        } else {
            self.poll_task = None;
        }
    }

    /// Re-lists the networks and their usage every [`POLL_INTERVAL`] and merges the
    /// result incrementally — only changed rows re-render, the search is
    /// preserved, and an unreachable engine degrades to the error state without
    /// spamming.
    fn start_poll_loop(&mut self, cx: &mut Context<Self>) {
        let engine = self.engine.clone();
        self.poll_task = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(POLL_INTERVAL).await;
                let fetch = engine.clone();
                let result = cx
                    .background_executor()
                    .spawn(async move {
                        let networks = fetch.list_networks()?;
                        let usage = fetch.container_usage().unwrap_or_default();
                        Ok::<_, crate::docker::services::DockerError>((sorted(networks), usage))
                    })
                    .await;
                if this
                    .update(cx, |this, cx| match result {
                        Ok((rows, usage)) => {
                            if this.state.merge(rows, usage) {
                                cx.notify();
                            }
                        }
                        Err(error) => {
                            if this.state.set_poll_error(error.message()) {
                                cx.notify();
                            }
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        }));
    }

    // ---- Keyboard navigation and context menu --------------------------------

    fn move_focus(&mut self, dir: FocusMove, cx: &mut Context<Self>) {
        let keys: Vec<String> = self.state.visible().iter().map(|row| row.id.clone()).collect();
        self.focused = next_focus(&keys, self.focused.as_deref(), dir);
        cx.notify();
    }

    fn on_move_up(&mut self, _: &DockerMoveUp, _: &mut Window, cx: &mut Context<Self>) {
        self.move_focus(FocusMove::Up, cx);
    }

    fn on_move_down(&mut self, _: &DockerMoveDown, _: &mut Window, cx: &mut Context<Self>) {
        self.move_focus(FocusMove::Down, cx);
    }

    fn on_refresh_action(&mut self, _: &DockerRefreshList, _: &mut Window, cx: &mut Context<Self>) {
        self.refresh(cx);
    }

    fn on_context_delete(
        &mut self,
        _: &DockerContextDelete,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(id) = self.context_target.clone() {
            if let Some(row) = self.state.visible().into_iter().find(|row| row.id == id) {
                // A predefined network's Delete is disabled in the menu, but guard
                // here too so a keybind or race cannot route around it.
                if row.is_predefined() {
                    return;
                }
                let name = row.name.clone();
                self.confirm_delete(id, name, window, cx);
            }
        }
    }

    /// Reloads the network list and the container usage together on the
    /// background executor, keeping the current rows on screen while it runs.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.state.begin_load();
        cx.notify();

        let engine = self.engine.clone();
        self.load_task = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let networks = engine.list_networks()?;
                    let usage = engine.container_usage().unwrap_or_default();
                    Ok::<_, crate::docker::services::DockerError>((sorted(networks), usage))
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok((rows, usage)) => {
                        this.state.set_usage(usage);
                        this.state.set_rows(rows);
                    }
                    Err(error) => this.state.set_error(error.message()),
                }
                cx.notify();
            });
        }));
    }

    fn run_delete(&mut self, id: String, cx: &mut Context<Self>) {
        let engine = self.engine.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { engine.remove_network(&id) })
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
                    entity.update(cx, |this, cx| this.run_delete(id.clone(), cx));
                    true
                })
        });
    }

    fn sync_language(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let language = Language::current(cx);
        if language == self.language {
            return;
        }
        self.language = language;
        let placeholder = t(Str::DockerSearchNetworks, cx);
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
                Button::new("docker-networks-refresh")
                    .small()
                    .icon(AppIcon::Refresh)
                    .label(t(Str::DockerRefresh, cx))
                    .on_click(cx.listener(|this, _, _, cx| this.refresh(cx))),
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
        if let LoadStatus::Failed(message) = self.state.status() {
            return self.render_error(t(message.clone(), cx), cx);
        }
        if matches!(self.state.status(), LoadStatus::Loading) && !self.state.has_rows() {
            return loading_skeleton(6, cx).into_any_element();
        }
        if self.state.is_empty() {
            return empty_state(
                AppIcon::Network,
                t(Str::NoNetworks, cx),
                Some(t(Str::NoNetworksHint, cx)),
                cx,
            )
            .into_any_element();
        }
        self.render_table(cx)
    }

    fn render_error(&self, message: SharedString, cx: &mut Context<Self>) -> gpui::AnyElement {
        error_state(t(Str::DockerUnreachableTitle, cx), message, cx)
            .child(
                Button::new("docker-networks-retry")
                    .small()
                    .icon(AppIcon::Refresh)
                    .label(t(Str::DockerRetry, cx))
                    .on_click(cx.listener(|this, _, _, cx| this.refresh(cx))),
            )
            .into_any_element()
    }

    fn render_table(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let rows = self.state.visible();
        if rows.is_empty() {
            return empty_state(AppIcon::Network, t(Str::NoNetworks, cx), None, cx)
                .into_any_element();
        }
        let now = now_unix();

        let mut blocks: Vec<gpui::AnyElement> = Vec::new();
        for row in rows {
            blocks.push(self.render_row(row.clone(), now, cx).into_any_element());
        }

        div()
            .id("docker-networks-scroll")
            .size_full()
            .overflow_scroll()
            .child(
                // `w_full` + `min_w`: the Name flex column fills a wide pane (no
                // dead space), and below the min width the table scrolls sideways.
                v_flex()
                    .w_full()
                    .min_w(TABLE_MIN_W)
                    .child(self.render_header(cx))
                    .children(blocks),
            )
            .into_any_element()
    }

    fn render_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
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
            .child(header_cell(t(Str::DockerColumnName, cx)).flex_1().min_w_0())
            .child(
                header_cell(t(Str::DockerColumnDriver, cx))
                    .w(DRIVER_W)
                    .flex_shrink_0(),
            )
            .child(
                header_cell(t(Str::DockerColumnScope, cx))
                    .w(SCOPE_W)
                    .flex_shrink_0(),
            )
            .child(
                header_cell(t(Str::Containers, cx))
                    .w(CONTAINERS_W)
                    .flex_shrink_0(),
            )
            .child(
                header_cell(t(Str::DockerColumnCreated, cx))
                    .w(CREATED_W)
                    .flex_shrink_0(),
            )
            .child(
                header_cell(t(Str::DockerColumnActions, cx))
                    .w(ACTIONS_W)
                    .flex_shrink_0(),
            )
    }

    fn render_row(&self, row: Network, now: i64, cx: &mut Context<Self>) -> impl IntoElement {
        let created = RelativeTime::since(row.created, now);
        let attached = self.state.usage().networks_using(&row.name);
        let focused = self.focused.as_deref() == Some(row.id.as_str());
        let focus_handle = self.focus_handle.clone();
        // Delete is disabled in the menu for a predefined network, mirroring the
        // row button.
        let deletable = !row.is_predefined();

        h_flex()
            .id(SharedString::from(format!("nrow-{}", row.id)))
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
            .when(focused, |this| {
                this.bg(cx.theme().accent.opacity(0.2))
                    .border_l_2()
                    .border_color(cx.theme().primary)
            })
            .on_mouse_down(
                MouseButton::Right,
                cx.listener({
                    let id = row.id.clone();
                    move |this, _, _, _| this.context_target = Some(id.clone())
                }),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .font_medium()
                    .truncate()
                    .child(SharedString::from(row.name.clone())),
            )
            .child(muted_cell(SharedString::from(row.driver.clone()), cx).w(DRIVER_W).flex_shrink_0())
            .child(muted_cell(SharedString::from(row.scope.clone()), cx).w(SCOPE_W).flex_shrink_0())
            .child(count_cell(attached, cx).w(CONTAINERS_W).flex_shrink_0())
            .child(muted_cell(t(created.label(), cx), cx).w(CREATED_W).flex_shrink_0())
            .child(
                div()
                    .w(ACTIONS_W)
                    .flex_shrink_0()
                    .child(self.render_actions(&row, cx)),
            )
            // Right-click: Delete (disabled for a predefined network) plus the
            // disabled Inspect placeholder.
            .context_menu(move |menu, _window, cx| {
                resource_context_menu(menu, focus_handle.clone(), deletable, cx)
            })
    }

    fn render_actions(&self, row: &Network, cx: &mut Context<Self>) -> impl IntoElement {
        let predefined = row.is_predefined();
        // A predefined network's Delete is disabled and says why; a removable one
        // reads the plain Delete tooltip.
        let delete_tooltip = if predefined {
            t(Str::DockerNetworkPredefined, cx)
        } else {
            t(Str::Delete, cx)
        };
        h_flex()
            .gap_1()
            .child(placeholder_button(
                SharedString::from(format!("inspect-{}", row.id)),
                AppIcon::Eye,
                t(Str::DockerInspect, cx),
            ))
            .child(action_button(
                SharedString::from(format!("delete-{}", row.id)),
                AppIcon::Trash,
                delete_tooltip,
                !predefined,
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

impl Focusable for NetworksView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for NetworksView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_language(window, cx);

        if self.needs_focus {
            self.needs_focus = false;
            self.focus_handle.focus(window, cx);
        }

        let action_error = self
            .state
            .action_error()
            .map(|message| t(message.clone(), cx));

        v_flex()
            .size_full()
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::on_move_up))
            .on_action(cx.listener(Self::on_move_down))
            .on_action(cx.listener(Self::on_refresh_action))
            .on_action(cx.listener(Self::on_context_delete))
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

/// Networks in display order: by name, case-insensitively.
fn sorted(mut rows: Vec<Network>) -> Vec<Network> {
    rows.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    rows
}
