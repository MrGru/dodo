//! The Database Explorer's connection list and object tree.
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
    Connections,
    NoConnections,
    NoConnectionsHint,
    Connect,
    Disconnect,
    Reconnect,
    EditConnection,
    DuplicateConnection,
    DeleteConnection,
    /// Appended to a duplicated connection's name so the two are told apart.
    CopySuffix,
    StatusConnected,
    StatusConnecting,
    StatusDisconnected,
    DeleteConnectionTitle,
    /// The connection's display name.
    DeleteConnectionMessage(String),
    TreeEmpty,
    TreeNotConnected,
    RefreshTree,
    QueryPlaceholder,
    /// The driver's own message, kept verbatim inside a translated frame.
    Unreachable(String),
    ServerError(String),
    ServerErrorCoded {
        code: String,
        detail: String,
    },
    /// dodo could not reach the server to *ask* it to stop. The driver's own
    /// words, kept verbatim inside a translated frame.
    CancelFailed(String),
    ExportSucceeded {
        rows: usize,
        path: String,
    },
    ExportCancelled,
    ExportFailed(String),
    CommandPlaceholder,

    // Database Explorer round 5: safe pending table-data mutations.
    EditUnsupported,
    EditNoColumns,
    EditMissingOrigin(String),
    EditMultipleTables,
    EditDuplicateColumn(String),
    EditNoUniqueIdentity(String),
    EditMissingIdentityColumns {
        table: String,
        columns: String,
    },
    EditMetadataFailed(String),
    EditIdentityColumn,
    EditIdentityUnavailable,
    EditUnsupportedCell,
    EditCellTitle(String),
    AddRowTitle,
    DuplicateRowTitle,
    CommitSucceeded(usize),
    CommitAffectedMismatch {
        statement: usize,
        actual: u64,
    },
    CommitFailed {
        statement: usize,
        detail: String,
    },
    CommitTransactionFailed(String),
    CommitBuildFailed,
    ResolvePending,
    EditDuplicateRows,
    SavedQueryDeleteTitle,
    SavedQueryDeleteMessage(String),
    SavedQueryScopeMismatch(String),
    HistoryClearTitle,
    HistoryClearMessage,
    CatalogSearchConnectionUnavailable(String),
    QuickNavOpenedConnection(String),
    QuickNavKeptStoredPassword(String),
    QuickNavCreatedConnection(String),
    QuickNavConnectionsLoading,
}
