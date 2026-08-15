//! The object tree, in terms no backend owns.
//!
//! # Why the tree is generic rather than a fixed ladder
//!
//! The obvious shape for a SQL browser is a fixed
//! databases → schemas → tables → columns ladder. It is also the shape that
//! makes the *second* backend expensive: SQLite has no schemas, and a key/value
//! store has neither. So a driver is asked one question — [give me the children
//! of this node](crate::database::services::Driver::children) — and answers with
//! whatever its own hierarchy has at that level. Nothing above `services/` knows
//! that PostgreSQL puts schemas under a database and SQLite does not, or that a
//! key/value store would put numbered keyspaces at the root.
//!
//! # [`NodeId`] is opaque above `services/`
//!
//! Each driver encodes whatever it needs to answer `children` again — a schema
//! name, a table's qualified name, a section marker. Nothing above the driver
//! parses one; it is handed back exactly as it was received. That is what keeps
//! the id format a driver's private business.
//!
//! # Labels are two different things and the distinction is load-bearing
//!
//! A table's row reads `users` — an identifier from the server, data, never
//! translated. A grouping row reads "Tables" — a word dodo chose, which a
//! Vietnamese user must not see in English. [`NodeLabel`] keeps them apart so
//! the i18n guard has something to enforce and a driver never has to know what
//! language the app is in.

use crate::i18n::{Str, db_catalog};

/// A driver's own way of naming an object. Opaque above `services/`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(pub String);

impl NodeId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// What kind of object a row is. Drives the icon and nothing else — no `match`
/// in a view branches on it for behaviour, which is what lets a later backend
/// add a variant without touching the tree's logic.
///
/// The variants are exactly the ones the shipped drivers emit, plus
/// [`NodeKind::Other`]. A backend with a concept dodo has not met uses `Other`
/// and gets a generic icon; when that concept is worth its own icon it becomes
/// a variant, and the only arm that has to change is the icon map.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NodeKind {
    Database,
    Schema,
    Table,
    View,
    Column,
    Index,
    Constraint,
    /// A numbered logical database or type group in a non-SQL store.
    Namespace,
    /// One key in a non-SQL store.
    Key,
    /// A grouping row dodo inserts — "Tables", "Columns" — rather than an
    /// object the server has. Always carries a [`NodeLabel::Group`].
    Folder,
    /// Anything else a driver reports.
    ///
    /// It exists because it is the escape hatch for a concept dodo has not met;
    /// `services::fake` exercises it so a later driver is not forced to grow
    /// this enum before it can render a tree.
    #[allow(
        dead_code,
        reason = "the escape hatch a non-SQL driver uses; see above"
    )]
    Other,
}

/// The word a grouping row shows. A closed set, because each one is a `Str`
/// variant in every language.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GroupLabel {
    Tables,
    Views,
    Columns,
    Indexes,
    Constraints,
    More,
}

impl GroupLabel {
    pub fn text(self) -> Str {
        match self {
            GroupLabel::Tables => db_catalog::Text::GroupTables.into(),
            GroupLabel::Views => db_catalog::Text::GroupViews.into(),
            GroupLabel::Columns => db_catalog::Text::GroupColumns.into(),
            GroupLabel::Indexes => db_catalog::Text::GroupIndexes.into(),
            GroupLabel::Constraints => db_catalog::Text::GroupConstraints.into(),
            GroupLabel::More => db_catalog::Text::GroupMore.into(),
        }
    }
}

/// What a tree row reads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NodeLabel {
    /// An identifier the server gave us. Data: shown byte-for-byte, in every
    /// language.
    Name(String),
    /// A word dodo chose. Translated.
    Group(GroupLabel),
}

/// One row of the object tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogNode {
    pub id: NodeId,
    pub kind: NodeKind,
    pub label: NodeLabel,
    /// The dimmed trailing text: a column's type, an index's uniqueness, a
    /// schema's owner. Data, never translated — it is the server's own words.
    pub detail: Option<String>,
    /// Whether to draw a disclosure triangle **without a round trip to find
    /// out**. A driver knows from the row it just read whether the thing can
    /// have children; making the tree ask would turn one query per expansion
    /// into one query per row.
    pub expandable: bool,
}

impl CatalogNode {
    /// A leaf named by the server.
    pub fn leaf(id: impl Into<String>, kind: NodeKind, label: impl Into<String>) -> Self {
        Self {
            id: NodeId::new(id),
            kind,
            label: NodeLabel::Name(label.into()),
            detail: None,
            expandable: false,
        }
    }

    /// A node named by the server that can be expanded.
    pub fn branch(id: impl Into<String>, kind: NodeKind, label: impl Into<String>) -> Self {
        Self {
            expandable: true,
            ..Self::leaf(id, kind, label)
        }
    }

    /// A grouping row dodo names itself. Always expandable: a group with
    /// nothing in it still opens, and shows that it is empty.
    pub fn group(id: impl Into<String>, label: GroupLabel) -> Self {
        Self {
            id: NodeId::new(id),
            kind: NodeKind::Folder,
            label: NodeLabel::Group(label),
            detail: None,
            expandable: true,
        }
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        let detail = detail.into();
        self.detail = (!detail.is_empty()).then_some(detail);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::{CatalogNode, GroupLabel, NodeId, NodeKind, NodeLabel};
    use crate::i18n::Language;

    #[test]
    fn a_leaf_is_not_expandable_and_a_branch_is() {
        let leaf = CatalogNode::leaf("c:users.id", NodeKind::Column, "id");
        assert!(!leaf.expandable);
        assert_eq!(leaf.label, NodeLabel::Name("id".into()));

        let branch = CatalogNode::branch("t:users", NodeKind::Table, "users");
        assert!(branch.expandable);
    }

    #[test]
    fn a_group_row_is_always_expandable_and_carries_a_translated_label() {
        let group = CatalogNode::group("g:tables", GroupLabel::Tables);
        assert!(group.expandable);
        assert_eq!(group.kind, NodeKind::Folder);
        assert_eq!(group.label, NodeLabel::Group(GroupLabel::Tables));
    }

    #[test]
    fn an_empty_detail_is_no_detail_rather_than_a_blank_suffix() {
        let node = CatalogNode::leaf("x", NodeKind::Column, "id").with_detail("");
        assert_eq!(node.detail, None);

        let typed = CatalogNode::leaf("x", NodeKind::Column, "id").with_detail("int4");
        assert_eq!(typed.detail.as_deref(), Some("int4"));
    }

    #[test]
    fn every_group_label_reads_in_every_language() {
        for label in [
            GroupLabel::Tables,
            GroupLabel::Views,
            GroupLabel::Columns,
            GroupLabel::Indexes,
            GroupLabel::Constraints,
            GroupLabel::More,
        ] {
            for language in Language::ALL {
                assert!(!label.text().text(language).trim().is_empty());
            }
        }
    }

    #[test]
    fn a_node_id_is_carried_verbatim() {
        let id = NodeId::new("pg:table:public.users");
        assert_eq!(id.as_str(), "pg:table:public.users");
        assert_eq!(id, NodeId::new("pg:table:public.users".to_string()));
    }
}
