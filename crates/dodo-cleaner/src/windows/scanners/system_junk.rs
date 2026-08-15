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

/// Only the current user's own temp directory (`%TEMP%`, ordinarily
/// `%LOCALAPPDATA%\Temp`) — never `%SystemRoot%\Temp`, which is shared across
/// every account and ordinarily needs administrator rights to clean. That
/// mirrors macOS's own choice not to scan anything outside the current
/// user's home for this category.
pub struct SystemJunkScanner;

impl SystemJunkScanner {
    pub fn new() -> Self {
        Self
    }
}

impl CleanerScanner for SystemJunkScanner {
    fn category(&self) -> CleanerCategory {
        CleanerCategory::SystemJunk
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
            category: CleanerCategory::SystemJunk,
            phase: ScanPhase::Preparing,
            current_path: None,
            scanned_entries: 0,
            discovered_items: 0,
            discovered_bytes: 0,
        });

        let roots = vec![ScanRoot {
            path: std::env::temp_dir(),
            max_depth: Some(2),
            follow_symlinks: false,
            cross_filesystems: false,
            include_hidden: true,
            aggregate_mode: AggregateMode::ImmediateChildren,
            permission: None,
            risk: RiskLevel::SafeRecreatable,
        }];

        run_aggregated_scan(
            CleanerCategory::SystemJunk,
            roots,
            ItemPolicy {
                risk: RiskLevel::SafeRecreatable,
                selection_policy: SelectionPolicy::SelectedByDefault,
                capabilities: vec![
                    ItemCapability::MoveToTrash,
                    ItemCapability::RevealInFinder,
                    ItemCapability::CopyPath,
                ],
            },
            |root| {
                format!(
                    "Aggregated recreatable temporary root inside {}.",
                    root.display()
                )
            },
            |_| "Temporary files".to_string(),
            progress,
            cancellation,
        )
    }
}
