//! The "aggregate every root's children into review candidates" shape shared
//! by System Junk and User Cache on every platform that has them.
//!
//! macOS's own `SystemJunkScanner` and `UserCacheScanner` (in
//! `cleaner::macos::scanners`) predate this and are left as they are —
//! they already shipped and are already tested, and touching working,
//! shipped code to deduplicate against code that did not exist yet is not
//! worth the regression risk. Windows' and Linux's scanners
//! (`cleaner::windows::scanners`, `cleaner::linux::scanners`) are new, so
//! they share this instead of re-typing the same loop twice more.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::Path;

use crate::core::cancellation::CancellationToken;
use crate::core::category::CleanerCategory;
use crate::core::errors::ScanError;
use crate::core::fs::scan_root;
use crate::core::item::{CleanableItem, CleanableItemId, ItemMetadata};
use crate::core::progress::ProgressSink;
use crate::core::report::{CategoryScanResult, PartialScanReason, ScanCompleteness, ScanWarning};
use crate::core::risk::{ItemCapability, RiskLevel, SelectionPolicy};
use crate::core::scan_root::ScanRoot;

/// Expands a `~/`-prefixed [`ScanRoot::path`] against `home`, unchanged
/// otherwise. `None` only when the root needs `home` and none was resolved
/// (see [`super::scan_context::ScanContext`]), in which case the root is
/// dropped rather than scanned literally as `~/...`.
pub fn expand_home(root: &ScanRoot, home: Option<&Path>) -> Option<ScanRoot> {
    let Some(path) = root.path.to_str() else {
        return Some(root.clone());
    };
    if !path.starts_with("~/") {
        return Some(root.clone());
    }
    let home = home?;
    Some(ScanRoot {
        path: home.join(&path[2..]),
        ..root.clone()
    })
}

fn item_id(path: &Path) -> CleanableItemId {
    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    CleanableItemId(hasher.finish())
}

fn item_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

/// Scans every root and turns each of its non-empty aggregated entries (per
/// `root.aggregate_mode`) into one [`CleanableItem`], sorted largest first.
/// `explanation` and `group_label` are called once per *root*, not per entry
/// — every entry aggregated out of the same root shares both.
///
/// `risk`/`selection_policy`/`capabilities` travel together as one
/// [`ItemPolicy`] rather than three more parameters — past seven,
/// `clippy::too_many_arguments` starts counting, and these three are always
/// chosen together by every caller anyway.
pub struct ItemPolicy {
    pub risk: RiskLevel,
    pub selection_policy: SelectionPolicy,
    pub capabilities: Vec<ItemCapability>,
}

pub fn run_aggregated_scan(
    category: CleanerCategory,
    roots: Vec<ScanRoot>,
    policy: ItemPolicy,
    explanation: impl Fn(&Path) -> String,
    group_label: impl Fn(&Path) -> String,
    progress: &dyn ProgressSink,
    cancellation: &CancellationToken,
) -> Result<CategoryScanResult, ScanError> {
    let ItemPolicy {
        risk,
        selection_policy,
        capabilities,
    } = policy;
    let mut items = Vec::new();
    let mut warnings = Vec::new();
    let mut skipped_roots = Vec::new();
    let mut scanned_entries = 0u64;
    let mut estimated_reclaimable_bytes = 0u64;

    for root in roots {
        if cancellation.is_cancelled() {
            return Err(ScanError::Cancelled);
        }
        match scan_root(&root, category, progress, cancellation) {
            Ok(result) => {
                scanned_entries += result.scanned_entries;
                warnings.extend(result.warnings);
                for entry in result.entries {
                    if entry.logical_size == 0 {
                        continue;
                    }
                    estimated_reclaimable_bytes += entry.logical_size;
                    items.push(CleanableItem {
                        id: item_id(entry.path.as_path()),
                        category,
                        group: Some(group_label(root.path.as_path())),
                        display_name: item_name(entry.path.as_path()),
                        path: entry.path,
                        logical_size: entry.logical_size,
                        allocated_size: None,
                        modified_at: entry.modified_at,
                        last_accessed_at: None,
                        risk,
                        selection_policy,
                        capabilities: capabilities.clone(),
                        explanation: explanation(root.path.as_path()),
                        warnings: Vec::new(),
                        metadata: ItemMetadata::Generic,
                    });
                }
            }
            Err(ScanError::RootUnavailable(_)) => skipped_roots.push(root.path.clone()),
            Err(err @ ScanError::Cancelled) => return Err(err),
            Err(error) => warnings.push(ScanWarning {
                message: format!("{}: {error:?}", root.path.display()),
            }),
        }
    }

    items.sort_by_key(|item| std::cmp::Reverse(item.logical_size));
    Ok(CategoryScanResult {
        category,
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

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    use super::{ItemPolicy, expand_home, run_aggregated_scan};
    use crate::core::cancellation::CancellationToken;
    use crate::core::category::CleanerCategory;
    use crate::core::progress::{ProgressSink, ScanProgress};
    use crate::core::risk::{RiskLevel, SelectionPolicy};
    use crate::core::scan_root::{AggregateMode, ScanRoot};

    struct NoopSink;
    impl ProgressSink for NoopSink {
        fn report(&self, _progress: ScanProgress) {}
    }

    fn root(path: PathBuf) -> ScanRoot {
        ScanRoot {
            path,
            max_depth: None,
            follow_symlinks: false,
            cross_filesystems: false,
            include_hidden: true,
            aggregate_mode: AggregateMode::ImmediateChildren,
            permission: None,
            risk: RiskLevel::SafeRecreatable,
        }
    }

    #[test]
    fn expand_home_only_rewrites_tilde_paths() {
        let home = PathBuf::from("/home/ada");
        let expanded = expand_home(&root(PathBuf::from("~/.cache")), Some(home.as_path()))
            .expect("has a home to expand against");
        assert_eq!(expanded.path, PathBuf::from("/home/ada/.cache"));

        let unchanged = expand_home(&root(PathBuf::from("/tmp")), Some(home.as_path()))
            .expect("absolute paths pass through");
        assert_eq!(unchanged.path, PathBuf::from("/tmp"));

        assert!(expand_home(&root(PathBuf::from("~/.cache")), None).is_none());
    }

    #[test]
    fn aggregates_each_top_level_child_into_one_item() {
        let temp = std::env::temp_dir().join(format!(
            "dodo-cleaner-generic-scanner-{}-{}",
            std::process::id(),
            line!()
        ));
        fs::create_dir_all(temp.join("app-one")).expect("creates first root");
        fs::create_dir_all(temp.join("app-two")).expect("creates second root");
        fs::write(temp.join("app-one").join("data.bin"), vec![0u8; 32]).expect("writes data");
        fs::write(temp.join("app-two").join("data.bin"), vec![0u8; 16]).expect("writes data");

        let result = run_aggregated_scan(
            CleanerCategory::UserCache,
            vec![root(temp.clone())],
            ItemPolicy {
                risk: RiskLevel::SafeRecreatable,
                selection_policy: SelectionPolicy::SelectedByDefault,
                capabilities: Vec::new(),
            },
            |root| format!("inside {}", root.display()),
            |_| "Cache".to_string(),
            &NoopSink,
            &CancellationToken::new(),
        )
        .expect("scans the temp root");

        assert_eq!(result.items.len(), 2);
        assert!(result.estimated_reclaimable_bytes >= 48);
        assert!(
            result
                .items
                .iter()
                .all(|item| item.group.as_deref() == Some("Cache"))
        );

        fs::remove_dir_all(&temp).expect("removes temp tree");
    }

    #[test]
    fn a_missing_root_is_reported_as_a_skipped_root_not_an_error() {
        let missing = std::env::temp_dir().join(format!(
            "dodo-cleaner-generic-scanner-missing-{}-{}",
            std::process::id(),
            line!()
        ));
        let progress = Arc::new(Mutex::new(Vec::new()));
        struct RecordingSink(Arc<Mutex<Vec<ScanProgress>>>);
        impl ProgressSink for RecordingSink {
            fn report(&self, progress: ScanProgress) {
                self.0.lock().expect("lock poisoned").push(progress);
            }
        }

        let result = run_aggregated_scan(
            CleanerCategory::SystemJunk,
            vec![root(missing.clone())],
            ItemPolicy {
                risk: RiskLevel::SafeRecreatable,
                selection_policy: SelectionPolicy::SelectedByDefault,
                capabilities: Vec::new(),
            },
            |_| String::new(),
            |_| String::new(),
            &RecordingSink(progress),
            &CancellationToken::new(),
        )
        .expect("a missing root is not a hard error");

        assert!(result.items.is_empty());
        assert!(matches!(
            result.completeness,
            crate::core::report::ScanCompleteness::Partial { .. }
        ));
    }
}
