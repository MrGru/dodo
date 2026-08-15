use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::core::cancellation::CancellationToken;
use crate::core::category::CleanerCategory;
use crate::core::errors::ScanError;
use crate::core::fs::scan_matching_files;
use crate::core::item::{CleanableItem, CleanableItemId, ItemMetadata, LargeFileMetadata};
use crate::core::permissions::MacPermission;
use crate::core::progress::{ProgressSink, ScanPhase, ScanProgress};
use crate::core::report::{CategoryScanResult, PartialScanReason, ScanCompleteness, ScanWarning};
use crate::core::risk::{ItemCapability, RiskLevel, SelectionPolicy};
use crate::core::scan_context::ScanContext;
use crate::core::scan_root::{AggregateMode, ScanRoot};
use crate::core::scanner::CleanerScanner;

const LARGE_FILE_THRESHOLD_BYTES: u64 = 100 * 1024 * 1024;
const OLD_FILE_AGE: Duration = Duration::from_secs(60 * 60 * 24 * 365);

pub struct LargeOldFilesScanner;

impl LargeOldFilesScanner {
    pub fn new() -> Self {
        Self
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

        for root in resolve_roots(context.user_home.as_deref()) {
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

fn resolve_roots(home: Option<&Path>) -> Vec<ScanRoot> {
    let Some(home) = home else {
        return Vec::new();
    };
    ["Downloads", "Desktop", "Documents", "Videos"]
        .into_iter()
        .map(|folder| default_root(home.join(folder)))
        .collect()
}

fn default_root(path: PathBuf) -> ScanRoot {
    ScanRoot {
        path,
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
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut hasher);
    CleanableItemId(hasher.finish())
}
