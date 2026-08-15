//! The Cleaner tool.
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
    // Cleaner.
    UnsupportedPlatform,
    Scan,
    CancelScan,
    NoResultsYet,
    StatusScanning,
    StatusCancelling,
    StatusPartial,
    StatusCompleted,
    StatusCleaning,
    StatusFailed,
    SectionCleanup,
    SectionApplications,
    SectionAdvanced,
    CategorySystemJunk,
    CategoryUserCache,
    CategoryMailFiles,
    CategoryTrashBins,
    CategoryLargeOldFiles,
    CategoryInstalledApps,
    CategoryOrphanedFiles,
    CategoryAiApps,
    CategoryXcodeJunk,
    CategoryHomebrewCache,
    CategoryNodeToolingCache,
    CategoryDockerCache,
    CategoryUniversalBinaries,
    CategoryLanguageFiles,
    Warnings,
    Path,
    Explanation,
    CopyPath,
    RevealInFinder,
    /// Only reachable from `results_table::reveal_label` on a Windows build
    /// (its Windows `#[cfg]` arm). The blocking `clippy` CI job runs on
    /// `macos-15`, where cfg strips that arm and this variant is never
    /// constructed at all — dead code on this platform's build, live on
    /// Windows'. Comes off if `reveal_label` ever stops being cfg'd per
    /// platform.
    #[allow(dead_code)]
    RevealInExplorer,
    /// See [`Text::RevealInExplorer`] — same reasoning, for Linux's arm.
    #[allow(dead_code)]
    RevealInFileManager,
    #[allow(
        dead_code,
        reason = "The results grid's overflow menu became one visible button per \
                  action on 2026-08-13, so nothing renders this any more. The \
                  variant and its translations stay so the string is not lost if \
                  the menu comes back; remove all three together if it does not."
    )]
    MoreActions,
    ColumnName,
    ColumnRisk,
    ColumnSize,
    ColumnActions,
    RiskSafe,
    RiskReview,
    RiskUserData,
    RiskAppChange,
    RiskProtected,
    SelectItem,
    DeselectItem,
    SelectSafeItems,
    CleanSelected,
    CleanupReport,
    CleanupConfirmTitle,
    CleanupConfirmMessage {
        count: usize,
        size: String,
    },
    CleanupSuccessCount(usize),
    CleanupFailureCount(usize),
    PermissionTitle,
    PermissionExplanation,
    PermissionOpenSettings,
    PartialPermissionDenied,
    PartialRootUnavailable,
    PartialCancelled,
    PartialUnsupported,
    BeginUninstallReview,
    UninstallReviewTitle {
        name: String,
    },
    UninstallLoading,
    UninstallRefusedProtected,
    UninstallRefusedNotApplication,
    UninstallRelatedFilesHeader,
    UninstallNoRelatedFiles,
    UninstallDestinationNote,
    UninstallScanOnlyBadge,
    UninstallMoveToTrash,
    UninstallClose,
    UninstallApplication,
    ConfidenceConfirmed,
    ConfidenceHigh,
    ConfidenceMedium,
    ConfidenceLow,
    ConfidenceSharedOrUnsafe,
    KeepItem,
    IgnoreStoreError(String),
    IgnoreStoreMissingVersion,
    IgnoreStoreUnsupportedVersion {
        found: u64,
        understood: u32,
    },
    DockerCleanupConfirmTitle,
    DockerCleanupConfirmMessage {
        count: usize,
        size: String,
    },

    // Cleaner UX/state refactor.
    ScanDescription,
    EntriesScannedCount(u64),
    BytesDiscovered(String),
    ReclaimableAmount(String),
    ItemsFound(usize),
    SafeItemsCount(usize),
    WarningCount(usize),
    SelectedSummary {
        count: usize,
        size: String,
    },
    CleanCount {
        count: usize,
        size: String,
    },
    ScanWarningsSummary(usize),
    ScanWarningsShowDetails,
    ScanWarningsHideDetails,
    Rescan,
    SelectAll,
    DeselectAll,
    PermissionNotNow,
    StatusCompletedWithWarnings,
    StatusCancelled,

    EmptyTrash,
    EmptyTrashConfirmTitle,
    EmptyTrashConfirmMessage {
        count: u64,
        size: String,
    },

    // Windows Cleaner Installed Apps.
    #[cfg_attr(
        not(target_os = "windows"),
        allow(dead_code, reason = "Windows-only Cleaner copy.")
    )]
    OpenInstalledAppsSettings,
}
