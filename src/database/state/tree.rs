//! The object tree's load state, and the outline the view draws from it.
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

use std::collections::HashMap;

use crate::database::models::catalog::{CatalogNode, NodeId};
use crate::database::models::error::DbError;

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

impl Load {
    pub fn is_loaded(&self) -> bool {
        matches!(self, Load::Loaded(_))
    }
}

/// What a placeholder row under an expandable node says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Notice {
    Loading,
    /// Loaded, and there was nothing in it. Said out loud, because an empty
    /// group that simply collapses again looks like a bug.
    Empty,
    Failed(DbError),
}

/// A row of the outline: either a real catalog node or a placeholder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Content {
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

#[derive(Default)]
pub struct CatalogTree {
    roots: Load,
    children: HashMap<NodeId, Load>,
    expanded: Vec<NodeId>,
}

impl CatalogTree {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn roots(&self) -> &Load {
        &self.roots
    }

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

    /// Forgets everything, expansion included. This is a disconnect: the tree
    /// belongs to a connection, and the next one may be a different database
    /// entirely.
    pub fn clear(&mut self) {
        self.roots = Load::Idle;
        self.children.clear();
        self.expanded.clear();
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
            Content::Node(_) => None,
        }
    }

    fn node_of(outline: &Outline) -> &CatalogNode {
        match &outline.content {
            Content::Node(node) => node,
            Content::Notice(notice) => panic!("expected a node, got {notice:?}"),
        }
    }

    #[test]
    fn a_fresh_tree_needs_its_roots_and_says_it_is_loading() {
        let tree = CatalogTree::new();
        assert!(tree.needs_roots());

        let outline = tree.outline();
        assert_eq!(outline.len(), 1);
        assert_eq!(notice_of(&outline[0]), Some(&Notice::Loading));
    }

    #[test]
    fn asking_for_the_roots_twice_only_loads_them_once() {
        let mut tree = CatalogTree::new();
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
        let mut tree = CatalogTree::new();
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
        let mut tree = CatalogTree::new();
        tree.set_roots(Ok(vec![column("id")]));
        assert!(tree.outline()[0].children.is_empty());
    }

    #[test]
    fn expanding_an_unloaded_node_asks_for_a_load_and_expanding_it_again_does_not() {
        let mut tree = CatalogTree::new();
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
        let mut tree = CatalogTree::new();
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
        let mut tree = CatalogTree::new();
        let db = NodeId::new("db");
        tree.set_roots(Ok(vec![database()]));
        tree.set_children(&db, Ok(Vec::new()));

        let outline = tree.outline();
        assert_eq!(notice_of(&outline[0].children[0]), Some(&Notice::Empty));
    }

    #[test]
    fn an_empty_database_says_so_at_the_root() {
        let mut tree = CatalogTree::new();
        tree.set_roots(Ok(Vec::new()));
        assert_eq!(notice_of(&tree.outline()[0]), Some(&Notice::Empty));
    }

    /// One node failing must not take the tree with it: the rest stays usable
    /// and the failure is reported where it happened.
    #[test]
    fn a_node_that_fails_to_load_reports_it_in_place() {
        let mut tree = CatalogTree::new();
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
        let mut tree = CatalogTree::new();
        let error = DbError::Unreachable("connection reset".into());
        tree.set_roots(Err(error.clone()));
        assert_eq!(notice_of(&tree.outline()[0]), Some(&Notice::Failed(error)));
    }

    /// Refresh keeps the shape of what the user has opened. Losing it would
    /// mean re-opening four levels to see a new column.
    #[test]
    fn refresh_forgets_the_data_and_keeps_the_expansion() {
        let mut tree = CatalogTree::new();
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

    /// Disconnecting is different: the next connection may be another database
    /// entirely, so keeping expansion would be keeping a stranger's shape.
    #[test]
    fn clearing_forgets_the_expansion_too() {
        let mut tree = CatalogTree::new();
        let db = NodeId::new("db");
        tree.set_roots(Ok(vec![database()]));
        tree.expand(&db);
        tree.set_children(&db, Ok(vec![table("users")]));

        tree.clear();

        assert!(tree.needs_roots());
        assert!(!tree.is_expanded(&db));
        assert_eq!(tree.load_of(&db), &Load::Idle);
    }

    /// A placeholder shares the tree's id space with real nodes, so its id must
    /// be one no driver can produce — two rows with the same element id is a
    /// class of gpui bug that is miserable to find.
    #[test]
    fn a_placeholders_id_can_never_collide_with_a_nodes() {
        let mut tree = CatalogTree::new();
        tree.set_roots(Ok(vec![database()]));

        let outline = tree.outline();
        let placeholder = &outline[0].children[0].id;
        assert!(placeholder.starts_with("db"));
        assert_ne!(placeholder, "db");
        assert!(placeholder.ends_with("·notice"));
    }

    #[test]
    fn every_id_in_the_outline_is_unique() {
        let mut tree = CatalogTree::new();
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

    #[test]
    fn load_reports_whether_it_holds_data() {
        assert!(!Load::Idle.is_loaded());
        assert!(!Load::Loading.is_loaded());
        assert!(Load::Loaded(Vec::new()).is_loaded());
        assert!(!Load::Failed(DbError::server("x")).is_loaded());
    }
}
