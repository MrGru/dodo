//! `C:\$Recycle.Bin` only — the system drive's Recycle Bin, not every fixed
//! drive's. Windows gives no portable way to enumerate drive letters without
//! calling into `GetLogicalDrives`, and the system drive covers the
//! overwhelming majority of installs; a future round can widen this once
//! that trade-off is worth a Win32 call nothing here can test.
//!
//! `$Recycle.Bin` nests one subfolder per SID, and only the current user's
//! own is normally readable — the other subfolders fail to `read_dir` with
//! `PermissionDenied`, which `core::fs::scan_root` already turns into a
//! per-entry warning rather than aborting the whole scan (see its module
//! doc). So this reports the *current user's* recycle bin size, same as
//! `TrashBinsScanner` on macOS reports only `~/.Trash`.

use crate::core::cancellation::CancellationToken;
use crate::core::category::CleanerCategory;
use crate::core::errors::ScanError;
use crate::core::generic_scanner::{ItemPolicy, run_aggregated_scan};
use crate::core::permissions::MacPermission;
use crate::core::progress::{ProgressSink, ScanPhase, ScanProgress};
use crate::core::report::CategoryScanResult;
use crate::core::risk::{ItemCapability, RiskLevel, SelectionPolicy};
use crate::core::scan_context::ScanContext;
use crate::core::scan_root::{AggregateMode, ScanRoot};
use crate::core::scanner::CleanerScanner;

pub struct TrashBinsScanner;

impl TrashBinsScanner {
    pub fn new() -> Self {
        Self
    }
}

impl CleanerScanner for TrashBinsScanner {
    fn category(&self) -> CleanerCategory {
        CleanerCategory::TrashBins
    }

    fn required_permissions(&self) -> &[MacPermission] {
        const NONE: &[MacPermission] = &[];
        NONE
    }

    fn scan(
        &self,
        _context: &ScanContext,
        progress: &dyn ProgressSink,
        cancellation: &CancellationToken,
    ) -> Result<CategoryScanResult, ScanError> {
        progress.report(ScanProgress {
            category: CleanerCategory::TrashBins,
            phase: ScanPhase::Preparing,
            current_path: None,
            scanned_entries: 0,
            discovered_items: 0,
            discovered_bytes: 0,
        });

        let system_drive = std::env::var_os("SystemDrive")
            .map(|value| value.to_string_lossy().into_owned())
            .unwrap_or_else(|| "C:".to_string());
        let roots = vec![ScanRoot {
            path: format!("{system_drive}\\$Recycle.Bin").into(),
            max_depth: None,
            follow_symlinks: false,
            cross_filesystems: false,
            include_hidden: true,
            aggregate_mode: AggregateMode::WholeRoot,
            permission: None,
            risk: RiskLevel::ReviewRecommended,
        }];

        run_aggregated_scan(
            CleanerCategory::TrashBins,
            roots,
            ItemPolicy {
                risk: RiskLevel::ReviewRecommended,
                selection_policy: SelectionPolicy::NeverBulkSelect,
                capabilities: vec![ItemCapability::RevealInFinder, ItemCapability::CopyPath],
            },
            |_| {
                "Recycle Bin contents are review-only here; emptying it is a separate future flow."
                    .to_string()
            },
            |_| "Recycle Bin".to_string(),
            progress,
            cancellation,
        )
    }
}
