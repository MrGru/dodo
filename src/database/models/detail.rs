//! Object-detail requests and backend-neutral metadata.
//!
//! A detail request names the tree's existing opaque [`NodeId`]. Only the
//! driver that created the id interprets it. Table data is the one paged
//! section: the offset is chosen by `state::detail` from the number of rows
//! actually kept, so a byte-budget stop cannot skip rows on the next page.

use crate::database::models::catalog::{NodeId, NodeKind};
use crate::i18n::Str;

/// Rows requested from the server for one table-data page.
pub const DATA_PAGE_SIZE: u64 = 100;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DetailTab {
    Data,
    Columns,
    Indexes,
    Constraints,
    Ddl,
}

impl DetailTab {
    pub const ALL: [Self; 5] = [
        Self::Data,
        Self::Columns,
        Self::Indexes,
        Self::Constraints,
        Self::Ddl,
    ];

    pub fn applies_to(self, kind: NodeKind) -> bool {
        match kind {
            NodeKind::Table => true,
            NodeKind::View => matches!(self, Self::Data | Self::Columns | Self::Ddl),
            NodeKind::Key => self == Self::Data,
            _ => false,
        }
    }

    pub fn label(self) -> Str {
        match self {
            Self::Data => Str::DbDetailData,
            Self::Columns => Str::DbGroupColumns,
            Self::Indexes => Str::DbGroupIndexes,
            Self::Constraints => Str::DbGroupConstraints,
            Self::Ddl => Str::DbDetailDdl,
        }
    }
}

/// Where the DDL shown by a backend came from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DdlSource {
    #[default]
    None,
    Server,
    Reconstructed,
}

/// A metadata-grid heading chosen by dodo rather than supplied by the server.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DetailField {
    Name,
    Type,
    Nullable,
    NotNull,
    Default,
    Unique,
    Primary,
    Definition,
}

impl DetailField {
    pub fn label(self) -> Str {
        match self {
            Self::Name => Str::DbFieldName,
            Self::Type => Str::DbFieldEngine,
            Self::Nullable => Str::DbDetailFieldNullable,
            Self::NotNull => Str::DbDetailFieldNotNull,
            Self::Default => Str::DbDetailFieldDefault,
            Self::Unique => Str::DbDetailFieldUnique,
            Self::Primary => Str::DbDetailFieldPrimary,
            Self::Definition => Str::DbDetailFieldDefinition,
        }
    }
}

/// A truthful qualification attached to metadata that a backend cannot fully
/// enumerate without parsing SQL.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DetailNotice {
    /// SQLite exposes primary, foreign-key and unique constraints as rows, but
    /// not CHECK constraints. Its stored DDL remains available in the DDL tab.
    SqliteConstraintsExcludeChecks,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DetailTarget {
    pub node: NodeId,
    pub kind: NodeKind,
    /// The server's identifier, displayed byte-for-byte.
    pub name: String,
}

impl DetailTarget {
    pub fn new(node: NodeId, kind: NodeKind, name: impl Into<String>) -> Self {
        Self {
            node,
            kind,
            name: name.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DetailRequest {
    pub target: DetailTarget,
    pub tab: DetailTab,
    /// Used only for [`DetailTab::Data`]. It is ignored for metadata and DDL.
    pub offset: u64,
}

impl DetailRequest {
    pub fn new(target: DetailTarget, tab: DetailTab, offset: u64) -> Self {
        Self {
            target,
            tab,
            offset,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DetailTab, DetailTarget};
    use crate::database::models::catalog::{NodeId, NodeKind};
    use crate::i18n::Language;

    #[test]
    fn tables_have_every_detail_and_views_hide_impossible_sections() {
        assert!(
            DetailTab::ALL
                .iter()
                .all(|tab| tab.applies_to(NodeKind::Table))
        );
        assert!(DetailTab::Data.applies_to(NodeKind::View));
        assert!(DetailTab::Columns.applies_to(NodeKind::View));
        assert!(DetailTab::Ddl.applies_to(NodeKind::View));
        assert!(!DetailTab::Indexes.applies_to(NodeKind::View));
        assert!(!DetailTab::Constraints.applies_to(NodeKind::View));
        assert!(DetailTab::Data.applies_to(NodeKind::Key));
        assert!(!DetailTab::Columns.applies_to(NodeKind::Key));
        assert!(!DetailTab::Data.applies_to(NodeKind::Column));
    }

    #[test]
    fn detail_labels_exist_in_every_language() {
        for tab in DetailTab::ALL {
            for language in Language::ALL {
                assert!(!tab.label().text(language).trim().is_empty());
            }
        }
    }

    #[test]
    fn a_target_keeps_the_catalog_identity_verbatim() {
        let target = DetailTarget::new(
            NodeId::new("t\u{1f}public\u{1f}odd.name"),
            NodeKind::Table,
            "odd.name",
        );
        assert_eq!(target.node.as_str(), "t\u{1f}public\u{1f}odd.name");
        assert_eq!(target.name, "odd.name");
    }
}
