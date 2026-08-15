//! The Database Explorer's query pane, result grid, history and saved queries.
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
    Query,
    Execute,
    Format,
    Running,
    NoStatement,
    Result,
    NoResultYet,
    NoResultYetHint,
    NoRows,
    /// How many rows the grid is holding.
    FooterRows(usize),
    /// How many rows a statement changed.
    FooterRowsAffected(u64),
    /// The elapsed time, already formatted — the unit is chosen by magnitude,
    /// so the value arrives as text rather than as a number plus a guess.
    FooterElapsed(String),
    /// The page bound stopped the read: how many rows are shown.
    FooterTruncated(usize),
    StatementLabel,
    ColumnNull,
    SelectConnection,
    SelectConnectionHint,

    // Round 2: query tabs.
    /// A tab's default title, numbered in the order tabs were opened.
    QueryTabTitle(usize),
    NewQueryTab,
    CloseQueryTab,

    // Round 2: cancelling a running statement, at the server.
    CancelQuery,
    CancelledTitle,
    CancelledHint,

    // Round 2: PostgreSQL's non-executing query plan.
    Explain,

    // Round 2: result-grid clipboard actions.
    CopyCell,
    CopyRow,

    // Round 2: full-result streaming export.
    ExportCsv,
    ExportJson,

    // Round 2: searchable in-session query history.
    History,
    HistorySearch,
    HistoryEmpty,
    HistoryNoMatches,
    EditCell,
    AddRow,
    DeleteRow,
    DuplicateRow,
    Commit,
    Rollback,
    EditSelectRow,
    EditNoPending,
    PendingChanges(usize),
    SetNull,
    IdentityRequired(String),
    CommitTitle,
    CommitSummary(usize),
    CommitExactStatements,
    CommitParameters,
    CommitLostUpdateNotice,
    CommitRunning,
    CommitStatementLabel(usize),
    ExpectedOneRow,

    // Database Explorer round 6: saved queries and persisted history.
    QueryStoreError(String),
    QueryStoreMissingVersion,
    QueryStoreUnsupportedVersion {
        found: u64,
        supported: u32,
    },
    SavedQueries,
    SaveQuery,
    SavedQuerySearch,
    SavedQueryEmpty,
    SavedQueryNoMatches,
    SavedQueryCreateTitle,
    SavedQueryEditTitle,
    SavedQueryName,
    SavedQueryNamePlaceholder,
    SavedQueryStatement,
    SavedQueryScope,
    SavedQueryPlaintextNotice,
    SavedQueryNameRequired,
    SavedQueryStatementRequired,
    SavedQueryEdit,
    SavedQueryDelete,
    HistoryClear,
    HistorySucceeded,
    HistoryFailed,
    HistoryJustNow,
    HistoryMinutesAgo(u64),
    HistoryHoursAgo(u64),
    HistoryDaysAgo(u64),
}
