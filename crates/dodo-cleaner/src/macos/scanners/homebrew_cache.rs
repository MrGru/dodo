//! `CleanerCategory::HomebrewCache` (Phase 11): Homebrew's own download
//! cache, never the Cellar and never installed formulae.
//!
//! # Detecting the cache root
//!
//! The ticket's priority order is: the `HOMEBREW_CACHE` environment
//! variable, then "safe Homebrew configuration output" (i.e. `brew
//! --cache`), then a known default location. This scanner implements the
//! first and third tiers only — [`resolve_cache_root`] — and deliberately
//! skips shelling out to `brew`. `docs/cleaner/known-limitations.md` records
//! why: the ticket itself allows skipping it ("Only add dependencies when
//! the active phase uses them", "avoid unnecessary process calls"), and a
//! *safe* invocation needs a bounded timeout `std::process::Command` has no
//! built-in support for, which is meaningfully more code for a tier this
//! phase does not require. `HOMEBREW_CACHE` plus the default location (which
//! is `~/Library/Caches/Homebrew` regardless of the Apple Silicon vs Intel
//! install prefix — the *cache* location, unlike the Cellar, has not
//! depended on `/opt/homebrew` vs `/usr/local`) already cover every real
//! installation.
//!
//! [`resolve_cache_root`] is `pub(crate)` and reused by
//! `macos::cleanup::policy_for` so cleanup's allow-list is always built from
//! the exact same root this scanner just scanned — never a second,
//! independently-derived guess at where Homebrew's cache lives.
//!
//! # Grouping
//!
//! - `Cask/` inside the cache root, if present, becomes its own "Cask cache"
//!   group.
//! - `Logs/` inside the cache root, if present, becomes its own "Logs" group.
//! - Everything else at the top level is one "Download cache" group —
//!   Homebrew does not otherwise separate formula bottles from source
//!   tarballs on disk, and this phase does not call `brew list`/`brew info`
//!   to do it another way (see `docs/cleaner/known-limitations.md`).
//!
//! No dedicated "stale downloads" sub-classification: `modified_at` is
//! already carried on every item and the result list is sorted by size, so
//! this phase does not add a second, separately-defined staleness rule on
//! top of Large & Old Files' — see `docs/cleaner/known-limitations.md`.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use crate::core::cancellation::CancellationToken;
use crate::core::category::CleanerCategory;
use crate::core::errors::ScanError;
use crate::core::fs::scan_root;
use crate::core::item::{CleanableItem, CleanableItemId, ItemMetadata};
use crate::core::permissions::MacPermission;
use crate::core::progress::{ProgressSink, ScanPhase, ScanProgress};
use crate::core::report::{CategoryScanResult, PartialScanReason, ScanCompleteness, ScanWarning};
use crate::core::risk::{ItemCapability, RiskLevel, SelectionPolicy};
use crate::core::scan_context::ScanContext;
use crate::core::scan_root::{AggregateMode, ScanRoot};
use crate::core::scanner::CleanerScanner;

const CASK_SUBDIR: &str = "Cask";
const LOGS_SUBDIR: &str = "Logs";

pub struct HomebrewCacheScanner {
    /// Injected only by tests (see [`HomebrewCacheScanner::with_cache_root`]);
    /// a real scan always goes through [`resolve_cache_root`] at scan time so
    /// it observes the real `HOMEBREW_CACHE` environment variable and the
    /// scan's own `user_home`, exactly like every other scanner here resolves
    /// its roots from `ScanContext`.
    forced_cache_root: Option<PathBuf>,
}

impl HomebrewCacheScanner {
    pub fn new() -> Self {
        Self {
            forced_cache_root: None,
        }
    }

    #[cfg(test)]
    fn with_cache_root(root: PathBuf) -> Self {
        Self {
            forced_cache_root: Some(root),
        }
    }
}

/// Resolves the Homebrew cache root the ticket's first and third detection
/// tiers describe: the `HOMEBREW_CACHE` environment variable if set to a
/// non-empty value, else `~/Library/Caches/Homebrew`. Returns `None` only
/// when neither an override nor a home directory is available — there is
/// nothing plausible left to scan.
///
/// Shared by [`HomebrewCacheScanner::scan`] and
/// `macos::cleanup::policy_for`, which both need to agree on exactly the same
/// root — see this module's doc comment.
pub(crate) fn resolve_cache_root(
    env_override: Option<PathBuf>,
    home: Option<&Path>,
) -> Option<PathBuf> {
    if let Some(path) = env_override.filter(|path| !path.as_os_str().is_empty()) {
        return Some(path);
    }
    home.map(|home| home.join("Library").join("Caches").join("Homebrew"))
}

impl CleanerScanner for HomebrewCacheScanner {
    fn category(&self) -> CleanerCategory {
        CleanerCategory::HomebrewCache
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
            category: CleanerCategory::HomebrewCache,
            phase: ScanPhase::Preparing,
            current_path: None,
            scanned_entries: 0,
            discovered_items: 0,
            discovered_bytes: 0,
        });

        let cache_root = match &self.forced_cache_root {
            Some(root) => Some(root.clone()),
            None => resolve_cache_root(
                std::env::var_os("HOMEBREW_CACHE").map(PathBuf::from),
                context.user_home.as_deref(),
            ),
        };

        let Some(cache_root) = cache_root else {
            return Ok(CategoryScanResult {
                category: CleanerCategory::HomebrewCache,
                items: Vec::new(),
                scanned_entries: 0,
                estimated_reclaimable_bytes: 0,
                warnings: Vec::new(),
                completeness: ScanCompleteness::Complete,
            });
        };

        let mut items = Vec::new();
        let mut warnings = Vec::new();
        let mut scanned_entries = 0;
        let mut skipped_roots = Vec::new();

        let cask_root = cache_root.join(CASK_SUBDIR);
        let logs_root = cache_root.join(LOGS_SUBDIR);

        match scan_root(
            &immediate_children_root(cache_root.clone(), RiskLevel::SafeRecreatable),
            CleanerCategory::HomebrewCache,
            progress,
            cancellation,
        ) {
            Ok(result) => {
                scanned_entries += result.scanned_entries;
                warnings.extend(result.warnings);
                for entry in result.entries {
                    if cancellation.is_cancelled() {
                        return Err(ScanError::Cancelled);
                    }
                    // `Cask/` and `Logs/` are scanned separately below so
                    // they get their own group rather than being counted
                    // twice.
                    if entry.path == cask_root || entry.path == logs_root {
                        continue;
                    }
                    if entry.logical_size == 0 {
                        continue;
                    }
                    items.push(build_item(
                        entry.path,
                        entry.logical_size,
                        entry.modified_at,
                        "Download cache",
                        "Homebrew download cache entry (formula bottle or source tarball); \
                         Homebrew re-downloads it if it is needed again.",
                    ));
                }
            }
            Err(ScanError::RootUnavailable(_)) => skipped_roots.push(cache_root.clone()),
            Err(err @ ScanError::Cancelled) => return Err(err),
            Err(error) => warnings.push(ScanWarning {
                message: format!("{}: {error:?}", cache_root.display()),
            }),
        }

        if cask_root.is_dir() {
            if cancellation.is_cancelled() {
                return Err(ScanError::Cancelled);
            }
            match scan_root(
                &immediate_children_root(cask_root.clone(), RiskLevel::SafeRecreatable),
                CleanerCategory::HomebrewCache,
                progress,
                cancellation,
            ) {
                Ok(result) => {
                    scanned_entries += result.scanned_entries;
                    warnings.extend(result.warnings);
                    for entry in result.entries {
                        if entry.logical_size == 0 {
                            continue;
                        }
                        items.push(build_item(
                            entry.path,
                            entry.logical_size,
                            entry.modified_at,
                            "Cask cache",
                            "Homebrew Cask download cache entry; re-downloaded if the cask is \
                             reinstalled.",
                        ));
                    }
                }
                Err(ScanError::RootUnavailable(_)) => {}
                Err(err @ ScanError::Cancelled) => return Err(err),
                Err(error) => warnings.push(ScanWarning {
                    message: format!("{}: {error:?}", cask_root.display()),
                }),
            }
        }

        if logs_root.is_dir() {
            if cancellation.is_cancelled() {
                return Err(ScanError::Cancelled);
            }
            match scan_root(
                &ScanRoot {
                    path: logs_root.clone(),
                    max_depth: None,
                    follow_symlinks: false,
                    cross_filesystems: false,
                    include_hidden: true,
                    aggregate_mode: AggregateMode::WholeRoot,
                    permission: None,
                    risk: RiskLevel::SafeRecreatable,
                },
                CleanerCategory::HomebrewCache,
                progress,
                cancellation,
            ) {
                Ok(result) => {
                    scanned_entries += result.scanned_entries;
                    warnings.extend(result.warnings);
                    if let Some(entry) = result.entries.into_iter().next()
                        && entry.logical_size > 0
                    {
                        items.push(build_item(
                            entry.path,
                            entry.logical_size,
                            entry.modified_at,
                            "Logs",
                            "Homebrew's own log output.",
                        ));
                    }
                }
                Err(ScanError::RootUnavailable(_)) => {}
                Err(err @ ScanError::Cancelled) => return Err(err),
                Err(error) => warnings.push(ScanWarning {
                    message: format!("{}: {error:?}", logs_root.display()),
                }),
            }
        }

        items.sort_by_key(|item| std::cmp::Reverse(item.logical_size));
        let estimated_reclaimable_bytes = items.iter().map(|item| item.logical_size).sum();
        Ok(CategoryScanResult {
            category: CleanerCategory::HomebrewCache,
            items,
            scanned_entries,
            estimated_reclaimable_bytes,
            warnings,
            completeness: if skipped_roots.is_empty() {
                ScanCompleteness::Complete
            } else {
                ScanCompleteness::Partial {
                    skipped_roots,
                    reason: PartialScanReason::RootUnavailable,
                }
            },
        })
    }
}

fn immediate_children_root(path: PathBuf, risk: RiskLevel) -> ScanRoot {
    ScanRoot {
        path,
        max_depth: None,
        follow_symlinks: false,
        cross_filesystems: false,
        include_hidden: true,
        aggregate_mode: AggregateMode::ImmediateChildren,
        permission: None,
        risk,
    }
}

fn build_item(
    path: PathBuf,
    logical_size: u64,
    modified_at: Option<std::time::SystemTime>,
    group: &str,
    explanation: &str,
) -> CleanableItem {
    CleanableItem {
        id: item_id(path.as_path()),
        category: CleanerCategory::HomebrewCache,
        group: Some(group.to_string()),
        display_name: item_name(path.as_path()),
        path,
        logical_size,
        allocated_size: None,
        modified_at,
        last_accessed_at: None,
        risk: RiskLevel::SafeRecreatable,
        selection_policy: SelectionPolicy::SelectedByDefault,
        capabilities: vec![
            ItemCapability::MoveToTrash,
            ItemCapability::RevealInFinder,
            ItemCapability::CopyPath,
        ],
        explanation: explanation.to_string(),
        warnings: Vec::new(),
        metadata: ItemMetadata::Generic,
    }
}

fn item_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

fn item_id(path: &Path) -> CleanableItemId {
    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    CleanableItemId(hasher.finish())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use crate::core::cancellation::CancellationToken;
    use crate::core::progress::{ProgressSink, ScanProgress};
    use crate::core::risk::{RiskLevel, SelectionPolicy};
    use crate::core::scan_context::ScanContext;
    use crate::core::scanner::CleanerScanner;
    use crate::macos::scanners::homebrew_cache::HomebrewCacheScanner;

    struct RecordingSink;
    impl ProgressSink for RecordingSink {
        fn report(&self, _progress: ScanProgress) {}
    }

    fn empty_context() -> ScanContext {
        ScanContext {
            started_at: std::time::SystemTime::now(),
            user_home: None,
        }
    }

    #[test]
    fn resolve_cache_root_prefers_the_environment_override() {
        use super::resolve_cache_root;
        use std::path::PathBuf;

        let resolved = resolve_cache_root(
            Some(PathBuf::from("/custom/cache")),
            Some(std::path::Path::new("/Users/example")),
        );
        assert_eq!(resolved, Some(PathBuf::from("/custom/cache")));
    }

    #[test]
    fn resolve_cache_root_falls_back_to_the_default_location() {
        use super::resolve_cache_root;
        use std::path::PathBuf;

        let resolved = resolve_cache_root(None, Some(std::path::Path::new("/Users/example")));
        assert_eq!(
            resolved,
            Some(PathBuf::from("/Users/example/Library/Caches/Homebrew"))
        );
    }

    #[test]
    fn resolve_cache_root_ignores_an_empty_override() {
        use super::resolve_cache_root;
        use std::path::PathBuf;

        let resolved = resolve_cache_root(
            Some(PathBuf::new()),
            Some(std::path::Path::new("/Users/example")),
        );
        assert_eq!(
            resolved,
            Some(PathBuf::from("/Users/example/Library/Caches/Homebrew"))
        );
    }

    #[test]
    fn resolve_cache_root_is_none_without_an_override_or_a_home() {
        use super::resolve_cache_root;

        assert_eq!(resolve_cache_root(None, None), None);
    }

    #[test]
    fn download_cache_is_safe_and_selected_by_default_and_cask_is_grouped_separately() {
        let temp = std::env::temp_dir().join(format!(
            "dodo-cleaner-homebrew-cache-{}",
            std::process::id()
        ));
        fs::create_dir_all(&temp).expect("creates cache root");
        fs::write(temp.join("some-bottle--1.2.3.tar.gz"), vec![0u8; 32]).expect("writes a bottle");
        let cask_dir = temp.join("Cask");
        fs::create_dir_all(&cask_dir).expect("creates Cask dir");
        fs::write(cask_dir.join("some-cask--4.5.6.dmg"), vec![0u8; 16]).expect("writes a cask");

        let scanner = HomebrewCacheScanner::with_cache_root(temp.clone());
        let result = scanner
            .scan(&empty_context(), &RecordingSink, &CancellationToken::new())
            .expect("scans the Homebrew cache");

        assert_eq!(result.items.len(), 2);
        let download = result
            .items
            .iter()
            .find(|item| item.group.as_deref() == Some("Download cache"))
            .expect("has a download-cache item");
        assert_eq!(download.risk, RiskLevel::SafeRecreatable);
        assert_eq!(
            download.selection_policy,
            SelectionPolicy::SelectedByDefault
        );
        let cask = result
            .items
            .iter()
            .find(|item| item.group.as_deref() == Some("Cask cache"))
            .expect("has a cask-cache item");
        assert_eq!(cask.risk, RiskLevel::SafeRecreatable);

        fs::remove_dir_all(&temp).expect("removes temp tree");
    }

    #[test]
    fn logs_subdirectory_becomes_its_own_group_when_present() {
        let temp =
            std::env::temp_dir().join(format!("dodo-cleaner-homebrew-logs-{}", std::process::id()));
        fs::create_dir_all(&temp).expect("creates cache root");
        let logs_dir = temp.join("Logs");
        fs::create_dir_all(&logs_dir).expect("creates Logs dir");
        fs::write(logs_dir.join("brew.log"), vec![0u8; 8]).expect("writes a log file");

        let scanner = HomebrewCacheScanner::with_cache_root(temp.clone());
        let result = scanner
            .scan(&empty_context(), &RecordingSink, &CancellationToken::new())
            .expect("scans the Homebrew cache");

        assert!(
            result
                .items
                .iter()
                .any(|item| item.group.as_deref() == Some("Logs"))
        );

        fs::remove_dir_all(&temp).expect("removes temp tree");
    }

    #[test]
    fn a_missing_cache_root_is_a_partial_scan_not_an_error() {
        let temp = std::env::temp_dir().join(format!(
            "dodo-cleaner-homebrew-missing-{}",
            std::process::id()
        ));

        let scanner = HomebrewCacheScanner::with_cache_root(temp.clone());
        let result = scanner
            .scan(&empty_context(), &RecordingSink, &CancellationToken::new())
            .expect("scan tolerates a missing cache root");

        assert!(result.items.is_empty());
        assert!(matches!(
            result.completeness,
            crate::core::report::ScanCompleteness::Partial { .. }
        ));
    }

    // `HomebrewCacheScanner::new()` reading the real `HOMEBREW_CACHE`
    // environment variable is deliberately not exercised here — it would
    // make this test's outcome depend on whatever the machine running it
    // happens to have exported, which `resolve_cache_root`'s own pure-function
    // tests above already cover deterministically.
}
