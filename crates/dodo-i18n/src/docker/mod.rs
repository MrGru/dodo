//! The Docker tool.
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
    // Docker module — sidebar section and page names. These are Docker's own
    // resource types (and the product name), the same words in both languages we
    // ship, so they are terms of art like JSON/JWT above rather than prose.
    Docker,
    Containers,
    Images,
    Volumes,
    Networks,

    // Docker module — Containers toolbar.
    SearchPlaceholder,
    Refresh,
    Filter,
    Create,

    // Docker module — Containers table columns.
    ColumnName,
    ColumnImage,
    ColumnStatus,
    ColumnCpu,
    ColumnPorts,
    ColumnLastStarted,
    ColumnActions,

    // Docker module — status badges.
    StatusRunning,
    StatusExited,
    StatusCreated,
    StatusRestarting,
    StatusPaused,
    StatusDead,
    StatusRemoving,
    StatusStopping,
    StatusUnknown,

    // Docker module — per-row actions and the delete confirmation.
    Start,
    Stop,
    Restart,
    DeleteTitle,
    /// "Permanently remove \"{name}\"? …" — the container name is user data.
    DeleteMessage(String),
    Cancel,

    // Docker module — empty and error states.
    NoContainers,
    NoContainersHint,
    Retry,
    /// bollard's own connection message is third-party English, kept verbatim.
    ConnectionError(String),
    /// bollard's own operation message is third-party English, kept verbatim.
    OperationError(String),

    // Docker module — row selection.
    SelectAll,
    SelectRow,

    // Docker module — Last Started relative time.
    RelNever,
    RelJustNow,
    RelSecondsAgo(u64),
    RelMinutesAgo(u64),
    RelHoursAgo(u64),
    RelDaysAgo(u64),
    RelWeeksAgo(u64),
    RelMonthsAgo(u64),
    RelYearsAgo(u64),

    // Docker module — error-state title (the detail follows below it).
    UnreachableTitle,

    // Docker module (round 2) — compose grouping.
    Ungrouped,
    GroupContainers(usize),
    GroupRunning(usize),

    // Docker module (round 2) — the filter popover.
    FilterWithCount(usize),
    FilterTitle,
    FilterProject,
    FilterPublishedPorts,
    FilterFavorites,
    FilterClear,

    // Docker module (round 2) — bulk actions on the selection.
    BulkSelected(usize),
    BulkStart,
    BulkStop,
    BulkDelete,
    BulkClear,
    BulkDeleteTitle,
    BulkDeleteMessage(usize),
    BulkFailures(usize),

    // Docker module (round 3) — Images, Volumes and Networks pages: their extra
    // column headers, per-resource search placeholders, empty states and the
    // shared Inspect action / N/A / "<none>" tokens.
    ColumnRepository,
    ColumnTag,
    ColumnImageId,
    ColumnSize,
    ColumnCreated,
    ColumnContainersUsing,
    ColumnDriver,
    ColumnMountPoint,
    ColumnScope,
    SearchImages,
    SearchVolumes,
    SearchNetworks,
    NoImages,
    NoImagesHint,
    NoVolumes,
    NoVolumesHint,
    NoNetworks,
    NoNetworksHint,
    NotAvailable,
    None,
    Inspect,
    NetworkPredefined,

    // Docker module (round 4) — right-click context-menu items for the container
    // detail views a later round fills in, and the section label that marks them
    // as not yet available.
    ViewLogs,
    OpenTerminal,
    ComingSoonLabel,

    // Docker module (round 5) — the read-only Inspect panel and Logs viewer:
    // their chrome, and the field labels the Inspect field list uses that no
    // table column already names.
    Details,
    RawJson,
    DetailErrorTitle,
    NoLogs,
    NoLogsHint,
    LogsTail(usize),
    Yes,
    No,
    FieldId,
    FieldCommand,
    FieldStarted,
    FieldExitCode,
    FieldRestartPolicy,
    FieldIpAddress,
    FieldMounts,
    FieldTags,
    FieldDigest,
    FieldArchitecture,
    FieldOs,
    FieldLayers,
    FieldLabels,
    FieldOptions,
    FieldInternal,
    FieldAttachable,
    FieldSubnet,
    FieldGateway,

    // Docker module (round 5) — the remaining "coming soon" placeholders, named
    // so they read as future features rather than broken controls.
    Pull,
    Build,
    Stats,

    // Docker module (round 6) — the tooltip on a row's identifying cell, which
    // is now the click target that opens the detail dialog. The dialog's own
    // tab labels reuse `Inspect` and `ViewLogs`.
    OpenDetails,

    // Docker module (round 7) — the Runtimes tab: automatic detection of the
    // container runtimes/daemons on this machine plus Start/Stop. The tab
    // title is a term of art like the other three page names; row names reuse
    // `Docker` for the Docker row, and `Start`/`Stop`/
    // `Refresh`/`OperationError` for the actions and their
    // failure, since those are exactly the same concepts already named above.
    Runtimes,
    RuntimesDescription,
    RuntimePodmanMachine,
    RuntimeKubernetes,
    RuntimeContainerd,
    RuntimeStatusRunning,
    RuntimeStatusStopped,
    RuntimeStatusNotInstalled,
    RuntimeStatusUnsupported,
    RuntimeStatusUnknown,
    RuntimeManagedExternally,
    RuntimeStarting,
    RuntimeStopping,
    RuntimeBinaryNotFound,
    RuntimeActionUnsupported,
}
