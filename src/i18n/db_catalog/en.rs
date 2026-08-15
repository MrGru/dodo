//! The English column of the Database Explorer's catalog.

use std::borrow::Cow;

use super::Text;

pub(crate) fn text(text: Text) -> Cow<'static, str> {
    match text {
        Text::StatusError => "Error".into(),
        Text::GroupTables => "Tables".into(),
        Text::GroupViews => "Views".into(),
        Text::GroupColumns => "Columns".into(),
        Text::GroupIndexes => "Indexes".into(),
        Text::GroupConstraints => "Constraints".into(),
        Text::TreeLoading => "Loading…".into(),
        Text::FooterCapped(count) => format!("{count} large values shortened").into(),
        Text::CancelledMessage => {
            "The server stopped the statement because you cancelled it.".into()
        }
        Text::DetailData => "Data".into(),
        Text::DetailDdl => "DDL".into(),
        Text::DetailFieldNullable => "Nullable".into(),
        Text::DetailFieldNotNull => "Not null".into(),
        Text::DetailFieldDefault => "Default".into(),
        Text::DetailFieldUnique => "Unique".into(),
        Text::DetailFieldPrimary => "Primary".into(),
        Text::DetailFieldDefinition => "Definition".into(),
        Text::DetailClose => "Close object detail".into(),
        Text::DetailUnavailable => "This detail is not available for this object.".into(),
        Text::DetailNoRows => "This object has no rows.".into(),
        Text::DetailNoMetadata => "No metadata was reported.".into(),
        Text::DetailPrevious => "Previous".into(),
        Text::DetailNext => "Next".into(),
        Text::DetailPage(page) => format!("Page {page}").into(),
        Text::DetailRowsRange { first, last } => format!("Rows {first}–{last}").into(),
        Text::DetailDdlReconstructed => {
            "Reconstructed from PostgreSQL catalog metadata; partitioning, inheritance, \
                 storage settings, comments and ownership may be omitted."
                .into()
        }
        Text::DetailConstraintsPartial => {
            "SQLite does not expose CHECK constraints as catalog rows. See the stored DDL for \
                 the complete definition."
                .into()
        }
        Text::DetailCopyDdl => "Copy DDL".into(),
        Text::DetailMetadataTruncated(count) => {
            format!("Showing the first {count} metadata rows.").into()
        }
        Text::GroupMore => "More…".into(),
        Text::CatalogSearch => "Search catalogs".into(),
        Text::CatalogSearchPlaceholder => "Search catalog objects…".into(),
        Text::CatalogSearchLoading => "Loading connected catalogs…".into(),
        Text::CatalogSearchEmpty => "No catalog objects were found.".into(),
        Text::CatalogSearchNoMatches => "No matching catalog objects.".into(),
        Text::CatalogSearchConnectedOnly => {
            "Search covers connected databases and builds one bounded in-memory catalog cache."
                .into()
        }
        Text::CatalogSearchTruncated(count) => {
            format!("Search stopped at the catalog limit after indexing {count} objects.").into()
        }
        Text::CatalogSearchPartial(count) => {
            format!("{count} catalog branch(es) could not be searched.").into()
        }
        Text::CatalogKindDatabase => "Database".into(),
        Text::CatalogKindSchema => "Schema".into(),
        Text::CatalogKindTable => "Table".into(),
        Text::CatalogKindView => "View".into(),
        Text::CatalogKindColumn => "Column".into(),
        Text::CatalogKindIndex => "Index".into(),
        Text::CatalogKindConstraint => "Constraint".into(),
        Text::CatalogKindNamespace => "Namespace".into(),
        Text::CatalogKindKey => "Key".into(),
        Text::CatalogKindObject => "Object".into(),
    }
}
