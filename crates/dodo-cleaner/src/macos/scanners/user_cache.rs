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

pub struct UserCacheScanner {
    roots: Vec<ScanRoot>,
}

impl UserCacheScanner {
    pub fn new() -> Self {
        Self {
            roots: default_roots(),
        }
    }

    #[cfg(test)]
    fn with_roots(roots: Vec<ScanRoot>) -> Self {
        Self { roots }
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

        let mut warnings = Vec::new();
        let mut items = Vec::new();
        let mut scanned_entries = 0;
        let mut estimated_reclaimable_bytes = 0;
        let mut skipped_roots = Vec::new();

        for root in self.resolve_roots(context) {
            if cancellation.is_cancelled() {
                return Err(ScanError::Cancelled);
            }
            match scan_root(&root, CleanerCategory::UserCache, progress, cancellation) {
                Ok(result) => {
                    scanned_entries += result.scanned_entries;
                    for entry in result.entries {
                        if entry.logical_size == 0 {
                            continue;
                        }
                        estimated_reclaimable_bytes += entry.logical_size;
                        items.push(CleanableItem {
                            id: item_id(entry.path.as_path()),
                            category: CleanerCategory::UserCache,
                            group: Some(display_group(root.path.as_path())),
                            display_name: item_name(entry.path.as_path()),
                            path: entry.path,
                            logical_size: entry.logical_size,
                            allocated_size: None,
                            modified_at: entry.modified_at,
                            last_accessed_at: None,
                            risk: RiskLevel::SafeRecreatable,
                            selection_policy: SelectionPolicy::SelectedByDefault,
                            capabilities: vec![
                                ItemCapability::MoveToTrash,
                                ItemCapability::RevealInFinder,
                                ItemCapability::CopyPath,
                            ],
                            explanation: format!(
                                "Aggregated cache root inside {}.",
                                root.path.display()
                            ),
                            warnings: Vec::new(),
                            metadata: ItemMetadata::Generic,
                        });
                    }
                    warnings.extend(result.warnings);
                }
                Err(ScanError::RootUnavailable(_)) => {
                    skipped_roots.push(root.path.clone());
                }
                Err(err @ ScanError::Cancelled) => return Err(err),
                Err(err) => {
                    skipped_roots.push(root.path.clone());
                    warnings.push(ScanWarning {
                        message: format!("{}: {err:?}", root.path.display()),
                    });
                }
            }
        }

        let completeness = if skipped_roots.is_empty() {
            ScanCompleteness::Complete
        } else {
            ScanCompleteness::Partial {
                skipped_roots,
                reason: PartialScanReason::RootUnavailable,
            }
        };

        items.sort_by_key(|item| std::cmp::Reverse(item.logical_size));

        Ok(CategoryScanResult {
            category: CleanerCategory::UserCache,
            items,
            scanned_entries,
            estimated_reclaimable_bytes,
            warnings,
            completeness,
        })
    }
}

impl UserCacheScanner {
    fn resolve_roots(&self, context: &ScanContext) -> Vec<ScanRoot> {
        self.roots
            .iter()
            .filter_map(|root| expand_home(root, context.user_home.as_deref()))
            .collect()
    }
}

fn default_roots() -> Vec<ScanRoot> {
    vec![
        ScanRoot {
            path: PathBuf::from("~/Library/Caches"),
            max_depth: None,
            follow_symlinks: false,
            cross_filesystems: false,
            include_hidden: true,
            aggregate_mode: AggregateMode::ImmediateChildren,
            permission: None,
            risk: RiskLevel::SafeRecreatable,
        },
        ScanRoot {
            path: PathBuf::from("~/.cache"),
            max_depth: None,
            follow_symlinks: false,
            cross_filesystems: false,
            include_hidden: true,
            aggregate_mode: AggregateMode::ImmediateChildren,
            permission: None,
            risk: RiskLevel::SafeRecreatable,
        },
    ]
}

fn expand_home(root: &ScanRoot, home: Option<&Path>) -> Option<ScanRoot> {
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

fn display_group(root: &Path) -> String {
    root.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("Cache")
        .to_string()
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
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    use crate::core::cancellation::CancellationToken;
    use crate::core::progress::{ProgressSink, ScanProgress};
    use crate::core::risk::RiskLevel;
    use crate::core::scan_context::ScanContext;
    use crate::core::scan_root::{AggregateMode, ScanRoot};
    use crate::core::scanner::CleanerScanner;
    use crate::macos::scanners::user_cache::UserCacheScanner;

    struct RecordingSink(Arc<Mutex<Vec<ScanProgress>>>);

    impl ProgressSink for RecordingSink {
        fn report(&self, progress: ScanProgress) {
            self.0.lock().expect("lock poisoned").push(progress);
        }
    }

    #[test]
    fn scanner_aggregates_each_top_level_cache_directory() {
        let temp =
            std::env::temp_dir().join(format!("dodo-cleaner-user-cache-{}", std::process::id()));
        let cache_root = temp.join("Library").join("Caches");
        fs::create_dir_all(cache_root.join("com.example.app").join("nested"))
            .expect("creates app cache");
        fs::create_dir_all(cache_root.join("org.example.tool")).expect("creates tool cache");
        fs::write(
            cache_root
                .join("com.example.app")
                .join("nested")
                .join("data.bin"),
            vec![0u8; 32],
        )
        .expect("writes app cache");
        fs::write(
            cache_root.join("org.example.tool").join("cache.bin"),
            vec![0u8; 16],
        )
        .expect("writes tool cache");

        let scanner = UserCacheScanner::with_roots(vec![ScanRoot {
            path: PathBuf::from("~/Library/Caches"),
            max_depth: None,
            follow_symlinks: false,
            cross_filesystems: false,
            include_hidden: true,
            aggregate_mode: AggregateMode::ImmediateChildren,
            permission: None,
            risk: RiskLevel::SafeRecreatable,
        }]);
        let progress = Arc::new(Mutex::new(Vec::new()));
        let sink = RecordingSink(progress.clone());
        let context = ScanContext {
            started_at: std::time::SystemTime::now(),
            user_home: Some(temp.clone()),
        };

        let result = scanner
            .scan(&context, &sink, &CancellationToken::new())
            .expect("scans cache root");

        assert_eq!(result.items.len(), 2);
        assert_eq!(result.scanned_entries, 2);
        assert!(result.estimated_reclaimable_bytes >= 48);
        assert!(
            progress.lock().expect("lock poisoned").len() >= 2,
            "expected throttled progress plus completion"
        );

        fs::remove_dir_all(&temp).expect("removes temp tree");
    }
}
