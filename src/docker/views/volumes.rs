//! The Volumes page: round 3's second list page.
//!
//! The same shape as the Images page — an engine handle, a
//! [`ResourceState<Volume>`] store and a search input, every call on the
//! background executor — over the volume columns. Size is `N/A` whenever the
//! engine did not report it (the common case), rather than blocking the page on a
//! size scan; "containers using" counts the container mounts. Delete is confirmed
//! then refused sanely while a container still mounts the volume.

use std::sync::Arc;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, Pixels, Render, SharedString, StatefulInteractiveElement as _, Styled as _,
    Task, Window, div, px,
};
use gpui_component::button::{Button, ButtonVariant};
use gpui_component::dialog::DialogButtonProps;
use gpui_component::input::{InputEvent, InputState};
use gpui_component::{
    ActiveTheme as _, Sizable as _, StyledExt as _, WindowExt as _, h_flex, v_flex,
};

use crate::app_icon::AppIcon;
use crate::docker::components::search_bar::search_bar;
use crate::docker::components::skeleton::loading_skeleton;
use crate::docker::components::states::{empty_state, error_state};
use crate::docker::components::toolbar::toolbar;
use crate::docker::models::size::format_size;
use crate::docker::models::volume::Volume;
use crate::docker::services::{DockerEngine, default_engine};
use crate::docker::state::containers::LoadStatus;
use crate::docker::state::resource::ResourceState;
use crate::docker::views::widgets::{action_button, count_cell, header_cell, muted_cell};
use crate::i18n::{Language, Str, t};

/// Fixed column widths. Name and Mount point take the remaining width as the two
/// flex columns and truncate.
const DRIVER_W: Pixels = px(120.);
const SIZE_W: Pixels = px(96.);
const USING_W: Pixels = px(132.);
const ACTIONS_W: Pixels = px(48.);
const SEARCH_W: Pixels = px(240.);
const TABLE_MIN_W: Pixels = px(820.);

pub struct VolumesView {
    engine: Arc<dyn DockerEngine>,
    state: ResourceState<Volume>,
    search: Entity<InputState>,
    load_task: Option<Task<()>>,
    loaded_once: bool,
    language: Language,
}

impl VolumesView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let placeholder = t(Str::DockerSearchVolumes, cx);
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

    /// Reloads the volume list and the container usage together on the background
    /// executor, keeping the current rows on screen while it runs.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.state.begin_load();
        cx.notify();

        let engine = self.engine.clone();
        self.load_task = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let volumes = engine.list_volumes()?;
                    let usage = engine.container_usage().unwrap_or_default();
                    Ok::<_, crate::docker::services::DockerError>((sorted(volumes), usage))
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

    fn run_delete(&mut self, name: String, cx: &mut Context<Self>) {
        let engine = self.engine.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { engine.remove_volume(&name) })
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

    fn confirm_delete(&mut self, name: String, window: &mut Window, cx: &mut Context<Self>) {
        let entity = cx.entity();
        window.open_alert_dialog(cx, move |alert, _window, cx| {
            let entity = entity.clone();
            let name = name.clone();
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
                    entity.update(cx, |this, cx| this.run_delete(name.clone(), cx));
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
        let placeholder = t(Str::DockerSearchVolumes, cx);
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
                Button::new("docker-volumes-refresh")
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
                AppIcon::HardDrive,
                t(Str::NoVolumes, cx),
                Some(t(Str::NoVolumesHint, cx)),
                cx,
            )
            .into_any_element();
        }
        self.render_table(cx)
    }

    fn render_error(&self, message: SharedString, cx: &mut Context<Self>) -> gpui::AnyElement {
        error_state(t(Str::DockerUnreachableTitle, cx), message, cx)
            .child(
                Button::new("docker-volumes-retry")
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
            return empty_state(AppIcon::HardDrive, t(Str::NoVolumes, cx), None, cx)
                .into_any_element();
        }

        let mut blocks: Vec<gpui::AnyElement> = Vec::new();
        for row in rows {
            blocks.push(self.render_row(row.clone(), cx).into_any_element());
        }

        div()
            .id("docker-volumes-scroll")
            .size_full()
            .overflow_scroll()
            .child(
                v_flex()
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
                header_cell(t(Str::DockerColumnMountPoint, cx))
                    .flex_1()
                    .min_w_0(),
            )
            .child(
                header_cell(t(Str::DockerColumnSize, cx))
                    .w(SIZE_W)
                    .flex_shrink_0(),
            )
            .child(
                header_cell(t(Str::DockerColumnContainersUsing, cx))
                    .w(USING_W)
                    .flex_shrink_0(),
            )
            .child(
                header_cell(t(Str::DockerColumnActions, cx))
                    .w(ACTIONS_W)
                    .flex_shrink_0(),
            )
    }

    fn render_row(&self, row: Volume, cx: &mut Context<Self>) -> impl IntoElement {
        let size = match row.size {
            Some(bytes) => SharedString::from(format_size(bytes)),
            None => t(Str::DockerNotAvailable, cx),
        };
        let using = self.state.usage().volumes_using(&row.name);

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
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .font_medium()
                    .truncate()
                    .child(SharedString::from(row.name.clone())),
            )
            .child(muted_cell(SharedString::from(row.driver.clone()), cx).w(DRIVER_W).flex_shrink_0())
            .child(
                muted_cell(SharedString::from(row.mountpoint.clone()), cx)
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .font_family(cx.theme().mono_font_family.clone()),
            )
            .child(muted_cell(size, cx).w(SIZE_W).flex_shrink_0())
            .child(count_cell(using, cx).w(USING_W).flex_shrink_0())
            .child(
                div()
                    .w(ACTIONS_W)
                    .flex_shrink_0()
                    .child(self.render_actions(&row, cx)),
            )
    }

    fn render_actions(&self, row: &Volume, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex().gap_1().child(action_button(
            SharedString::from(format!("delete-{}", row.name)),
            AppIcon::Trash,
            t(Str::Delete, cx),
            true,
            ButtonVariant::Danger,
            cx.listener({
                let name = row.name.clone();
                move |this, _, window, cx| this.confirm_delete(name.clone(), window, cx)
            }),
        ))
    }
}

impl Render for VolumesView {
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

/// Volumes in display order: by name, case-insensitively.
fn sorted(mut rows: Vec<Volume>) -> Vec<Volume> {
    rows.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    rows
}
