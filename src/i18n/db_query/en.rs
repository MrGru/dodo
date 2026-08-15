//! The English column of the Database Explorer's query pane.

use std::borrow::Cow;

use super::Text;

pub(crate) fn text(text: Text) -> Cow<'static, str> {
    match text {
        Text::Query => "Query".into(),
        Text::Execute => "Execute".into(),
        Text::Format => "Format".into(),
        Text::Running => "Running…".into(),
        Text::NoStatement => "There is nothing to run.".into(),
        Text::Result => "Result".into(),
        Text::NoResultYet => "No result yet".into(),
        Text::NoResultYetHint => {
                "Run a statement to see its rows here.".into()
            }
        Text::NoRows => "The statement returned no rows.".into(),
        Text::FooterRows(count) => match count {
                1 => "1 row".into(),
                other => format!("{other} rows").into(),
            },
        Text::FooterRowsAffected(count) => match count {
                1 => "1 row affected".into(),
                other => format!("{other} rows affected").into(),
            },
        Text::FooterElapsed(elapsed) => format!("in {elapsed}").into(),
        Text::FooterTruncated(shown) => {
                format!("showing the first {shown} — the statement returned more").into()
            }
        Text::StatementLabel => "Statement".into(),
        Text::ColumnNull => "NULL".into(),
        Text::SelectConnection => "Select a connection".into(),
        Text::SelectConnectionHint => {
                "Choose one on the left to browse it and run queries.".into()
            }
        Text::QueryTabTitle(number) => format!("Query {number}").into(),
        Text::NewQueryTab => "New query".into(),
        Text::CloseQueryTab => "Close query".into(),
        Text::CancelQuery => "Cancel".into(),
        Text::CancelledTitle => "Cancelled".into(),
        Text::CancelledHint => {
                "The server confirmed it stopped, so nothing is still running there.".into()
            }
        Text::Explain => "Explain".into(),
        Text::CopyCell => "Copy cell".into(),
        Text::CopyRow => "Copy row".into(),
        Text::ExportCsv => "Export CSV".into(),
        Text::ExportJson => "Export JSON".into(),
        Text::History => "History".into(),
        Text::HistorySearch => "Search query history…".into(),
        Text::HistoryEmpty => "No queries have run yet.".into(),
        Text::HistoryNoMatches => "No matching queries.".into(),
        Text::EditCell => "Edit cell".into(),
        Text::AddRow => "Add row".into(),
        Text::DeleteRow => "Delete row".into(),
        Text::DuplicateRow => "Duplicate row".into(),
        Text::Commit => "Commit".into(),
        Text::Rollback => "Rollback".into(),
        Text::EditSelectRow => "Select a row first.".into(),
        Text::EditNoPending => "There are no pending changes.".into(),
        Text::PendingChanges(count) => {
                format!("{count} pending row change(s)").into()
            }
        Text::SetNull => "NULL".into(),
        Text::IdentityRequired(columns) => format!(
                "Enter a new value for non-generated identity column(s): {columns}."
            )
            .into(),
        Text::CommitTitle => "Confirm database changes".into(),
        Text::CommitSummary(count) => format!(
                "This transaction expects exactly {count} affected row(s). Review every statement before committing."
            )
            .into(),
        Text::CommitExactStatements => "Generated statements".into(),
        Text::CommitParameters => "Bound parameters".into(),
        Text::CommitLostUpdateNotice => {
                "Concurrent changes are not detected in this version; committing may overwrite a newer value from another client.".into()
            }
        Text::CommitRunning => "Committing changes…".into(),
        Text::CommitStatementLabel(number) => {
                format!("Statement {number}").into()
            }
        Text::ExpectedOneRow => "Expected affected rows: 1".into(),
        Text::QueryStoreError(detail) => {
                format!("Saved queries and history could not be read or written: {detail}").into()
            }
        Text::QueryStoreMissingVersion => {
                "The saved-query file has no version and was not loaded.".into()
            }
        Text::QueryStoreUnsupportedVersion { found, supported } => format!(
                "The saved-query file uses version {found}; this Dodo supports up to {supported}."
            )
            .into(),
        Text::SavedQueries => "Saved queries".into(),
        Text::SaveQuery => "Save query".into(),
        Text::SavedQuerySearch => "Search saved queries…".into(),
        Text::SavedQueryEmpty => "No saved queries yet.".into(),
        Text::SavedQueryNoMatches => "No matching saved queries.".into(),
        Text::SavedQueryCreateTitle => "Save query".into(),
        Text::SavedQueryEditTitle => "Edit saved query".into(),
        Text::SavedQueryName => "Name".into(),
        Text::SavedQueryNamePlaceholder => "e.g. Recent orders".into(),
        Text::SavedQueryStatement => "Query".into(),
        Text::SavedQueryScope => "Connection".into(),
        Text::SavedQueryPlaintextNotice => {
                "Saved queries are stored as plain text on this device. Remove passwords and other secrets before saving."
                    .into()
            }
        Text::SavedQueryNameRequired => "Enter a name for this query.".into(),
        Text::SavedQueryStatementRequired => "Enter query text to save.".into(),
        Text::SavedQueryEdit => "Edit saved query".into(),
        Text::SavedQueryDelete => "Delete saved query".into(),
        Text::HistoryClear => "Clear history".into(),
        Text::HistorySucceeded => "Succeeded".into(),
        Text::HistoryFailed => "Failed".into(),
        Text::HistoryJustNow => "Just now".into(),
        Text::HistoryMinutesAgo(minutes) => format!("{minutes}m ago").into(),
        Text::HistoryHoursAgo(hours) => format!("{hours}h ago").into(),
        Text::HistoryDaysAgo(days) => format!("{days}d ago").into(),
    }
}
