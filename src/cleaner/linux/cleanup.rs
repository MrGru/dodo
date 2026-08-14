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
use crate::cleaner::linux::platform::move_to_trash;
use crate::cleaner::node_tooling_cache;
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

/// Filesystem categories get explicit scanner-derived roots. Docker Cache
/// routes to the shared CLI pruner instead; hidden categories cannot produce
/// items here.
fn policy_for(item: &CleanableItem) -> DeletionPolicy {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut allowed_roots = vec![AllowedRoot {
        path: PathBuf::from("/tmp"),
        allowed_categories: vec![CleanerCategory::SystemJunk],
    }];
    if let Some(home) = home.as_ref() {
        let cache_home = std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".cache"));
        allowed_roots.push(AllowedRoot {
            path: cache_home,
            allowed_categories: vec![CleanerCategory::UserCache],
        });
        for folder in ["Downloads", "Desktop", "Documents", "Videos"] {
            allowed_roots.push(AllowedRoot {
                path: home.join(folder),
                allowed_categories: vec![CleanerCategory::LargeOldFiles],
            });
        }
    }

    let node_environment = (item.category == CleanerCategory::NodeToolingCache)
        .then(|| node_tooling_cache::snapshot_environment(HostOs::Unix, home.as_deref()));
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
        .then(|| ai_apps::environment(HostOs::Unix, home.as_deref()));
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

    let mut protected_paths = vec![
        PathBuf::from("/"),
        PathBuf::from("/home"),
        PathBuf::from("/usr"),
        PathBuf::from("/etc"),
        PathBuf::from("/bin"),
        PathBuf::from("/sbin"),
        PathBuf::from("/boot"),
        PathBuf::from("/root"),
        PathBuf::from("/var"),
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
