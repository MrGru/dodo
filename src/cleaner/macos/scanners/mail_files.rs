use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::Path;

use crate::cleaner::core::cancellation::CancellationToken;
use crate::cleaner::core::category::CleanerCategory;
use crate::cleaner::core::errors::ScanError;
use crate::cleaner::core::fs::scan_root;
use crate::cleaner::core::item::{CleanableItem, CleanableItemId, ItemMetadata, MailFileMetadata};
use crate::cleaner::core::permissions::MacPermission;
use crate::cleaner::core::progress::{ProgressSink, ScanPhase, ScanProgress};
use crate::cleaner::core::report::{
    CategoryScanResult, PartialScanReason, ScanCompleteness, ScanWarning,
};
use crate::cleaner::core::risk::{ItemCapability, RiskLevel, SelectionPolicy};
use crate::cleaner::core::scan_context::ScanContext;
use crate::cleaner::core::scan_root::{AggregateMode, ScanRoot};
use crate::cleaner::core::scanner::CleanerScanner;

pub struct MailFilesScanner;

impl MailFilesScanner {
    pub fn new() -> Self {
        Self
    }
}

impl CleanerScanner for MailFilesScanner {
    fn category(&self) -> CleanerCategory {
        CleanerCategory::MailFiles
    }

    fn required_permissions(&self) -> &[MacPermission] {
        const FULL_DISK_ACCESS: &[MacPermission] = &[MacPermission::FullDiskAccess];
        FULL_DISK_ACCESS
    }

    fn scan(
        &self,
        context: &ScanContext,
        progress: &dyn ProgressSink,
        cancellation: &CancellationToken,
    ) -> Result<CategoryScanResult, ScanError> {
        progress.report(ScanProgress {
            category: CleanerCategory::MailFiles,
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
        let roots = attachment_roots(context.user_home.as_deref());
        for root in roots {
            if cancellation.is_cancelled() {
                return Err(ScanError::Cancelled);
            }
            match scan_root(&root, CleanerCategory::MailFiles, progress, cancellation) {
                Ok(result) => {
                    scanned_entries += result.scanned_entries;
                    warnings.extend(result.warnings);
                    for entry in result.entries {
                        if entry.logical_size == 0 {
                            continue;
                        }
                        items.push(CleanableItem {
                            id: item_id(entry.path.as_path()),
                            category: CleanerCategory::MailFiles,
                            group: Some(mail_group(root.path.as_path())),
                            display_name: entry
                                .path
                                .file_name()
                                .and_then(|name| name.to_str())
                                .unwrap_or("Mail file")
                                .to_string(),
                            path: entry.path.clone(),
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
                            explanation:
                                "Mail attachment or downloaded Mail file. Review before cleaning; it may need to be downloaded again."
                                    .into(),
                            warnings: Vec::new(),
                            metadata: ItemMetadata::MailFile(MailFileMetadata {
                                account_hint: account_hint(entry.path.as_path(), root.path.as_path()),
                            }),
                        });
                    }
                }
                Err(ScanError::PermissionDenied(_)) => skipped_roots.push(root.path.clone()),
                Err(ScanError::RootUnavailable(_)) => skipped_roots.push(root.path.clone()),
                Err(err @ ScanError::Cancelled) => return Err(err),
                Err(error) => warnings.push(ScanWarning {
                    message: format!("{}: {error:?}", root.path.display()),
                }),
            }
        }

        items.sort_by_key(|item| std::cmp::Reverse(item.logical_size));
        Ok(CategoryScanResult {
            category: CleanerCategory::MailFiles,
            estimated_reclaimable_bytes: items.iter().map(|item| item.logical_size).sum(),
            items,
            scanned_entries,
            warnings,
            completeness: if skipped_roots.is_empty() {
                ScanCompleteness::Complete
            } else {
                ScanCompleteness::Partial {
                    skipped_roots,
                    reason: PartialScanReason::PermissionDenied,
                }
            },
        })
    }
}

pub fn attachment_roots(home: Option<&Path>) -> Vec<ScanRoot> {
    let Some(home) = home else {
        return Vec::new();
    };
    let mut roots = Vec::new();
    for base in [
        home.join("Library").join("Mail"),
        home.join("Library")
            .join("Containers")
            .join("com.apple.mail")
            .join("Data")
            .join("Library")
            .join("Mail"),
    ] {
        roots.extend(versioned_attachment_roots(base.as_path()));
    }
    roots
}

fn versioned_attachment_roots(base: &Path) -> Vec<ScanRoot> {
    let Ok(entries) = fs::read_dir(base) else {
        return Vec::new();
    };
    let mut roots = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with('V'))
        {
            continue;
        }
        for relative in [
            Path::new("MailData").join("Attachments"),
            Path::new("MailData").join("Downloads"),
        ] {
            roots.push(ScanRoot {
                path: path.join(&relative),
                max_depth: None,
                follow_symlinks: false,
                cross_filesystems: false,
                include_hidden: true,
                aggregate_mode: AggregateMode::EveryFile,
                permission: Some(MacPermission::FullDiskAccess),
                risk: RiskLevel::UserData,
            });
        }
    }
    roots
}

fn mail_group(root: &Path) -> String {
    if root.ends_with("Attachments") {
        "Mail attachments".into()
    } else {
        "Mail downloads".into()
    }
}

fn account_hint(path: &Path, root: &Path) -> Option<String> {
    path.strip_prefix(root)
        .ok()
        .and_then(|relative| relative.components().next())
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
}

fn item_id(path: &Path) -> CleanableItemId {
    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    CleanableItemId(hasher.finish())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::SystemTime;

    use crate::cleaner::core::cancellation::CancellationToken;
    use crate::cleaner::core::item::ItemMetadata;
    use crate::cleaner::core::progress::{ProgressSink, ScanProgress};
    use crate::cleaner::core::scan_context::ScanContext;
    use crate::cleaner::core::scanner::CleanerScanner;
    use crate::cleaner::macos::scanners::mail_files::MailFilesScanner;

    struct RecordingSink;
    impl ProgressSink for RecordingSink {
        fn report(&self, _progress: ScanProgress) {}
    }

    #[test]
    fn scanner_finds_versioned_attachment_roots() {
        let temp =
            std::env::temp_dir().join(format!("dodo-cleaner-mail-files-{}", std::process::id()));
        let attachment_dir = temp
            .join("Library")
            .join("Mail")
            .join("V10")
            .join("MailData")
            .join("Attachments")
            .join("Account");
        fs::create_dir_all(&attachment_dir).expect("creates attachments");
        fs::write(attachment_dir.join("invoice.pdf"), b"mail").expect("writes attachment");

        let scanner = MailFilesScanner::new();
        let result = scanner
            .scan(
                &ScanContext {
                    started_at: SystemTime::now(),
                    user_home: Some(temp.clone()),
                },
                &RecordingSink,
                &CancellationToken::new(),
            )
            .expect("scans mail files");

        assert_eq!(result.items.len(), 1);
        assert!(matches!(
            result.items[0].metadata,
            ItemMetadata::MailFile(_)
        ));
        fs::remove_dir_all(&temp).expect("removes temp tree");
    }
}
