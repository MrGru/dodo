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

/// The XDG Base Directory spec's cache home — `$XDG_CACHE_HOME`, defaulting
/// to `~/.cache` — which is exactly the second root macOS's own
/// `UserCacheScanner` already lists (for the rare case of a cross-platform
/// tool writing there under Rosetta or a container). Every top-level
/// directory under it is an aggregated candidate, same as `~/Library/Caches`
/// on macOS.
pub struct UserCacheScanner;

impl UserCacheScanner {
    pub fn new() -> Self {
        Self
    }
}

impl CleanerScanner for UserCacheScanner {
    fn category(&self) -> CleanerCategory {
        CleanerCategory::UserCache
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
            category: CleanerCategory::UserCache,
            phase: ScanPhase::Preparing,
            current_path: None,
            scanned_entries: 0,
            discovered_items: 0,
            discovered_bytes: 0,
        });

        let policy = ItemPolicy {
            risk: RiskLevel::SafeRecreatable,
            selection_policy: SelectionPolicy::SelectedByDefault,
            capabilities: vec![
                ItemCapability::MoveToTrash,
                ItemCapability::RevealInFinder,
                ItemCapability::CopyPath,
            ],
        };
        let Some(cache_home) = cache_home(context.user_home.as_deref()) else {
            return run_aggregated_scan(
                CleanerCategory::UserCache,
                Vec::new(),
                policy,
                |_| String::new(),
                |_| String::new(),
                progress,
                cancellation,
            );
        };

        run_aggregated_scan(
            CleanerCategory::UserCache,
            vec![ScanRoot {
                path: cache_home,
                max_depth: None,
                follow_symlinks: false,
                cross_filesystems: false,
                include_hidden: true,
                aggregate_mode: AggregateMode::ImmediateChildren,
                permission: None,
                risk: RiskLevel::SafeRecreatable,
            }],
            policy,
            |root| format!("Aggregated cache root inside {}.", root.display()),
            |_| "Cache".to_string(),
            progress,
            cancellation,
        )
    }
}

/// `$XDG_CACHE_HOME` when set (it must be an absolute path per spec), else
/// `<home>/.cache`. `None` only when neither is available, matching every
/// other Linux scanner's "no home, no roots" posture.
fn cache_home(home: Option<&std::path::Path>) -> Option<PathBuf> {
    std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| home.map(|home| home.join(".cache")))
}
