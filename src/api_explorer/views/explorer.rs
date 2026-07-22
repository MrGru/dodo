//! The API Explorer page: collections on the left, request over response on
//! the right.
//!
//! This view owns the open tabs and the transport, and is the only place that
//! starts a request. The panes themselves are rendered by the sibling modules,
//! which add their own `impl ApiExplorer` blocks so that each stays a
//! screenful rather than one file rendering the whole page.

use gpui::{
    App, AppContext as _, Context, Entity, FocusHandle, Focusable, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, Styled as _, Window, div, px,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::InputState;
use gpui_component::resizable::{h_resizable, resizable_panel, v_resizable};
use gpui_component::{ActiveTheme as _, Sizable as _, h_flex, v_flex};

use crate::api_explorer::SendRequest;
use crate::api_explorer::services::{Protocol, TransportRegistry};
use crate::api_explorer::state::collection::CollectionState;
use crate::api_explorer::state::tab::RequestTabState;
use crate::api_explorer::state::ui::{COLLECTIONS_WIDTH, REQUEST_HEIGHT, UiState};
use crate::app_icon::AppIcon;
use crate::i18n::{Language, Str, t};

/// The key context the send shortcut is bound in. Matching happens up the
/// focus chain, so the binding fires from inside the URL field too.
pub const KEY_CONTEXT: &str = "ApiExplorer";

pub struct ApiExplorer {
    /// Every open request. Each is its own entity, so sending in one leaves the
    /// others untouched.
    pub(super) tabs: Vec<Entity<RequestTabState>>,
    pub(super) ui: UiState,
    pub(super) collections: CollectionState,
    /// The protocol backends. The view asks for the one matching the request's
    /// protocol and never names a concrete client, so a second protocol is a
    /// registry change rather than a view change.
    transports: TransportRegistry,
    /// Whether the method dropdown is showing. Held here rather than inside the
    /// popover so that picking a method can close it.
    pub(super) method_menu_open: bool,
    /// Whether the request-naming popover is showing.
    pub(super) save_menu_open: bool,
    /// The name field inside that popover. One field shared by every tab: only
    /// one popover can be open at a time, and it is filled from the active tab
    /// each time it opens.
    pub(super) name_input: Entity<InputState>,
    /// The language the widget-held strings were built for; see
    /// [`Self::sync_language`].
    language: Language,
    focus_handle: FocusHandle,
}

impl ApiExplorer {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let first = cx.new(|cx| RequestTabState::new(window, cx));
        let name_placeholder = t(Str::NameRequestPlaceholder, cx);
        let name_input =
            cx.new(|cx| InputState::new(window, cx).placeholder(name_placeholder));

        Self {
            tabs: vec![first],
            ui: UiState::new(cx),
            collections: CollectionState::default(),
            transports: TransportRegistry::with_defaults(),
            method_menu_open: false,
            save_menu_open: false,
            name_input,
            language: Language::current(cx),
            focus_handle: cx.focus_handle(),
        }
    }

    /// The tab currently in front, if there is one.
    pub(super) fn active_tab(&self) -> Option<&Entity<RequestTabState>> {
        self.tabs.get(self.ui.active_tab)
    }

    pub(super) fn open_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let tab = cx.new(|cx| RequestTabState::new(window, cx));
        self.tabs.push(tab);
        self.ui.active_tab = self.tabs.len() - 1;
        cx.notify();
    }

    pub(super) fn close_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.tabs.len() {
            return;
        }

        // Cancel before dropping so an in-flight request stops immediately
        // rather than running to completion against a tab nobody can see.
        self.tabs[index].update(cx, |tab, _| tab.cancel());
        self.tabs.remove(index);

        // Closing a tab before the active one shifts everything left.
        if index < self.ui.active_tab {
            self.ui.active_tab = self.ui.active_tab.saturating_sub(1);
        }
        self.ui.clamp_active(self.tabs.len());

        // The page always has at least one request open: an empty page with no
        // way back would be a dead end.
        if self.tabs.is_empty() {
            cx.notify();
            return;
        }
        cx.notify();
    }

    /// Sends the request in the active tab. Bound to Cmd/Ctrl+Enter and to the
    /// Send button.
    pub(super) fn send_active(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.active_tab().cloned() else {
            return;
        };
        // Phase 1 declares HTTP; a request that carries its own protocol picks
        // the backend the same way.
        let Some(transport) = self.transports.get(Protocol::Http) else {
            return;
        };
        tab.update(cx, |tab, cx| tab.send(transport, window, cx));
        cx.notify();
    }

    fn on_send_action(&mut self, _: &SendRequest, window: &mut Window, cx: &mut Context<Self>) {
        self.send_active(window, cx);
    }

    /// Re-pushes the strings that library widgets hold internally rather than
    /// rebuilding each frame — every URL field and every key/value cell.
    /// Cheap and idempotent: does nothing unless the language changed.
    fn sync_language(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let language = Language::current(cx);
        if language == self.language {
            return;
        }
        self.language = language;

        let url_placeholder = t(Str::UrlPlaceholder, cx);
        for tab in &self.tabs {
            tab.update(cx, |tab, cx| {
                tab.request.url.update(cx, |state, cx| {
                    state.set_placeholder(url_placeholder.clone(), window, cx);
                });
            });
        }

        let name_placeholder = t(Str::NameRequestPlaceholder, cx);
        self.name_input.update(cx, |state, cx| {
            state.set_placeholder(name_placeholder, window, cx);
        });

        // Each key/value cell holds its own placeholder too.
        for tab in &self.tabs {
            tab.update(cx, |tab, cx| {
                tab.request.sync_row_placeholders(window, cx);
            });
        }
    }

    /// The narrow strip that replaces the Collections panel when it is
    /// collapsed, holding the control that brings it back.
    fn collapsed_rail(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        v_flex()
            .h_full()
            .w(px(40.))
            .py_2()
            .items_center()
            .border_r_1()
            .border_color(cx.theme().border)
            .child(
                Button::new("expand-collections")
                    .ghost()
                    .small()
                    .icon(AppIcon::PanelLeftOpen)
                    .tooltip(t(Str::ShowCollections, cx))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.ui.collections_collapsed = false;
                        cx.notify();
                    })),
            )
            .into_any_element()
    }

    /// Request editor over response viewer, with the draggable divider between
    /// them.
    fn request_and_response(&self, window: &mut Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        v_flex()
            .size_full()
            .min_w_0()
            .child(self.render_tab_strip(cx))
            .child(
                v_resizable("api-explorer-rows")
                    .with_state(&self.ui.inner_split)
                    .child(
                        resizable_panel()
                            .size(REQUEST_HEIGHT)
                            .size_range(px(120.)..px(900.))
                            .child(self.render_request_editor(window, cx)),
                    )
                    .child(
                        resizable_panel()
                            .size_range(px(120.)..px(900.))
                            .child(self.render_response_viewer(cx)),
                    ),
            )
            .into_any_element()
    }
}

impl Focusable for ApiExplorer {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for ApiExplorer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_language(window, cx);

        let right = self.request_and_response(window, cx);

        div()
            .size_full()
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::on_send_action))
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(cx.theme().border)
            .overflow_hidden()
            .child(
                // Collapsing swaps the resizable group for a plain row: a
                // hidden panel would still take part in the drag arithmetic.
                if self.ui.collections_collapsed {
                    h_flex()
                        .size_full()
                        .child(self.collapsed_rail(cx))
                        .child(div().flex_1().min_w_0().h_full().child(right))
                        .into_any_element()
                } else {
                    h_resizable("api-explorer-split")
                        .with_state(&self.ui.outer_split)
                        .child(
                            resizable_panel()
                                .size(COLLECTIONS_WIDTH)
                                .size_range(px(150.)..px(460.))
                                .child(self.render_collections_panel(cx)),
                        )
                        .child(resizable_panel().child(right))
                        .into_any_element()
                },
            )
    }
}
