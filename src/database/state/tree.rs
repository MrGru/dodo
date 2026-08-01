//! The object tree's load state, and the outline the view draws from it.
//!
//! # One tree, several roots
//!
//! The left panel is a single tree whose **roots are the saved connections**;
//! a connection's databases, schemas and tables hang under it. [`Forest`] is
//! that arrangement: one [`CatalogTree`] per connection, plus which connection
//! roots the user has opened.
//!
//! Two connections can perfectly well produce the same node id — `postgres`
//! twice over, on two servers — so every element id in the combined outline is
//! qualified by the connection it belongs to. [`RowRef`] is the whole of that
//! encoding, in both directions, and it is what a widget event is turned back
//! into.
//!
//! # Nothing is loaded until it is expanded
//!
//! This type holds one [`Load`] per node — never asked for, being asked for,
//! loaded, or failed — and the driver is called exactly once per expansion.
//! A database with two hundred tables costs two hundred rows and one query, not
//! two hundred queries.
//!
//! # Why the model owns expansion rather than the widget
//!
//! `gpui_component`'s `TreeItem` keeps its expanded flag inside itself and
//! `TreeState::set_items` replaces the lot — so every time a node's children
//! arrive and the items are rebuilt, the widget's own expansion state would be
//! lost. Holding it here instead means the rebuild is a pure function of this
//! model, and a node that was open before its grandchild loaded is still open
//! after.
//!
//! There is a second reason, and it is the one that would otherwise be a bug:
//! `TreeItem::is_folder` is `children.len() > 0`, so a node whose children have
//! not been fetched yet draws **no disclosure triangle and emits no expand
//! event** — the tree could never be opened at all. [`CatalogTree::outline`]
//! therefore gives every expandable node a placeholder child ([`Notice`]) until
//! its real children arrive. That placeholder is what the user reads as
//! "Loading…", "Nothing here" or the error, and it is what makes the triangle
//! exist.

use std::collections::{HashMap, HashSet};

use crate::database::models::catalog::{CatalogNode, NodeId};
use crate::database::models::error::DbError;
use crate::database::state::connections::Status;

/// How far along one node's children are.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum Load {
    /// Never asked for. The ordinary state of everything below the first row.
    #[default]
    Idle,
    Loading,
    Loaded(Vec<CatalogNode>),
    Failed(DbError),
}

/// What a placeholder row under an expandable node says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Notice {
    Loading,
    /// Loaded, and there was nothing in it. Said out loud, because an empty
    /// group that simply collapses again looks like a bug.
    Empty,
    Failed(DbError),
    /// Under a connection root that is not connected. Only a connection can be
    /// in this state; a catalog node's parent is by definition connected.
    NotConnected,
}

/// A row of the outline: a connection root, a real catalog node, or a
/// placeholder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Content {
    /// One saved connection, by id. The name, engine and status are the view's
    /// to look up — this layer holds no copy of them, so the two cannot drift.
    Connection(u64),
    Node(CatalogNode),
    Notice(Notice),
}

/// One node of the outline the view turns into `TreeItem`s.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outline {
    /// The element id. A real node's own id, or a synthetic one for a
    /// placeholder — distinct from every node id because it ends in a suffix a
    /// driver's id never has.
    pub id: String,
    pub content: Content,
    pub expanded: bool,
    pub children: Vec<Outline>,
}

/// The suffix that makes a placeholder's element id unmistakably not a node's.
const NOTICE_SUFFIX: &str = "\u{1f}·notice";

/// The separator between the parts of a qualified element id. A unit separator,
/// which no server puts in an identifier and no driver puts in a node id.
const SEPARATOR: char = '\u{1f}';
/// What every qualified element id starts with, so a stray id from somewhere
/// else fails to parse rather than resolving to connection 0.
const ROW_PREFIX: &str = "c";

/// Which connection a row of the combined outline belongs to, and which node
/// inside it.
///
/// The tree widget knows one string per row, so this is both halves of the
/// encoding: [`root`](Self::root) and [`child`](Self::child) build the id the
/// outline carries, and [`parse`](Self::parse) turns a widget event back into
/// something this layer can act on. Qualifying is not decoration — two
/// connections to two servers routinely produce the same node id, and two rows
/// sharing an element id is a class of gpui bug that is miserable to find.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowRef {
    pub connection: u64,
    /// `None` for the connection's own root row.
    pub node: Option<NodeId>,
}

impl RowRef {
    /// The element id of a connection's own root row.
    pub fn root(connection: u64) -> String {
        format!("{ROW_PREFIX}{SEPARATOR}{connection}")
    }

    /// The element id of a row *under* `connection`.
    pub fn child(connection: u64, node: &str) -> String {
        format!("{ROW_PREFIX}{SEPARATOR}{connection}{SEPARATOR}{node}")
    }

    /// Reads an element id back. `None` for anything this module did not write.
    pub fn parse(id: &str) -> Option<Self> {
        let rest = id.strip_prefix(ROW_PREFIX)?.strip_prefix(SEPARATOR)?;
        // The node id may itself contain separators — a driver's ids do — so
        // only the *first* one after the connection number is ours.
        let (number, node) = match rest.split_once(SEPARATOR) {
            Some((number, node)) => (number, Some(NodeId::new(node.to_string()))),
            None => (rest, None),
        };
        Some(Self {
            connection: number.parse().ok()?,
            node,
        })
    }
}

/// One tree per connection, and which connection roots are open.
///
/// The connections themselves live in
/// [`ConnectionsState`](crate::database::state::connections::ConnectionsState);
/// this holds only what browsing them costs. A connection with no entry here
/// has simply never been opened, which is the ordinary state of most of them.
#[derive(Default)]
pub struct Forest {
    trees: HashMap<u64, CatalogTree>,
    open: HashSet<u64>,
}

impl Forest {
    pub fn new() -> Self {
        Self::default()
    }

    /// One connection's tree, if it has ever been opened. The view reaches for
    /// [`tree_mut`](Self::tree_mut) instead — it is always about to change
    /// something — so this is here for the assertions below.
    #[cfg(test)]
    pub fn tree(&self, connection: u64) -> Option<&CatalogTree> {
        self.trees.get(&connection)
    }

    /// The tree for `connection`, created empty if this is the first time it
    /// has been asked for.
    pub fn tree_mut(&mut self, connection: u64) -> &mut CatalogTree {
        self.trees.entry(connection).or_default()
    }

    pub fn is_open(&self, connection: u64) -> bool {
        self.open.contains(&connection)
    }

    pub fn open(&mut self, connection: u64) {
        self.open.insert(connection);
    }

    pub fn close(&mut self, connection: u64) {
        self.open.remove(&connection);
    }

    /// Forgets everything about `connection` — what was loaded and that it was
    /// open. This is a disconnect or a delete: the next session may be a
    /// different database entirely, so keeping the shape would be keeping a
    /// stranger's.
    pub fn forget(&mut self, connection: u64) {
        self.trees.remove(&connection);
        self.open.remove(&connection);
    }

    /// The whole panel as the view should draw it: one root per connection, in
    /// the order given, each carrying whatever its status allows.
    pub fn outline<'a>(&self, roots: impl IntoIterator<Item = (u64, &'a Status)>) -> Vec<Outline> {
        roots
            .into_iter()
            .map(|(connection, status)| self.root_of(connection, status))
            .collect()
    }

    fn root_of(&self, connection: u64, status: &Status) -> Outline {
        // Always at least one child, open or not: `TreeItem::is_folder` is
        // `children.len() > 0`, so a childless root would draw no disclosure
        // arrow and could never be opened — the same trap the catalog nodes
        // below have, for the same reason.
        let children = match status {
            Status::Connected => match self.trees.get(&connection) {
                Some(tree) => qualify(tree.outline(), connection),
                None => vec![self.placeholder(connection, Notice::Loading)],
            },
            Status::Connecting => vec![self.placeholder(connection, Notice::Loading)],
            Status::Error(error) => {
                vec![self.placeholder(connection, Notice::Failed(error.clone()))]
            }
            Status::Disconnected => vec![self.placeholder(connection, Notice::NotConnected)],
        };

        Outline {
            id: RowRef::root(connection),
            content: Content::Connection(connection),
            expanded: self.is_open(connection),
            children,
        }
    }

    fn placeholder(&self, connection: u64, message: Notice) -> Outline {
        notice(message, &RowRef::root(connection))
    }
}

/// Re-ids one connection's outline so its rows cannot collide with another's.
fn qualify(rows: Vec<Outline>, connection: u64) -> Vec<Outline> {
    rows.into_iter()
        .map(|row| Outline {
            id: RowRef::child(connection, &row.id),
            content: row.content,
            expanded: row.expanded,
            children: qualify(row.children, connection),
        })
        .collect()
}

#[derive(Default)]
pub struct CatalogTree {
    roots: Load,
    children: HashMap<NodeId, Load>,
    expanded: Vec<NodeId>,
}

impl CatalogTree {
    pub fn load_of(&self, id: &NodeId) -> &Load {
        self.children.get(id).unwrap_or(&Load::Idle)
    }

    pub fn is_expanded(&self, id: &NodeId) -> bool {
        self.expanded.iter().any(|open| open == id)
    }

    /// Whether the roots still need fetching. Idempotent, so a view may call it
    /// on every render without starting a second load.
    pub fn needs_roots(&self) -> bool {
        matches!(self.roots, Load::Idle)
    }

    pub fn begin_roots(&mut self) {
        self.roots = Load::Loading;
    }

    pub fn set_roots(&mut self, result: Result<Vec<CatalogNode>, DbError>) {
        self.roots = into_load(result);
    }

    /// Marks `id` open, and reports whether its children have to be fetched.
    ///
    /// Returning `false` for a node already loading is what stops a
    /// double-click, or a collapse and re-expand, from starting a second query
    /// for the same node.
    pub fn expand(&mut self, id: &NodeId) -> bool {
        if !self.is_expanded(id) {
            self.expanded.push(id.clone());
        }
        matches!(self.load_of(id), Load::Idle)
    }

    pub fn collapse(&mut self, id: &NodeId) {
        self.expanded.retain(|open| open != id);
    }

    pub fn begin_children(&mut self, id: &NodeId) {
        self.children.insert(id.clone(), Load::Loading);
    }

    pub fn set_children(&mut self, id: &NodeId, result: Result<Vec<CatalogNode>, DbError>) {
        self.children.insert(id.clone(), into_load(result));
    }

    /// Forgets every loaded child and every failure, keeping which nodes were
    /// open.
    ///
    /// This is Refresh, and keeping the expansion is the whole point: a user
    /// who has opened four levels to reach a table wants to see that table's
    /// new column, not to start again from the root.
    pub fn refresh(&mut self) {
        self.roots = Load::Idle;
        self.children.clear();
    }

    /// The whole tree as the view should draw it.
    pub fn outline(&self) -> Vec<Outline> {
        self.outline_of(&self.roots)
    }

    fn outline_of(&self, load: &Load) -> Vec<Outline> {
        match load {
            Load::Idle | Load::Loading => vec![notice(Notice::Loading, "\u{1f}root")],
            Load::Failed(error) => vec![notice(Notice::Failed(error.clone()), "\u{1f}root")],
            Load::Loaded(nodes) if nodes.is_empty() => {
                vec![notice(Notice::Empty, "\u{1f}root")]
            }
            Load::Loaded(nodes) => nodes.iter().map(|node| self.outline_node(node)).collect(),
        }
    }

    fn outline_node(&self, node: &CatalogNode) -> Outline {
        let expanded = self.is_expanded(&node.id);
        // A leaf has no children and needs none: `TreeItem` draws no triangle
        // for it, which is correct.
        let children = if node.expandable {
            match self.load_of(&node.id) {
                Load::Loaded(nodes) if nodes.is_empty() => {
                    vec![notice(Notice::Empty, node.id.as_str())]
                }
                Load::Loaded(nodes) => nodes.iter().map(|child| self.outline_node(child)).collect(),
                Load::Failed(error) => {
                    vec![notice(Notice::Failed(error.clone()), node.id.as_str())]
                }
                // Idle as well as Loading: the placeholder is what gives the
                // node a disclosure triangle before anything has been fetched,
                // and without a triangle the node can never be opened.
                Load::Idle | Load::Loading => vec![notice(Notice::Loading, node.id.as_str())],
            }
        } else {
            Vec::new()
        };

        Outline {
            id: node.id.as_str().to_string(),
            content: Content::Node(node.clone()),
            expanded,
            children,
        }
    }
}

fn notice(notice: Notice, owner: &str) -> Outline {
    Outline {
        id: format!("{owner}{NOTICE_SUFFIX}"),
        content: Content::Notice(notice),
        expanded: false,
        children: Vec::new(),
    }
}

fn into_load(result: Result<Vec<CatalogNode>, DbError>) -> Load {
    match result {
        Ok(nodes) => Load::Loaded(nodes),
        Err(error) => Load::Failed(error),
    }
}

#[cfg(test)]
mod tests {
    use super::{CatalogTree, Content, Load, Notice, Outline};
    use crate::database::models::catalog::{CatalogNode, NodeId, NodeKind};
    use crate::database::models::error::DbError;

    fn database() -> CatalogNode {
        CatalogNode::branch("db", NodeKind::Database, "shop")
    }

    fn table(name: &str) -> CatalogNode {
        CatalogNode::branch(format!("t\u{1f}{name}"), NodeKind::Table, name)
    }

    fn column(name: &str) -> CatalogNode {
        CatalogNode::leaf(format!("c\u{1f}{name}"), NodeKind::Column, name)
    }

    fn notice_of(outline: &Outline) -> Option<&Notice> {
        match &outline.content {
            Content::Notice(notice) => Some(notice),
            Content::Node(_) | Content::Connection(_) => None,
        }
    }

    fn node_of(outline: &Outline) -> &CatalogNode {
        match &outline.content {
            Content::Node(node) => node,
            other => panic!("expected a node, got {other:?}"),
        }
    }

    #[test]
    fn a_fresh_tree_needs_its_roots_and_says_it_is_loading() {
        let tree = CatalogTree::default();
        assert!(tree.needs_roots());

        let outline = tree.outline();
        assert_eq!(outline.len(), 1);
        assert_eq!(notice_of(&outline[0]), Some(&Notice::Loading));
    }

    #[test]
    fn asking_for_the_roots_twice_only_loads_them_once() {
        let mut tree = CatalogTree::default();
        tree.begin_roots();
        assert!(
            !tree.needs_roots(),
            "a load already in flight must not be started again"
        );
    }

    /// The bug this placeholder exists to prevent: `TreeItem::is_folder` is
    /// `children.len() > 0`, so an expandable node with nothing under it draws
    /// no triangle and can never emit the expand event that would load it.
    #[test]
    fn an_expandable_node_has_a_child_before_anything_is_loaded() {
        let mut tree = CatalogTree::default();
        tree.set_roots(Ok(vec![database()]));

        let outline = tree.outline();
        assert_eq!(outline.len(), 1);
        assert_eq!(
            outline[0].children.len(),
            1,
            "without a placeholder child the node has no disclosure triangle"
        );
        assert_eq!(notice_of(&outline[0].children[0]), Some(&Notice::Loading));
        assert!(!outline[0].expanded);
    }

    #[test]
    fn a_leaf_gets_no_placeholder_and_so_no_triangle() {
        let mut tree = CatalogTree::default();
        tree.set_roots(Ok(vec![column("id")]));
        assert!(tree.outline()[0].children.is_empty());
    }

    #[test]
    fn expanding_an_unloaded_node_asks_for_a_load_and_expanding_it_again_does_not() {
        let mut tree = CatalogTree::default();
        let id = NodeId::new("db");

        assert!(tree.expand(&id), "the first expand must fetch");
        assert!(tree.is_expanded(&id));

        tree.begin_children(&id);
        assert!(
            !tree.expand(&id),
            "a load already in flight must not be started again"
        );

        tree.set_children(&id, Ok(vec![table("users")]));
        tree.collapse(&id);
        assert!(!tree.is_expanded(&id));
        assert!(
            !tree.expand(&id),
            "re-expanding a loaded node must not re-query it"
        );
    }

    #[test]
    fn loaded_children_replace_the_placeholder_and_nest() {
        let mut tree = CatalogTree::default();
        let db = NodeId::new("db");
        tree.set_roots(Ok(vec![database()]));
        tree.expand(&db);
        tree.set_children(&db, Ok(vec![table("users"), table("orders")]));

        let outline = tree.outline();
        assert!(outline[0].expanded);
        assert_eq!(outline[0].children.len(), 2);
        assert_eq!(node_of(&outline[0].children[0]).id, table("users").id);
        assert_eq!(
            outline[0].children[0].children.len(),
            1,
            "each table is itself expandable and gets its own placeholder"
        );
    }

    #[test]
    fn an_empty_result_says_so_rather_than_collapsing_silently() {
        let mut tree = CatalogTree::default();
        let db = NodeId::new("db");
        tree.set_roots(Ok(vec![database()]));
        tree.set_children(&db, Ok(Vec::new()));

        let outline = tree.outline();
        assert_eq!(notice_of(&outline[0].children[0]), Some(&Notice::Empty));
    }

    #[test]
    fn an_empty_database_says_so_at_the_root() {
        let mut tree = CatalogTree::default();
        tree.set_roots(Ok(Vec::new()));
        assert_eq!(notice_of(&tree.outline()[0]), Some(&Notice::Empty));
    }

    /// One node failing must not take the tree with it: the rest stays usable
    /// and the failure is reported where it happened.
    #[test]
    fn a_node_that_fails_to_load_reports_it_in_place() {
        let mut tree = CatalogTree::default();
        let db = NodeId::new("db");
        let error = DbError::server("permission denied");

        tree.set_roots(Ok(vec![database(), table("orders")]));
        tree.set_children(&db, Err(error.clone()));

        let outline = tree.outline();
        assert_eq!(
            notice_of(&outline[0].children[0]),
            Some(&Notice::Failed(error))
        );
        assert_eq!(
            outline.len(),
            2,
            "the sibling that loaded is still there and still usable"
        );
    }

    #[test]
    fn a_failed_root_load_is_reported_at_the_root() {
        let mut tree = CatalogTree::default();
        let error = DbError::Unreachable("connection reset".into());
        tree.set_roots(Err(error.clone()));
        assert_eq!(notice_of(&tree.outline()[0]), Some(&Notice::Failed(error)));
    }

    /// Refresh keeps the shape of what the user has opened. Losing it would
    /// mean re-opening four levels to see a new column.
    #[test]
    fn refresh_forgets_the_data_and_keeps_the_expansion() {
        let mut tree = CatalogTree::default();
        let db = NodeId::new("db");
        tree.set_roots(Ok(vec![database()]));
        tree.expand(&db);
        tree.set_children(&db, Ok(vec![table("users")]));

        tree.refresh();

        assert!(tree.needs_roots());
        assert_eq!(tree.load_of(&db), &Load::Idle);
        assert!(
            tree.is_expanded(&db),
            "the user's expansion survives a refresh"
        );
    }

    /// A placeholder shares the tree's id space with real nodes, so its id must
    /// be one no driver can produce — two rows with the same element id is a
    /// class of gpui bug that is miserable to find.
    #[test]
    fn a_placeholders_id_can_never_collide_with_a_nodes() {
        let mut tree = CatalogTree::default();
        tree.set_roots(Ok(vec![database()]));

        let outline = tree.outline();
        let placeholder = &outline[0].children[0].id;
        assert!(placeholder.starts_with("db"));
        assert_ne!(placeholder, "db");
        assert!(placeholder.ends_with("·notice"));
    }

    #[test]
    fn every_id_in_the_outline_is_unique() {
        let mut tree = CatalogTree::default();
        let db = NodeId::new("db");
        tree.set_roots(Ok(vec![database()]));
        tree.expand(&db);
        tree.set_children(&db, Ok(vec![table("users"), table("orders")]));

        let mut ids = Vec::new();
        collect_ids(&tree.outline(), &mut ids);
        let mut sorted = ids.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), ids.len(), "duplicate ids in {ids:?}");
    }

    fn collect_ids(outline: &[Outline], into: &mut Vec<String>) {
        for node in outline {
            into.push(node.id.clone());
            collect_ids(&node.children, into);
        }
    }

    // ---- the forest ------------------------------------------------------

    mod forest {
        use super::super::{Content, Forest, Notice, Outline, RowRef};
        use super::{database, notice_of, table};
        use crate::database::models::catalog::NodeId;
        use crate::database::models::error::DbError;
        use crate::database::state::connections::Status;

        fn connection_of(outline: &Outline) -> u64 {
            match &outline.content {
                Content::Connection(id) => *id,
                other => panic!("expected a connection root, got {other:?}"),
            }
        }

        #[test]
        fn a_row_id_round_trips_through_its_connection() {
            let root = RowRef::parse(&RowRef::root(7)).expect("parses");
            assert_eq!(root.connection, 7);
            assert_eq!(root.node, None);

            let child = RowRef::child(7, "db\u{1f}shop");
            let parsed = RowRef::parse(&child).expect("parses");
            assert_eq!(parsed.connection, 7);
            assert_eq!(
                parsed.node,
                Some(NodeId::new("db\u{1f}shop")),
                "a driver's own separators belong to the node, not to the encoding"
            );
        }

        #[test]
        fn an_id_this_module_did_not_write_does_not_parse() {
            for id in ["", "db", "c", "c\u{1f}", "c\u{1f}x", "x\u{1f}1"] {
                assert_eq!(RowRef::parse(id), None, "{id:?} must not resolve");
            }
        }

        /// The reason ids are qualified at all: two connections to two servers
        /// routinely hold the same node id.
        #[test]
        fn two_connections_showing_the_same_object_have_different_element_ids() {
            let mut forest = Forest::new();
            forest.open(1);
            forest.open(2);
            for connection in [1, 2] {
                forest
                    .tree_mut(connection)
                    .set_roots(Ok(vec![database(), table("users")]));
            }

            let outline = forest.outline([(1, &Status::Connected), (2, &Status::Connected)]);
            let mut ids = Vec::new();
            super::collect_ids(&outline, &mut ids);
            let mut sorted = ids.clone();
            sorted.sort();
            sorted.dedup();
            assert_eq!(sorted.len(), ids.len(), "duplicate ids in {ids:?}");

            // And every one of them still says which connection it came from.
            for id in &ids {
                let parsed = RowRef::parse(id).expect("every row id parses");
                assert!(parsed.connection == 1 || parsed.connection == 2);
            }
        }

        /// Without a child a root draws no disclosure arrow and can never be
        /// opened — `TreeItem::is_folder` is `children.len() > 0`. That holds
        /// for a connection exactly as it does for a catalog node.
        #[test]
        fn every_connection_root_has_a_child_whatever_its_status() {
            let forest = Forest::new();
            for status in [
                Status::Disconnected,
                Status::Connecting,
                Status::Connected,
                Status::Error(DbError::server("boom")),
            ] {
                let outline = forest.outline([(1, &status)]);
                assert_eq!(outline.len(), 1);
                assert_eq!(connection_of(&outline[0]), 1);
                assert_eq!(
                    outline[0].children.len(),
                    1,
                    "no child means no disclosure arrow ({status:?})"
                );
            }
        }

        #[test]
        fn a_disconnected_root_says_so_and_a_failed_one_says_why() {
            let forest = Forest::new();
            let error = DbError::server("permission denied");

            let outline = forest.outline([(1, &Status::Disconnected)]);
            assert_eq!(
                notice_of(&outline[0].children[0]),
                Some(&Notice::NotConnected)
            );

            let outline = forest.outline([(1, &Status::Error(error.clone()))]);
            assert_eq!(
                notice_of(&outline[0].children[0]),
                Some(&Notice::Failed(error))
            );
        }

        #[test]
        fn opening_a_root_is_what_expands_it_and_closing_it_is_not_forgetting_it() {
            let mut forest = Forest::new();
            forest.tree_mut(1).set_roots(Ok(vec![database()]));

            assert!(!forest.outline([(1, &Status::Connected)])[0].expanded);
            forest.open(1);
            assert!(forest.outline([(1, &Status::Connected)])[0].expanded);

            forest.close(1);
            assert!(!forest.is_open(1));
            assert!(
                forest.tree(1).is_some_and(|tree| !tree.needs_roots()),
                "closing a root must not throw away what was loaded"
            );
        }

        /// Disconnecting is different from collapsing: the next session may be
        /// another database entirely.
        #[test]
        fn forgetting_a_connection_drops_its_tree_and_closes_it() {
            let mut forest = Forest::new();
            forest.open(1);
            forest.tree_mut(1).set_roots(Ok(vec![database()]));

            forest.forget(1);
            assert!(!forest.is_open(1));
            assert!(forest.tree(1).is_none());
        }

        /// One connection's state is its own. A failure on one root must leave
        /// the connection beside it usable.
        #[test]
        fn connections_do_not_share_load_state() {
            let mut forest = Forest::new();
            let db = NodeId::new("db");
            forest.open(1);
            forest.open(2);
            forest.tree_mut(1).set_roots(Ok(vec![database()]));
            forest.tree_mut(1).expand(&db);
            forest
                .tree_mut(1)
                .set_children(&db, Ok(vec![table("users")]));
            forest.tree_mut(2).set_roots(Ok(vec![database()]));

            let outline = forest.outline([(1, &Status::Connected), (2, &Status::Connected)]);
            assert_eq!(outline[0].children[0].children.len(), 1);
            assert_eq!(
                notice_of(&outline[1].children[0].children[0]),
                Some(&Notice::Loading),
                "the second connection has loaded nothing under its database"
            );
        }

        #[test]
        fn the_roots_keep_the_order_the_caller_gave() {
            let forest = Forest::new();
            let outline = forest.outline([
                (3, &Status::Disconnected),
                (1, &Status::Disconnected),
                (2, &Status::Disconnected),
            ]);
            let ids: Vec<u64> = outline.iter().map(connection_of).collect();
            assert_eq!(ids, vec![3, 1, 2]);
        }
    }
}
