use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use crate::cleaner::core::cancellation::CancellationToken;
use crate::cleaner::core::category::CleanerCategory;
use crate::cleaner::core::errors::ScanError;
use crate::cleaner::core::progress::{ProgressSink, ScanPhase, ScanProgress};
use crate::cleaner::core::report::ScanWarning;
use crate::cleaner::core::scan_root::{AggregateMode, ScanRoot};

const PROGRESS_INTERVAL: Duration = Duration::from_millis(125);

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RootScanResult {
    pub root: PathBuf,
    pub entries: Vec<AggregatedEntry>,
    pub scanned_entries: u64,
    pub warnings: Vec<ScanWarning>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct AggregatedEntry {
    pub path: PathBuf,
    pub logical_size: u64,
    pub modified_at: Option<SystemTime>,
    pub scanned_entries: u64,
}

#[derive(Clone, Copy, Debug, Default)]
struct TreeStats {
    logical_size: u64,
    scanned_entries: u64,
    modified_at: Option<SystemTime>,
}

struct TraversalCtx<'a> {
    root: &'a ScanRoot,
    root_device: Option<u64>,
    category: CleanerCategory,
    progress: &'a dyn ProgressSink,
    cancellation: &'a CancellationToken,
}

pub fn scan_root(
    root: &ScanRoot,
    category: CleanerCategory,
    progress: &dyn ProgressSink,
    cancellation: &CancellationToken,
) -> Result<RootScanResult, ScanError> {
    let metadata =
        fs::symlink_metadata(&root.path).map_err(|err| map_root_error(root.path.clone(), err))?;
    if metadata.file_type().is_symlink() {
        return Err(ScanError::InvalidMetadata(root.path.clone()));
    }

    let mut reporter = ProgressReporter::new(category);
    reporter.report(progress, ScanPhase::DiscoveringRoots, None, 0, 0, 0);

    let root_device = device_id(&metadata);
    let ctx = TraversalCtx {
        root,
        root_device,
        category,
        progress,
        cancellation,
    };
    let mut warnings = Vec::new();
    let mut scanned_entries = 0;
    let entries = match root.aggregate_mode {
        AggregateMode::ImmediateChildren => {
            scan_immediate_children(&ctx, &mut reporter, &mut scanned_entries, &mut warnings)?
        }
        AggregateMode::WholeRoot | AggregateMode::TopLevelDirectory => {
            let stats = measure_path(&root.path, &ctx, &mut reporter, &mut warnings)?;
            scanned_entries += stats.scanned_entries;
            vec![AggregatedEntry {
                path: root.path.clone(),
                logical_size: stats.logical_size,
                modified_at: stats.modified_at,
                scanned_entries: stats.scanned_entries,
            }]
        }
        AggregateMode::EveryFile => {
            scan_every_file(&ctx, &mut reporter, &mut scanned_entries, &mut warnings)?
        }
    };

    reporter.report(
        progress,
        ScanPhase::Completed,
        Some(root.path.clone()),
        scanned_entries,
        entries.len() as u64,
        entries.iter().map(|entry| entry.logical_size).sum(),
    );

    Ok(RootScanResult {
        root: root.path.clone(),
        entries,
        scanned_entries,
        warnings,
    })
}

fn scan_immediate_children(
    ctx: &TraversalCtx,
    reporter: &mut ProgressReporter,
    scanned_entries: &mut u64,
    warnings: &mut Vec<ScanWarning>,
) -> Result<Vec<AggregatedEntry>, ScanError> {
    let root = ctx.root;
    let mut entries = Vec::new();
    let read_dir =
        fs::read_dir(&root.path).map_err(|err| map_root_error(root.path.clone(), err))?;
    for child in read_dir {
        if ctx.cancellation.is_cancelled() {
            return Err(ScanError::Cancelled);
        }
        let child = match child {
            Ok(child) => child,
            Err(err) => {
                warnings.push(ScanWarning {
                    message: format!("Failed to enumerate {}: {err}", root.path.display()),
                });
                continue;
            }
        };
        let path = child.path();
        if should_skip_name(path.as_path(), root.include_hidden) {
            continue;
        }
        let stats = measure_path(&path, ctx, reporter, warnings)?;
        *scanned_entries += stats.scanned_entries;
        entries.push(AggregatedEntry {
            path,
            logical_size: stats.logical_size,
            modified_at: stats.modified_at,
            scanned_entries: stats.scanned_entries,
        });
    }
    Ok(entries)
}

fn scan_every_file(
    ctx: &TraversalCtx,
    reporter: &mut ProgressReporter,
    scanned_entries: &mut u64,
    warnings: &mut Vec<ScanWarning>,
) -> Result<Vec<AggregatedEntry>, ScanError> {
    let root = ctx.root;
    let mut queue = VecDeque::from([(root.path.clone(), 0usize)]);
    let mut entries = Vec::new();
    while let Some((path, depth)) = queue.pop_front() {
        if ctx.cancellation.is_cancelled() {
            return Err(ScanError::Cancelled);
        }
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(err) => {
                warnings.push(ScanWarning {
                    message: format!("Failed to inspect {}: {err}", path.display()),
                });
                continue;
            }
        };
        if metadata.file_type().is_symlink() {
            warnings.push(ScanWarning {
                message: format!("Skipped symlink {}", path.display()),
            });
            continue;
        }
        if metadata.is_file() {
            *scanned_entries += 1;
            reporter.report(
                ctx.progress,
                ScanPhase::Traversing,
                Some(path.clone()),
                *scanned_entries,
                entries.len() as u64,
                entries
                    .iter()
                    .map(|entry: &AggregatedEntry| entry.logical_size)
                    .sum::<u64>()
                    + metadata.len(),
            );
            entries.push(AggregatedEntry {
                path,
                logical_size: metadata.len(),
                modified_at: metadata.modified().ok(),
                scanned_entries: 1,
            });
            continue;
        }
        if metadata.is_dir() {
            if root.max_depth.is_some_and(|max_depth| depth >= max_depth) {
                continue;
            }
            if !same_filesystem(
                ctx.root_device,
                device_id(&metadata),
                root.cross_filesystems,
            ) {
                warnings.push(ScanWarning {
                    message: format!("Skipped mounted volume at {}", path.display()),
                });
                continue;
            }
            let read_dir = match fs::read_dir(&path) {
                Ok(read_dir) => read_dir,
                Err(err) => {
                    warnings.push(ScanWarning {
                        message: format!("Failed to read {}: {err}", path.display()),
                    });
                    continue;
                }
            };
            for child in read_dir {
                let child = match child {
                    Ok(child) => child,
                    Err(err) => {
                        warnings.push(ScanWarning {
                            message: format!("Failed to enumerate {}: {err}", path.display()),
                        });
                        continue;
                    }
                };
                let child_path = child.path();
                if should_skip_name(child_path.as_path(), root.include_hidden) {
                    continue;
                }
                queue.push_back((child_path, depth + 1));
            }
        }
    }
    let _ = ctx.category;
    Ok(entries)
}

fn measure_path(
    path: &Path,
    ctx: &TraversalCtx,
    reporter: &mut ProgressReporter,
    warnings: &mut Vec<ScanWarning>,
) -> Result<TreeStats, ScanError> {
    let root = ctx.root;
    let metadata = fs::symlink_metadata(path).map_err(|err| ScanError::Io {
        path: path.to_path_buf(),
        source: err,
    })?;
    if metadata.file_type().is_symlink() {
        warnings.push(ScanWarning {
            message: format!("Skipped symlink {}", path.display()),
        });
        return Ok(TreeStats::default());
    }
    if metadata.is_file() {
        reporter.report(
            ctx.progress,
            ScanPhase::Traversing,
            Some(path.to_path_buf()),
            1,
            1,
            metadata.len(),
        );
        return Ok(TreeStats {
            logical_size: metadata.len(),
            scanned_entries: 1,
            modified_at: metadata.modified().ok(),
        });
    }
    if !metadata.is_dir() {
        return Ok(TreeStats::default());
    }
    if !same_filesystem(
        ctx.root_device,
        device_id(&metadata),
        root.cross_filesystems,
    ) {
        warnings.push(ScanWarning {
            message: format!("Skipped mounted volume at {}", path.display()),
        });
        return Ok(TreeStats::default());
    }

    let mut stats = TreeStats::default();
    let mut queue = VecDeque::from([(path.to_path_buf(), 0usize)]);
    while let Some((current, depth)) = queue.pop_front() {
        if ctx.cancellation.is_cancelled() {
            return Err(ScanError::Cancelled);
        }
        let metadata = match fs::symlink_metadata(&current) {
            Ok(metadata) => metadata,
            Err(err) => {
                warnings.push(ScanWarning {
                    message: format!("Failed to inspect {}: {err}", current.display()),
                });
                continue;
            }
        };
        if metadata.file_type().is_symlink() {
            warnings.push(ScanWarning {
                message: format!("Skipped symlink {}", current.display()),
            });
            continue;
        }
        let modified = metadata.modified().ok();
        stats.modified_at = latest(stats.modified_at, modified);
        if metadata.is_file() {
            stats.scanned_entries += 1;
            stats.logical_size += metadata.len();
        } else if metadata.is_dir() {
            if root.max_depth.is_some_and(|max_depth| depth >= max_depth) {
                continue;
            }
            if !same_filesystem(
                ctx.root_device,
                device_id(&metadata),
                root.cross_filesystems,
            ) {
                warnings.push(ScanWarning {
                    message: format!("Skipped mounted volume at {}", current.display()),
                });
                continue;
            }
            let read_dir = match fs::read_dir(&current) {
                Ok(read_dir) => read_dir,
                Err(err) => {
                    warnings.push(ScanWarning {
                        message: format!("Failed to read {}: {err}", current.display()),
                    });
                    continue;
                }
            };
            for child in read_dir {
                let child = match child {
                    Ok(child) => child,
                    Err(err) => {
                        warnings.push(ScanWarning {
                            message: format!("Failed to enumerate {}: {err}", current.display()),
                        });
                        continue;
                    }
                };
                let child_path = child.path();
                if should_skip_name(child_path.as_path(), root.include_hidden) {
                    continue;
                }
                queue.push_back((child_path.clone(), depth + 1));
            }
        }
        reporter.report(
            ctx.progress,
            ScanPhase::Traversing,
            Some(current),
            stats.scanned_entries,
            0,
            stats.logical_size,
        );
    }
    let _ = ctx.category;
    Ok(stats)
}

fn latest(left: Option<SystemTime>, right: Option<SystemTime>) -> Option<SystemTime> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.max(right)),
        (Some(left), None) => Some(left),
        (None, Some(right)) => Some(right),
        (None, None) => None,
    }
}

fn should_skip_name(path: &Path, include_hidden: bool) -> bool {
    !include_hidden
        && path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with('.'))
}

fn same_filesystem(
    root_device: Option<u64>,
    current_device: Option<u64>,
    cross_filesystems: bool,
) -> bool {
    cross_filesystems
        || root_device.is_none()
        || current_device.is_none()
        || root_device == current_device
}

fn map_root_error(path: PathBuf, error: std::io::Error) -> ScanError {
    match error.kind() {
        std::io::ErrorKind::NotFound => ScanError::RootUnavailable(path),
        std::io::ErrorKind::PermissionDenied => ScanError::PermissionDenied(path),
        _ => ScanError::Io {
            path,
            source: error,
        },
    }
}

#[cfg(unix)]
fn device_id(metadata: &fs::Metadata) -> Option<u64> {
    use std::os::unix::fs::MetadataExt as _;

    Some(metadata.dev())
}

#[cfg(not(unix))]
fn device_id(_metadata: &fs::Metadata) -> Option<u64> {
    None
}

struct ProgressReporter {
    category: CleanerCategory,
    last_sent_at: Option<Instant>,
}

impl ProgressReporter {
    fn new(category: CleanerCategory) -> Self {
        Self {
            category,
            last_sent_at: None,
        }
    }

    fn report(
        &mut self,
        progress: &dyn ProgressSink,
        phase: ScanPhase,
        current_path: Option<PathBuf>,
        scanned_entries: u64,
        discovered_items: u64,
        discovered_bytes: u64,
    ) {
        let now = Instant::now();
        let should_send = self.last_sent_at.is_none_or(|last| {
            phase == ScanPhase::Completed || now.duration_since(last) >= PROGRESS_INTERVAL
        });
        if !should_send {
            return;
        }
        self.last_sent_at = Some(now);
        progress.report(ScanProgress {
            category: self.category,
            phase,
            current_path,
            scanned_entries,
            discovered_items,
            discovered_bytes,
        });
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::{Arc, Mutex};

    use crate::cleaner::core::cancellation::CancellationToken;
    use crate::cleaner::core::category::CleanerCategory;
    use crate::cleaner::core::fs::scan_root;
    use crate::cleaner::core::progress::{ProgressSink, ScanProgress};
    use crate::cleaner::core::risk::RiskLevel;
    use crate::cleaner::core::scan_root::{AggregateMode, ScanRoot};

    struct RecordingSink(Arc<Mutex<Vec<ScanProgress>>>);

    impl ProgressSink for RecordingSink {
        fn report(&self, progress: ScanProgress) {
            self.0.lock().expect("lock poisoned").push(progress);
        }
    }

    #[test]
    fn immediate_children_are_aggregated_without_nested_duplicates() {
        let temp = std::env::temp_dir().join(format!("dodo-cleaner-fs-{}", std::process::id()));
        let root = temp.join("Caches");
        fs::create_dir_all(root.join("AppA").join("nested")).expect("creates AppA");
        fs::create_dir_all(root.join("AppB")).expect("creates AppB");
        fs::write(
            root.join("AppA").join("nested").join("data.bin"),
            vec![0u8; 16],
        )
        .expect("writes AppA file");
        fs::write(root.join("AppB").join("cache.bin"), vec![0u8; 8]).expect("writes AppB file");

        let result = scan_root(
            &ScanRoot {
                path: root.clone(),
                max_depth: None,
                follow_symlinks: false,
                cross_filesystems: false,
                include_hidden: true,
                aggregate_mode: AggregateMode::ImmediateChildren,
                permission: None,
                risk: RiskLevel::SafeRecreatable,
            },
            CleanerCategory::UserCache,
            &RecordingSink(Arc::new(Mutex::new(Vec::new()))),
            &CancellationToken::new(),
        )
        .expect("scans");

        assert_eq!(result.entries.len(), 2);
        assert_eq!(result.scanned_entries, 2);

        fs::remove_dir_all(&temp).expect("removes temp tree");
    }
}
