//! The Images page: the first of round 3's three list pages.
//!
//! Owns the engine handle, a [`ResourceState<Image>`] store and the search
//! input, and starts every Docker call on the background executor — never the UI
//! thread — exactly as the Containers page does. One load fetches the image list
//! and the container usage together, so the "containers using" column is derived
//! from the live container set rather than the engine's own (often uncalculated)
//! counter. Delete is confirmed first, then refused sanely when a container still
//! uses the image; Inspect opens the shared read-only
//! [`DetailPanel`](crate::docker::views::detail::DetailPanel). Pull and Build are
//! the toolbar's two labelled placeholders.

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
use crate::docker::models::image::Image;
use crate::docker::models::inspect::InspectKind;
use crate::docker::models::size::format_size;
use crate::docker::models::time::RelativeTime;
use crate::docker::services::{DockerEngine, default_engine};
use crate::docker::state::containers::LoadStatus;
use crate::docker::state::focus::{FocusMove, next_focus};
use crate::docker::state::resource::ResourceState;
use crate::docker::views::detail::DetailPanel;
use crate::docker::views::widgets::{
    action_button, coming_soon_button, count_cell, header_cell, muted_cell, now_unix,
    resource_context_menu,
};
use crate::docker::{
    DockerCloseDetail, DockerContextDelete, DockerContextInspect, DockerMoveDown, DockerMoveUp,
    DockerRefreshList, KEY_CONTEXT, POLL_INTERVAL,
};
use crate::i18n::{Language, Str, t};

/// Fixed column widths shared by the header and every row so they line up.
/// Repository takes the remaining width as the one flex column and truncates.
const TAG_W: Pixels = px(120.);
const ID_W: Pixels = px(116.);
const SIZE_W: Pixels = px(90.);
const CREATED_W: Pixels = px(132.);
const USING_W: Pixels = px(132.);
const ACTIONS_W: Pixels = px(84.);
const SEARCH_W: Pixels = px(240.);
/// The table's minimum width; below it the table scrolls horizontally rather
/// than crushing the flex Repository column.
const TABLE_MIN_W: Pixels = px(840.);

pub struct ImagesView {
    engine: Arc<dyn DockerEngine>,
    state: ResourceState<Image>,
    search: Entity<InputState>,
    /// The in-flight load; held so a new refresh replaces (cancels) the old.
    load_task: Option<Task<()>>,
    /// The background auto-refresh loop, present only while this is the active,
    /// visible page.
    poll_task: Option<Task<()>>,
    /// The list's focus handle: keyboard nav is scoped to it (see [`KEY_CONTEXT`]).
    focus_handle: FocusHandle,
    /// The keyboard-highlighted row (an image id). `None` until the first arrow.
    focused: Option<String>,
    /// The row a right-click opened the menu on; the Delete and Inspect actions
    /// read it.
    context_target: Option<String>,
    /// The read-only Inspect overlay, closed until a row action opens it.
    detail: DetailPanel,
    /// Set when the page becomes active so `render` focuses the list once.
    needs_focus: bool,
    /// Whether the first load has been kicked off; makes [`Self::ensure_loaded`]
    /// idempotent so returning to the page preserves its rows.
    loaded_once: bool,
    /// The language the search placeholder was built for; see [`Self::sync_language`].
    language: Language,
}

impl ImagesView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let placeholder = t(Str::DockerSearchImages, cx);
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

    /// Re-lists the images and their usage every [`POLL_INTERVAL`] on the
    /// background executor and merges the result incrementally — only changed rows
    /// re-render, and the search query is preserved. An unreachable engine
    /// degrades to the error state without spamming; the table returns on the next
    /// good tick.
    fn start_poll_loop(&mut self, cx: &mut Context<Self>) {
        let engine = self.engine.clone();
        self.poll_task = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(POLL_INTERVAL).await;
                let fetch = engine.clone();
                let result = cx
                    .background_executor()
                    .spawn(async move {
                        let images = fetch.list_images()?;
                        let usage = fetch.container_usage().unwrap_or_default();
                        Ok::<_, crate::docker::services::DockerError>((sorted(images), usage))
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
        let keys: Vec<String> = self
            .state
            .visible()
            .iter()
            .map(|row| row.id.clone())
            .collect();
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
                let name = row.confirm_label();
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
            let title = self.row_label(&id);
            self.open_inspect(id, title, window, cx);
        }
    }

    fn on_close_detail(&mut self, _: &DockerCloseDetail, _: &mut Window, cx: &mut Context<Self>) {
        if self.detail.is_open() {
            self.detail.close();
            cx.notify();
        }
    }

    /// Opens the read-only Inspect panel on one image; the fetch runs on the
    /// background executor.
    fn open_inspect(
        &mut self,
        id: String,
        title: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let engine = self.engine.clone();
        self.detail.open_inspect(
            engine,
            InspectKind::Image,
            id,
            title,
            Self::detail_mut,
            window,
            cx,
        );
    }

    /// How the panel labels this row until the engine's own name arrives: the
    /// `repo:tag` reference, or the short id.
    fn row_label(&self, id: &str) -> String {
        self.state
            .visible()
            .into_iter()
            .find(|row| row.id == id)
            .map(|row| row.confirm_label())
            .unwrap_or_else(|| id.to_string())
    }

    /// Reloads the image list and the container usage together on the background
    /// executor, keeping the current rows on screen while it runs. A failure to
    /// reach the engine becomes the error state.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.state.begin_load();
        cx.notify();

        let engine = self.engine.clone();
        self.load_task = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let images = engine.list_images()?;
                    // Usage is best-effort: if it fails the list still renders,
                    // with the counts reading zero.
                    let usage = engine.container_usage().unwrap_or_default();
                    Ok::<_, crate::docker::services::DockerError>((sorted(images), usage))
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

    /// Runs a delete on the background executor, then reloads. A refusal — most
    /// often the image being in use — surfaces as the inline banner.
    fn run_delete(&mut self, id: String, cx: &mut Context<Self>) {
        let engine = self.engine.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { engine.remove_image(&id) })
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
                    entity.update(cx, |this, cx| this.run_delete(id.clone(), cx));
                    true
                })
        });
    }

    /// Re-pushes the search placeholder when the language changes.
    fn sync_language(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let language = Language::current(cx);
        if language == self.language {
            return;
        }
        self.language = language;
        let placeholder = t(Str::DockerSearchImages, cx);
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
                Button::new("docker-images-refresh")
                    .small()
                    .icon(AppIcon::Refresh)
                    .label(t(Str::DockerRefresh, cx))
                    .on_click(cx.listener(|this, _, _, cx| this.refresh(cx))),
            )
            // Pull and Build are the registry/creation flows a later round adds:
            // present, disabled and labelled, so they read as planned features.
            .child(coming_soon_button(
                "docker-images-pull".into(),
                AppIcon::Import,
                t(Str::DockerPull, cx),
                cx,
            ))
            .child(coming_soon_button(
                "docker-images-build".into(),
                AppIcon::Layers,
                t(Str::DockerBuild, cx),
                cx,
            ))
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
                AppIcon::Layers,
                t(Str::NoImages, cx),
                Some(t(Str::NoImagesHint, cx)),
                cx,
            )
            .into_any_element();
        }
        self.render_table(cx)
    }

    fn render_error(&self, message: SharedString, cx: &mut Context<Self>) -> gpui::AnyElement {
        error_state(t(Str::DockerUnreachableTitle, cx), message, cx)
            .child(
                Button::new("docker-images-retry")
                    .small()
                    .icon(AppIcon::Refresh)
                    .label(t(Str::DockerRetry, cx))
                    .on_click(cx.listener(|this, _, _, cx| this.refresh(cx))),
            )
            .into_any_element()
    }

    fn render_table(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let rows = self.state.visible();
        // Rows exist but the search hides them all: a centred empty note.
        if rows.is_empty() {
            return empty_state(AppIcon::Layers, t(Str::NoImages, cx), None, cx).into_any_element();
        }
        let now = now_unix();

        let mut blocks: Vec<gpui::AnyElement> = Vec::new();
        for row in rows {
            blocks.push(self.render_row(row.clone(), now, cx).into_any_element());
        }

        div()
            .id("docker-images-scroll")
            .size_full()
            .overflow_scroll()
            .child(
                // `w_full` + `min_w`: flex columns fill a wide pane (no dead
                // space), and below the min width the table scrolls horizontally.
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
            .child(
                header_cell(t(Str::DockerColumnRepository, cx))
                    .flex_1()
                    .min_w_0(),
            )
            .child(
                header_cell(t(Str::DockerColumnTag, cx))
                    .w(TAG_W)
                    .flex_shrink_0(),
            )
            .child(
                header_cell(t(Str::DockerColumnImageId, cx))
                    .w(ID_W)
                    .flex_shrink_0(),
            )
            .child(
                header_cell(t(Str::DockerColumnSize, cx))
                    .w(SIZE_W)
                    .flex_shrink_0(),
            )
            .child(
                header_cell(t(Str::DockerColumnCreated, cx))
                    .w(CREATED_W)
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

    fn render_row(&self, row: Image, now: i64, cx: &mut Context<Self>) -> impl IntoElement {
        let repository = match &row.repository {
            Some(repo) => SharedString::from(repo.clone()),
            None => t(Str::DockerNone, cx),
        };
        let tag = match &row.tag {
            Some(tag) => SharedString::from(tag.clone()),
            None => t(Str::DockerNone, cx),
        };
        let short_id = SharedString::from(row.short_id());
        let size = SharedString::from(format_size(row.size));
        let created = RelativeTime::since(Some(row.created), now);
        let using = self.state.usage().images_using(&row.id);
        let focused = self.focused.as_deref() == Some(row.id.as_str());
        let focus_handle = self.focus_handle.clone();

        h_flex()
            .id(SharedString::from(format!("irow-{}", row.id)))
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
                    .when(row.repository.is_none(), |this| {
                        this.text_color(cx.theme().muted_foreground)
                    })
                    .child(repository),
            )
            .child(muted_cell(tag, cx).w(TAG_W).flex_shrink_0().truncate())
            .child(
                muted_cell(short_id, cx)
                    .w(ID_W)
                    .flex_shrink_0()
                    .font_family(cx.theme().mono_font_family.clone()),
            )
            .child(muted_cell(size, cx).w(SIZE_W).flex_shrink_0())
            .child(
                muted_cell(t(created.label(), cx), cx)
                    .w(CREATED_W)
                    .flex_shrink_0(),
            )
            .child(count_cell(using, cx).w(USING_W).flex_shrink_0())
            .child(
                div()
                    .w(ACTIONS_W)
                    .flex_shrink_0()
                    .child(self.render_actions(&row, cx)),
            )
            // Right-click: Delete (always available for an image) plus the
            // disabled Inspect placeholder.
            .context_menu(move |menu, _window, cx| {
                resource_context_menu(menu, focus_handle.clone(), true, cx)
            })
    }

    fn render_actions(&self, row: &Image, cx: &mut Context<Self>) -> impl IntoElement {
        // A label for the confirmation and the detail panel: the reference if
        // tagged, else the short id.
        let name = row.confirm_label();
        h_flex()
            .gap_1()
            .child(action_button(
                SharedString::from(format!("inspect-{}", row.id)),
                AppIcon::Eye,
                t(Str::DockerInspect, cx),
                true,
                ButtonVariant::Ghost,
                cx.listener({
                    let id = row.id.clone();
                    let title = name.clone();
                    move |this, _, window, cx| {
                        this.open_inspect(id.clone(), title.clone(), window, cx)
                    }
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
                    move |this, _, window, cx| {
                        this.confirm_delete(id.clone(), name.clone(), window, cx)
                    }
                }),
            ))
    }
}

impl Focusable for ImagesView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for ImagesView {
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
            .relative()
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::on_move_up))
            .on_action(cx.listener(Self::on_move_down))
            .on_action(cx.listener(Self::on_refresh_action))
            .on_action(cx.listener(Self::on_context_delete))
            .on_action(cx.listener(Self::on_context_inspect))
            .on_action(cx.listener(Self::on_close_detail))
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
            .children(detail)
    }
}

/// Images in display order: tagged first (an untagged `<none>` image sinks to
/// the bottom), then by repository and tag, case-insensitively.
fn sorted(mut rows: Vec<Image>) -> Vec<Image> {
    rows.sort_by(|a, b| {
        a.is_untagged()
            .cmp(&b.is_untagged())
            .then_with(|| sort_key(a).cmp(&sort_key(b)))
    });
    rows
}

/// The lowercased `repository:tag` an image sorts by.
fn sort_key(image: &Image) -> String {
    format!(
        "{}:{}",
        image.repository.as_deref().unwrap_or_default(),
        image.tag.as_deref().unwrap_or_default()
    )
    .to_lowercase()
}
