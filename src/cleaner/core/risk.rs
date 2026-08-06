#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RiskLevel {
    SafeRecreatable,
    ReviewRecommended,
    UserData,
    ApplicationMutation,
    Protected,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SelectionPolicy {
    SelectedByDefault,
    NotSelectedByDefault,
    NeverBulkSelect,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ItemCapability {
    MoveToTrash,
    EmptyTrash,
    RevealInFinder,
    CopyPath,
    UninstallApplication,
    RemoveArchitecture,
    RemoveLocalization,
    RunExternalCleanup,
    /// Marks an orphan-detection candidate as reviewed-and-kept (Phase 10):
    /// exclude it from future scans without cleaning it up. See
    /// `crate::cleaner::core::ignore` and
    /// `crate::cleaner::services::ignore_store`.
    MarkAsKept,
}
