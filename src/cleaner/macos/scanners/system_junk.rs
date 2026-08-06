use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use crate::cleaner::core::cancellation::CancellationToken;
use crate::cleaner::core::category::CleanerCategory;
use crate::cleaner::core::errors::ScanError;
use crate::cleaner::core::fs::scan_root;
use crate::cleaner::core::item::{CleanableItem, CleanableItemId, ItemMetadata};
use crate::cleaner::core::permissions::MacPermission;
use crate::cleaner::core::progress::{ProgressSink, ScanPhase, ScanProgress};
use crate::cleaner::core::report::{CategoryScanResult, PartialScanReason, ScanCompleteness};
use crate::cleaner::core::risk::{ItemCapability, RiskLevel, SelectionPolicy};
use crate::cleaner::core::scan_context::ScanContext;
use crate::cleaner::core::scan_root::{AggregateMode, ScanRoot};
use crate::cleaner::core::scanner::CleanerScanner;

pub struct SystemJunkScanner {
    roots: Vec<ScanRoot>,
}

impl SystemJunkScanner {
    pub fn new() -> Self {
        Self {
            roots: vec![
                ScanRoot {
                    path: PathBuf::from("~/Library/Logs"),
                    max_depth: None,
                    follow_symlinks: false,
                    cross_filesystems: false,
                    include_hidden: true,
                    aggregate_mode: AggregateMode::ImmediateChildren,
                    permission: None,
                    risk: RiskLevel::SafeRecreatable,
                },
                ScanRoot {
                    path: PathBuf::from("/tmp"),
                    max_depth: Some(2),
                    follow_symlinks: false,
                    cross_filesystems: false,
                    include_hidden: true,
                    aggregate_mode: AggregateMode::ImmediateChildren,
                    permission: None,
                    risk: RiskLevel::SafeRecreatable,
                },
            ],
        }
    }
}

impl CleanerScanner for SystemJunkScanner {
    fn category(&self) -> CleanerCategory {
        CleanerCategory::SystemJunk
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
            category: CleanerCategory::SystemJunk,
            phase: ScanPhase::Preparing,
            current_path: None,
            scanned_entries: 0,
            discovered_items: 0,
            discovered_bytes: 0,
        });

        let mut items = Vec::new();
        let mut warnings = Vec::new();
        let mut skipped_roots = Vec::new();
        let mut scanned_entries = 0;
        let mut estimated_reclaimable_bytes = 0;

        for root in self.resolve_roots(context) {
            if cancellation.is_cancelled() {
                return Err(ScanError::Cancelled);
            }
            match scan_root(&root, CleanerCategory::SystemJunk, progress, cancellation) {
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
                            category: CleanerCategory::SystemJunk,
                            group: Some(root_label(root.path.as_path())),
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
                                "Aggregated recreatable system-junk root inside {}.",
                                root.path.display()
                            ),
                            warnings: Vec::new(),
                            metadata: ItemMetadata::Generic,
                        });
                    }
                }
                Err(ScanError::RootUnavailable(_)) => skipped_roots.push(root.path.clone()),
                Err(err @ ScanError::Cancelled) => return Err(err),
                Err(error) => warnings.push(crate::cleaner::core::report::ScanWarning {
                    message: format!("{}: {error:?}", root.path.display()),
                }),
            }
        }

        items.sort_by_key(|item| std::cmp::Reverse(item.logical_size));
        Ok(CategoryScanResult {
            category: CleanerCategory::SystemJunk,
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

impl SystemJunkScanner {
    fn resolve_roots(&self, context: &ScanContext) -> Vec<ScanRoot> {
        self.roots
            .iter()
            .filter_map(|root| expand_home(root, context.user_home.as_deref()))
            .collect()
    }
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

fn root_label(path: &Path) -> String {
    if path == Path::new("/tmp") {
        "Temporary files".into()
    } else {
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("Logs")
            .to_string()
    }
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
