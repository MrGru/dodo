//! One sample per [`Text`] variant, for the language tests.
//!
//! `samples!` also emits an exhaustive `match` over [`Text`], so a variant
//! with no entry here is a compile error.

use crate::i18n::tests::{NUMBER, NUMBER_TEXT, Sample, plain, term, with};

use super::Text;

samples! {
    plain StatusError;
    plain GroupTables;
    plain GroupViews;
    plain GroupColumns;
    plain GroupIndexes;
    plain GroupConstraints;
    plain TreeLoading;
    with FooterCapped(NUMBER) [NUMBER_TEXT];
    plain CancelledMessage;
    plain DetailData;
    term DetailDdl;
    plain DetailFieldNullable;
    plain DetailFieldNotNull;
    plain DetailFieldDefault;
    plain DetailFieldUnique;
    plain DetailFieldPrimary;
    plain DetailFieldDefinition;
    plain DetailClose;
    plain DetailUnavailable;
    plain DetailNoRows;
    plain DetailNoMetadata;
    plain DetailPrevious;
    plain DetailNext;
    with DetailPage(NUMBER) [NUMBER_TEXT];
    with DetailRowsRange { first: NUMBER as u64, last: 77 } [NUMBER_TEXT, "77"];
    plain DetailDdlReconstructed;
    plain DetailConstraintsPartial;
    plain DetailCopyDdl;
    with DetailMetadataTruncated(NUMBER) [NUMBER_TEXT];
    plain GroupMore;
    plain CatalogSearch;
    plain CatalogSearchPlaceholder;
    plain CatalogSearchLoading;
    plain CatalogSearchEmpty;
    plain CatalogSearchNoMatches;
    plain CatalogSearchConnectedOnly;
    with CatalogSearchTruncated(NUMBER) [NUMBER_TEXT];
    with CatalogSearchPartial(NUMBER) [NUMBER_TEXT];
    plain CatalogKindDatabase;
    plain CatalogKindSchema;
    plain CatalogKindTable;
    plain CatalogKindView;
    plain CatalogKindColumn;
    plain CatalogKindIndex;
    plain CatalogKindConstraint;
    plain CatalogKindNamespace;
    plain CatalogKindKey;
    plain CatalogKindObject;
}
