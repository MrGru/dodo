//! `CleanerCategory::OrphanedFiles` (Phase 10): the scanner that turns
//! [`orphans::find_orphans_from`]'s candidates into [`CleanableItem`]s.
//!
//! Thin by design: everything that decides *whether* a path is orphaned
//! lives in [`crate::macos::applications::orphans`] (matching) and
//! [`crate::macos::scanners::installed_apps::installed_app_identities`]
//! (the installed-app index this scanner builds first). This file only:
//!
//! - builds that index (or, in tests, an injected one);
//! - loads the "keep" list (see [`crate::core::ignore`] and
//!   [`crate::services::ignore_store`]) and filters kept paths out,
//!   so a kept item does not reappear on rescan;
//! - turns each surviving [`orphans::OrphanCandidate`] into a
//!   [`CleanableItem`] — risk, selection policy and capabilities follow
//!   [`MatchConfidence`] the same way
//!   [`crate::macos::applications::review`]'s `build_candidate`
//!   does for Phase 9's leftover candidates: only `Confirmed`, non-system
//!   items default-select, `SharedOrUnsafe` items are never bulk-selected,
//!   and a system-scope item never gets `MoveToTrash` at all.
//!
//! # Full Disk Access
//!
//! `required_permissions()` returns [`MacPermission::FullDiskAccess`], the
//! same as `MailFilesScanner`. `views::cleaner_view::CleanerView::start_scan`
//! already gates *every* FDA-requiring scanner uniformly — substituting a
//! `ScanCompleteness::Partial` result and never calling `scan()` at all
//! unless permission is granted — so this scanner does not re-implement that
//! check; see `docs/cleaner/permissions.md`.
//!
//! # Completeness
//!
//! Always reported as [`ScanCompleteness::Complete`]. A missing leftover
//! root (no `~/Library/LaunchAgents` at all, say) is not a failure — Phase
//! 9's `find_leftovers` treats a missing root the same way, silently moving
//! on — and the one failure mode that *would* need `Partial` (no Full Disk
//! Access) never reaches `scan()` at all, per the note above.

use std::path::PathBuf;
use std::sync::Arc;

use crate::core::cancellation::CancellationToken;
use crate::core::category::CleanerCategory;
use crate::core::errors::ScanError;
use crate::core::fs::measure_size;
use crate::core::ignore::IgnoredItemsDocument;
use crate::core::item::{
    CleanableItem, CleanableItemId, ItemMetadata, ItemWarning, OrphanedFileMetadata,
};
use crate::core::permissions::MacPermission;
use crate::core::progress::{ProgressSink, ScanPhase, ScanProgress};
use crate::core::report::{CategoryScanResult, ScanCompleteness, ScanWarning};
use crate::core::risk::{ItemCapability, RiskLevel, SelectionPolicy};
use crate::core::scan_context::ScanContext;
use crate::core::scanner::CleanerScanner;
use crate::macos::applications::confidence::{self, MatchConfidence};
use crate::macos::applications::locations::{self, LocationScope};
use crate::macos::applications::orphans::{self, OrphanCandidate};
use crate::services::ignore_store::{DiskOrphanIgnoreStore, OrphanIgnoreStore};

use super::installed_apps;

pub struct OrphanedFilesScanner {
    /// `None` in production: the installed-app index is built from the real
    /// default application roots via `installed_apps::installed_app_identities`.
    /// `Some` only in tests, mirroring `InstalledAppsScanner::with_roots` — a
    /// test must never depend on the real `/Applications`.
    app_roots: Option<Vec<PathBuf>>,
    /// Same idea for the system-scope leftover root: `None` means the real
    /// `/Library` in production, `Some` only in tests.
    system_library: Option<PathBuf>,
    ignore_store: Arc<dyn OrphanIgnoreStore>,
}

impl OrphanedFilesScanner {
    pub fn new() -> Self {
        Self {
            app_roots: None,
            system_library: None,
            ignore_store: Arc::new(DiskOrphanIgnoreStore::new()),
        }
    }

    #[cfg(test)]
    fn with_test_environment(
        app_roots: Vec<PathBuf>,
        system_library: PathBuf,
        ignore_store: Arc<dyn OrphanIgnoreStore>,
    ) -> Self {
        Self {
            app_roots: Some(app_roots),
            system_library: Some(system_library),
            ignore_store,
        }
    }
}

impl CleanerScanner for OrphanedFilesScanner {
    fn category(&self) -> CleanerCategory {
        CleanerCategory::OrphanedFiles
    }

    fn required_permissions(&self) -> &[MacPermission] {
        const FULL_DISK_ACCESS: &[MacPermission] = &[MacPermission::FullDiskAccess];
        FULL_DISK_ACCESS
    }

    fn scan(
        &self,
        context: &ScanContext,
        progress: &dyn ProgressSink,
        cancellation: &CancellationToken,
    ) -> Result<CategoryScanResult, ScanError> {
        progress.report(ScanProgress {
            category: CleanerCategory::OrphanedFiles,
            phase: ScanPhase::Preparing,
            current_path: None,
            scanned_entries: 0,
            discovered_items: 0,
            discovered_bytes: 0,
        });
        if cancellation.is_cancelled() {
            return Err(ScanError::Cancelled);
        }

        let home = context.user_home.as_deref();
        let index = match &self.app_roots {
            Some(roots) => installed_apps::identities_from_roots(roots),
            None => installed_apps::installed_app_identities(home),
        };
        let mut warnings = Vec::new();
        let ignored = match self.ignore_store.load() {
            Ok(document) => document,
            Err(error) => {
                warnings.push(ScanWarning {
                    message: format!(
                        "cleaner-ignored-items.json could not be read; previously kept items may reappear: {error:?}"
                    ),
                });
                IgnoredItemsDocument::default()
            }
        };

        // Production always scans the real `/Library` via `find_orphans`;
        // only tests inject a fake `system_library` through
        // `with_test_environment`, mirroring how Phase 9's `find_leftovers` /
        // `find_leftovers_from` split works.
        let candidates = match &self.system_library {
            Some(system_library) => {
                orphans::find_orphans_from(&index, home, system_library.as_path())
            }
            None => orphans::find_orphans(&index, home),
        };

        let mut items = Vec::new();
        for candidate in candidates {
            if cancellation.is_cancelled() {
                return Err(ScanError::Cancelled);
            }
            if ignored.is_ignored(candidate.path.as_path()) {
                continue;
            }
            progress.report(ScanProgress {
                category: CleanerCategory::OrphanedFiles,
                phase: ScanPhase::Classifying,
                current_path: Some(candidate.path.clone()),
                scanned_entries: items.len() as u64 + 1,
                discovered_items: items.len() as u64,
                discovered_bytes: items
                    .iter()
                    .map(|item: &CleanableItem| item.logical_size)
                    .sum(),
            });
            items.push(build_item(candidate));
        }

        items.sort_by_key(|item| std::cmp::Reverse(item.logical_size));
        Ok(CategoryScanResult {
            category: CleanerCategory::OrphanedFiles,
            estimated_reclaimable_bytes: items.iter().map(|item| item.logical_size).sum(),
            scanned_entries: items.len() as u64,
            items,
            warnings,
            completeness: ScanCompleteness::Complete,
        })
    }
}

/// Turns one [`OrphanCandidate`] into a [`CleanableItem`], mirroring
/// `applications::review::build_candidate`'s risk/selection/capability rules
/// for the same [`MatchConfidence`] buckets.
fn build_item(candidate: OrphanCandidate) -> CleanableItem {
    let is_system = candidate.scope == LocationScope::System;

    let selection_policy = match candidate.confidence {
        MatchConfidence::Confirmed if !is_system => SelectionPolicy::SelectedByDefault,
        MatchConfidence::SharedOrUnsafe => SelectionPolicy::NeverBulkSelect,
        _ => SelectionPolicy::NotSelectedByDefault,
    };
    let risk = if is_system {
        RiskLevel::Protected
    } else if candidate.confidence == MatchConfidence::SharedOrUnsafe {
        RiskLevel::UserData
    } else {
        RiskLevel::ReviewRecommended
    };

    // "Keep" is offered on every orphan candidate, system-scope included: a
    // user reviewing a scan-only system entry should still be able to say
    // "stop showing me this" even though there is nothing to clean up.
    let mut capabilities = vec![
        ItemCapability::RevealInFinder,
        ItemCapability::CopyPath,
        ItemCapability::MarkAsKept,
    ];
    if !is_system {
        capabilities.push(ItemCapability::MoveToTrash);
    }

    let mut warnings = Vec::new();
    if is_system {
        warnings.push(ItemWarning {
            message: "System-owned location: scan-only until a privileged helper exists."
                .to_string(),
        });
    }

    let display_name = candidate
        .path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("Orphaned item")
        .to_string();
    let logical_size = if is_system {
        0
    } else {
        measure_size(
            candidate.path.as_path(),
            CleanerCategory::OrphanedFiles,
            RiskLevel::ReviewRecommended,
        )
    };

    let explanation = format!(
        "{} confidence: {} ({}).",
        confidence::confidence_label(candidate.confidence),
        orphans::reason_label(candidate.reason),
        locations::location_label(candidate.location),
    );

    CleanableItem {
        id: item_id(candidate.path.as_path()),
        category: CleanerCategory::OrphanedFiles,
        group: Some(locations::location_label(candidate.location).to_string()),
        display_name,
        path: candidate.path,
        logical_size,
        allocated_size: None,
        modified_at: None,
        last_accessed_at: None,
        risk,
        selection_policy,
        capabilities,
        explanation,
        warnings,
        metadata: ItemMetadata::OrphanedFile(OrphanedFileMetadata {
            reason: candidate.reason,
        }),
    }
}

fn item_id(path: &std::path::Path) -> CleanableItemId {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    CleanableItemId(hasher.finish())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use super::*;
    use crate::core::ignore::path_signature;
    use crate::core::progress::ScanProgress;
    use crate::services::ignore_store::InMemoryOrphanIgnoreStore;

    struct RecordingSink;
    impl ProgressSink for RecordingSink {
        fn report(&self, _progress: ScanProgress) {}
    }

    fn temp_home(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "dodo-cleaner-orphaned-files-{label}-{}-{}",
            std::process::id(),
            label.len()
        ))
    }

    fn scan(
        home: &Path,
        system_library: &Path,
        ignore_store: Arc<dyn OrphanIgnoreStore>,
    ) -> CategoryScanResult {
        let scanner = OrphanedFilesScanner::with_test_environment(
            Vec::new(), // no installed apps in the index
            system_library.to_path_buf(),
            ignore_store,
        );
        scanner
            .scan(
                &ScanContext {
                    started_at: std::time::SystemTime::now(),
                    user_home: Some(home.to_path_buf()),
                },
                &RecordingSink,
                &CancellationToken::new(),
            )
            .expect("scans orphaned files")
    }

    #[test]
    fn an_orphaned_container_becomes_a_confirmed_selected_item() {
        let home = temp_home("confirmed");
        let system_library = temp_home("confirmed-system");
        fs::create_dir_all(
            home.join("Library")
                .join("Containers")
                .join("com.gonecorp.OldApp"),
        )
        .expect("creates orphaned container");

        let result = scan(
            &home,
            &system_library,
            Arc::new(InMemoryOrphanIgnoreStore::default()),
        );

        assert_eq!(result.items.len(), 1);
        let item = &result.items[0];
        assert_eq!(item.category, CleanerCategory::OrphanedFiles);
        assert_eq!(item.selection_policy, SelectionPolicy::SelectedByDefault);
        assert!(item.capabilities.contains(&ItemCapability::MarkAsKept));
        assert!(item.capabilities.contains(&ItemCapability::MoveToTrash));
        assert!(matches!(
            item.metadata,
            ItemMetadata::OrphanedFile(OrphanedFileMetadata {
                reason: crate::core::item::OrphanReason::BundleIdentifierNotInstalled,
            })
        ));

        fs::remove_dir_all(&home).expect("removes temp home");
        let _ = fs::remove_dir_all(&system_library);
    }

    #[test]
    fn a_kept_item_does_not_reappear_on_rescan() {
        let home = temp_home("kept");
        let system_library = temp_home("kept-system");
        let orphan_path = home
            .join("Library")
            .join("Containers")
            .join("com.gonecorp.OldApp");
        fs::create_dir_all(&orphan_path).expect("creates orphaned container");

        let store = Arc::new(InMemoryOrphanIgnoreStore::default());
        let first = scan(&home, &system_library, store.clone());
        assert_eq!(first.items.len(), 1, "the orphan is found the first time");

        let mut document = store.load().expect("loads");
        document.keep(&orphan_path);
        store
            .persist(&document)
            .expect("persists the keep decision");

        let second = scan(&home, &system_library, store.clone());
        assert!(
            second.items.is_empty(),
            "a kept path must not reappear on rescan"
        );

        fs::remove_dir_all(&home).expect("removes temp home");
        let _ = fs::remove_dir_all(&system_library);
    }

    #[test]
    fn a_system_scope_item_has_no_move_to_trash_capability() {
        let home = temp_home("system-scope");
        let system_library = temp_home("system-scope-library");
        let launch_daemons = system_library.join("LaunchDaemons");
        fs::create_dir_all(&launch_daemons).expect("creates fake system LaunchDaemons");
        fs::write(launch_daemons.join("com.gonecorp.daemon.plist"), b"")
            .expect("writes fake launch daemon plist");

        let result = scan(
            &home,
            &system_library,
            Arc::new(InMemoryOrphanIgnoreStore::default()),
        );

        assert_eq!(result.items.len(), 1);
        let item = &result.items[0];
        assert_eq!(item.risk, RiskLevel::Protected);
        assert!(!item.capabilities.contains(&ItemCapability::MoveToTrash));
        assert!(item.capabilities.contains(&ItemCapability::MarkAsKept));

        let _ = fs::remove_dir_all(&home);
        fs::remove_dir_all(&system_library).expect("removes fake system library");
    }

    #[test]
    fn a_broken_ignore_store_produces_a_warning_but_still_scans() {
        struct AlwaysFails;
        impl OrphanIgnoreStore for AlwaysFails {
            fn load(
                &self,
            ) -> Result<IgnoredItemsDocument, crate::services::ignore_store::OrphanIgnoreStoreError>
            {
                Err(crate::services::ignore_store::OrphanIgnoreStoreError::Io(
                    "boom".to_string(),
                ))
            }
            fn persist(
                &self,
                _document: &IgnoredItemsDocument,
            ) -> Result<(), crate::services::ignore_store::OrphanIgnoreStoreError> {
                Ok(())
            }
        }

        let home = temp_home("broken-store");
        let system_library = temp_home("broken-store-system");
        fs::create_dir_all(
            home.join("Library")
                .join("Containers")
                .join("com.gonecorp.OldApp"),
        )
        .expect("creates orphaned container");

        let result = scan(&home, &system_library, Arc::new(AlwaysFails));
        assert_eq!(result.items.len(), 1, "scanning still proceeds");
        assert!(
            !result.warnings.is_empty(),
            "a broken ignore store must surface a warning"
        );

        fs::remove_dir_all(&home).expect("removes temp home");
        let _ = fs::remove_dir_all(&system_library);
    }

    #[test]
    fn path_signature_matches_what_the_view_would_persist() {
        // Sanity check that the ignore store's key and `keep`'s key agree —
        // both go through `core::ignore::path_signature`.
        let path = Path::new("/Users/someone/Library/Containers/com.gonecorp.OldApp");
        let mut document = IgnoredItemsDocument::default();
        document.keep(path);
        assert!(document.ignored_paths.contains(&path_signature(path)));
    }
}
