//! The Database Explorer's page: one tree on the left, the query editor and its
//! result on the right.
//!
//! # The connections *are* the tree's roots
//!
//! Not a connection list above an object tree — one tree, whose top level is
//! the saved connections and whose branches are their databases, schemas and
//! tables. That is why nothing here clears "the tree": every connection has its
//! own, they are all open at once, and
//! [`Forest`](crate::database::state::tree::Forest) holds the lot. Selecting a
//! connection therefore costs nothing, which is what lets the user read one
//! database's columns while the editor runs against another.
//!
//! The per-connection actions — Connect, Disconnect, Edit, Duplicate, Delete —
//! are a **right-click context menu on the root row**, built by
//! `connections_panel`, not buttons on the row. The row itself carries the
//! engine's mark, the status dot, and a hover card with the connection's
//! details.
//!
//! # Everything that touches a database happens off the UI thread
//!
//! Connecting, pinging, expanding a tree node, running a statement and writing
//! `connections.json` are all blocking, and every one of them is spawned onto
//! GPUI's background executor. Nothing in this file calls a
//! [`Driver`](crate::database::services::Driver) method directly.
//!
//! # The live handles live here, not in `state/`
//!
//! `state::connections` holds what is *saved* and what each connection's status
//! is; the `Arc<dyn Driver>` handles are this view's, keyed by profile id. That
//! is what keeps the state layer testable with no server, and it is why
//! disconnecting is "drop the handle and clear the tree" in one place.
//!
//! # The object tree is rebuilt from the model, never mutated in the widget
//!
//! `TreeState::set_items` replaces the widget's items — and their expanded
//! flags — so the widget cannot be the authority on what is open.
//! [`Forest`](crate::database::state::tree::Forest) is, and this view rebuilds
//! the items from it whenever it changes. `state::tree`'s module doc has the
//! second, sharper reason.
//!
//! # Several query tabs, one result grid
//!
//! [`QueryTabs`] holds them: each tab has its own editor entity, its own run in
//! flight and its own [`QueryState`]. The `TableState` is **shared** — only one
//! grid is ever on screen — so switching tabs re-fills the delegate from the
//! newly active tab's result ([`DatabaseView::show_active_result`]). A
//! background run therefore has to look its tab up **by id**, not by index: the
//! user may close a tab to its left, or switch away, while it is still running.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use gpui::{
    App, AppContext as _, ClipboardItem, Context, Entity, FocusHandle, Focusable, Hsla,
    InteractiveElement as _, IntoElement, ParentElement as _, Pixels, Render, SharedString,
    Styled as _, Subscription, Task, Window, div, px,
};
use gpui_component::input::InputState;
use gpui_component::resizable::{ResizableState, h_resizable, resizable_panel};
use gpui_component::table::TableState;
use gpui_component::tree::{TreeEvent, TreeItem, TreeState};
use gpui_component::{ActiveTheme as _, WindowExt as _};

use crate::app_icon::AppIcon;
use crate::database::models::catalog::{NodeId, NodeKind, NodeLabel};
use crate::database::models::connection::ConnectionProfile;
use crate::database::models::engine::Engine;
use crate::database::models::page::PageBudget;
use crate::database::models::sql_format;
use crate::database::services::connection_store::{ConnectionStore, DiskConnectionStore};
use crate::database::services::export::{self, ExportFormat};
use crate::database::services::{self, Driver};
use crate::database::state::connections::{ConnectionsState, Status};
use crate::database::state::query::{self, QueryState};
use crate::database::state::tabs::{QueryTab, QueryTabs};
use crate::database::state::tree::{Content, Forest, Notice, Outline, RowRef};
use crate::database::views::connection_form::{self, ConnectionForm, FormEvent};
use crate::database::views::result_grid::ResultDelegate;
use crate::database::{DatabaseCopyCell, DatabaseCopyRow, KEY_CONTEXT};
use crate::i18n::{Language, Str, t};
use crate::paths::data_dir;

/// The left panel's default width, and the range the divider allows.
const PANEL_WIDTH: Pixels = px(280.);
const PANEL_MIN: Pixels = px(200.);
const PANEL_MAX: Pixels = px(520.);

/// The query editor's default height. The result grid takes the rest, because
/// the result is what grows.
pub(super) const EDITOR_HEIGHT: Pixels = px(200.);
pub(super) const EDITOR_MIN: Pixels = px(90.);

/// How far each tree level is indented, and the padding of the first.
pub(super) const TREE_INDENT: Pixels = px(14.);
pub(super) const TREE_PADDING: Pixels = px(8.);

pub struct DatabaseView {
    pub(super) connections: ConnectionsState,
    /// The live handles. Present exactly for the connections whose status is
    /// [`Status::Connected`].
    drivers: HashMap<u64, Arc<dyn Driver>>,
    store: Arc<dyn ConnectionStore>,

    /// One [`CatalogTree`](crate::database::state::tree::CatalogTree) per
    /// connection, plus which connection roots are open. The panel is one tree
    /// whose roots are the connections; this is that arrangement.
    pub(super) forest: Forest,
    pub(super) tree_state: Entity<TreeState>,

    /// The open query tabs. Always at least one.
    pub(super) tabs: QueryTabs,
    /// Shared by every tab, because only one result is ever on screen.
    pub(super) table: Entity<TableState<ResultDelegate>>,

    outer_split: Entity<ResizableState>,
    pub(super) inner_split: Entity<ResizableState>,

    /// The open connection form, kept so its subscription outlives the dialog.
    form: Option<Entity<ConnectionForm>>,
    _form_subscription: Option<Subscription>,
    _tree_subscription: Subscription,

    /// In-flight work, held so a new request replaces the old one. A query's
    /// task is **not** here: it belongs to its tab, so a run in one tab is not
    /// cancelled by a run in another.
    connect_task: Option<Task<()>>,
    children_task: Option<Task<()>>,
    save_task: Option<Task<()>>,

    /// A failure from the store itself — the file could not be read or written.
    /// Held as a [`Str`] rather than rendered text so it re-translates.
    pub(super) store_error: Option<Str>,

    focus_handle: FocusHandle,
    language: Language,
}

impl DatabaseView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let tree_state = cx.new(|cx| TreeState::new(cx));
        let tree_subscription =
            cx.subscribe(&tree_state, |this, _, event: &TreeEvent, cx| match event {
                TreeEvent::Expanded(id) => this.on_expanded(id.to_string(), cx),
                TreeEvent::Collapsed(id) => this.on_collapsed(id.to_string(), cx),
            });

        let table = cx.new(|cx| {
            TableState::new(ResultDelegate::default(), window, cx)
                .cell_selectable(true)
                .row_header(false)
        });

        let mut this = Self {
            connections: ConnectionsState::new(),
            drivers: HashMap::new(),
            store: Arc::new(DiskConnectionStore::new()),
            forest: Forest::new(),
            tree_state,
            tabs: QueryTabs::new(),
            table,
            outer_split: cx.new(|_| ResizableState::default()),
            inner_split: cx.new(|_| ResizableState::default()),
            form: None,
            _form_subscription: None,
            _tree_subscription: tree_subscription,
            connect_task: None,
            children_task: None,
            save_task: None,
            store_error: None,
            focus_handle: cx.focus_handle(),
            language: Language::current(cx),
        };
        // The page always has an editor: an empty tab strip with nothing under
        // it is a dead end with no way back.
        this.open_tab(window, cx);
        this.load_saved(cx);
        this
    }

    // ---- query tabs ------------------------------------------------------

    /// Builds one editor. Every tab gets its own, so a switch keeps each tab's
    /// cursor, scroll position and undo history.
    fn new_editor(&self, window: &mut Window, cx: &mut Context<Self>) -> Entity<InputState> {
        let placeholder = t(Str::DbQueryPlaceholder, cx);
        cx.new(|cx| {
            InputState::new(window, cx)
                // `code_editor` first: it *replaces* the mode, so anything set
                // before it is discarded.
                .code_editor(Engine::PostgreSql.editor_language())
                .multi_line(true)
                .line_number(true)
                .soft_wrap(false)
                .placeholder(placeholder)
        })
    }

    /// Opens a new tab and makes it the one on screen.
    pub(super) fn open_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let editor = self.new_editor(window, cx);
        let (id, number) = self.tabs.allocate();
        self.tabs.push(QueryTab::new(id, number, editor));
        // The new tab has no result, so the shared grid must stop showing the
        // old tab's rows under it.
        self.show_active_result(cx);
        cx.notify();
    }

    pub(super) fn select_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        if index == self.tabs.active_index() {
            return;
        }
        self.tabs.select(index);
        self.show_active_result(cx);
        cx.notify();
    }

    /// Closes the tab at `index`.
    ///
    /// Closing the **last** tab empties it rather than removing it: the page
    /// must always have an editor. `QueryTabs::close` refuses that case and
    /// this is where the replacement happens.
    ///
    /// Either way the tab's run is **cancelled at the server** first. Dropping
    /// its task only stops dodo waiting; the statement would keep burning
    /// server CPU and holding the connection for a tab nobody can see any more.
    pub(super) fn close_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.stop_tab(index, cx);
        if self.tabs.close(index).is_none() {
            if index >= self.tabs.len() {
                return;
            }
            let editor = self.new_editor(window, cx);
            let (id, number) = self.tabs.allocate();
            let tabs = &mut self.tabs;
            tabs.select(0);
            if let Some(tab) = tabs.active_mut() {
                *tab = QueryTab::new(id, number, editor);
            }
        }
        self.show_active_result(cx);
        cx.notify();
    }

    /// Stops whatever the tab at `index` is running and forgets its handle.
    ///
    /// Detached rather than held, because the tab that would have held the task
    /// is about to be dropped — and the request still has to reach the server.
    /// Nothing waits for the answer: there is no tab left to tell.
    fn stop_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(handle) = self.tabs.tab_mut(index).and_then(|tab| tab.cancel.take()) else {
            return;
        };
        cx.background_executor()
            .spawn(async move {
                let _ = handle.cancel();
            })
            .detach();
    }

    /// Re-fills the shared grid from the active tab's result.
    ///
    /// One `TableState` serves every tab, so this is what stops a switched-to
    /// tab showing the rows of the tab that was on screen before it.
    fn show_active_result(&mut self, cx: &mut Context<Self>) {
        let result = match self.tabs.active().map(|tab| &tab.query) {
            Some(QueryState::Done(outcome)) => {
                Some((outcome.columns.clone(), outcome.rows.clone()))
            }
            _ => None,
        };
        self.table.update(cx, |state, cx| {
            match result {
                Some((columns, rows)) => state.delegate_mut().set(columns, rows),
                None => state.delegate_mut().clear(),
            }
            state.refresh(cx);
        });
    }

    // ---- persistence -----------------------------------------------------

    /// Reads `connections.json` on the background executor. Never on the UI
    /// thread: the file is read once at startup and a slow disk must not hold
    /// the first frame.
    fn load_saved(&mut self, cx: &mut Context<Self>) {
        let store = self.store.clone();
        cx.spawn(async move |this, cx| {
            let loaded = cx
                .background_executor()
                .spawn(async move { store.load() })
                .await;

            let _ = this.update(cx, |this, cx| {
                match loaded {
                    Ok(document) => this.connections.adopt(document),
                    Err(error) => {
                        // Still mark the list loaded: an unreadable file is not
                        // a reason to leave the panel showing a spinner
                        // forever, and the banner says what happened.
                        this.connections.adopt(Default::default());
                        this.store_error = Some(error.message());
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Writes the document back. Called after every change to the list, so a
    /// crash never loses more than the edit in progress.
    fn persist(&mut self, cx: &mut Context<Self>) {
        let store = self.store.clone();
        let document = self.connections.document().clone();
        self.save_task = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { store.persist(&document) })
                .await;

            let _ = this.update(cx, |this, cx| {
                this.store_error = result.err().map(|error| error.message());
                this.save_task = None;
                cx.notify();
            });
        }));
    }

    // ---- connections -----------------------------------------------------

    /// Makes `id` the connection the query editor runs against.
    ///
    /// Selecting no longer touches the tree, and that is the point of one tree
    /// with several roots: every connection's objects stay loaded and open
    /// while the user reads one of them and runs a statement against another.
    pub(super) fn select(&mut self, id: u64, cx: &mut Context<Self>) {
        if self.connections.selected_id() == Some(id) {
            return;
        }
        self.connections.select(Some(id));
        self.sync_tree_items(cx);
        self.persist(cx);
        cx.notify();
    }

    pub(super) fn connect(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(profile) = self.connections.find(id).cloned() else {
            return;
        };
        if self.connections.status(id).is_busy() {
            return;
        }

        self.connections.set_status(id, Status::Connecting);
        cx.notify();

        self.connect_task = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { services::connect(&profile) })
                .await;

            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(driver) => {
                        this.drivers.insert(id, driver);
                        this.connections.set_status(id, Status::Connected);
                        // Only if the user has the root open. Connecting from
                        // the context menu with the root shut should not load a
                        // catalog nobody is looking at.
                        if this.forest.is_open(id) {
                            this.ensure_tree(id, cx);
                        }
                    }
                    Err(error) => {
                        this.drivers.remove(&id);
                        this.connections.set_status(id, Status::Error(error));
                    }
                }
                this.connect_task = None;
                this.sync_tree_items(cx);
                cx.notify();
            });
        }));
    }

    pub(super) fn disconnect(&mut self, id: u64, cx: &mut Context<Self>) {
        self.drivers.remove(&id);
        self.connections.set_status(id, Status::Disconnected);
        // The whole tree under this root goes: the next session may be a
        // different database entirely, so keeping the shape would be keeping a
        // stranger's. Every other connection's tree is untouched.
        self.forest.forget(id);
        self.sync_tree_items(cx);
        cx.notify();
    }

    pub(super) fn open_form(
        &mut self,
        profile: ConnectionProfile,
        editing: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let form = connection_form::open(profile, editing, window, cx);
        self._form_subscription = Some(cx.subscribe(&form, |this, _, event: &FormEvent, cx| {
            let FormEvent::Saved(profile) = event;
            this.on_form_saved(*profile.clone(), cx);
        }));
        self.form = Some(form);
    }

    fn on_form_saved(&mut self, profile: ConnectionProfile, cx: &mut Context<Self>) {
        let id = profile.id;
        // `save` reports whether the edit moved the connection somewhere else,
        // in which case the live handle points at the old database and must go.
        if self.connections.save(profile) {
            self.drivers.remove(&id);
            self.forest.forget(id);
        }
        self.sync_tree_items(cx);
        self.persist(cx);
        cx.notify();
    }

    pub(super) fn duplicate(&mut self, id: u64, cx: &mut Context<Self>) {
        let suffix = Str::DbCopySuffix.text(Language::current(cx)).into_owned();
        if self.connections.duplicate(id, &suffix).is_some() {
            self.sync_tree_items(cx);
            self.persist(cx);
            cx.notify();
        }
    }

    pub(super) fn delete(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(profile) = self.connections.find(id) else {
            return;
        };
        let name = profile.display_name();
        let view = cx.entity();

        window.open_dialog(cx, move |dialog, _, cx| {
            let view = view.clone();
            dialog
                .title(t(Str::DbDeleteConnectionTitle, cx))
                .child(t(Str::DbDeleteConnectionMessage(name.clone()), cx))
                .on_ok(move |_, window, cx| {
                    view.update(cx, |this, cx| {
                        this.drivers.remove(&id);
                        this.connections.delete(id);
                        this.forest.forget(id);
                        this.sync_tree_items(cx);
                        this.persist(cx);
                        cx.notify();
                    });
                    window.close_dialog(cx);
                    true
                })
        });
    }

    // ---- the object tree -------------------------------------------------

    /// The driver behind the selected connection, if it is connected. What the
    /// query editor runs against.
    pub(super) fn active_driver(&self) -> Option<Arc<dyn Driver>> {
        self.connections
            .selected_id()
            .and_then(|id| self.drivers.get(&id).cloned())
    }

    /// Starts one connection's root load if it is needed. Idempotent, so it is
    /// safe to call from anywhere that might have made a tree relevant.
    fn ensure_tree(&mut self, connection: u64, cx: &mut Context<Self>) {
        let Some(driver) = self.drivers.get(&connection).cloned() else {
            return;
        };
        if !self.forest.tree_mut(connection).needs_roots() {
            return;
        }
        self.forest.tree_mut(connection).begin_roots();
        self.sync_tree_items(cx);

        self.children_task = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { driver.children(None) })
                .await;

            let _ = this.update(cx, |this, cx| {
                this.forest.tree_mut(connection).set_roots(result);
                this.sync_tree_items(cx);
                this.children_task = None;
                cx.notify();
            });
        }));
    }

    /// A row was opened. Which row is [`RowRef`]'s to say: the tree carries
    /// several connections and their node ids can legitimately collide.
    fn on_expanded(&mut self, id: String, cx: &mut Context<Self>) {
        let Some(row) = RowRef::parse(&id) else {
            return;
        };
        match row.node {
            None => self.on_connection_expanded(row.connection, cx),
            Some(node) => self.on_node_expanded(row.connection, node, cx),
        }
    }

    /// Opening a connection root **connects it**, the way every database client
    /// does. A root that opened onto a dead end saying "not connected" would be
    /// a worse answer than the one the user obviously wanted, and the status dot
    /// and the context menu still give explicit control either way.
    fn on_connection_expanded(&mut self, connection: u64, cx: &mut Context<Self>) {
        self.forest.open(connection);
        // Opening a connection is also choosing it: the editor below now has an
        // obvious target, and the alternative is a tree whose open branch and
        // whose Execute button disagree.
        self.select(connection, cx);

        match self.connections.status(connection) {
            Status::Connected => self.ensure_tree(connection, cx),
            Status::Connecting => {}
            // Including `Error`: opening a root that failed last time is the
            // natural way to ask for another go.
            Status::Disconnected | Status::Error(_) => self.connect(connection, cx),
        }
        self.sync_tree_items(cx);
        cx.notify();
    }

    fn on_node_expanded(&mut self, connection: u64, node: NodeId, cx: &mut Context<Self>) {
        // A placeholder row is not a node; the widget will not expand one, and
        // it has no children to fetch.
        if !self.forest.tree_mut(connection).expand(&node) {
            self.sync_tree_items(cx);
            cx.notify();
            return;
        }

        let Some(driver) = self.drivers.get(&connection).cloned() else {
            return;
        };
        self.forest.tree_mut(connection).begin_children(&node);
        self.sync_tree_items(cx);

        self.children_task = Some(cx.spawn(async move |this, cx| {
            let lookup = node.clone();
            let result = cx
                .background_executor()
                .spawn(async move { driver.children(Some(&lookup)) })
                .await;

            let _ = this.update(cx, |this, cx| {
                this.forest.tree_mut(connection).set_children(&node, result);
                this.sync_tree_items(cx);
                this.children_task = None;
                cx.notify();
            });
        }));
    }

    fn on_collapsed(&mut self, id: String, cx: &mut Context<Self>) {
        let Some(row) = RowRef::parse(&id) else {
            return;
        };
        match row.node {
            // Closing a root is not disconnecting: what was loaded stays loaded,
            // so opening it again is instant.
            None => self.forest.close(row.connection),
            Some(node) => self.forest.tree_mut(row.connection).collapse(&node),
        }
        cx.notify();
    }

    /// Re-reads the selected connection's catalog, keeping what the user has
    /// opened. Nothing else in the tree is disturbed.
    pub(super) fn refresh_tree(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.connections.selected_id() else {
            return;
        };
        self.forest.tree_mut(id).refresh();
        self.sync_tree_items(cx);
        self.ensure_tree(id, cx);
        cx.notify();
    }

    /// The whole panel as rows: one root per saved connection, in the order the
    /// user arranged them, each carrying whatever its status allows.
    ///
    /// Both the widget's items and the per-row look map are built from this, so
    /// the two cannot disagree about what is on screen.
    pub(super) fn outline(&self) -> Vec<Outline> {
        let statuses: Vec<(u64, Status)> = self
            .connections
            .profiles()
            .iter()
            .map(|profile| (profile.id, self.connections.status(profile.id).clone()))
            .collect();
        self.forest
            .outline(statuses.iter().map(|(id, status)| (*id, status)))
    }

    /// Rebuilds the widget's items from [`Forest`].
    fn sync_tree_items(&mut self, cx: &mut Context<Self>) {
        let items: Vec<TreeItem> = self
            .outline()
            .iter()
            .map(|row| build_item(row, &self.connections, cx))
            .collect();
        self.tree_state.update(cx, |state, cx| {
            state.set_items(items, cx);
        });
    }

    // ---- the query -------------------------------------------------------

    pub(super) fn execute(&mut self, cx: &mut Context<Self>) {
        self.start_query(false, cx);
    }

    pub(super) fn explain(&mut self, cx: &mut Context<Self>) {
        self.start_query(true, cx);
    }

    fn start_query(&mut self, explain: bool, cx: &mut Context<Self>) {
        let Some(driver) = self.active_driver() else {
            return;
        };
        if explain && !driver.capabilities().explain {
            return;
        }
        let Some(tab) = self.tabs.active() else {
            return;
        };
        if tab.is_running() {
            return;
        }

        let id = tab.id;
        let buffer = tab.editor.read(cx).value().to_string();
        let budget = PageBudget::default();
        // **Before** the statement starts, not when Cancel is pressed: the
        // driver's connection is locked for as long as the query runs, so a
        // handle asked for later would block behind the query it must stop.
        let cancel = driver.cancel_handle();

        if let Some(tab) = self.tabs.active_mut() {
            tab.query = QueryState::Running;
            tab.cancel = cancel;
            tab.notice = None;
            tab.notice_success = false;
        }
        // The grid is shared, so a run that leaves the previous result on
        // screen would attribute those rows to the statement now in flight.
        self.show_active_result(cx);
        cx.notify();

        let task = cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    if explain {
                        query::explain(driver.as_ref(), &buffer, budget)
                    } else {
                        query::run(driver.as_ref(), &buffer, budget)
                    }
                })
                .await;

            let _ = this.update(cx, |this, cx| {
                // By id, not by index: the user may have closed a tab to the
                // left of this one, or closed this one, while it ran.
                let Some(tab) = this.tabs.find_mut(id) else {
                    return;
                };
                tab.query = match result {
                    Ok(outcome) => QueryState::Done(outcome),
                    // The previous result stays cleared: leaving it on screen
                    // beside a failure makes it look like the failed statement
                    // produced it.
                    Err(failure) => QueryState::Failed(failure),
                };
                // Nothing is running any more, so the handle would cancel
                // whatever runs next.
                tab.cancel = None;
                // Only if this tab is the one being looked at — a run that
                // finishes in a background tab must not repaint the grid the
                // user is reading.
                if this.tabs.active().is_some_and(|active| active.id == id) {
                    this.show_active_result(cx);
                }
                cx.notify();
            });
        });
        if let Some(tab) = self.tabs.active_mut() {
            tab.run_task = Some(task);
        }
    }

    /// Asks the server to stop the active tab's statement.
    ///
    /// The handle is **not** taken from the tab: a cancel that lost the race is
    /// not a reason to make a second press impossible, and the run's own
    /// completion is what clears it. The call itself is blocking — PostgreSQL's
    /// opens a second connection — so it goes to the background executor like
    /// every other driver call in this file.
    ///
    /// Nothing here decides that the query stopped. The query says so, by
    /// coming back as [`DbError::Cancelled`](crate::database::models::error::DbError::Cancelled)
    /// from the server; until it does, the tab stays `Running` and the button
    /// stays where it is.
    pub(super) fn cancel(&mut self, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.active() else {
            return;
        };
        let Some(handle) = tab.cancel.clone() else {
            return;
        };
        let id = tab.id;

        let task = cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { handle.cancel() })
                .await;

            // Only a failure to *reach* the server is worth saying anything
            // about, and even then the query state is left alone: dodo could
            // not ask, so it has no idea whether the statement is still
            // running, and moving the tab out of `Running` would be a claim it
            // cannot support.
            if let Err(error) = result {
                let _ = this.update(cx, |this, cx| {
                    if let Some(tab) = this.tabs.find_mut(id) {
                        // Held as a `Str` rather than rendered text, so a
                        // banner already on screen re-translates.
                        tab.notice = Some(Str::DbCancelFailed(error.detail().to_string()));
                        tab.notice_success = false;
                    }
                    cx.notify();
                });
            }
        });
        if let Some(tab) = self.tabs.active_mut() {
            tab.cancel_task = Some(task);
        }
        cx.notify();
    }

    /// Re-runs the statement behind the displayed grid into a file-backed sink.
    /// The bounded rows on screen are never used as the export source.
    pub(super) fn export(&mut self, format: ExportFormat, cx: &mut Context<Self>) {
        let Some(driver) = self.active_driver() else {
            return;
        };
        let Some(tab) = self.tabs.active() else {
            return;
        };
        if tab.is_running() {
            return;
        }
        let QueryState::Done(outcome) = &tab.query else {
            return;
        };
        if !outcome.has_grid() {
            return;
        }

        let id = tab.id;
        let statement = outcome.statement.clone();
        let directory = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(data_dir);
        let filename = format!("query.{}", format.extension());
        let receiver = cx.prompt_for_new_path(&directory, Some(&filename));

        let task = cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(path))) = receiver.await else {
                return;
            };

            let cancel = driver.cancel_handle();
            let Ok(started) = this.update(cx, |this, cx| {
                let Some(tab) = this.tabs.find_mut(id) else {
                    return false;
                };
                if tab.is_running() {
                    return false;
                }
                tab.exporting = true;
                tab.cancel = cancel;
                tab.notice = None;
                tab.notice_success = false;
                cx.notify();
                true
            }) else {
                return;
            };
            if !started {
                return;
            }

            let shown_path = path.display().to_string();
            let result = cx
                .background_executor()
                .spawn(async move { export::export(driver.as_ref(), &statement, &path, format) })
                .await;

            let _ = this.update(cx, |this, cx| {
                let Some(tab) = this.tabs.find_mut(id) else {
                    return;
                };
                tab.exporting = false;
                tab.cancel = None;
                match result {
                    Ok(rows) => {
                        tab.notice = Some(Str::DbExportSucceeded {
                            rows,
                            path: shown_path,
                        });
                        tab.notice_success = true;
                    }
                    Err(error) if error.is_cancelled() => {
                        tab.notice = Some(Str::DbExportCancelled);
                        tab.notice_success = false;
                    }
                    Err(error) => {
                        tab.notice = Some(Str::DbExportFailed(error.detail()));
                        tab.notice_success = false;
                    }
                }
                cx.notify();
            });
        });
        if let Some(tab) = self.tabs.active_mut() {
            tab.run_task = Some(task);
        }
    }

    fn copy_cell(&mut self, _: &DatabaseCopyCell, _: &mut Window, cx: &mut Context<Self>) {
        let text = {
            let table = self.table.read(cx);
            table
                .selected_cell()
                .and_then(|(row, column)| table.delegate().copy_cell_text(row, column))
        };
        if let Some(text) = text {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    fn copy_row(&mut self, _: &DatabaseCopyRow, _: &mut Window, cx: &mut Context<Self>) {
        let text = {
            let table = self.table.read(cx);
            table
                .selected_cell()
                .and_then(|(row, _)| table.delegate().copy_row_text(row))
        };
        if let Some(text) = text {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    pub(super) fn format(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let engine = self
            .connections
            .selected()
            .map(|profile| profile.engine)
            .unwrap_or_default();
        let Some(editor) = self.tabs.active().map(|tab| tab.editor.clone()) else {
            return;
        };
        let formatted = sql_format::format(&editor.read(cx).value(), engine);
        editor.update(cx, |state, cx| {
            // `replace_all` rather than `set_value`: formatting is undoable,
            // which is what makes it safe to try.
            state.replace_all(formatted, window, cx);
        });
    }

    /// Re-points every tab's editor grammar at the selected connection's
    /// language, and re-pushes the placeholder after a language change.
    ///
    /// This runs from `render`, so **both** halves below are load-bearing and
    /// both were missing in round 1, which is why the editor drew black text:
    /// `set_highlighter` throws the highlighter away and cancels its parse task
    /// without scheduling a new one, so calling it every frame guaranteed there
    /// was never a highlighter to paint with, and calling it *at all* without a
    /// following `refresh` leaves the editor uncoloured until the next
    /// keystroke.
    /// [`EditorLanguage`](crate::database::state::editor::EditorLanguage) is
    /// where the whole diagnosis is written down, and it is per tab because
    /// each tab has its own editor to guard.
    fn sync_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // The *driver* is the authority once one is connected — that is what
        // `Capabilities` is for, and it is how a backend whose console is not
        // SQL stops being coloured as though it were. Before connecting there
        // is no driver to ask, so the selected engine's answer stands in.
        let language = match self.active_driver() {
            Some(driver) => driver.capabilities().editor_language,
            None => self
                .connections
                .selected()
                .map(|profile| profile.engine.editor_language())
                .unwrap_or_else(|| Engine::PostgreSql.editor_language()),
        };

        let current = Language::current(cx);
        let retranslate = self.language != current;
        self.language = current;
        let placeholder = retranslate.then(|| t(Str::DbQueryPlaceholder, cx));

        // Every tab, not just the active one: a background tab whose editor was
        // never re-pointed would draw black text the moment it is switched to.
        let editors: Vec<(usize, Entity<InputState>)> = self
            .tabs
            .tabs()
            .iter()
            .enumerate()
            .map(|(index, tab)| (index, tab.editor.clone()))
            .collect();
        for (index, editor) in editors {
            let repoint = self
                .tabs
                .tab_mut(index)
                .is_some_and(|tab| tab.language.adopt(language));
            if repoint {
                editor.update(cx, |state, cx| {
                    state.set_highlighter(language, cx);
                    // `refresh` is the only public way to say "re-run syntax
                    // highlighting on the next render"; without it the grammar
                    // is set and nothing ever parses with it.
                    state.refresh(cx);
                });
            }
            if let Some(placeholder) = placeholder.clone() {
                editor.update(cx, |state, cx| {
                    state.set_placeholder(placeholder, window, cx);
                });
            }
        }
    }
}

/// One outline row as a `TreeItem`.
///
/// The label carries only the identifier; everything else — the icon, the
/// status dot, the dimmed detail, the hover card — is looked up by element id
/// when the row is drawn (see [`DatabaseView::render_tree`]), because `TreeItem`
/// has room for a string and nothing else.
fn build_item(outline: &Outline, connections: &ConnectionsState, cx: &App) -> TreeItem {
    let label: SharedString = match &outline.content {
        // A connection's own name — data, never translated, and the same
        // fallback the connection form shows.
        Content::Connection(id) => connections
            .find(*id)
            .map(|profile| SharedString::from(profile.display_name()))
            .unwrap_or_default(),
        Content::Node(node) => match &node.label {
            NodeLabel::Name(name) => name.clone().into(),
            NodeLabel::Group(group) => t(group.text(), cx),
        },
        Content::Notice(notice) => notice_label(notice, cx),
    };

    let item = TreeItem::new(outline.id.clone(), label)
        .expanded(outline.expanded)
        // A placeholder is not selectable and cannot be expanded: it is a
        // message, not an object.
        .disabled(matches!(outline.content, Content::Notice(_)));

    item.children(
        outline
            .children
            .iter()
            .map(|child| build_item(child, connections, cx))
            .collect::<Vec<_>>(),
    )
}

fn notice_label(notice: &Notice, cx: &App) -> SharedString {
    match notice {
        Notice::Loading => t(Str::DbTreeLoading, cx),
        Notice::Empty => t(Str::DbTreeEmpty, cx),
        Notice::Failed(error) => t(error.message(), cx),
        Notice::NotConnected => t(Str::DbTreeNotConnected, cx),
    }
}

/// How one tree row is drawn: everything `TreeItem`'s single label cannot hold.
///
/// A connection root needs strictly more than an object row — a status dot, a
/// hover card, and which menu items apply — so the two are separate variants
/// rather than one struct with four fields that are `None` most of the time.
#[derive(Clone)]
pub(super) enum RowLook {
    Connection(ConnectionLook),
    Object {
        icon: AppIcon,
        detail: Option<SharedString>,
        muted: bool,
    },
}

/// Everything a connection's root row draws, resolved once per frame.
///
/// Owned rather than borrowed, because the tree's render closure is `'static`
/// and cannot hold a reference to the view.
#[derive(Clone)]
pub(super) struct ConnectionLook {
    pub(super) id: u64,
    pub(super) icon: AppIcon,
    /// The status dot's colour, and the colour of the word beside it.
    pub(super) dot: Hsla,
    pub(super) status: SharedString,
    /// The hover card's rows, already translated. **Never the password** — see
    /// [`ConnectionProfile::details`].
    pub(super) details: Vec<(SharedString, SharedString)>,
    pub(super) connected: bool,
    pub(super) busy: bool,
    /// The last attempt failed. Only changes a menu label — Reconnect rather
    /// than Connect — but that word is the difference between "start" and "try
    /// that again".
    pub(super) failed: bool,
}

/// The icon for a node kind. The only place a `NodeKind` is matched on, which
/// is what "adding a backend does not change the views" means in practice: a
/// new variant adds an arm here and nowhere else.
fn node_icon(kind: NodeKind) -> AppIcon {
    match kind {
        NodeKind::Database => AppIcon::Database,
        NodeKind::Schema => AppIcon::Folder,
        NodeKind::Table => AppIcon::Table,
        NodeKind::View => AppIcon::Eye,
        NodeKind::Column => AppIcon::Columns,
        NodeKind::Index => AppIcon::SortAscending,
        NodeKind::Constraint => AppIcon::Key,
        NodeKind::Folder => AppIcon::FolderOpen,
        NodeKind::Other => AppIcon::File,
    }
}

/// The glyph on a connection's root row. The `Engine` → icon mapping lives here
/// rather than on `Engine` because an icon is GPUI and `models/` names none.
pub(super) fn engine_icon(engine: Engine) -> AppIcon {
    match engine {
        Engine::PostgreSql => AppIcon::PostgreSql,
        Engine::Sqlite => AppIcon::Sqlite,
    }
}

/// The look of every row in the outline, by element id.
pub(super) fn row_looks(
    outline: &[Outline],
    connections: &ConnectionsState,
    into: &mut HashMap<SharedString, RowLook>,
    cx: &App,
) {
    for row in outline {
        let look = match &row.content {
            Content::Connection(id) => RowLook::Connection(connection_look(*id, connections, cx)),
            Content::Node(node) => RowLook::Object {
                icon: node_icon(node.kind),
                detail: node.detail.clone().map(SharedString::from),
                muted: false,
            },
            Content::Notice(notice) => RowLook::Object {
                icon: match notice {
                    Notice::Failed(_) => AppIcon::AlertTriangle,
                    _ => AppIcon::Ellipsis,
                },
                detail: None,
                muted: true,
            },
        };
        into.insert(SharedString::from(row.id.clone()), look);
        row_looks(&row.children, connections, into, cx);
    }
}

fn connection_look(id: u64, connections: &ConnectionsState, cx: &App) -> ConnectionLook {
    let status = connections.status(id);
    let dot = match status {
        Status::Connected => cx.theme().success,
        Status::Connecting => cx.theme().warning,
        Status::Error(_) => cx.theme().danger,
        Status::Disconnected => cx.theme().muted_foreground,
    };
    let profile = connections.find(id);

    ConnectionLook {
        id,
        icon: profile
            .map(|profile| engine_icon(profile.engine))
            .unwrap_or(AppIcon::Database),
        dot,
        status: t(status.label(), cx),
        details: profile
            .map(|profile| {
                profile
                    .details()
                    .into_iter()
                    .map(|(field, value)| (t(field.label(), cx), SharedString::from(value)))
                    .collect()
            })
            .unwrap_or_default(),
        connected: status.is_connected(),
        busy: status.is_busy(),
        failed: matches!(status, Status::Error(_)),
    }
}

impl Focusable for DatabaseView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for DatabaseView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_editor(window, cx);

        let panel = self.render_panel(cx);
        let right = self.render_workspace(cx);

        div()
            .size_full()
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::copy_cell))
            .on_action(cx.listener(Self::copy_row))
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(cx.theme().border)
            .overflow_hidden()
            .child(
                h_resizable("db-split")
                    .with_state(&self.outer_split)
                    .child(
                        resizable_panel()
                            .size(PANEL_WIDTH)
                            .size_range(PANEL_MIN..PANEL_MAX)
                            .child(panel),
                    )
                    .child(resizable_panel().child(right)),
            )
    }
}
