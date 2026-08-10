use std::path::PathBuf;

use crate::cleaner::core::cancellation::CancellationToken;
use crate::cleaner::core::category::CleanerCategory;
use crate::cleaner::core::errors::ScanError;
use crate::cleaner::core::generic_scanner::{ItemPolicy, run_aggregated_scan};
use crate::cleaner::core::permissions::MacPermission;
use crate::cleaner::core::progress::{ProgressSink, ScanPhase, ScanProgress};
use crate::cleaner::core::report::CategoryScanResult;
use crate::cleaner::core::risk::{ItemCapability, RiskLevel, SelectionPolicy};
use crate::cleaner::core::scan_context::ScanContext;
use crate::cleaner::core::scan_root::{AggregateMode, ScanRoot};
use crate::cleaner::core::scanner::CleanerScanner;

/// `$XDG_DATA_HOME/Trash/files` (defaulting to `~/.local/share/Trash/files`)
/// only — the home trash the freedesktop.org spec defines, not the
/// per-mounted-volume `.Trash-<uid>` directories the same spec also allows.
/// macOS's own `TrashBinsScanner` does scan its per-volume equivalent
/// (`/Volumes/*/.Trashes/<uid>`); Linux's mount points are far less
/// enumerable in a portable way, so that parity is left for a future round.
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
        context: &ScanContext,
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

        let roots = trash_files_root(context.user_home.as_deref())
            .into_iter()
            .map(|path| ScanRoot {
                path,
                max_depth: None,
                follow_symlinks: false,
                cross_filesystems: false,
                include_hidden: true,
                aggregate_mode: AggregateMode::WholeRoot,
                permission: None,
                risk: RiskLevel::ReviewRecommended,
            })
            .collect();

        run_aggregated_scan(
            CleanerCategory::TrashBins,
            roots,
            ItemPolicy {
                risk: RiskLevel::ReviewRecommended,
                selection_policy: SelectionPolicy::NeverBulkSelect,
                capabilities: vec![ItemCapability::RevealInFinder, ItemCapability::CopyPath],
            },
            |_| {
                "Trash contents are review-only here; emptying Trash is a separate future flow."
                    .to_string()
            },
            |_| "Home Trash".to_string(),
            progress,
            cancellation,
        )
    }
}

fn trash_files_root(home: Option<&std::path::Path>) -> Option<PathBuf> {
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| home.map(|home| home.join(".local").join("share")))?;
    Some(data_home.join("Trash").join("files"))
}
