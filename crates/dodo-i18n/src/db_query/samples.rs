//! One sample per [`Text`] variant, for the language tests.
//!
//! `samples!` also emits an exhaustive `match` over [`Text`], so a variant
//! with no entry here is a compile error.

use crate::tests::{DETAIL, NUMBER, NUMBER_TEXT, Sample, plain, term, with};

use super::Text;

samples! {
    plain Query;
    plain Execute;
    plain Format;
    plain Running;
    plain NoStatement;
    plain Result;
    plain NoResultYet;
    plain NoResultYetHint;
    plain NoRows;
    with FooterRows(NUMBER) [NUMBER_TEXT];
    with FooterRowsAffected(NUMBER as u64) [NUMBER_TEXT];
    with FooterElapsed(DETAIL.into()) [DETAIL];
    with FooterTruncated(NUMBER) [NUMBER_TEXT];
    plain StatementLabel;
    term ColumnNull;
    plain SelectConnection;
    plain SelectConnectionHint;
    with QueryTabTitle(NUMBER) [NUMBER_TEXT];
    plain NewQueryTab;
    plain CloseQueryTab;
    plain CancelQuery;
    plain CancelledTitle;
    plain CancelledHint;
    plain Explain;
    plain CopyCell;
    plain CopyRow;
    plain ExportCsv;
    plain ExportJson;
    plain History;
    plain HistorySearch;
    plain HistoryEmpty;
    plain HistoryNoMatches;
    plain EditCell;
    plain AddRow;
    plain DeleteRow;
    plain DuplicateRow;
    plain Commit;
    plain Rollback;
    plain EditSelectRow;
    plain EditNoPending;
    with PendingChanges(NUMBER) [NUMBER_TEXT];
    term SetNull;
    with IdentityRequired(DETAIL.into()) [DETAIL];
    plain CommitTitle;
    with CommitSummary(NUMBER) [NUMBER_TEXT];
    plain CommitExactStatements;
    plain CommitParameters;
    plain CommitLostUpdateNotice;
    plain CommitRunning;
    with CommitStatementLabel(NUMBER) [NUMBER_TEXT];
    plain ExpectedOneRow;
    with QueryStoreError(DETAIL.into()) [DETAIL];
    plain QueryStoreMissingVersion;
    with QueryStoreUnsupportedVersion { found: NUMBER as u64, supported: 77 } [NUMBER_TEXT, "77"];
    plain SavedQueries;
    plain SaveQuery;
    plain SavedQuerySearch;
    plain SavedQueryEmpty;
    plain SavedQueryNoMatches;
    plain SavedQueryCreateTitle;
    plain SavedQueryEditTitle;
    plain SavedQueryName;
    plain SavedQueryNamePlaceholder;
    plain SavedQueryStatement;
    plain SavedQueryScope;
    plain SavedQueryPlaintextNotice;
    plain SavedQueryNameRequired;
    plain SavedQueryStatementRequired;
    plain SavedQueryEdit;
    plain SavedQueryDelete;
    plain HistoryClear;
    plain HistorySucceeded;
    plain HistoryFailed;
    plain HistoryJustNow;
    with HistoryMinutesAgo(NUMBER as u64) [NUMBER_TEXT];
    with HistoryHoursAgo(NUMBER as u64) [NUMBER_TEXT];
    with HistoryDaysAgo(NUMBER as u64) [NUMBER_TEXT];
}
