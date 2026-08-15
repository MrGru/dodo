//! The Database Explorer's catalog search and object detail panes.
//!
//! `en` and `vi` each render every variant below; the compiler names any
//! string a language has not been given.

pub(crate) mod en;
pub(crate) mod vi;

#[cfg(test)]
pub(crate) mod samples;

/// The strings this area owns.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Text {
    StatusError,
    GroupTables,
    GroupViews,
    GroupColumns,
    GroupIndexes,
    GroupConstraints,
    TreeLoading,
    /// How many oversized cells were shortened to fit the page budget.
    FooterCapped(usize),
    /// What [`DbError::Cancelled`](crate::database::models::error::DbError)
    /// reads as wherever a driver error is shown.
    CancelledMessage,

    // Database Explorer round 3: table and view detail.
    DetailData,
    DetailDdl,
    DetailFieldNullable,
    DetailFieldNotNull,
    DetailFieldDefault,
    DetailFieldUnique,
    DetailFieldPrimary,
    DetailFieldDefinition,
    DetailClose,
    DetailUnavailable,
    DetailNoRows,
    DetailNoMetadata,
    DetailPrevious,
    DetailNext,
    DetailPage(usize),
    DetailRowsRange {
        first: u64,
        last: u64,
    },
    DetailDdlReconstructed,
    DetailConstraintsPartial,
    DetailCopyDdl,
    DetailMetadataTruncated(usize),
    GroupMore,

    // Database Explorer round 6: bounded global catalog search.
    CatalogSearch,
    CatalogSearchPlaceholder,
    CatalogSearchLoading,
    CatalogSearchEmpty,
    CatalogSearchNoMatches,
    CatalogSearchConnectedOnly,
    CatalogSearchTruncated(usize),
    CatalogSearchPartial(usize),
    CatalogKindDatabase,
    CatalogKindSchema,
    CatalogKindTable,
    CatalogKindView,
    CatalogKindColumn,
    CatalogKindIndex,
    CatalogKindConstraint,
    CatalogKindNamespace,
    CatalogKindKey,
    CatalogKindObject,
}
