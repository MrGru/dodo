use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::os::unix::fs::MetadataExt as _;
use std::path::Path;

use crate::cleaner::core::cancellation::CancellationToken;
use crate::cleaner::core::category::CleanerCategory;
use crate::cleaner::core::errors::ScanError;
use crate::cleaner::core::fs::scan_root;
use crate::cleaner::core::item::{CleanableItem, CleanableItemId, ItemMetadata};
use crate::cleaner::core::permissions::MacPermission;
use crate::cleaner::core::progress::{ProgressSink, ScanPhase, ScanProgress};
use crate::cleaner::core::report::{
    CategoryScanResult, PartialScanReason, ScanCompleteness, ScanWarning,
};
use crate::cleaner::core::risk::{ItemCapability, RiskLevel, SelectionPolicy};
use crate::cleaner::core::scan_context::ScanContext;
use crate::cleaner::core::scan_root::{AggregateMode, ScanRoot};
use crate::cleaner::core::scanner::CleanerScanner;

pub struct TrashBinsScanner;

impl TrashBinsScanner {
    pub fn new() -> Self {
        Self
    }
}

impl CleanerScanner for TrashBinsScanner {
    fn category(&self) -> CleanerCategory {
        CleanerCategory::TrashBins
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
            category: CleanerCategory::TrashBins,
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
        for root in discover_roots(context) {
            if cancellation.is_cancelled() {
                return Err(ScanError::Cancelled);
            }
            match scan_root(&root, CleanerCategory::TrashBins, progress, cancellation) {
                Ok(result) => {
                    scanned_entries += result.scanned_entries;
                    warnings.extend(result.warnings);
                    if let Some(entry) = result.entries.into_iter().next() {
                        items.push(CleanableItem {
                            id: item_id(entry.path.as_path()),
                            category: CleanerCategory::TrashBins,
                            group: Some(trash_group(entry.path.as_path())),
                            display_name: item_name(entry.path.as_path()),
                            path: entry.path,
                            logical_size: entry.logical_size,
                            allocated_size: None,
                            modified_at: entry.modified_at,
                            last_accessed_at: None,
                            risk: RiskLevel::ReviewRecommended,
                            selection_policy: SelectionPolicy::NeverBulkSelect,
                            capabilities: vec![ItemCapability::EmptyTrash],
                            explanation: "Empty Trash removes this bin's contents permanently."
                                .into(),
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
            category: CleanerCategory::TrashBins,
            estimated_reclaimable_bytes: items.iter().map(|item| item.logical_size).sum(),
            items,
            scanned_entries,
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

fn discover_roots(context: &ScanContext) -> Vec<ScanRoot> {
    let mut roots = Vec::new();
    if let Some(home) = context.user_home.as_ref() {
        roots.push(ScanRoot {
            path: home.join(".Trash"),
            max_depth: None,
            follow_symlinks: false,
            cross_filesystems: false,
            include_hidden: true,
            aggregate_mode: AggregateMode::WholeRoot,
            permission: None,
            risk: RiskLevel::ReviewRecommended,
        });

        if let Ok(metadata) = std::fs::metadata(home) {
            let uid = metadata.uid();
            if let Ok(volumes) = std::fs::read_dir("/Volumes") {
                for volume in volumes.flatten() {
                    roots.push(ScanRoot {
                        path: volume.path().join(".Trashes").join(uid.to_string()),
                        max_depth: None,
                        follow_symlinks: false,
                        cross_filesystems: false,
                        include_hidden: true,
                        aggregate_mode: AggregateMode::WholeRoot,
                        permission: None,
                        risk: RiskLevel::ReviewRecommended,
                    });
                }
            }
        }
    }
    roots
}

fn trash_group(path: &Path) -> String {
    if path.ends_with(".Trash") {
        "Home Trash".into()
    } else {
        path.parent()
            .and_then(Path::parent)
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            .map(|name| format!("Volume {name}"))
            .unwrap_or_else(|| "External Trash".into())
    }
}

fn item_name(path: &Path) -> String {
    trash_group(path)
}

fn item_id(path: &Path) -> CleanableItemId {
    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    CleanableItemId(hasher.finish())
}
