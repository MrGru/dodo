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
use gpui_component::input::{InputEvent, InputState};
use gpui_component::resizable::{h_resizable, resizable_panel, v_resizable};
use gpui_component::{ActiveTheme as _, Selectable as _, h_flex, v_flex};

use crate::app_icon::AppIcon;
use crate::i18n::{Language, LanguageExt, Str, api_collections, api_explorer, api_variables, t};
use crate::models::collection::{CollectionTree, NodeId};
use crate::models::script::{SkipReason, VariableWrite, is_runnable};
use crate::models::script_consent::{ConsentDecision, ConsentKey, ConsentLedger};
use crate::models::snapshot::RequestSnapshot;
use crate::paths::data_dir;
use crate::services::collection_import::parse_import;
use crate::services::collection_store::{CollectionStore, DiskCollectionStore};
use crate::services::consent_store::{ConsentStore, DiskConsentStore};
use crate::services::environment_import::parse_environment_import;
use crate::services::script::{QuickJsEngine, ScriptContext, ScriptEngine};
use crate::services::send::ScriptJob;
use crate::services::variable_store::{DiskVariableStore, VariableStore, VariableStoreError};
use crate::services::{Protocol, TransportRegistry, curl, file_export, file_picker};
use crate::state::collection::CollectionState;
use crate::state::environment::EnvironmentState;
use crate::state::history::{History, HistoryRecord};
use crate::state::request::{ScriptSlot, is_bulk_change};
use crate::state::tab::{RequestTabState, ScriptWrites};
use crate::state::ui::{
    COLLECTIONS_WIDTH, LeftPanel, REQUEST_MIN_HEIGHT, RESPONSE_HEIGHT, UiState,
};
use crate::views::{generate_code, script_consent};
use crate::{ScriptPolicy, SendRequest};

/// The key context the send shortcut is bound in. Matching happens up the
/// focus chain, so the binding fires from inside the URL field too.
pub const KEY_CONTEXT: &str = "ApiExplorer";

pub struct ApiExplorer {
    /// Every open request. Each is its own entity, so sending in one leaves the
    /// others untouched.
    pub(super) tabs: Vec<Entity<RequestTabState>>,
    pub(super) ui: UiState,
    pub(super) collections: CollectionState,
    /// The environments and their variables, page-wide rather than per-tab:
    /// switching environment re-resolves every open request, which is the point
    /// of having one.
    pub(super) environments: EnvironmentState,
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
    /// Where environments are persisted, behind a trait for the same reason.
    variable_store: Arc<dyn VariableStore>,
    /// Which imported scripts the user has already approved, and where those
    /// approvals are kept.
    pub(super) consent: ConsentLedger,
    consent_store: Arc<dyn ConsentStore>,
    /// The JavaScript engine. An `Arc<dyn ScriptEngine>` for the same reason
    /// the transport is a trait object: this view never learns which engine is
    /// behind it, and a build without one swaps in `NullEngine`.
    engine: Arc<dyn ScriptEngine>,
    /// Whether the method dropdown is showing. Held here rather than inside the
    /// popover so that picking a method can close it.
    pub(super) method_menu_open: bool,
    /// Whether the environment picker's popover is showing, held here for the
    /// same reason: choosing an environment has to close it.
    pub(super) environment_menu_open: bool,
    /// Whether the request-naming popover is showing.
    pub(super) save_menu_open: bool,
    /// Whether the Auth tab's scheme dropdown is showing.
    pub(super) auth_menu_open: bool,
    /// Whether each Scripts-tab templates popover is showing. Held here rather
    /// than inside the popover so picking a template can close it.
    pub(super) pre_template_menu_open: bool,
    pub(super) post_template_menu_open: bool,
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
        Self::watch_tab(&first, window, cx);

        let name_placeholder = t(api_explorer::Text::NameRequestPlaceholder, cx);
        let name_input = cx.new(|cx| InputState::new(window, cx).placeholder(name_placeholder));
        let search_placeholder = t(api_explorer::Text::SearchCollectionsPlaceholder, cx);
        let search_input = cx.new(|cx| InputState::new(window, cx).placeholder(search_placeholder));
        let rename_placeholder = t(api_variables::Text::NamePlaceholder, cx);
        let rename_input = cx.new(|cx| InputState::new(window, cx).placeholder(rename_placeholder));

        let collection_store: Arc<dyn CollectionStore> = Arc::new(DiskCollectionStore::new());
        Self::load_collections(collection_store.clone(), cx);

        let variable_store: Arc<dyn VariableStore> = Arc::new(DiskVariableStore::new());
        Self::load_environments(variable_store.clone(), cx);

        let consent_store: Arc<dyn ConsentStore> = Arc::new(DiskConsentStore::new());
        Self::load_consent(consent_store.clone(), cx);

        Self {
            tabs: vec![first],
            ui: UiState::new(cx),
            collections: CollectionState::default(),
            environments: EnvironmentState::default(),
            history: History::default(),
            transports: TransportRegistry::with_defaults(),
            collection_store,
            variable_store,
            consent: ConsentLedger::default(),
            consent_store,
            engine: Arc::new(QuickJsEngine),
            method_menu_open: false,
            environment_menu_open: false,
            save_menu_open: false,
            auth_menu_open: false,
            pre_template_menu_open: false,
            post_template_menu_open: false,
            name_input,
            search_input,
            rename_input,
            rename_target: None,
            node_menu_open: None,
            language: Language::current(cx),
            focus_handle: cx.focus_handle(),
        }
    }

    /// Subscribes to a tab so its completed requests land in history, and to
    /// its URL field so that a pasted cURL command becomes a request rather
    /// than a very long URL.
    ///
    /// Every tab — the first, a new one, or one opened from a saved request —
    /// is watched through here, so nothing sent can escape the record.
    fn watch_tab(tab: &Entity<RequestTabState>, window: &mut Window, cx: &mut Context<Self>) {
        cx.subscribe(tab, |this, _tab, record: &HistoryRecord, cx| {
            this.history.record(record.clone());
            cx.notify();
        })
        .detach();

        // Variables are cross-tab state, so a tab never writes them: it says
        // what its script did and the page — which owns the environments and
        // the store — applies and persists. The same seam history already uses.
        cx.subscribe(tab, |this, _tab, writes: &ScriptWrites, cx| {
            this.apply_script_writes(&writes.0, cx);
        })
        .detach();

        let url = tab.read(cx).request.url.clone();
        let watched = tab.clone();
        cx.subscribe_in(
            &url,
            window,
            move |this, url, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::Change) {
                    let text = url.read(cx).value().to_string();
                    this.on_url_changed(&watched, text, window, cx);
                }
            },
        )
        .detach();

        // Both script editors are re-parsed after a pause, so a syntax error is
        // found where it was typed rather than discovered by pressing Send. The
        // engine is the page's, which is what keeps the editor and the Console
        // from disagreeing.
        for slot in ScriptSlot::ALL {
            let editor = tab.read(cx).request.script_editor(slot).clone();
            let checked = tab.clone();
            cx.subscribe_in(
                &editor,
                window,
                move |this, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::Change) {
                        let engine = this.engine.clone();
                        checked.update(cx, |tab, cx| tab.check_script(slot, engine, window, cx));
                    }
                },
            )
            .detach();
        }
    }

    /// Reacts to the URL field changing, which is where a pasted cURL command
    /// is caught.
    ///
    /// Two things have to be true before anything happens: the change has to
    /// look like a **paste** rather than a keystroke ([`is_bulk_change`]), and
    /// the text has to parse as a cURL command with a URL in it. Without the
    /// first check, typing the letters of the word "curl" into the box would
    /// trip the importer halfway through.
    ///
    /// Nothing this method writes back can re-enter it: `InputState::set_value`
    /// clears the widget's `emit_events` flag for the duration, so a
    /// programmatic write is silent where a paste is not. `last_url` is still
    /// kept in step by hand on both branches, so the *next* real edit compares
    /// against what is actually on screen.
    ///
    /// # Nothing is overwritten
    ///
    /// A parsed command opens a **new tab**, and the field it was pasted into
    /// is put back exactly as it was — so unsaved work in the tab you happened
    /// to be looking at cannot be lost, and there is nothing to undo. The one
    /// exception is a tab that has nothing in it (unnamed, unedited, empty
    /// URL): reusing that is what stops "paste, paste, paste" from leaving a
    /// trail of blank tabs, and by definition loses nothing.
    ///
    /// That policy lives in [`Self::adopt_pasted_request`], because quick
    /// navigation reaches it too — a `curl` command pasted at the app rather
    /// than into this field must land in exactly the same place.
    fn on_url_changed(
        &mut self,
        tab: &Entity<RequestTabState>,
        text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let previous = tab.read(cx).request.last_url.clone();
        if previous == text {
            return;
        }
        tab.update(cx, |state, _| state.request.last_url = text.clone());

        if !is_bulk_change(&previous, &text) || !curl::looks_like_curl(&text) {
            return;
        }
        let Some(snapshot) = curl::parse(&text) else {
            return;
        };

        // The tab pasted into is reusable only if there is nothing in it to
        // protect. Judged before the field is restored, because `previous` is
        // what was there before the paste.
        let reuse = is_pristine(tab, &previous, cx).then(|| tab.clone());

        if reuse.is_none() {
            // Put the field back before opening the new tab, so the tab left
            // behind is byte-for-byte what it was.
            tab.update(cx, |state, cx| {
                state.request.last_url = previous.clone();
                state
                    .request
                    .url
                    .update(cx, |input, cx| input.set_value(previous, window, cx));
            });
        }

        self.adopt_pasted_request(snapshot, reuse, window, cx);
    }

    /// Puts a request that arrived from outside in front of the user.
    ///
    /// Both cURL paste routes end here — the URL field above, and quick
    /// navigation through [`Self::accept_curl`] — so there is one policy and not
    /// two. `reuse` is the tab whose contents the caller has established are
    /// worth nothing; `None` means open a fresh one.
    ///
    /// Either way the tab is marked dirty: imported is not saved, and the
    /// unsaved dot is the honest state.
    fn adopt_pasted_request(
        &mut self,
        snapshot: RequestSnapshot,
        reuse: Option<Entity<RequestTabState>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(tab) = reuse {
            tab.update(cx, |state, cx| {
                state.request.apply_snapshot(&snapshot, None, window, cx);
                state.request.dirty = true;
                cx.notify();
            });
            self.refresh_body_file(&tab, cx);
        } else {
            self.open_snapshot(snapshot, None, None, window, cx);
            if let Some(opened) = self.active_tab().cloned() {
                opened.update(cx, |state, cx| {
                    state.request.dirty = true;
                    cx.notify();
                });
                self.refresh_body_file(&opened, cx);
            }
        }
        cx.notify();
    }

    /// Quick navigation's entry point (`quick_nav`): a cURL command that was
    /// pasted at the app rather than into the URL field.
    ///
    /// Deliberately the **same** path as the URL field's, right down to the
    /// blank-tab reuse — a pasted command behaves identically however it
    /// arrived, and there is no second import to keep in step.
    pub fn accept_curl(
        &mut self,
        snapshot: RequestSnapshot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let reuse = self.active_tab().cloned().filter(|tab| {
            let url = tab.read(cx).request.last_url.clone();
            is_pristine(tab, &url, cx)
        });
        self.adopt_pasted_request(snapshot, reuse, window, cx);
    }

    /// Re-reads the size of the file a restored Binary body points at.
    ///
    /// The size is a property of the file rather than of the request, so it is
    /// never saved; a snapshot arriving from a collection, from history or from
    /// a pasted cURL command asks the filesystem again — on the background
    /// executor, like every other `stat` in this module.
    fn refresh_body_file(&self, tab: &Entity<RequestTabState>, cx: &mut Context<Self>) {
        let path = tab.read(cx).request.binary_path.clone();
        if path.trim().is_empty() {
            return;
        }
        file_picker::refresh_size(tab.clone(), PathBuf::from(path), cx, |state, size, cx| {
            state.request.binary_size = size;
            cx.notify();
        });
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

    // ---- Environments --------------------------------------------------------

    /// Loads the saved environments off disk on the background executor, then
    /// installs them.
    ///
    /// A missing file is an empty document; a read, parse or **schema version**
    /// failure is shown rather than swallowed. That last one is the whole point
    /// of the version field: a file from a newer dodo leaves the picker empty
    /// and says why, instead of half-loading someone's tokens.
    fn load_environments(store: Arc<dyn VariableStore>, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let loaded = cx
                .background_executor()
                .spawn(async move { store.load() })
                .await;
            let _ = this.update(cx, |this, cx| {
                match loaded {
                    Ok(document) => this.environments.set_document(document),
                    Err(error) => this.environments.mark_load_failed(error.message()),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Writes the current environments to the store on the background executor.
    /// Called after every structural edit and when a field loses focus, not on
    /// every keystroke.
    pub(super) fn persist_environments(&mut self, cx: &mut Context<Self>) {
        let document = self.environments.document().clone();
        let store = self.variable_store.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { store.persist(&document) })
                .await;
            if let Err(error) = result {
                let _ = this.update(cx, |this, cx| {
                    this.environments.set_error(Some(error.message()));
                    cx.notify();
                });
            }
        })
        .detach();
    }

    // ---- Script approvals ----------------------------------------------------

    /// Loads the approvals off disk on the background executor.
    ///
    /// A failure **fails closed**: the ledger stays empty, so every imported
    /// script asks again. The alternative — treating an unreadable file as
    /// "everything is approved" — is the one outcome this feature exists to
    /// prevent. It is reported through the collections banner rather than
    /// silently, because the user is about to be asked questions they thought
    /// they had answered.
    fn load_consent(store: Arc<dyn ConsentStore>, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let loaded = cx
                .background_executor()
                .spawn(async move { store.load() })
                .await;
            let _ = this.update(cx, |this, cx| {
                match loaded {
                    Ok(document) => this.consent.set_document(document),
                    Err(error) => this.collections.set_error(Some(error.message())),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn persist_consent(&mut self, cx: &mut Context<Self>) {
        let document = self.consent.document().clone();
        let store = self.consent_store.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { store.persist(&document) })
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

    /// Reads an environment file the user picks and merges it in.
    ///
    /// Its own picker rather than the collections one: the two file shapes are
    /// different, and guessing which a picked file is would turn a mis-click
    /// into a puzzling error.
    pub(super) fn import_environments(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, Option<u64>, &mut Window, &mut Context<Self>) + 'static,
    ) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: None,
        });
        cx.spawn_in(window, async move |this, cx| {
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
            let _ = this.update_in(cx, |this, window, cx| {
                let selected = match read {
                    Ok(bytes) => match parse_environment_import(&bytes) {
                        Ok(environments) => {
                            let selected = this.environments.import(environments);
                            this.environments.set_error(None);
                            this.persist_environments(cx);
                            selected
                        }
                        Err(error) => {
                            this.environments.set_error(Some(error.message()));
                            None
                        }
                    },
                    Err(detail) => {
                        this.environments
                            .set_error(Some(VariableStoreError::Io { detail }.message()));
                        None
                    }
                };
                then(this, selected, window, cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// The tab currently in front, if there is one.
    pub(super) fn active_tab(&self) -> Option<&Entity<RequestTabState>> {
        self.tabs.get(self.ui.active_tab)
    }

    pub(super) fn open_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let tab = cx.new(|cx| RequestTabState::new(window, cx));
        Self::watch_tab(&tab, window, cx);
        self.tabs.push(tab);
        self.ui.active_tab = self.tabs.len() - 1;
        cx.notify();
    }

    /// Opens a saved request or a history entry in a fresh tab, restoring its
    /// full state. The tab is watched like any other, so resending from it is
    /// recorded again.
    /// `node` is the collection node the snapshot came from, when it came from
    /// one. It is half of a script's consent key, so a history reopen or a
    /// pasted cURL command passes `None` rather than borrowing someone else's.
    pub(super) fn open_snapshot(
        &mut self,
        snapshot: RequestSnapshot,
        name: Option<gpui::SharedString>,
        node: Option<NodeId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let tab = cx.new(|cx| {
            let mut state = RequestTabState::new(window, cx);
            state.request.apply_snapshot(&snapshot, name, window, cx);
            state.request.origin_node = node;
            state
        });
        Self::watch_tab(&tab, window, cx);
        self.tabs.push(tab.clone());
        self.ui.active_tab = self.tabs.len() - 1;
        self.refresh_body_file(&tab, cx);
        cx.notify();
    }

    // ---- Collections ---------------------------------------------------------

    /// Adds a new, empty collection and opens its rename popover so the user can
    /// name it immediately.
    pub(super) fn create_collection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = t(api_explorer::Text::DefaultCollectionName, cx).to_string();
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
        let name = t(api_explorer::Text::DefaultFolderName, cx).to_string();
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
    pub(super) fn begin_rename(&mut self, id: NodeId, window: &mut Window, cx: &mut Context<Self>) {
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
        if let Some(index) = tab_index_for_node(
            self.tabs.iter().map(|tab| tab.read(cx).request.origin_node),
            id,
        ) {
            self.ui.active_tab = index;
            cx.notify();
            return;
        }

        let opened = self
            .collections
            .tree()
            .snapshot(id)
            .cloned()
            .map(|snapshot| {
                let name = self
                    .collections
                    .tree()
                    .roots()
                    .iter()
                    .find_map(|node| find_name(node, id));
                (snapshot, name)
            });
        if let Some((snapshot, name)) = opened {
            self.open_snapshot(snapshot, name.map(Into::into), Some(id), window, cx);
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
        let default_collection = t(api_explorer::Text::DefaultCollectionName, cx).to_string();
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
                        Ok(imported) => {
                            this.collections.tree_mut().import(imported.roots);
                            this.collections.set_error(None);
                            this.persist_collections(cx);
                            // A Postman collection's own `variable` array comes
                            // with it: without them every `{{baseUrl}}` in the
                            // requests just imported would fail to resolve.
                            if !imported.variables.is_empty() {
                                this.environments
                                    .import_collection_variables(imported.variables);
                                this.persist_environments(cx);
                            }
                        }
                        Err(error) => this.collections.set_error(Some(error.message())),
                    },
                    Err(detail) => this.collections.set_error(Some(
                        api_collections::Text::CollectionImportError(detail).into(),
                    )),
                }
                cx.notify();
            });
        })
        .detach();
    }

    // ---- History -------------------------------------------------------------

    pub(super) fn reopen_history(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(snapshot) = self.history.snapshot(id).cloned() {
            self.open_snapshot(snapshot, None, None, window, cx);
        }
    }

    /// Reopens a history entry and immediately sends it again.
    pub(super) fn resend_history(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(snapshot) = self.history.snapshot(id).cloned() {
            self.open_snapshot(snapshot, None, None, window, cx);
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
        let default_collection = t(api_explorer::Text::DefaultCollectionName, cx).to_string();
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

    // ---- Generated code ------------------------------------------------------

    /// Opens the Generate code dialog for the request in front.
    ///
    /// Both of the dialog's inputs are read **here** and handed over as plain
    /// data: the dialog holds no page handle, so it cannot read the page it was
    /// opened from — which is what makes the leasing trap that
    /// `views::generate_code` documents impossible rather than merely avoided.
    /// The variable set is read at the moment the button is pressed, exactly as
    /// `start_send` reads it, so the snippet resolves against the environment
    /// that was active then.
    pub(super) fn open_generate_code(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.active_tab().cloned() else {
            return;
        };
        let snapshot = tab.read(cx).request.snapshot(cx);
        let variables = self.environments.variable_set();
        generate_code::open(snapshot, variables, window, cx);
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
    ///
    /// The one decision that happens *here* rather than on the background
    /// executor is whether the pre-request script may run: the setting and the
    /// approvals ledger both live on the UI thread, and asking the user is by
    /// definition a UI act. Everything after that is one background job.
    pub(super) fn send_active(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.active_tab().cloned() else {
            return;
        };

        let (origin, node, pre, post, name) = {
            let state = &tab.read(cx).request;
            (
                state.script_origin,
                state.origin_node,
                state.pre_request_script.read(cx).value().to_string(),
                state.post_response_script.read(cx).value().to_string(),
                state.display_name(cx).to_string(),
            )
        };

        // One decision for both hooks: they belong to the same request and run
        // in the same send, so two prompts for one Send would be theatre.
        match self
            .consent
            .decide(ScriptPolicy::current(cx), origin, node, &pre, &post)
        {
            ConsentDecision::NoScript => {
                self.start_send(&tab, (ScriptJob::None, ScriptJob::None), window, cx)
            }
            ConsentDecision::Run => {
                let jobs = self.script_jobs(&tab, cx);
                self.start_send(&tab, jobs, window, cx);
            }
            ConsentDecision::Skip => self.start_send(
                &tab,
                (
                    ScriptJob::Skipped(SkipReason::PolicyDisabled),
                    ScriptJob::Skipped(SkipReason::PolicyDisabled),
                ),
                window,
                cx,
            ),
            // The send does not start until the prompt is answered; dismissing
            // it leaves the request exactly where it was.
            ConsentDecision::Ask { re_armed } => script_consent::open(
                cx.entity(),
                tab,
                name,
                script_consent::Scripts {
                    pre: pre.clone(),
                    post: post.clone(),
                    re_armed,
                },
                ConsentKey::new(node, &pre, &post),
                window,
                cx,
            ),
        }
    }

    /// The consent prompt's "Run script": remember the approval, then send.
    pub(super) fn approve_script_and_send(
        &mut self,
        tab: &Entity<RequestTabState>,
        key: ConsentKey,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.consent.approve(&key);
        self.persist_consent(cx);
        let jobs = self.script_jobs(tab, cx);
        self.start_send(tab, jobs, window, cx);
    }

    /// The consent prompt's "Send without it". Deliberately **not** recorded:
    /// declining once is not a durable statement, and the next send should ask
    /// again.
    pub(super) fn send_without_script(
        &mut self,
        tab: &Entity<RequestTabState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.start_send(
            tab,
            (
                ScriptJob::Skipped(SkipReason::ConsentDeclined),
                ScriptJob::Skipped(SkipReason::ConsentDeclined),
            ),
            window,
            cx,
        );
    }

    /// Everything the engine needs about the scopes, read on the UI thread —
    /// one job per hook, and [`ScriptJob::None`] for a hook with no script so
    /// the send path can tell "empty" from "declined".
    fn script_jobs(
        &self,
        tab: &Entity<RequestTabState>,
        cx: &Context<Self>,
    ) -> (ScriptJob, ScriptJob) {
        let state = &tab.read(cx).request;
        let environment =
            ScriptContext::scope_map(self.environments.variables(self.environments.active_id()));
        let collection = ScriptContext::scope_map(self.environments.variables(None));
        let request_name = state.display_name(cx).to_string();

        let job = |source: String| {
            if is_runnable(&source) {
                ScriptJob::Run {
                    source,
                    environment: environment.clone(),
                    collection: collection.clone(),
                    request_name: request_name.clone(),
                }
            } else {
                ScriptJob::None
            }
        };

        (
            job(state.pre_request_script.read(cx).value().to_string()),
            job(state.post_response_script.read(cx).value().to_string()),
        )
    }

    fn start_send(
        &mut self,
        tab: &Entity<RequestTabState>,
        scripts: (ScriptJob, ScriptJob),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Phase 1 declares HTTP; a request that carries its own protocol picks
        // the backend the same way.
        let Some(transport) = self.transports.get(Protocol::Http) else {
            return;
        };
        // Read here, on the UI thread, so the request resolves against the
        // environment that was active when Send was pressed.
        let variables = self.environments.variable_set();
        let engine = self.engine.clone();
        tab.update(cx, |tab, cx| {
            tab.send(transport, engine, variables, scripts, window, cx)
        });
        cx.notify();
    }

    /// Applies the variables a pre-request script wrote and saves them.
    ///
    /// The rules live on [`EnvironmentState::apply_script_write`], which is
    /// pure and testable; this is the part that needs `cx`.
    fn apply_script_writes(&mut self, writes: &[VariableWrite], cx: &mut Context<Self>) {
        // A plain loop rather than `any`, which short-circuits: every write
        // has to be applied, not just the ones before the first that landed.
        let mut changed = false;
        for write in writes {
            changed |= self.environments.apply_script_write(write);
        }

        if changed {
            self.persist_environments(cx);
            cx.notify();
        }
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
            (
                &self.name_input,
                Str::from(api_explorer::Text::NameRequestPlaceholder),
            ),
            (
                &self.search_input,
                Str::from(api_explorer::Text::SearchCollectionsPlaceholder),
            ),
            (
                &self.rename_input,
                Str::from(api_variables::Text::NamePlaceholder),
            ),
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
                api_collections::Text::Collections.into(),
                "rail-collections",
            ))
            .child(rail_button(
                LeftPanel::History,
                AppIcon::Clock,
                api_collections::Text::History.into(),
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
    fn request_and_response(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        v_flex()
            .size_full()
            .min_w_0()
            .child(self.render_tab_strip(cx))
            .child(
                // The request-tab strip above stays outside this clipped,
                // resizable region, so it remains visible while a pane scrolls
                // or the response divider is dragged upward.
                div().flex_1().min_h_0().child(
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

/// Whether this tab holds nothing worth protecting, so an arriving request may
/// take it over instead of opening a new one.
///
/// `url` is the tab's URL text as it stood *before* whatever prompted the
/// question — which is not always what the field holds now, because a paste into
/// the URL field has already replaced it by the time this is asked.
fn is_pristine(tab: &Entity<RequestTabState>, url: &str, cx: &App) -> bool {
    let state = tab.read(cx);
    state.request.name.is_none() && !state.request.dirty && url.trim().is_empty()
}

/// The index of the tab opened from collection node `id`.
fn tab_index_for_node(
    origins: impl IntoIterator<Item = Option<NodeId>>,
    id: NodeId,
) -> Option<usize> {
    origins.into_iter().position(|origin| origin == Some(id))
}

/// The name of node `id` if it is somewhere in this subtree.
fn find_name(node: &crate::models::collection::Node, id: NodeId) -> Option<String> {
    if node.id == id {
        return Some(node.name.clone());
    }
    node.children.iter().find_map(|child| find_name(child, id))
}

#[cfg(test)]
mod tests {
    use super::tab_index_for_node;

    #[test]
    fn finds_the_existing_tab_for_a_collection_request() {
        assert_eq!(tab_index_for_node([None, Some(7), Some(9)], 7), Some(1));
        assert_eq!(tab_index_for_node([None, Some(7), Some(9)], 8), None);
    }
}
