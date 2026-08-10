use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::cleaner::core::category::CleanerCategory;
use crate::cleaner::core::errors::CleanupError;
use crate::cleaner::core::item::CleanableItem;
use crate::cleaner::core::report::{CleanupItemFailure, CleanupItemSuccess, CleanupReport};
use crate::cleaner::core::safety::{
    AllowedRoot, DeletionPolicy, dedupe_nested_paths, validate_path,
};
use crate::cleaner::linux::platform::move_to_trash;
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

/// Only the three generic categories that ever carry `MoveToTrash` on Linux
/// (System Junk, User Cache, Large & Old Files — Trash Bins is
/// review-only, same as on macOS and Windows) get an allowed root here.
/// Every macOS-only category has no scanner on Linux at all, so it can never
/// produce an item this function is asked to validate.
fn policy_for(_item: &CleanableItem) -> DeletionPolicy {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut allowed_roots = vec![AllowedRoot {
        path: PathBuf::from("/tmp"),
        allow_root_itself: false,
        allowed_categories: vec![CleanerCategory::SystemJunk],
    }];
    if let Some(home) = home.as_ref() {
        let cache_home = std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".cache"));
        allowed_roots.push(AllowedRoot {
            path: cache_home,
            allow_root_itself: false,
            allowed_categories: vec![CleanerCategory::UserCache],
        });
        for folder in ["Downloads", "Desktop", "Documents", "Videos"] {
            allowed_roots.push(AllowedRoot {
                path: home.join(folder),
                allow_root_itself: false,
                allowed_categories: vec![CleanerCategory::LargeOldFiles],
            });
        }
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
