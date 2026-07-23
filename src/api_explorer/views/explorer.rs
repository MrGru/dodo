//! The API Explorer page: collections on the left, request over response on
//! the right.
//!
//! This view owns the open tabs and the transport, and is the only place that
//! starts a request. The panes themselves are rendered by the sibling modules,
//! which add their own `impl ApiExplorer` blocks so that each stays a
//! screenful rather than one file rendering the whole page.

use std::path::PathBuf;
use std::sync::Arc;

use gpui::{
    App, AppContext as _, Context, Entity, FocusHandle, Focusable, InteractiveElement as _,
    IntoElement, ParentElement as _, PathPromptOptions, Render, Styled as _, Window, div, px,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::InputState;
use gpui_component::resizable::{h_resizable, resizable_panel, v_resizable};
use gpui_component::{ActiveTheme as _, Selectable as _, h_flex, v_flex};

use crate::api_explorer::SendRequest;
use crate::api_explorer::models::collection::{CollectionTree, NodeId};
use crate::api_explorer::models::snapshot::RequestSnapshot;
use crate::api_explorer::services::collection_import::parse_import;
use crate::api_explorer::services::collection_store::{
    CollectionStore, DiskCollectionStore, data_dir,
};
use crate::api_explorer::services::file_export;
use crate::api_explorer::services::{Protocol, TransportRegistry};
use crate::api_explorer::state::collection::CollectionState;
use crate::api_explorer::state::history::{History, HistoryRecord};
use crate::api_explorer::state::tab::RequestTabState;
use crate::api_explorer::state::ui::{
    COLLECTIONS_WIDTH, LeftPanel, REQUEST_MIN_HEIGHT, RESPONSE_HEIGHT, UiState,
};
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
    /// The in-memory request history, fed by each tab completing.
    pub(super) history: History,
    /// The protocol backends. The view asks for the one matching the request's
    /// protocol and never names a concrete client, so a second protocol is a
    /// registry change rather than a view change.
    transports: TransportRegistry,
    /// Where collections are persisted. An `Arc<dyn CollectionStore>` for the
    /// same reason the transport is a trait object: this view never learns
    /// whether they are on disk or in memory.
    collection_store: Arc<dyn CollectionStore>,
    /// Whether the method dropdown is showing. Held here rather than inside the
    /// popover so that picking a method can close it.
    pub(super) method_menu_open: bool,
    /// Whether the request-naming popover is showing.
    pub(super) save_menu_open: bool,
    /// Whether the Body tab's type dropdown is showing.
    pub(super) body_menu_open: bool,
    /// Whether the Auth tab's scheme dropdown is showing.
    pub(super) auth_menu_open: bool,
    /// The name field inside that popover. One field shared by every tab: only
    /// one popover can be open at a time, and it is filled from the active tab
    /// each time it opens.
    pub(super) name_input: Entity<InputState>,
    /// The Collections panel's search box. Filters the tree as it is typed.
    pub(super) search_input: Entity<InputState>,
    /// The rename popover's field, shared the same way `name_input` is.
    pub(super) rename_input: Entity<InputState>,
    /// Which node's rename popover is open, if any.
    pub(super) rename_target: Option<NodeId>,
    /// Which node's action menu is open, if any.
    pub(super) node_menu_open: Option<NodeId>,
    /// The language the widget-held strings were built for; see
    /// [`Self::sync_language`].
    language: Language,
    focus_handle: FocusHandle,
}

impl ApiExplorer {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let first = cx.new(|cx| RequestTabState::new(window, cx));
        Self::watch_tab(&first, cx);

        let name_placeholder = t(Str::NameRequestPlaceholder, cx);
        let name_input = cx.new(|cx| InputState::new(window, cx).placeholder(name_placeholder));
        let search_placeholder = t(Str::SearchCollectionsPlaceholder, cx);
        let search_input =
            cx.new(|cx| InputState::new(window, cx).placeholder(search_placeholder));
        let rename_placeholder = t(Str::NamePlaceholder, cx);
        let rename_input =
            cx.new(|cx| InputState::new(window, cx).placeholder(rename_placeholder));

        let collection_store: Arc<dyn CollectionStore> = Arc::new(DiskCollectionStore::new());
        Self::load_collections(collection_store.clone(), cx);

        Self {
            tabs: vec![first],
            ui: UiState::new(cx),
            collections: CollectionState::default(),
            history: History::default(),
            transports: TransportRegistry::with_defaults(),
            collection_store,
            method_menu_open: false,
            save_menu_open: false,
            body_menu_open: false,
            auth_menu_open: false,
            name_input,
            search_input,
            rename_input,
            rename_target: None,
            node_menu_open: None,
            language: Language::current(cx),
            focus_handle: cx.focus_handle(),
        }
    }

    /// Subscribes to a tab so its completed requests land in history. Every tab
    /// — the first, a new one, or one opened from a saved request — is watched
    /// through here, so nothing sent can escape the record.
    fn watch_tab(tab: &Entity<RequestTabState>, cx: &mut Context<Self>) {
        cx.subscribe(tab, |this, _tab, record: &HistoryRecord, cx| {
            this.history.record(record.clone());
            cx.notify();
        })
        .detach();
    }

    /// Loads the saved collections off disk on the background executor, then
    /// installs them. Runs once at construction; a missing file is an empty
    /// tree, a read/parse failure is shown rather than swallowed.
    fn load_collections(store: Arc<dyn CollectionStore>, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let loaded = cx
                .background_executor()
                .spawn(async move { store.load() })
                .await;
            let _ = this.update(cx, |this, cx| {
                match loaded {
                    Ok(roots) => this.collections.set_tree(CollectionTree::from_roots(roots)),
                    Err(error) => this.collections.set_error(Some(error.message())),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Writes the current tree to the store on the background executor. Called
    /// after every edit; a failure is surfaced on the panel, not swallowed.
    pub(super) fn persist_collections(&mut self, cx: &mut Context<Self>) {
        let roots = self.collections.tree().roots().to_vec();
        let store = self.collection_store.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { store.persist(&roots) })
                .await;
            if let Err(error) = result {
                let _ = this.update(cx, |this, cx| {
                    this.collections.set_error(Some(error.message()));
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// The tab currently in front, if there is one.
    pub(super) fn active_tab(&self) -> Option<&Entity<RequestTabState>> {
        self.tabs.get(self.ui.active_tab)
    }

    pub(super) fn open_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let tab = cx.new(|cx| RequestTabState::new(window, cx));
        Self::watch_tab(&tab, cx);
        self.tabs.push(tab);
        self.ui.active_tab = self.tabs.len() - 1;
        cx.notify();
    }

    /// Opens a saved request or a history entry in a fresh tab, restoring its
    /// full state. The tab is watched like any other, so resending from it is
    /// recorded again.
    pub(super) fn open_snapshot(
        &mut self,
        snapshot: RequestSnapshot,
        name: Option<gpui::SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let tab = cx.new(|cx| {
            let mut state = RequestTabState::new(window, cx);
            state.request.apply_snapshot(&snapshot, name, window, cx);
            state
        });
        Self::watch_tab(&tab, cx);
        self.tabs.push(tab);
        self.ui.active_tab = self.tabs.len() - 1;
        cx.notify();
    }

    // ---- Collections ---------------------------------------------------------

    /// Adds a new, empty collection and opens its rename popover so the user can
    /// name it immediately.
    pub(super) fn create_collection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = t(Str::DefaultCollectionName, cx).to_string();
        let id = self.collections.tree_mut().add_collection(name);
        self.collections.set_error(None);
        self.persist_collections(cx);
        self.begin_rename(id, window, cx);
    }

    /// Adds a folder under `parent` and opens its rename popover.
    pub(super) fn create_folder(
        &mut self,
        parent: NodeId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = t(Str::DefaultFolderName, cx).to_string();
        if let Some(id) = self.collections.tree_mut().add_folder(parent, name) {
            self.persist_collections(cx);
            self.begin_rename(id, window, cx);
        }
    }

    pub(super) fn toggle_node(&mut self, id: NodeId, expanded: bool, cx: &mut Context<Self>) {
        self.collections.tree_mut().set_expanded(id, expanded);
        self.persist_collections(cx);
        cx.notify();
    }

    pub(super) fn duplicate_node(&mut self, id: NodeId, cx: &mut Context<Self>) {
        self.collections.tree_mut().duplicate(id);
        self.node_menu_open = None;
        self.persist_collections(cx);
        cx.notify();
    }

    pub(super) fn delete_node(&mut self, id: NodeId, cx: &mut Context<Self>) {
        self.collections.tree_mut().remove(id);
        self.node_menu_open = None;
        if self.rename_target == Some(id) {
            self.rename_target = None;
        }
        self.persist_collections(cx);
        cx.notify();
    }

    /// Opens the rename popover for a node, seeding the field with its name.
    pub(super) fn begin_rename(
        &mut self,
        id: NodeId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let current = self
            .collections
            .tree()
            .roots()
            .iter()
            .find_map(|node| find_name(node, id))
            .unwrap_or_default();
        self.rename_input.update(cx, |state, cx| {
            state.set_value(current, window, cx);
        });
        self.rename_target = Some(id);
        self.node_menu_open = None;
        cx.notify();
    }

    pub(super) fn confirm_rename(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.rename_target else {
            return;
        };
        let name = self.rename_input.read(cx).value().trim().to_string();
        if !name.is_empty() {
            self.collections.tree_mut().rename(id, name);
            self.persist_collections(cx);
        }
        self.rename_target = None;
        cx.notify();
    }

    /// Opens a saved request from the tree into a tab.
    pub(super) fn open_saved_request(
        &mut self,
        id: NodeId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let opened = self.collections.tree().snapshot(id).cloned().map(|snapshot| {
            let name = self
                .collections
                .tree()
                .roots()
                .iter()
                .find_map(|node| find_name(node, id));
            (snapshot, name)
        });
        if let Some((snapshot, name)) = opened {
            self.open_snapshot(snapshot, name.map(Into::into), window, cx);
        }
    }

    /// Saves the active request into a collection under `name`, and names the
    /// tab. The save button and Duplicate both store requests this way.
    pub(super) fn save_active_request(&mut self, name: String, cx: &mut Context<Self>) {
        let Some(tab) = self.active_tab().cloned() else {
            return;
        };
        let trimmed = name.trim().to_string();
        if trimmed.is_empty() {
            return;
        }
        let snapshot = tab.read(cx).request.snapshot(cx);
        let default_collection = t(Str::DefaultCollectionName, cx).to_string();
        let collection = self
            .collections
            .tree_mut()
            .first_or_new_collection(default_collection);
        self.collections
            .tree_mut()
            .add_request(collection, trimmed.clone(), snapshot);
        self.collections.set_error(None);
        self.persist_collections(cx);

        tab.update(cx, |state, cx| {
            state.request.name = Some(trimmed.into());
            state.request.dirty = false;
            cx.notify();
        });
        cx.notify();
    }

    /// Imports a collection file the user picks (dodo's own or a Postman v2
    /// collection) and merges it into the tree.
    pub(super) fn import_collections(&mut self, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: None,
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = receiver.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let read = cx
                .background_executor()
                .spawn(async move { std::fs::read(&path).map_err(|err| err.to_string()) })
                .await;
            let _ = this.update(cx, |this, cx| {
                match read {
                    Ok(bytes) => match parse_import(&bytes) {
                        Ok(roots) => {
                            this.collections.tree_mut().import(roots);
                            this.collections.set_error(None);
                            this.persist_collections(cx);
                        }
                        Err(error) => this.collections.set_error(Some(error.message())),
                    },
                    Err(detail) => this
                        .collections
                        .set_error(Some(Str::CollectionImportError(detail))),
                }
                cx.notify();
            });
        })
        .detach();
    }

    // ---- History -------------------------------------------------------------

    pub(super) fn reopen_history(
        &mut self,
        id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(snapshot) = self.history.snapshot(id).cloned() {
            self.open_snapshot(snapshot, None, window, cx);
        }
    }

    /// Reopens a history entry and immediately sends it again.
    pub(super) fn resend_history(
        &mut self,
        id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(snapshot) = self.history.snapshot(id).cloned() {
            self.open_snapshot(snapshot, None, window, cx);
            self.send_active(window, cx);
        }
    }

    /// Saves a history entry into the collections, so a one-off request can be
    /// kept without reopening and re-saving it.
    pub(super) fn duplicate_history(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(snapshot) = self.history.snapshot(id).cloned() else {
            return;
        };
        let name = snapshot.summary();
        let default_collection = t(Str::DefaultCollectionName, cx).to_string();
        let collection = self
            .collections
            .tree_mut()
            .first_or_new_collection(default_collection);
        self.collections
            .tree_mut()
            .add_request(collection, name, snapshot);
        self.collections.set_error(None);
        self.persist_collections(cx);
        cx.notify();
    }

    pub(super) fn delete_history(&mut self, id: u64, cx: &mut Context<Self>) {
        self.history.remove(id);
        cx.notify();
    }

    pub(super) fn clear_history(&mut self, cx: &mut Context<Self>) {
        self.history.clear();
        cx.notify();
    }

    // ---- Response body export ------------------------------------------------

    /// Saves the active response body to a file the user picks.
    pub(super) fn save_body_to_file(&mut self, cx: &mut Context<Self>) {
        let Some(tab) = self.active_tab().cloned() else {
            return;
        };
        let Some(body) = tab
            .read(cx)
            .response
            .exchange()
            .map(|exchange| exchange.body.clone())
        else {
            return;
        };
        let directory = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(data_dir);
        let receiver = cx.prompt_for_new_path(&directory, Some("response.txt"));
        cx.spawn(async move |_, cx| {
            let Ok(Ok(Some(path))) = receiver.await else {
                return;
            };
            cx.background_executor()
                .spawn(async move {
                    let _ = file_export::write_file(&path, body.as_bytes());
                })
                .await;
        })
        .detach();
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

        for (field, str) in [
            (&self.name_input, Str::NameRequestPlaceholder),
            (&self.search_input, Str::SearchCollectionsPlaceholder),
            (&self.rename_input, Str::NamePlaceholder),
        ] {
            let placeholder = t(str, cx);
            field.update(cx, |state, cx| {
                state.set_placeholder(placeholder, window, cx);
            });
        }

        // The URL field, every key/value cell, both script panes and every auth
        // field hold their own placeholder; `RequestState` owns the sweep so
        // that adding a field is one edit there rather than two.
        for tab in &self.tabs {
            tab.update(cx, |tab, cx| {
                tab.request.sync_placeholders(window, cx);
            });
        }
    }

    /// The far-left rail, always visible: it selects which panel the left
    /// column shows (Collections or History) and collapses it. Clicking the
    /// panel that is already showing collapses the column; clicking the other
    /// switches to it and expands.
    fn left_rail(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let collapsed = self.ui.panel_collapsed;
        let active = self.ui.left_panel;

        let rail_button = |panel: LeftPanel, icon: AppIcon, tooltip: Str, id: &'static str| {
            let selected = !collapsed && active == panel;
            Button::new(id)
                .ghost()
                .selected(selected)
                .icon(icon)
                .tooltip(t(tooltip, cx))
                .on_click(cx.listener(move |this, _, _, cx| {
                    if this.ui.left_panel == panel && !this.ui.panel_collapsed {
                        this.ui.panel_collapsed = true;
                    } else {
                        this.ui.left_panel = panel;
                        this.ui.panel_collapsed = false;
                    }
                    cx.notify();
                }))
        };

        v_flex()
            .h_full()
            .w(px(44.))
            .py_2()
            .gap_1()
            .items_center()
            .border_r_1()
            .border_color(cx.theme().border)
            .child(rail_button(
                LeftPanel::Collections,
                AppIcon::Folder,
                Str::Collections,
                "rail-collections",
            ))
            .child(rail_button(
                LeftPanel::History,
                AppIcon::Clock,
                Str::History,
                "rail-history",
            ))
            .into_any_element()
    }

    /// The left column's body: the selected panel.
    fn left_panel(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        match self.ui.left_panel {
            LeftPanel::Collections => self.render_collections_panel(cx),
            LeftPanel::History => self.render_history_panel(cx),
        }
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
                    // The request pane has no fixed size, so it grows to fill
                    // whatever the response pane leaves — the request is the
                    // pane being edited before a response exists, and each of
                    // its tabs fills the height rather than a fixed stub.
                    .child(
                        resizable_panel()
                            .size_range(REQUEST_MIN_HEIGHT..px(1200.))
                            .child(self.render_request_editor(window, cx)),
                    )
                    // The response pane is the sized one: it opens at a sensible
                    // default and holds it, so a response arriving does not
                    // reshuffle the split, and the divider is still draggable.
                    .child(
                        resizable_panel()
                            .size(RESPONSE_HEIGHT)
                            .size_range(px(120.)..px(1200.))
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

        let panel = self.left_panel(cx);

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
                h_flex()
                    .size_full()
                    // The rail is always present; only the panel beside it
                    // collapses. A collapsed panel is dropped from the resizable
                    // group entirely — a hidden panel would still take part in
                    // the drag arithmetic.
                    .child(self.left_rail(cx))
                    .child(if self.ui.panel_collapsed {
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .child(right)
                            .into_any_element()
                    } else {
                        h_resizable("api-explorer-split")
                            .with_state(&self.ui.outer_split)
                            .child(
                                resizable_panel()
                                    .size(COLLECTIONS_WIDTH)
                                    .size_range(px(150.)..px(460.))
                                    .child(panel),
                            )
                            .child(resizable_panel().child(right))
                            .into_any_element()
                    }),
            )
    }
}

/// The name of node `id` if it is somewhere in this subtree.
fn find_name(node: &crate::api_explorer::models::collection::Node, id: NodeId) -> Option<String> {
    if node.id == id {
        return Some(node.name.clone());
    }
    node.children.iter().find_map(|child| find_name(child, id))
}
