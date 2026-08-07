use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::cleaner::core::category::CleanerCategory;
use crate::cleaner::core::errors::CleanupError;
use crate::cleaner::core::item::CleanableItem;
use crate::cleaner::core::report::{CleanupItemFailure, CleanupItemSuccess, CleanupReport};
use crate::cleaner::core::safety::{
    AllowedRoot, DeletionPolicy, dedupe_nested_paths, validate_path,
};
use crate::cleaner::macos::applications::locations;
use crate::cleaner::macos::platform::move_to_trash;
use crate::cleaner::macos::scanners::{
    ai_apps, homebrew_cache, mail_files, node_tooling_cache, xcode_junk,
};
use crate::paths;

pub fn cleanup_items(items: &[CleanableItem]) -> CleanupReport {
    let mut by_path = BTreeMap::new();
    for item in items {
        by_path.insert(item.path.clone(), item);
    }

    let mut successes = Vec::new();
    let mut failures = Vec::new();
    for path in dedupe_nested_paths(by_path.keys().cloned().collect()) {
        let Some(item) = by_path.get(&path) else {
            continue;
        };
        let policy = policy_for(item);
        match validate_path(path.as_path(), item.category, &policy) {
            Ok(()) => match move_to_trash(path.as_path()) {
                Ok(receipt) => successes.push(CleanupItemSuccess {
                    id: item.id,
                    path: receipt.original_path,
                    trashed_path: receipt.trashed_path,
                    logical_size: item.logical_size,
                }),
                Err(message) => failures.push(CleanupItemFailure {
                    id: item.id,
                    path: path.clone(),
                    error: CleanupError::Trash(message),
                }),
            },
            Err(error) => failures.push(CleanupItemFailure {
                id: item.id,
                path: path.clone(),
                error: CleanupError::Safety(error),
            }),
        }
    }

    CleanupReport {
        estimated_reclaimed_bytes: successes.iter().map(|success| success.logical_size).sum(),
        successes,
        failures,
    }
}

fn policy_for(_item: &CleanableItem) -> DeletionPolicy {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut allowed_roots = Vec::new();
    if let Some(home) = home.as_ref() {
        allowed_roots.push(AllowedRoot {
            path: home.join("Library").join("Caches"),
            allow_root_itself: false,
            allowed_categories: vec![CleanerCategory::UserCache],
        });
        allowed_roots.push(AllowedRoot {
            path: home.join(".cache"),
            allow_root_itself: false,
            allowed_categories: vec![CleanerCategory::UserCache],
        });
        allowed_roots.push(AllowedRoot {
            path: home.join("Library").join("Logs"),
            allow_root_itself: false,
            allowed_categories: vec![CleanerCategory::SystemJunk],
        });
        for folder in ["Downloads", "Desktop", "Documents", "Movies"] {
            allowed_roots.push(AllowedRoot {
                path: home.join(folder),
                allow_root_itself: false,
                allowed_categories: vec![CleanerCategory::LargeOldFiles],
            });
        }
        for root in mail_files::attachment_roots(Some(home.as_path())) {
            allowed_roots.push(AllowedRoot {
                path: root.path,
                allow_root_itself: false,
                allowed_categories: vec![CleanerCategory::MailFiles],
            });
        }

        // Uninstall review (Phase 9): the app bundle itself, plus every
        // *user*-scope leftover location it may have matched. System-scope
        // locations (`/Library/...`) are deliberately absent — they are
        // scan-only until a privileged helper exists, so anything under them
        // fails `OutsideAllowedRoot` regardless of what the review dialog
        // shows or what the user selects.
        for root in [home.join("Applications"), PathBuf::from("/Applications")] {
            allowed_roots.push(AllowedRoot {
                path: root,
                allow_root_itself: false,
                allowed_categories: vec![CleanerCategory::InstalledApps],
            });
        }
        // Phase 10 orphan candidates are found under this exact same
        // user-scope leftover root list (see
        // `macos::applications::orphans::find_orphans_from`), so they share
        // these `AllowedRoot` entries with the uninstall review workflow
        // rather than getting a second, duplicate set. System-scope orphan
        // candidates stay scan-only for the same reason Phase 9's do:
        // nothing below adds `/Library/...` for `OrphanedFiles` either.
        for root in locations::user_scope_leftover_roots(home.as_path()) {
            allowed_roots.push(AllowedRoot {
                path: root,
                allow_root_itself: false,
                allowed_categories: vec![
                    CleanerCategory::InstalledApps,
                    CleanerCategory::OrphanedFiles,
                ],
            });
        }

        // Xcode Junk (Phase 11): only the three sub-paths
        // `xcode_junk::cleanup_allowed_roots` marks "normally recreatable" —
        // DerivedData, the SwiftUI preview cache and CoreSimulator's own
        // `Caches` — are allow-listed. Archives, iOS DeviceSupport,
        // CoreSimulator Devices, XCTestDevices and the SwiftPM cache stay
        // scan-only; see `macos::scanners::xcode_junk` and
        // `docs/cleaner/known-limitations.md`.
        for root in xcode_junk::cleanup_allowed_roots(home.as_path()) {
            allowed_roots.push(AllowedRoot {
                path: root,
                allow_root_itself: false,
                allowed_categories: vec![CleanerCategory::XcodeJunk],
            });
        }

        // Homebrew Cache (Phase 11): scoped to the exact detected cache root
        // only — never `/opt/homebrew` or `/usr/local` broadly, and never the
        // Cellar. Reuses the scanner's own detection order (the
        // `HOMEBREW_CACHE` environment variable, else the default location)
        // so cleanup can never authorize a root the scan itself did not use.
        if let Some(cache_root) = homebrew_cache::resolve_cache_root(
            std::env::var_os("HOMEBREW_CACHE").map(PathBuf::from),
            Some(home.as_path()),
        ) {
            allowed_roots.push(AllowedRoot {
                path: cache_root,
                allow_root_itself: false,
                allowed_categories: vec![CleanerCategory::HomebrewCache],
            });
        }

        // Node Tooling Cache (Phase 11): allow-lists only the locations
        // `node_tooling_cache::cleanup_allowed_roots` marks `allow_cleanup:
        // true` for the exact same environment snapshot the scan itself
        // would take. pnpm's shared store, and anything a future Nub
        // detection might ever report, are never included — see
        // `macos::scanners::node_tooling_cache` and
        // `docs/cleaner/known-limitations.md`.
        let node_environment = node_tooling_cache::snapshot_environment(Some(home.as_path()));
        for root in node_tooling_cache::cleanup_allowed_roots(&node_environment) {
            allowed_roots.push(AllowedRoot {
                path: root,
                allow_root_itself: false,
                allowed_categories: vec![CleanerCategory::NodeToolingCache],
            });
        }

        // AI Apps (Phase 12): only locations `AiAppRole::allow_cleanup`
        // permits — Logs and Cache — are allow-listed. Models, Application
        // support and Chat history stay scan-only; see
        // `macos::scanners::ai_apps` and `docs/cleaner/known-limitations.md`.
        for root in ai_apps::cleanup_allowed_roots(home.as_path()) {
            allowed_roots.push(AllowedRoot {
                path: root,
                allow_root_itself: false,
                allowed_categories: vec![CleanerCategory::AiApps],
            });
        }
    }
    allowed_roots.push(AllowedRoot {
        path: PathBuf::from("/tmp"),
        allow_root_itself: false,
        allowed_categories: vec![CleanerCategory::SystemJunk],
    });

    let mut protected_paths = vec![
        PathBuf::from("/"),
        PathBuf::from("/Applications"),
        PathBuf::from("/System"),
        PathBuf::from("/Library"),
        PathBuf::from("/Users"),
        PathBuf::from("/Volumes"),
        PathBuf::from("/private"),
        PathBuf::from("/bin"),
        PathBuf::from("/sbin"),
        PathBuf::from("/usr"),
        paths::data_dir(),
    ];
    if let Some(home) = home {
        protected_paths.push(home);
    }
    if let Ok(exe) = std::env::current_exe() {
        protected_paths.push(exe);
    }

    DeletionPolicy {
        allowed_roots,
        protected_paths,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::cleaner::core::category::CleanerCategory;
    use crate::cleaner::core::item::{CleanableItem, CleanableItemId, ItemMetadata};
    use crate::cleaner::core::risk::{RiskLevel, SelectionPolicy};
    use crate::cleaner::macos::cleanup::policy_for;

    #[test]
    fn user_cache_policy_has_explicit_allowed_roots() {
        let policy = policy_for(&CleanableItem {
            id: CleanableItemId(1),
            category: CleanerCategory::UserCache,
            group: None,
            display_name: String::new(),
            path: PathBuf::from("/tmp/cache"),
            logical_size: 0,
            allocated_size: None,
            modified_at: None,
            last_accessed_at: None,
            risk: RiskLevel::SafeRecreatable,
            selection_policy: SelectionPolicy::SelectedByDefault,
            capabilities: Vec::new(),
            explanation: String::new(),
            warnings: Vec::new(),
            metadata: ItemMetadata::Generic,
        });

        assert!(
            policy
                .allowed_roots
                .iter()
                .all(|root| !root.allow_root_itself),
            "cache cleanup must not authorize deleting a whole allowed root directly"
        );
        assert!(
            policy.allowed_roots.iter().any(|root| {
                root.allowed_categories
                    .contains(&CleanerCategory::LargeOldFiles)
                    && root.path.ends_with("Downloads")
            }),
            "large-and-old cleanup must stay limited to explicit user folders"
        );
    }

    #[test]
    fn installed_apps_policy_covers_user_scope_leftovers_but_not_system_scope() {
        let policy = policy_for(&CleanableItem {
            id: CleanableItemId(1),
            category: CleanerCategory::InstalledApps,
            group: None,
            display_name: String::new(),
            path: PathBuf::from("/Applications/Example.app"),
            logical_size: 0,
            allocated_size: None,
            modified_at: None,
            last_accessed_at: None,
            risk: RiskLevel::ReviewRecommended,
            selection_policy: SelectionPolicy::NotSelectedByDefault,
            capabilities: Vec::new(),
            explanation: String::new(),
            warnings: Vec::new(),
            metadata: ItemMetadata::Generic,
        });

        let app_roots: Vec<_> = policy
            .allowed_roots
            .iter()
            .filter(|root| {
                root.allowed_categories
                    .contains(&CleanerCategory::InstalledApps)
            })
            .collect();
        assert!(
            app_roots
                .iter()
                .any(|root| root.path.as_path() == std::path::Path::new("/Applications")),
            "the app bundle root must be allow-listed for InstalledApps"
        );
        assert!(
            app_roots
                .iter()
                .any(|root| root.path.ends_with("Library/Application Support")),
            "user-scope leftover roots must be allow-listed for InstalledApps"
        );
        assert!(
            app_roots.iter().all(|root| !root.allow_root_itself),
            "an allowed root must never authorize deleting itself outright"
        );
        assert!(
            app_roots
                .iter()
                .all(|root| !root.path.starts_with("/Library")),
            "system-scope leftover locations must stay scan-only, never allow-listed"
        );
    }

    #[test]
    fn orphaned_files_policy_covers_the_same_user_scope_leftovers_but_not_system_scope() {
        let policy = policy_for(&CleanableItem {
            id: CleanableItemId(1),
            category: CleanerCategory::OrphanedFiles,
            group: None,
            display_name: String::new(),
            path: PathBuf::from("/tmp/orphan"),
            logical_size: 0,
            allocated_size: None,
            modified_at: None,
            last_accessed_at: None,
            risk: RiskLevel::ReviewRecommended,
            selection_policy: SelectionPolicy::NotSelectedByDefault,
            capabilities: Vec::new(),
            explanation: String::new(),
            warnings: Vec::new(),
            metadata: ItemMetadata::Generic,
        });

        let orphan_roots: Vec<_> = policy
            .allowed_roots
            .iter()
            .filter(|root| {
                root.allowed_categories
                    .contains(&CleanerCategory::OrphanedFiles)
            })
            .collect();
        assert!(
            orphan_roots
                .iter()
                .any(|root| root.path.ends_with("Library/Application Support")),
            "user-scope leftover roots must be allow-listed for OrphanedFiles"
        );
        assert!(
            !orphan_roots
                .iter()
                .any(|root| root.path.as_path() == std::path::Path::new("/Applications")),
            "OrphanedFiles never includes an app bundle root — only leftover locations"
        );
        assert!(
            orphan_roots.iter().all(|root| !root.allow_root_itself),
            "an allowed root must never authorize deleting itself outright"
        );
        assert!(
            orphan_roots
                .iter()
                .all(|root| !root.path.starts_with("/Library")),
            "system-scope leftover locations must stay scan-only, never allow-listed"
        );
    }

    #[test]
    fn xcode_junk_policy_covers_only_the_three_recreatable_subpaths() {
        let policy = policy_for(&CleanableItem {
            id: CleanableItemId(1),
            category: CleanerCategory::XcodeJunk,
            group: None,
            display_name: String::new(),
            path: PathBuf::from("/tmp/derived-data"),
            logical_size: 0,
            allocated_size: None,
            modified_at: None,
            last_accessed_at: None,
            risk: RiskLevel::SafeRecreatable,
            selection_policy: SelectionPolicy::SelectedByDefault,
            capabilities: Vec::new(),
            explanation: String::new(),
            warnings: Vec::new(),
            metadata: ItemMetadata::Generic,
        });

        let xcode_roots: Vec<_> = policy
            .allowed_roots
            .iter()
            .filter(|root| {
                root.allowed_categories
                    .contains(&CleanerCategory::XcodeJunk)
            })
            .collect();
        assert_eq!(
            xcode_roots.len(),
            3,
            "only DerivedData, Previews and CoreSimulator/Caches may be allow-listed"
        );
        assert!(
            xcode_roots
                .iter()
                .any(|root| root.path.ends_with("DerivedData"))
        );
        assert!(
            xcode_roots
                .iter()
                .any(|root| root.path.ends_with("Previews"))
        );
        assert!(
            xcode_roots
                .iter()
                .any(|root| root.path.ends_with("CoreSimulator/Caches"))
        );
        assert!(
            !xcode_roots
                .iter()
                .any(|root| root.path.ends_with("Archives")),
            "Xcode Archives must stay scan-only"
        );
        assert!(
            xcode_roots.iter().all(|root| !root.allow_root_itself),
            "an allowed root must never authorize deleting itself outright"
        );
    }

    #[test]
    fn homebrew_cache_policy_covers_the_detected_cache_root_only() {
        let policy = policy_for(&CleanableItem {
            id: CleanableItemId(1),
            category: CleanerCategory::HomebrewCache,
            group: None,
            display_name: String::new(),
            path: PathBuf::from("/tmp/homebrew-cache"),
            logical_size: 0,
            allocated_size: None,
            modified_at: None,
            last_accessed_at: None,
            risk: RiskLevel::SafeRecreatable,
            selection_policy: SelectionPolicy::SelectedByDefault,
            capabilities: Vec::new(),
            explanation: String::new(),
            warnings: Vec::new(),
            metadata: ItemMetadata::Generic,
        });

        let homebrew_roots: Vec<_> = policy
            .allowed_roots
            .iter()
            .filter(|root| {
                root.allowed_categories
                    .contains(&CleanerCategory::HomebrewCache)
            })
            .collect();
        assert_eq!(
            homebrew_roots.len(),
            1,
            "exactly one detected cache root should be allow-listed"
        );
        assert!(
            homebrew_roots
                .iter()
                .all(|root| !root.path.ends_with("Cellar")),
            "the Cellar must never be allow-listed"
        );
        assert!(
            homebrew_roots.iter().all(|root| !root.allow_root_itself),
            "an allowed root must never authorize deleting itself outright"
        );
    }

    #[test]
    fn node_tooling_cache_policy_never_allow_lists_the_pnpm_store() {
        let policy = policy_for(&CleanableItem {
            id: CleanableItemId(1),
            category: CleanerCategory::NodeToolingCache,
            group: None,
            display_name: String::new(),
            path: PathBuf::from("/tmp/node-tooling-cache"),
            logical_size: 0,
            allocated_size: None,
            modified_at: None,
            last_accessed_at: None,
            risk: RiskLevel::SafeRecreatable,
            selection_policy: SelectionPolicy::SelectedByDefault,
            capabilities: Vec::new(),
            explanation: String::new(),
            warnings: Vec::new(),
            metadata: ItemMetadata::Generic,
        });

        let node_roots: Vec<_> = policy
            .allowed_roots
            .iter()
            .filter(|root| {
                root.allowed_categories
                    .contains(&CleanerCategory::NodeToolingCache)
            })
            .collect();
        assert!(
            node_roots.iter().all(|root| !root.path.ends_with("store")),
            "pnpm's content-addressable store must never be allow-listed"
        );
        assert!(
            node_roots.iter().all(|root| !root.allow_root_itself),
            "an allowed root must never authorize deleting itself outright"
        );
    }
}
