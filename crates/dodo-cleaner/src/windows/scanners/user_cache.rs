//! Windows has no single canonical "every app's cache lives one level under
//! here" directory the way macOS's `~/Library/Caches` is a guaranteed-
//! disposable convention — `%LOCALAPPDATA%` mixes real application data in
//! with caches, so scanning it wholesale the way `UserCacheScanner` scans
//! `~/Library/Caches` would risk aggregating things that are not safe to
//! throw away. This scanner stays narrow instead: the handful of browser
//! cache folders whose location and disposability are both well known.
//! Firefox's profile folder name is a per-install random string, so its
//! root is discovered by listing `Profiles/` rather than hard-coded.

use std::path::{Path, PathBuf};

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
        _context: &ScanContext,
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

        run_aggregated_scan(
            CleanerCategory::UserCache,
            browser_cache_roots(),
            ItemPolicy {
                risk: RiskLevel::SafeRecreatable,
                selection_policy: SelectionPolicy::SelectedByDefault,
                capabilities: vec![
                    ItemCapability::MoveToTrash,
                    ItemCapability::RevealInFinder,
                    ItemCapability::CopyPath,
                ],
            },
            |root| format!("Aggregated browser cache root inside {}.", root.display()),
            group_label,
            progress,
            cancellation,
        )
    }
}

fn browser_cache_roots() -> Vec<ScanRoot> {
    let mut roots = Vec::new();
    let Some(local_app_data) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) else {
        return roots;
    };

    for chrome_like in [
        local_app_data
            .join("Google")
            .join("Chrome")
            .join("User Data")
            .join("Default")
            .join("Cache"),
        local_app_data
            .join("Microsoft")
            .join("Edge")
            .join("User Data")
            .join("Default")
            .join("Cache"),
    ] {
        roots.push(whole_root(chrome_like));
    }

    let firefox_profiles = local_app_data
        .join("Mozilla")
        .join("Firefox")
        .join("Profiles");
    if let Ok(entries) = std::fs::read_dir(&firefox_profiles) {
        for entry in entries.flatten() {
            let cache2 = entry.path().join("cache2");
            if cache2.is_dir() {
                roots.push(whole_root(cache2));
            }
        }
    }

    roots
}

fn whole_root(path: PathBuf) -> ScanRoot {
    ScanRoot {
        path,
        max_depth: None,
        follow_symlinks: false,
        cross_filesystems: false,
        include_hidden: true,
        aggregate_mode: AggregateMode::WholeRoot,
        permission: None,
        risk: RiskLevel::SafeRecreatable,
    }
}

fn group_label(path: &Path) -> String {
    if path.to_string_lossy().contains("Google") {
        "Google Chrome cache".to_string()
    } else if path.to_string_lossy().contains("Edge") {
        "Microsoft Edge cache".to_string()
    } else if path.to_string_lossy().contains("Mozilla") {
        "Firefox cache".to_string()
    } else {
        "Browser cache".to_string()
    }
}
