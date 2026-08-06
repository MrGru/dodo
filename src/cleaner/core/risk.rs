/// How dangerous an item is to remove. The mock scanners only ever produce
/// `SafeRecreatable`; the rest are the classifications real scanners assign.
/// `#[allow(dead_code)]` comes off with the first real scanner.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[allow(dead_code)]
pub enum RiskLevel {
    SafeRecreatable,
    ReviewRecommended,
    UserData,
    ApplicationMutation,
    Protected,
}

/// Whether an item starts ticked, and whether it may be bulk-selected at all.
/// Pending with [`RiskLevel`]: the mock scanners mark everything
/// `SelectedByDefault`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[allow(dead_code)]
pub enum SelectionPolicy {
    SelectedByDefault,
    NotSelectedByDefault,
    NeverBulkSelect,
}

/// What the UI may offer for an item. Round 1 has no destructive cleanup path,
/// so the mock scanners only claim the two read-only capabilities; the rest
/// arrive with the actions that perform them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[allow(dead_code)]
pub enum ItemCapability {
    MoveToTrash,
    EmptyTrash,
    RevealInFinder,
    CopyPath,
    UninstallApplication,
    RemoveArchitecture,
    RemoveLocalization,
    RunExternalCleanup,
}
