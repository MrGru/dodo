//! One sample per [`Text`] variant, for the language tests.
//!
//! `samples!` also emits an exhaustive `match` over [`Text`], so a variant
//! with no entry here is a compile error.

use crate::tests::{DETAIL, NUMBER, NUMBER_TEXT, Sample, plain, with};

use super::Text;

samples! {
    plain Connections;
    plain NoConnections;
    plain NoConnectionsHint;
    plain Connect;
    plain Disconnect;
    plain Reconnect;
    plain EditConnection;
    plain DuplicateConnection;
    plain DeleteConnection;
    plain CopySuffix;
    plain StatusConnected;
    plain StatusConnecting;
    plain StatusDisconnected;
    plain DeleteConnectionTitle;
    with DeleteConnectionMessage(DETAIL.into()) [DETAIL];
    plain TreeEmpty;
    plain TreeNotConnected;
    plain RefreshTree;
    plain QueryPlaceholder;
    with Unreachable(DETAIL.into()) [DETAIL];
    with ServerError(DETAIL.into()) [DETAIL];
    with ServerErrorCoded { code: "42P01".into(), detail: DETAIL.into() } ["42P01", DETAIL];
    with CancelFailed(DETAIL.into()) [DETAIL];
    with ExportSucceeded { rows: NUMBER, path: DETAIL.into() } [NUMBER_TEXT, DETAIL];
    plain ExportCancelled;
    with ExportFailed(DETAIL.into()) [DETAIL];
    plain CommandPlaceholder;
    plain EditUnsupported;
    plain EditNoColumns;
    with EditMissingOrigin(DETAIL.into()) [DETAIL];
    plain EditMultipleTables;
    with EditDuplicateColumn(DETAIL.into()) [DETAIL];
    with EditNoUniqueIdentity(DETAIL.into()) [DETAIL];
    with EditMissingIdentityColumns { table: DETAIL.into(), columns: "sentinel-columns".into() } [DETAIL, "sentinel-columns"];
    with EditMetadataFailed(DETAIL.into()) [DETAIL];
    plain EditIdentityColumn;
    plain EditIdentityUnavailable;
    plain EditUnsupportedCell;
    with EditCellTitle(DETAIL.into()) [DETAIL];
    plain AddRowTitle;
    plain DuplicateRowTitle;
    with CommitSucceeded(NUMBER) [NUMBER_TEXT];
    with CommitAffectedMismatch { statement: NUMBER, actual: 77 } [NUMBER_TEXT, "77"];
    with CommitFailed { statement: NUMBER, detail: DETAIL.into() } [NUMBER_TEXT, DETAIL];
    with CommitTransactionFailed(DETAIL.into()) [DETAIL];
    plain CommitBuildFailed;
    plain ResolvePending;
    plain EditDuplicateRows;
    plain SavedQueryDeleteTitle;
    with SavedQueryDeleteMessage(DETAIL.into()) [DETAIL];
    with SavedQueryScopeMismatch(DETAIL.into()) [DETAIL];
    plain HistoryClearTitle;
    plain HistoryClearMessage;
    with CatalogSearchConnectionUnavailable(DETAIL.into()) [DETAIL];
    with QuickNavOpenedConnection(DETAIL.into()) [DETAIL];
    with QuickNavKeptStoredPassword(DETAIL.into()) [DETAIL];
    with QuickNavCreatedConnection(DETAIL.into()) [DETAIL];
    plain QuickNavConnectionsLoading;
}
