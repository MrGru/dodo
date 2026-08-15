//! The Runtimes page (round 7): automatic detection of the container
//! runtimes/daemons on this machine — Docker, Podman Machine, Kubernetes,
//! containerd — with a Start/Stop button per row.
//!
//! The simplest of the five pages: four fixed rows, no search, no selection,
//! no keyboard navigation and no detail dialog, so unlike its siblings it
//! carries no [`KEY_CONTEXT`](crate::KEY_CONTEXT) key bindings at
//! all — there is nothing here a row-navigation key would do. Detection and
//! control go through [`RuntimeService`], never named beyond this file and
//! [`DockerView`](super::DockerView), the same seam
//! [`DockerEngine`](crate::services::DockerEngine) is for the other
//! four pages.

use std::sync::Arc;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, Context, InteractiveElement as _, IntoElement, ParentElement as _, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, Task, Window, div,
};
use gpui_component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Icon, Sizable as _, StyledExt as _, h_flex, v_flex,
};

use crate::POLL_INTERVAL;
use crate::app_icon::AppIcon;
use crate::components::skeleton::loading_skeleton;
use crate::components::status_badge::status_badge;
use crate::components::toolbar::toolbar;
use crate::i18n::{docker, t};
use crate::models::runtime::{RuntimeInfo, RuntimeKind, RuntimeStatus};
use crate::services::runtime::{RuntimeError, RuntimeService, default_runtime_service};
use crate::state::runtime::RuntimeListState;

/// The row icon for `kind`. Chosen from icons already on the Docker rail
/// rather than new artwork — the same reuse the module leans on everywhere
/// else (`HardDrive` alone already labels a Volumes tab, an API Explorer
/// response panel and a Cleaner category). None of these claim to be the
/// runtime's logo; see `models::status`'s note on `PostgreSql`/`Sqlite` for
/// why dodo does not draw vendor marks.
fn runtime_icon(kind: RuntimeKind) -> AppIcon {
    match kind {
        RuntimeKind::Docker => AppIcon::Container,
        RuntimeKind::PodmanMachine => AppIcon::HardDrive,
        RuntimeKind::Kubernetes => AppIcon::Network,
        RuntimeKind::Containerd => AppIcon::Layers,
    }
}

pub struct RuntimesView {
    service: Arc<dyn RuntimeService>,
    state: RuntimeListState,
    load_task: Option<Task<()>>,
    action_task: Option<Task<()>>,
    /// The background auto-refresh loop, present only while this is the
    /// active, visible page — the same lifecycle [`DockerView::sync_polling`]
    /// drives for every other page.
    ///
    /// [`DockerView::sync_polling`]: super::DockerView::sync_polling
    poll_task: Option<Task<()>>,
    loaded_once: bool,
}

impl RuntimesView {
    pub fn new(_window: &mut Window, _cx: &mut Context<Self>) -> Self {
        Self {
            service: default_runtime_service(),
            state: RuntimeListState::default(),
            load_task: None,
            action_task: None,
            poll_task: None,
            loaded_once: false,
        }
    }

    pub fn ensure_loaded(&mut self, cx: &mut Context<Self>) {
        if !self.loaded_once {
            self.loaded_once = true;
            self.refresh(cx);
        }
    }

    /// Re-detects every kind on the background executor. Keeps the current
    /// rows on screen while it runs — there is no error state for the whole
    /// page, only per-row status, so a refresh never has anything to blank.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.state.begin_load();
        cx.notify();

        let service = self.service.clone();
        self.load_task = Some(cx.spawn(async move |this, cx| {
            let rows = cx
                .background_executor()
                .spawn(async move { service.detect_all() })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.state.set_rows(rows);
                cx.notify();
            });
        }));
    }

    /// Starts or stops the background auto-refresh loop; [`DockerView`] drives
    /// this so only the active, visible page polls. Idempotent.
    ///
    /// [`DockerView`]: super::DockerView
    pub fn set_polling(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if enabled {
            if self.poll_task.is_some() {
                return;
            }
            self.start_poll_loop(cx);
        } else {
            self.poll_task = None;
        }
    }

    fn start_poll_loop(&mut self, cx: &mut Context<Self>) {
        let service = self.service.clone();
        self.poll_task = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(POLL_INTERVAL).await;
                let poll_service = service.clone();
                let rows = cx
                    .background_executor()
                    .spawn(async move { poll_service.detect_all() })
                    .await;
                let stopped = this
                    .update(cx, |this, cx| {
                        // An in-flight Start/Stop already knows what it is
                        // doing; a stale poll tick landing on top of it would
                        // flicker the button back before the action's own
                        // refresh lands.
                        if this.state.pending().is_none() {
                            this.state.set_rows(rows);
                            cx.notify();
                        }
                    })
                    .is_err();
                if stopped {
                    break;
                }
            }
        }));
    }

    fn on_start(&mut self, kind: RuntimeKind, cx: &mut Context<Self>) {
        self.run_action(kind, |service, kind| service.start(kind), cx);
    }

    fn on_stop(&mut self, kind: RuntimeKind, cx: &mut Context<Self>) {
        self.run_action(kind, |service, kind| service.stop(kind), cx);
    }

    fn run_action(
        &mut self,
        kind: RuntimeKind,
        action: fn(&dyn RuntimeService, RuntimeKind) -> Result<(), RuntimeError>,
        cx: &mut Context<Self>,
    ) {
        self.state.begin_action(kind);
        cx.notify();

        let service = self.service.clone();
        self.action_task = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { action(service.as_ref(), kind) })
                .await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(()) => {
                        this.state.finish_action();
                        this.refresh(cx);
                    }
                    Err(error) => this.state.set_action_error(error.message()),
                }
                cx.notify();
            });
        }));
    }

    // ---- Rendering -----------------------------------------------------------

    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        toolbar(cx)
            .child(
                v_flex()
                    .gap_0()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .text_sm()
                            .font_medium()
                            .child(t(docker::Text::Runtimes, cx)),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(t(docker::Text::RuntimesDescription, cx)),
                    ),
            )
            .child(
                Button::new("docker-runtimes-refresh")
                    .small()
                    .icon(AppIcon::Refresh)
                    .label(t(docker::Text::Refresh, cx))
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
        if self.state.is_loading() {
            return loading_skeleton(RuntimeKind::ALL.len(), cx).into_any_element();
        }
        v_flex()
            .id("docker-runtimes-scroll")
            .size_full()
            .overflow_scroll()
            .children(
                self.state
                    .rows()
                    .iter()
                    .map(|row| self.render_row(row, cx).into_any_element()),
            )
            .into_any_element()
    }

    fn render_row(&self, row: &RuntimeInfo, cx: &mut Context<Self>) -> impl IntoElement {
        let detail = row.detail.clone();
        h_flex()
            .id(SharedString::from(format!("runtime-row-{:?}", row.kind)))
            .w_full()
            .flex_shrink_0()
            .items_center()
            .gap_3()
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(cx.theme().border.opacity(0.5))
            .child(
                Icon::new(runtime_icon(row.kind))
                    .size_4()
                    .text_color(cx.theme().muted_foreground),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_0()
                    .child(div().text_sm().font_medium().child(t(row.kind.title(), cx)))
                    .when_some(detail, |this, detail| {
                        this.child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .truncate()
                                .child(SharedString::from(detail)),
                        )
                    }),
            )
            .child(status_badge(
                t(row.status.label(), cx),
                row.status.color(cx),
                cx,
            ))
            .child(self.render_action(row, cx))
    }

    fn render_action(&self, row: &RuntimeInfo, cx: &mut Context<Self>) -> gpui::AnyElement {
        let kind = row.kind;
        let pending = self.state.is_pending(kind);

        if matches!(row.status, RuntimeStatus::Running) {
            let label = if pending {
                t(docker::Text::RuntimeStopping, cx)
            } else {
                t(docker::Text::Stop, cx)
            };
            return Button::new(SharedString::from(format!("runtime-stop-{kind:?}")))
                .small()
                .with_variant(ButtonVariant::Danger)
                .icon(AppIcon::Stop)
                .label(label)
                .disabled(pending || !row.can_stop)
                .on_click(cx.listener(move |this, _, _, cx| this.on_stop(kind, cx)))
                .into_any_element();
        }

        let label = if pending {
            t(docker::Text::RuntimeStarting, cx)
        } else {
            t(docker::Text::Start, cx)
        };
        let mut button = Button::new(SharedString::from(format!("runtime-start-{kind:?}")))
            .small()
            .icon(AppIcon::Play)
            .label(label)
            .disabled(pending || !row.can_start);
        if !row.can_start && !pending {
            let tooltip = match row.status {
                RuntimeStatus::Unsupported => t(docker::Text::RuntimeStatusUnsupported, cx),
                RuntimeStatus::NotInstalled => t(docker::Text::RuntimeStatusNotInstalled, cx),
                _ if kind == RuntimeKind::Kubernetes => {
                    t(docker::Text::RuntimeManagedExternally, cx)
                }
                _ => t(docker::Text::RuntimeStatusUnknown, cx),
            };
            button = button.tooltip(tooltip);
        }
        button
            .on_click(cx.listener(move |this, _, _, cx| this.on_start(kind, cx)))
            .into_any_element()
    }
}

impl Render for RuntimesView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
