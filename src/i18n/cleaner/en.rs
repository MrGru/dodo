//! The English column of the Cleaner.

use std::borrow::Cow;

use super::Text;

pub(crate) fn text(text: Text) -> Cow<'static, str> {
    match text {
        Text::UnsupportedPlatform => {
                "Cleaner is currently available on macOS. Windows and Linux support will be added in future versions.".into()
            }
        Text::Scan => "Scan".into(),
        Text::CancelScan => "Cancel".into(),
        Text::NoResultsYet => {
                "No scan results for this category yet.".into()
            }
        Text::StatusScanning => "Scanning".into(),
        Text::StatusCancelling => "Cancelling".into(),
        Text::StatusPartial => "Partially completed".into(),
        Text::StatusCompleted => "Completed".into(),
        Text::StatusCleaning => "Cleaning".into(),
        Text::StatusFailed => "Failed".into(),
        Text::SectionCleanup => "Cleanup".into(),
        Text::SectionApplications => "Applications".into(),
        Text::SectionAdvanced => "Advanced".into(),
        Text::CategorySystemJunk => "System Junk".into(),
        Text::CategoryUserCache => "User Cache".into(),
        Text::CategoryMailFiles => "Mail Files".into(),
        Text::CategoryTrashBins => "Trash Bins".into(),
        Text::CategoryLargeOldFiles => "Large & Old Files".into(),
        Text::CategoryInstalledApps => "Installed Apps".into(),
        Text::CategoryOrphanedFiles => "Orphaned Files".into(),
        Text::CategoryAiApps => "AI Apps".into(),
        Text::CategoryXcodeJunk => "Xcode Junk".into(),
        Text::CategoryHomebrewCache => "Homebrew Cache".into(),
        Text::CategoryNodeToolingCache => {
                "Node Tooling Cache".into()
            }
        Text::CategoryDockerCache => "Docker Cache".into(),
        Text::CategoryUniversalBinaries => {
                "Universal Binaries".into()
            }
        Text::CategoryLanguageFiles => "Language Files".into(),
        Text::Warnings => "Warnings".into(),
        Text::Path => "Path".into(),
        Text::Explanation => "Explanation".into(),
        Text::CopyPath => "Copy path".into(),
        Text::RevealInFinder => "Reveal in Finder".into(),
        Text::RevealInExplorer => "Reveal in Explorer".into(),
        Text::RevealInFileManager => {
                "Reveal in file manager".into()
            }
        Text::MoreActions => "More actions".into(),
        Text::ColumnName => "Name".into(),
        Text::ColumnRisk => "Risk".into(),
        Text::ColumnSize => "Size".into(),
        Text::ColumnActions => "Actions".into(),
        Text::RiskSafe => "Safe".into(),
        Text::RiskReview => "Review".into(),
        Text::RiskUserData => "User Data".into(),
        Text::RiskAppChange => "App Change".into(),
        Text::RiskProtected => "Protected".into(),
        Text::SelectItem => "Select".into(),
        Text::DeselectItem => "Deselect".into(),
        Text::SelectSafeItems => "Select safe items".into(),
        Text::CleanSelected => "Clean selected".into(),
        Text::CleanupReport => "Cleanup report".into(),
        Text::CleanupConfirmTitle => {
                "Move selected items to Trash?".into()
            }
        Text::CleanupConfirmMessage { count, size } => format!("{count} items will be moved to the macOS Trash. Estimated size: {size}.").into(),
        Text::CleanupSuccessCount(count) => {
                format!("Moved to Trash: {count}").into()
            }
        Text::CleanupFailureCount(count) => {
                format!("Failed: {count}").into()
            }
        Text::PermissionTitle => {
                "Full Disk Access".into()
            }
        Text::PermissionExplanation => {
                "Some Cleaner categories need Full Disk Access to inspect protected macOS data safely."
                    .into()
            }
        Text::PermissionOpenSettings => {
                "Open settings".into()
            }
        Text::PartialPermissionDenied => {
                "Some locations were skipped because permission was denied.".into()
            }
        Text::PartialRootUnavailable => {
                "Some configured roots were unavailable on this machine.".into()
            }
        Text::PartialCancelled => {
                "The scan was cancelled before every root completed.".into()
            }
        Text::PartialUnsupported => {
                "This category will land in a later Cleaner phase.".into()
            }
        Text::BeginUninstallReview => {
                "Begin uninstall review".into()
            }
        Text::UninstallReviewTitle { name } => {
                format!("Uninstall {name}?").into()
            }
        Text::UninstallLoading => {
                "Analyzing related files…".into()
            }
        Text::UninstallRefusedProtected => {
                "System apps cannot be uninstalled.".into()
            }
        Text::UninstallRefusedNotApplication => {
                "This item cannot be reviewed for uninstall.".into()
            }
        Text::UninstallRelatedFilesHeader => {
                "Related files".into()
            }
        Text::UninstallNoRelatedFiles => {
                "No related files were found.".into()
            }
        Text::UninstallDestinationNote => {
                "The app and every checked file will move to the macOS Trash. You can restore them from Trash until it is emptied."
                    .into()
            }
        Text::UninstallScanOnlyBadge => {
                "Scan-only (system location)".into()
            }
        Text::UninstallMoveToTrash => "Move to Trash".into(),
        Text::UninstallClose => "Close".into(),
        Text::UninstallApplication => "Uninstall".into(),
        Text::ConfidenceConfirmed => "Confirmed".into(),
        Text::ConfidenceHigh => "High".into(),
        Text::ConfidenceMedium => "Medium".into(),
        Text::ConfidenceLow => "Low".into(),
        Text::ConfidenceSharedOrUnsafe => "Shared or unsafe".into(),
        Text::KeepItem => "Keep".into(),
        Text::IgnoreStoreError(detail) => format!(
                "cleaner-ignored-items.json could not be read or written: {detail}"
            )
            .into(),
        Text::IgnoreStoreMissingVersion => {
                "cleaner-ignored-items.json carries no version, so it was not written by dodo. \
                 It is being left alone and no items are marked kept."
                    .into()
            }
        Text::IgnoreStoreUnsupportedVersion { found, understood } => format!(
                "cleaner-ignored-items.json is version {found}; this dodo understands \
                 {understood}. The file is being left alone and no items are marked kept."
            )
            .into(),
        Text::DockerCleanupConfirmTitle => {
                "Remove selected Docker objects?".into()
            }
        Text::DockerCleanupConfirmMessage { count, size } => format!(
                "{count} Docker objects will be removed via the Docker CLI. This does not use \
                 the Trash and cannot be undone through dodo. Estimated size: {size}."
            )
            .into(),
        Text::ScanDescription => {
                "Scan this category for files that can be safely removed.".into()
            }
        Text::EntriesScannedCount(count) => {
                format!("{count} entries scanned").into()
            }
        Text::BytesDiscovered(size) => {
                format!("{size} discovered").into()
            }
        Text::ReclaimableAmount(size) => {
                format!("{size} reclaimable").into()
            }
        Text::ItemsFound(count) => format!("{count} items").into(),
        Text::SafeItemsCount(count) => {
                format!("{count} safe").into()
            }
        Text::WarningCount(count) => {
                let word = if count == 1 { "warning" } else { "warnings" };
                format!("{count} {word}").into()
            }
        Text::SelectedSummary { count, size } => {
                format!("{count} selected · {size}").into()
            }
        Text::CleanCount { count, size } => {
                format!("Clean {count} items · {size}").into()
            }
        Text::ScanWarningsSummary(count) => {
                let word = if count == 1 { "location" } else { "locations" };
                format!("{count} {word} could not be scanned").into()
            }
        Text::ScanWarningsShowDetails => "Show details".into(),
        Text::ScanWarningsHideDetails => "Hide details".into(),
        Text::Rescan => "Rescan".into(),
        Text::SelectAll => "Select all".into(),
        Text::DeselectAll => "Deselect all".into(),
        Text::PermissionNotNow => "Not now".into(),
        Text::StatusCompletedWithWarnings => {
                "Completed with warnings".into()
            }
        Text::StatusCancelled => "Cancelled".into(),
        Text::EmptyTrash => "Empty Trash".into(),
        Text::EmptyTrashConfirmTitle => "Empty Trash?".into(),
        Text::EmptyTrashConfirmMessage { count, size } => format!("{count} items will be permanently deleted. Estimated size: {size}.").into(),
        Text::OpenInstalledAppsSettings => {
                "Open Windows Installed Apps".into()
            }
    }
}
