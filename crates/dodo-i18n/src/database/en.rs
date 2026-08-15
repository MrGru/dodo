//! The English column of the Database Explorer's connections.

use std::borrow::Cow;

use super::Text;

pub(crate) fn text(text: Text) -> Cow<'static, str> {
    match text {
        Text::Connections => "Connections".into(),
        Text::NoConnections => "No connections yet".into(),
        Text::NoConnectionsHint => {
                "Add one to browse a database and run queries.".into()
            }
        Text::Connect => "Connect".into(),
        Text::Disconnect => "Disconnect".into(),
        Text::Reconnect => "Reconnect".into(),
        Text::EditConnection => "Edit".into(),
        Text::DuplicateConnection => "Duplicate".into(),
        Text::DeleteConnection => "Delete".into(),
        Text::CopySuffix => "copy".into(),
        Text::StatusConnected => "Connected".into(),
        Text::StatusConnecting => "Connecting…".into(),
        Text::StatusDisconnected => "Disconnected".into(),
        Text::DeleteConnectionTitle => "Delete connection?".into(),
        Text::DeleteConnectionMessage(name) => {
                format!("\"{name}\" will be removed from this list. The database itself is left alone.")
                    .into()
            }
        Text::TreeEmpty => "Nothing here".into(),
        Text::TreeNotConnected => "Not connected".into(),
        Text::RefreshTree => "Refresh".into(),
        Text::QueryPlaceholder => {
                "Write SQL here, then press Execute.".into()
            }
        Text::Unreachable(detail) => {
                format!("The database could not be reached: {detail}").into()
            }
        Text::ServerError(detail) => {
                format!("The server rejected the statement: {detail}").into()
            }
        Text::ServerErrorCoded { code, detail } => {
                format!("The server rejected the statement ({code}): {detail}").into()
            }
        Text::CancelFailed(detail) => format!(
                "dodo could not reach the server to cancel, so the statement may still be \
                 running: {detail}"
            )
            .into(),
        Text::ExportSucceeded { rows, path } => {
                format!("Exported {rows} rows to {path}.").into()
            }
        Text::ExportCancelled => "Export cancelled.".into(),
        Text::ExportFailed(detail) => {
                format!("The result could not be exported: {detail}").into()
            }
        Text::CommandPlaceholder => {
                "Enter one Redis command per line.".into()
            }
        Text::EditUnsupported => {
                "This result is read-only: this database does not support safe table editing.".into()
            }
        Text::EditNoColumns => {
                "This result is read-only because it has no columns.".into()
            }
        Text::EditMissingOrigin(column) => format!(
                "This result is read-only: column {column} does not come from one base table."
            )
            .into(),
        Text::EditMultipleTables => {
                "This result is read-only because it combines several tables.".into()
            }
        Text::EditDuplicateColumn(column) => format!(
                "This result is read-only because base column {column} appears more than once."
            )
            .into(),
        Text::EditNoUniqueIdentity(table) => format!(
                "This result is read-only: {table} has no primary key or non-null unique index."
            )
            .into(),
        Text::EditMissingIdentityColumns { table, columns } => format!(
                "This result is read-only: identity column(s) {columns} from {table} are not in the result."
            )
            .into(),
        Text::EditMetadataFailed(detail) => {
                format!("This result is read-only because identity metadata could not be loaded: {detail}").into()
            }
        Text::EditIdentityColumn => {
                "Identity columns cannot be edited in place.".into()
            }
        Text::EditIdentityUnavailable => {
                "This row cannot be changed because its complete identity value is unavailable.".into()
            }
        Text::EditUnsupportedCell => {
                "This cell cannot be edited safely in this result.".into()
            }
        Text::EditCellTitle(column) => {
                format!("Edit {column}").into()
            }
        Text::AddRowTitle => "Add row".into(),
        Text::DuplicateRowTitle => "Duplicate row".into(),
        Text::CommitSucceeded(count) => {
                format!("Committed {count} row change(s).").into()
            }
        Text::CommitAffectedMismatch { statement, actual } => format!(
                "Statement {statement} matched {actual} rows instead of exactly 1. The whole transaction was rolled back."
            )
            .into(),
        Text::CommitFailed { statement, detail } => format!(
                "Statement {statement} failed: {detail}. The whole transaction was rolled back."
            )
            .into(),
        Text::CommitTransactionFailed(detail) => {
                format!("The transaction could not complete: {detail}").into()
            }
        Text::CommitBuildFailed => {
                "The pending changes could not be generated safely.".into()
            }
        Text::ResolvePending => {
                "Commit or Rollback the pending changes first.".into()
            }
        Text::EditDuplicateRows => {
                "This result is read-only because more than one displayed row has the same unique identity.".into()
            }
        Text::SavedQueryDeleteTitle => "Delete saved query?".into(),
        Text::SavedQueryDeleteMessage(name) => {
                format!("Delete “{name}”? This cannot be undone.").into()
            }
        Text::SavedQueryScopeMismatch(name) => format!(
                "Opened as text only because its saved connection “{name}” is missing or now points elsewhere. Select the intended connection before running it."
            )
            .into(),
        Text::HistoryClearTitle => "Clear query history?".into(),
        Text::HistoryClearMessage => {
                "Delete all persisted query history? Saved queries are not affected.".into()
            }
        Text::CatalogSearchConnectionUnavailable(name) => format!(
                "The catalog result cannot be opened because connection “{name}” is no longer connected or now points elsewhere."
            )
            .into(),
        Text::QuickNavOpenedConnection(name) => {
                format!("Opened the saved connection \"{name}\".").into()
            }
        Text::QuickNavKeptStoredPassword(name) => format!(
                "Opened the saved connection \"{name}\". Its stored password was kept; the pasted \
                 one was not used."
            )
            .into(),
        Text::QuickNavCreatedConnection(name) => {
                format!("Created the connection \"{name}\" from the pasted URI.").into()
            }
        Text::QuickNavConnectionsLoading => {
                "The saved connections are still loading, so nothing was created. Paste the URI \
                 again in a moment."
                    .into()
            }
    }
}
