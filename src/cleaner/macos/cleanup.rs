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
use crate::cleaner::macos::scanners::mail_files;
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
}
