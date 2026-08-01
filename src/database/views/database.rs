//! The Database Explorer's page: connections and objects on the left, the
//! query editor and its result on the right.
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
//! [`CatalogTree`] is, and this view rebuilds the items from it whenever it
//! changes. `state::tree`'s module doc has the second, sharper reason.

use std::collections::HashMap;
use std::sync::Arc;

use gpui::{
    App, AppContext as _, Context, Entity, FocusHandle, Focusable, InteractiveElement as _,
    IntoElement, ParentElement as _, Pixels, Render, SharedString, Styled as _, Subscription, Task,
    Window, div, px,
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
use crate::database::services::{self, Driver};
use crate::database::state::connections::{ConnectionsState, Status};
use crate::database::state::query::{self, QueryState};
use crate::database::state::tree::{CatalogTree, Content, Notice};
use crate::database::views::connection_form::{self, ConnectionForm, FormEvent};
use crate::database::views::result_grid::ResultDelegate;
use crate::i18n::{Language, Str, t};

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

    pub(super) tree: CatalogTree,
    pub(super) tree_state: Entity<TreeState>,

    pub(super) editor: Entity<InputState>,
    pub(super) table: Entity<TableState<ResultDelegate>>,
    pub(super) query: QueryState,

    outer_split: Entity<ResizableState>,
    pub(super) inner_split: Entity<ResizableState>,

    /// The open connection form, kept so its subscription outlives the dialog.
    form: Option<Entity<ConnectionForm>>,
    _form_subscription: Option<Subscription>,
    _tree_subscription: Subscription,

    /// In-flight work, held so a new request replaces the old one.
    connect_task: Option<Task<()>>,
    children_task: Option<Task<()>>,
    run_task: Option<Task<()>>,
    save_task: Option<Task<()>>,

    /// A failure from the store itself — the file could not be read or written.
    /// Held as a [`Str`] rather than rendered text so it re-translates.
    pub(super) store_error: Option<Str>,

    focus_handle: FocusHandle,
    language: Language,
}

impl DatabaseView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let placeholder = t(Str::DbQueryPlaceholder, cx);
        let editor = cx.new(|cx| {
            InputState::new(window, cx)
                // `code_editor` first: it *replaces* the mode, so anything set
                // before it is discarded.
                .code_editor(Engine::PostgreSql.editor_language())
                .multi_line(true)
                .line_number(true)
                .soft_wrap(false)
                .placeholder(placeholder)
        });

        let tree_state = cx.new(|cx| TreeState::new(cx));
        let tree_subscription =
            cx.subscribe(&tree_state, |this, _, event: &TreeEvent, cx| match event {
                TreeEvent::Expanded(id) => this.on_expanded(id.to_string(), cx),
                TreeEvent::Collapsed(id) => this.on_collapsed(id.to_string(), cx),
            });

        let table = cx.new(|cx| TableState::new(ResultDelegate::default(), window, cx));

        let mut this = Self {
            connections: ConnectionsState::new(),
            drivers: HashMap::new(),
            store: Arc::new(DiskConnectionStore::new()),
            tree: CatalogTree::new(),
            tree_state,
            editor,
            table,
            query: QueryState::Idle,
            outer_split: cx.new(|_| ResizableState::default()),
            inner_split: cx.new(|_| ResizableState::default()),
            form: None,
            _form_subscription: None,
            _tree_subscription: tree_subscription,
            connect_task: None,
            children_task: None,
            run_task: None,
            save_task: None,
            store_error: None,
            focus_handle: cx.focus_handle(),
            language: Language::current(cx),
        };
        this.load_saved(cx);
        this
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

    pub(super) fn select(&mut self, id: u64, cx: &mut Context<Self>) {
        if self.connections.selected_id() == Some(id) {
            return;
        }
        self.connections.select(Some(id));
        // The tree belongs to a connection, so switching connections starts a
        // new one rather than showing the previous database's objects.
        self.tree.clear();
        self.sync_tree_items(cx);
        self.ensure_tree(cx);
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
                        if this.connections.selected_id() == Some(id) {
                            this.tree.clear();
                            this.ensure_tree(cx);
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
        if self.connections.selected_id() == Some(id) {
            self.tree.clear();
            self.sync_tree_items(cx);
        }
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
            self.tree.clear();
            self.sync_tree_items(cx);
        }
        self.persist(cx);
        cx.notify();
    }

    pub(super) fn duplicate(&mut self, id: u64, cx: &mut Context<Self>) {
        let suffix = Str::DbCopySuffix.text(Language::current(cx)).into_owned();
        if self.connections.duplicate(id, &suffix).is_some() {
            self.tree.clear();
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
                        this.tree.clear();
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

    /// The driver behind the selected connection, if it is connected.
    pub(super) fn active_driver(&self) -> Option<Arc<dyn Driver>> {
        self.connections
            .selected_id()
            .and_then(|id| self.drivers.get(&id).cloned())
    }

    /// Starts the root load if it is needed. Idempotent, so it is safe to call
    /// from anywhere that might have made a tree relevant.
    fn ensure_tree(&mut self, cx: &mut Context<Self>) {
        let Some(driver) = self.active_driver() else {
            return;
        };
        if !self.tree.needs_roots() {
            return;
        }
        self.tree.begin_roots();
        self.sync_tree_items(cx);

        self.children_task = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { driver.children(None) })
                .await;

            let _ = this.update(cx, |this, cx| {
                this.tree.set_roots(result);
                this.sync_tree_items(cx);
                this.children_task = None;
                cx.notify();
            });
        }));
    }

    fn on_expanded(&mut self, id: String, cx: &mut Context<Self>) {
        // A placeholder row is not a node; the widget will not expand one, and
        // it has no children to fetch.
        let node = NodeId::new(id);
        if !self.tree.expand(&node) {
            self.sync_tree_items(cx);
            cx.notify();
            return;
        }

        let Some(driver) = self.active_driver() else {
            return;
        };
        self.tree.begin_children(&node);
        self.sync_tree_items(cx);

        self.children_task = Some(cx.spawn(async move |this, cx| {
            let lookup = node.clone();
            let result = cx
                .background_executor()
                .spawn(async move { driver.children(Some(&lookup)) })
                .await;

            let _ = this.update(cx, |this, cx| {
                this.tree.set_children(&node, result);
                this.sync_tree_items(cx);
                this.children_task = None;
                cx.notify();
            });
        }));
    }

    fn on_collapsed(&mut self, id: String, cx: &mut Context<Self>) {
        self.tree.collapse(&NodeId::new(id));
        cx.notify();
    }

    pub(super) fn refresh_tree(&mut self, cx: &mut Context<Self>) {
        self.tree.refresh();
        self.sync_tree_items(cx);
        self.ensure_tree(cx);
        cx.notify();
    }

    /// Rebuilds the widget's items from [`CatalogTree`].
    fn sync_tree_items(&mut self, cx: &mut Context<Self>) {
        let items: Vec<TreeItem> = self
            .tree
            .outline()
            .iter()
            .map(|outline| build_item(outline, cx))
            .collect();
        self.tree_state.update(cx, |state, cx| {
            state.set_items(items, cx);
        });
    }

    // ---- the query -------------------------------------------------------

    pub(super) fn execute(&mut self, cx: &mut Context<Self>) {
        let Some(driver) = self.active_driver() else {
            return;
        };
        let buffer = self.editor.read(cx).value().to_string();
        let budget = PageBudget::default();

        self.query = QueryState::Running;
        cx.notify();

        self.run_task = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { query::run(driver.as_ref(), &buffer, budget) })
                .await;

            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(outcome) => {
                        let columns = outcome.columns.clone();
                        let rows = outcome.rows.clone();
                        this.table.update(cx, |state, cx| {
                            state.delegate_mut().set(columns, rows);
                            state.refresh(cx);
                        });
                        this.query = QueryState::Done(outcome);
                    }
                    Err(failure) => {
                        // The previous result is cleared: leaving it on screen
                        // beside a failure makes it look like the failed
                        // statement produced it.
                        this.table.update(cx, |state, cx| {
                            state.delegate_mut().clear();
                            state.refresh(cx);
                        });
                        this.query = QueryState::Failed(failure);
                    }
                }
                this.run_task = None;
                cx.notify();
            });
        }));
    }

    pub(super) fn format(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let engine = self
            .connections
            .selected()
            .map(|profile| profile.engine)
            .unwrap_or_default();
        let formatted = sql_format::format(&self.editor.read(cx).value(), engine);
        self.editor.update(cx, |state, cx| {
            // `replace_all` rather than `set_value`: formatting is undoable,
            // which is what makes it safe to try.
            state.replace_all(formatted, window, cx);
        });
    }

    /// Re-points the editor's grammar at the selected connection's language,
    /// and re-pushes the placeholder after a language change.
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
        self.editor.update(cx, |state, cx| {
            state.set_highlighter(language, cx);
        });

        let current = Language::current(cx);
        if self.language != current {
            self.language = current;
            let placeholder = t(Str::DbQueryPlaceholder, cx);
            self.editor.update(cx, |state, cx| {
                state.set_placeholder(placeholder, window, cx);
            });
        }
    }
}

/// One outline node as a `TreeItem`.
///
/// The label carries only the identifier; the icon and the dimmed detail are
/// looked up by id when the row is drawn (see [`DatabaseView::render_tree`]),
/// because `TreeItem` has room for a string and nothing else.
fn build_item(outline: &crate::database::state::tree::Outline, cx: &App) -> TreeItem {
    let label: SharedString = match &outline.content {
        Content::Node(node) => match &node.label {
            NodeLabel::Name(name) => name.clone().into(),
            NodeLabel::Group(group) => t(group.text(), cx),
        },
        Content::Notice(notice) => match notice {
            Notice::Loading => t(Str::DbTreeLoading, cx),
            Notice::Empty => t(Str::DbTreeEmpty, cx),
            Notice::Failed(error) => t(error.message(), cx),
        },
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
            .map(|child| build_item(child, cx))
            .collect::<Vec<_>>(),
    )
}

/// How one tree row is drawn: everything `TreeItem`'s single label cannot hold.
#[derive(Clone)]
pub(super) struct RowLook {
    pub(super) icon: AppIcon,
    pub(super) detail: Option<SharedString>,
    pub(super) muted: bool,
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

/// The look of every row in the outline, by element id.
pub(super) fn row_looks(
    outline: &[crate::database::state::tree::Outline],
    into: &mut HashMap<SharedString, RowLook>,
) {
    for row in outline {
        let look = match &row.content {
            Content::Node(node) => RowLook {
                icon: node_icon(node.kind),
                detail: node.detail.clone().map(SharedString::from),
                muted: false,
            },
            Content::Notice(notice) => RowLook {
                icon: match notice {
                    Notice::Failed(_) => AppIcon::AlertTriangle,
                    _ => AppIcon::Ellipsis,
                },
                detail: None,
                muted: true,
            },
        };
        into.insert(SharedString::from(row.id.clone()), look);
        row_looks(&row.children, into);
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
            .track_focus(&self.focus_handle)
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
