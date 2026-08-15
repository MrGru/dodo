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

/// `/tmp` only — the exact same root, at the exact same aggregate mode, that
/// macOS's own `SystemJunkScanner` already scans and ships tested. `/var/tmp`
/// is deliberately left out: it is sticky-bit shared like `/tmp` but survives
/// reboots, so its contents are more often something a long-running daemon
/// still needs than ordinary junk.
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
            path: "/tmp".into(),
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
            |_| "Temporary files inside /tmp.".to_string(),
            |_| "Temporary files".to_string(),
            progress,
            cancellation,
        )
    }
}
