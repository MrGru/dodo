use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::cleaner::ai_apps;
use crate::cleaner::core::category::CleanerCategory;
use crate::cleaner::core::errors::CleanupError;
use crate::cleaner::core::item::CleanableItem;
use crate::cleaner::core::report::{CleanupItemFailure, CleanupItemSuccess, CleanupReport};
use crate::cleaner::core::safety::{
    AllowedRoot, DeletionPolicy, dedupe_nested_paths, validate_path,
};
use crate::cleaner::node_tooling_cache;
use crate::cleaner::windows::platform::move_to_trash;
use crate::paths::{self, HostOs};

pub fn cleanup_items(items: &[CleanableItem]) -> CleanupReport {
    let mut by_path = BTreeMap::new();
    for item in items {
        by_path.insert(item.path.clone(), item);
    }

    let mut successes = Vec::new();
    let mut failures = Vec::new();
    let host = HostOs::current();
    let policy = items.first().map(policy_for);
    for path in dedupe_nested_paths(host, by_path.keys().cloned().collect()) {
        let Some(item) = by_path.get(&path) else {
            continue;
        };
        let Some(policy) = policy.as_ref() else {
            continue;
        };
        match validate_path(host, path.as_path(), item.category, policy) {
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

/// Filesystem categories use explicit scanner-derived roots. Docker Cache's
/// synthetic paths are routed to the shared CLI pruner before this function.
fn policy_for(item: &CleanableItem) -> DeletionPolicy {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from);
    let mut allowed_roots = Vec::new();
    if let Some(home) = home.as_ref() {
        allowed_roots.push(AllowedRoot {
            path: std::env::temp_dir(),
            allowed_categories: vec![CleanerCategory::SystemJunk],
        });
        for folder in ["Downloads", "Desktop", "Documents", "Videos"] {
            allowed_roots.push(AllowedRoot {
                path: home.join(folder),
                allowed_categories: vec![CleanerCategory::LargeOldFiles],
            });
        }
        if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) {
            for cache_root in [
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
                allowed_roots.push(AllowedRoot {
                    path: cache_root,
                    allowed_categories: vec![CleanerCategory::UserCache],
                });
            }
            let firefox_profiles = local_app_data
                .join("Mozilla")
                .join("Firefox")
                .join("Profiles");
            if let Ok(entries) = std::fs::read_dir(&firefox_profiles) {
                for entry in entries.flatten() {
                    allowed_roots.push(AllowedRoot {
                        path: entry.path().join("cache2"),
                        allowed_categories: vec![CleanerCategory::UserCache],
                    });
                }
            }
        }
    }

    let node_environment = (item.category == CleanerCategory::NodeToolingCache)
        .then(|| node_tooling_cache::snapshot_environment(HostOs::Windows, home.as_deref()));
    for root in node_environment
        .as_ref()
        .into_iter()
        .flat_map(|environment| node_tooling_cache::cleanup_allowed_roots(environment))
    {
        allowed_roots.push(AllowedRoot {
            path: root,
            allowed_categories: vec![CleanerCategory::NodeToolingCache],
        });
    }

    let ai_environment = (item.category == CleanerCategory::AiApps)
        .then(|| ai_apps::environment(HostOs::Windows, home.as_deref()));
    for root in ai_environment
        .as_ref()
        .into_iter()
        .flat_map(ai_apps::cleanup_allowed_roots)
    {
        allowed_roots.push(AllowedRoot {
            path: root,
            allowed_categories: vec![CleanerCategory::AiApps],
        });
    }

    // Trash Bins is review-only on Windows, the same as on macOS — no
    // `MoveToTrash` capability ever reaches this function for it (see
    // `windows::scanners::trash_bins`), so no allowed root is needed here.

    let mut protected_paths = vec![
        PathBuf::from("C:\\Windows"),
        PathBuf::from("C:\\Program Files"),
        PathBuf::from("C:\\Program Files (x86)"),
        PathBuf::from("C:\\Users"),
        paths::data_dir(),
    ];
    if let Some(home) = home.as_ref() {
        protected_paths.push(home.clone());
    }
    if let Some(environment) = node_environment.as_ref() {
        protected_paths.extend(node_tooling_cache::cleanup_denied_roots(environment));
    }
    if let Ok(exe) = std::env::current_exe() {
        protected_paths.push(exe);
    }

    DeletionPolicy {
        allowed_roots,
        protected_paths,
        user_home: home,
    }
}
