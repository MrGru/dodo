use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::cleaner::core::cancellation::CancellationToken;
use crate::cleaner::core::category::CleanerCategory;
use crate::cleaner::core::errors::ScanError;
use crate::cleaner::core::fs::scan_matching_files;
use crate::cleaner::core::item::{CleanableItem, CleanableItemId, ItemMetadata, LargeFileMetadata};
use crate::cleaner::core::permissions::MacPermission;
use crate::cleaner::core::progress::{ProgressSink, ScanPhase, ScanProgress};
use crate::cleaner::core::report::{
    CategoryScanResult, PartialScanReason, ScanCompleteness, ScanWarning,
};
use crate::cleaner::core::risk::{ItemCapability, RiskLevel, SelectionPolicy};
use crate::cleaner::core::scan_context::ScanContext;
use crate::cleaner::core::scan_root::{AggregateMode, ScanRoot};
use crate::cleaner::core::scanner::CleanerScanner;

const LARGE_FILE_THRESHOLD_BYTES: u64 = 100 * 1024 * 1024;
const OLD_FILE_AGE: Duration = Duration::from_secs(60 * 60 * 24 * 365);

pub struct LargeOldFilesScanner {
    roots: Vec<ScanRoot>,
}

impl LargeOldFilesScanner {
    pub fn new() -> Self {
        Self {
            roots: vec![
                default_root("~/Downloads"),
                default_root("~/Desktop"),
                default_root("~/Documents"),
                default_root("~/Movies"),
            ],
        }
    }

    #[cfg(test)]
    fn with_roots(roots: Vec<ScanRoot>) -> Self {
        Self { roots }
    }
}

impl CleanerScanner for LargeOldFilesScanner {
    fn category(&self) -> CleanerCategory {
        CleanerCategory::LargeOldFiles
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
            category: CleanerCategory::LargeOldFiles,
            phase: ScanPhase::Preparing,
            current_path: None,
            scanned_entries: 0,
            discovered_items: 0,
            discovered_bytes: 0,
        });

        let mut items = Vec::new();
        let mut warnings = Vec::new();
        let mut scanned_entries = 0;
        let mut skipped_roots = Vec::new();

        for root in resolve_roots(&self.roots, context.user_home.as_deref()) {
            if cancellation.is_cancelled() {
                return Err(ScanError::Cancelled);
            }
            match scan_matching_files(
                &root,
                CleanerCategory::LargeOldFiles,
                progress,
                cancellation,
                |size, modified_at| qualifies(size, modified_at, context.started_at),
            ) {
                Ok(result) => {
                    scanned_entries += result.scanned_entries;
                    warnings.extend(result.warnings);
                    for entry in result.entries {
                        let extension = entry
                            .path
                            .extension()
                            .and_then(|extension| extension.to_str())
                            .map(ToOwned::to_owned);
                        items.push(CleanableItem {
                            id: item_id(entry.path.as_path()),
                            category: CleanerCategory::LargeOldFiles,
                            group: Some(root_group(root.path.as_path())),
                            display_name: item_name(entry.path.as_path()),
                            path: entry.path,
                            logical_size: entry.logical_size,
                            allocated_size: None,
                            modified_at: entry.modified_at,
                            last_accessed_at: None,
                            risk: RiskLevel::UserData,
                            selection_policy: SelectionPolicy::NotSelectedByDefault,
                            capabilities: vec![
                                ItemCapability::MoveToTrash,
                                ItemCapability::RevealInFinder,
                                ItemCapability::CopyPath,
                            ],
                            explanation: explanation(
                                entry.logical_size,
                                entry.modified_at,
                                context.started_at,
                            ),
                            warnings: Vec::new(),
                            metadata: ItemMetadata::LargeFile(LargeFileMetadata { extension }),
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
        let estimated_reclaimable_bytes = items.iter().map(|item| item.logical_size).sum();
        Ok(CategoryScanResult {
            category: CleanerCategory::LargeOldFiles,
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

fn default_root(path: &str) -> ScanRoot {
    ScanRoot {
        path: PathBuf::from(path),
        max_depth: None,
        follow_symlinks: false,
        cross_filesystems: false,
        include_hidden: false,
        aggregate_mode: AggregateMode::EveryFile,
        permission: None,
        risk: RiskLevel::UserData,
    }
}

fn qualifies(size: u64, modified_at: Option<SystemTime>, started_at: SystemTime) -> bool {
    size >= LARGE_FILE_THRESHOLD_BYTES
        || modified_at
            .and_then(|modified_at| started_at.duration_since(modified_at).ok())
            .is_some_and(|age| age >= OLD_FILE_AGE)
}

fn explanation(size: u64, modified_at: Option<SystemTime>, started_at: SystemTime) -> String {
    let is_large = size >= LARGE_FILE_THRESHOLD_BYTES;
    let is_old = modified_at
        .and_then(|modified_at| started_at.duration_since(modified_at).ok())
        .is_some_and(|age| age >= OLD_FILE_AGE);
    match (is_large, is_old) {
        (true, true) => "Large file and not modified for at least one year.".into(),
        (true, false) => "Large file over the default 100 MiB review threshold.".into(),
        (false, true) => "Old file not modified for at least one year.".into(),
        (false, false) => "Review file.".into(),
    }
}

fn resolve_roots(roots: &[ScanRoot], home: Option<&Path>) -> Vec<ScanRoot> {
    roots
        .iter()
        .filter_map(|root| expand_home(root, home))
        .collect()
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

fn root_group(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("Files")
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
    use std::time::{Duration, SystemTime};

    use crate::cleaner::core::cancellation::CancellationToken;
    use crate::cleaner::core::progress::{ProgressSink, ScanProgress};
    use crate::cleaner::core::scan_context::ScanContext;
    use crate::cleaner::core::scan_root::{AggregateMode, ScanRoot};
    use crate::cleaner::core::scanner::CleanerScanner;
    use crate::cleaner::macos::scanners::large_old_files::{LargeOldFilesScanner, OLD_FILE_AGE};

    struct RecordingSink(Arc<Mutex<Vec<ScanProgress>>>);

    impl ProgressSink for RecordingSink {
        fn report(&self, progress: ScanProgress) {
            self.0.lock().expect("lock poisoned").push(progress);
        }
    }

    #[test]
    fn scanner_finds_large_and_old_files() {
        let temp =
            std::env::temp_dir().join(format!("dodo-cleaner-large-old-{}", std::process::id()));
        let downloads = temp.join("Downloads");
        fs::create_dir_all(&downloads).expect("creates downloads");
        let old = downloads.join("old.txt");
        let large = downloads.join("large.bin");
        fs::write(&old, b"old").expect("writes old file");
        fs::File::create(&large)
            .and_then(|file| file.set_len(101 * 1024 * 1024))
            .expect("creates sparse large file");

        let scanner = LargeOldFilesScanner::with_roots(vec![ScanRoot {
            path: PathBuf::from("~/Downloads"),
            max_depth: None,
            follow_symlinks: false,
            cross_filesystems: false,
            include_hidden: false,
            aggregate_mode: AggregateMode::EveryFile,
            permission: None,
            risk: crate::cleaner::core::risk::RiskLevel::UserData,
        }]);
        let context = ScanContext {
            started_at: SystemTime::now() + Duration::from_secs(OLD_FILE_AGE.as_secs() + 5),
            user_home: Some(temp.clone()),
        };
        let result = scanner
            .scan(
                &context,
                &RecordingSink(Arc::new(Mutex::new(Vec::new()))),
                &CancellationToken::new(),
            )
            .expect("scans");

        assert_eq!(result.items.len(), 2);
        assert_eq!(result.items[0].display_name, "large.bin");
        assert_eq!(result.items[1].display_name, "old.txt");
        assert!(result.warnings.is_empty());
        fs::remove_dir_all(&temp).expect("removes temp tree");
    }

    #[test]
    fn scanner_displays_only_qualifying_sparse_file() {
        const FRESH_FILE_COUNT: usize = 128;
        const CANDIDATE_SIZE: u64 = 101 * 1024 * 1024;

        let temp = std::env::temp_dir().join(format!(
            "dodo-cleaner-large-old-matching-{}-{}",
            std::process::id(),
            line!()
        ));
        let downloads = temp.join("Downloads");
        fs::create_dir_all(&downloads).expect("creates downloads");
        for index in 0..FRESH_FILE_COUNT {
            fs::write(downloads.join(format!("fresh-{index}")), b"x").expect("writes fresh file");
        }
        let candidate = downloads.join("sparse-large.bin");
        fs::File::create(&candidate)
            .and_then(|file| file.set_len(CANDIDATE_SIZE))
            .expect("creates sparse candidate");
        let skipped_link = downloads.join("skipped-link");
        std::os::unix::fs::symlink(&candidate, &skipped_link).expect("creates symlink");

        let scanner = LargeOldFilesScanner::with_roots(vec![ScanRoot {
            path: PathBuf::from("~/Downloads"),
            max_depth: None,
            follow_symlinks: false,
            cross_filesystems: false,
            include_hidden: false,
            aggregate_mode: AggregateMode::EveryFile,
            permission: None,
            risk: crate::cleaner::core::risk::RiskLevel::UserData,
        }]);
        let result = scanner
            .scan(
                &ScanContext {
                    started_at: SystemTime::now(),
                    user_home: Some(temp.clone()),
                },
                &RecordingSink(Arc::new(Mutex::new(Vec::new()))),
                &CancellationToken::new(),
            )
            .expect("scans");

        assert_eq!(result.scanned_entries, (FRESH_FILE_COUNT + 1) as u64);
        assert_eq!(result.items.len(), 1);
        assert_eq!(result.items[0].display_name, "sparse-large.bin");
        assert_eq!(result.items[0].path, candidate);
        assert_eq!(result.items[0].logical_size, CANDIDATE_SIZE);
        assert_eq!(result.estimated_reclaimable_bytes, CANDIDATE_SIZE);
        assert_eq!(result.warnings.len(), 1);
        assert_eq!(
            result.warnings[0].message,
            format!("Skipped symlink {}", skipped_link.display())
        );

        fs::remove_dir_all(&temp).expect("removes temp tree");
    }
}
