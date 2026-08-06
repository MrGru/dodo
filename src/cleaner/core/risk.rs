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
}
