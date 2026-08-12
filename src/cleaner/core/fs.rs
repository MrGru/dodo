use std::collections::{HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use crate::cleaner::core::cancellation::CancellationToken;
use crate::cleaner::core::category::CleanerCategory;
use crate::cleaner::core::errors::ScanError;
use crate::cleaner::core::progress::{ProgressSink, ScanPhase, ScanProgress};
use crate::cleaner::core::report::ScanWarning;
use crate::cleaner::core::risk::RiskLevel;
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

struct ReadDirFrame {
    path: PathBuf,
    depth: usize,
    entries: fs::ReadDir,
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

/// Streams `EveryFile` roots with a metadata predicate, retaining only
/// matching files. `scan_root` remains the unfiltered path for callers that
/// need every file.
pub(crate) fn scan_matching_files(
    root: &ScanRoot,
    category: CleanerCategory,
    progress: &dyn ProgressSink,
    cancellation: &CancellationToken,
    matches: impl Fn(u64, Option<SystemTime>) -> bool,
) -> Result<RootScanResult, ScanError> {
    scan_matching_files_with_frame_observer(root, category, progress, cancellation, matches, |_| {})
}

fn scan_matching_files_with_frame_observer(
    root: &ScanRoot,
    category: CleanerCategory,
    progress: &dyn ProgressSink,
    cancellation: &CancellationToken,
    matches: impl Fn(u64, Option<SystemTime>) -> bool,
    mut observe_frame_count: impl FnMut(usize),
) -> Result<RootScanResult, ScanError> {
    let metadata =
        fs::symlink_metadata(&root.path).map_err(|err| map_root_error(root.path.clone(), err))?;
    if metadata.file_type().is_symlink() {
        return Err(ScanError::InvalidMetadata(root.path.clone()));
    }

    let mut reporter = ProgressReporter::new(category);
    reporter.report(progress, ScanPhase::DiscoveringRoots, None, 0, 0, 0);

    let ctx = TraversalCtx {
        root,
        root_device: device_id(&metadata),
        category,
        progress,
        cancellation,
    };
    let mut warnings = Vec::new();
    let mut scanned_entries = 0;
    let mut discovered_bytes = 0;
    let entries = scan_matching_every_file(
        &ctx,
        &mut reporter,
        &mut scanned_entries,
        &mut discovered_bytes,
        &mut warnings,
        &matches,
        &mut observe_frame_count,
    )?;

    reporter.report(
        progress,
        ScanPhase::Completed,
        Some(root.path.clone()),
        scanned_entries,
        scanned_entries,
        discovered_bytes,
    );

    Ok(RootScanResult {
        root: root.path.clone(),
        entries,
        scanned_entries,
        warnings,
    })
}

fn scan_matching_every_file<M, O>(
    ctx: &TraversalCtx,
    reporter: &mut ProgressReporter,
    scanned_entries: &mut u64,
    discovered_bytes: &mut u64,
    warnings: &mut Vec<ScanWarning>,
    matches: &M,
    observe_frame_count: &mut O,
) -> Result<Vec<AggregatedEntry>, ScanError>
where
    M: Fn(u64, Option<SystemTime>) -> bool,
    O: FnMut(usize),
{
    let root = ctx.root;
    let mut entries = Vec::new();
    let mut seen_inodes = HashSet::new();
    let mut frames: Vec<ReadDirFrame> = Vec::new();
    let mut next_path = Some((root.path.clone(), 0usize));

    loop {
        if ctx.cancellation.is_cancelled() {
            return Err(ScanError::Cancelled);
        }

        let (path, depth) = if let Some(path) = next_path.take() {
            path
        } else {
            let (depth, child) = match frames.last_mut() {
                Some(frame) => (frame.depth + 1, frame.entries.next()),
                None => break,
            };
            match child {
                Some(Ok(child)) => {
                    let path = child.path();
                    if should_skip_name(path.as_path(), root.include_hidden) {
                        continue;
                    }
                    (path, depth)
                }
                Some(Err(err)) => {
                    let path = &frames.last().expect("frame remains while reading").path;
                    warnings.push(ScanWarning {
                        message: format!("Failed to enumerate {}: {err}", path.display()),
                    });
                    continue;
                }
                None => {
                    frames.pop();
                    continue;
                }
            }
        };

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
            if let Some(identity) = hard_link_identity(&metadata)
                && !seen_inodes.insert(identity)
            {
                continue;
            }
            *scanned_entries += 1;
            *discovered_bytes += metadata.len();
            let modified_at = metadata.modified().ok();
            reporter.report(
                ctx.progress,
                ScanPhase::Traversing,
                Some(path.clone()),
                *scanned_entries,
                *scanned_entries - 1,
                *discovered_bytes,
            );
            if matches(metadata.len(), modified_at) {
                entries.push(AggregatedEntry {
                    path,
                    logical_size: metadata.len(),
                    modified_at,
                    scanned_entries: 1,
                });
            }
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
            let entries_in_directory = match fs::read_dir(&path) {
                Ok(entries) => entries,
                Err(err) => {
                    warnings.push(ScanWarning {
                        message: format!("Failed to read {}: {err}", path.display()),
                    });
                    continue;
                }
            };
            frames.push(ReadDirFrame {
                path,
                depth,
                entries: entries_in_directory,
            });
            observe_frame_count(frames.len());
        }
    }
    let _ = ctx.category;
    Ok(entries)
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
    let mut seen_inodes = HashSet::new();
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
            if let Some(identity) = hard_link_identity(&metadata)
                && !seen_inodes.insert(identity)
            {
                // Another path already accounted for this exact inode —
                // skip counting the same on-disk content twice.
                continue;
            }
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
    let mut seen_inodes = HashSet::new();
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
            if let Some(identity) = hard_link_identity(&metadata)
                && !seen_inodes.insert(identity)
            {
                continue;
            }
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

/// Best-effort size measurement for a single path, reusing this module's own
/// bounded/aggregated traversal rather than a second implementation. Returns
/// `0` on any error (missing path, permission denied, cancelled) — every
/// caller already labels a size produced this way as *estimated*.
///
/// Shared by the Phase 9 uninstall review workflow
/// (`macos::applications::review`) and Phase 10 orphan detection
/// (`macos::scanners::orphaned_files`): both need the size of one already-
/// identified path outside the category's own main traversal, and neither
/// needs progress reporting or cancellation for a single best-effort call.
pub fn measure_size(path: &Path, category: CleanerCategory, risk: RiskLevel) -> u64 {
    struct NoopProgress;
    impl ProgressSink for NoopProgress {
        fn report(&self, _progress: ScanProgress) {}
    }

    let root = ScanRoot {
        path: path.to_path_buf(),
        max_depth: None,
        follow_symlinks: false,
        cross_filesystems: false,
        include_hidden: true,
        aggregate_mode: AggregateMode::WholeRoot,
        permission: None,
        risk,
    };
    scan_root(&root, category, &NoopProgress, &CancellationToken::new())
        .ok()
        .and_then(|result| result.entries.first().map(|entry| entry.logical_size))
        .unwrap_or(0)
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

/// `Some((device, inode))` only when the entry actually has more than one
/// hard link — anything with exactly one link has nothing to dedupe against,
/// so it never enters `seen_inodes` at all, keeping that set limited to
/// entries that can actually collide. `None` on a platform (or filesystem)
/// where this cannot be determined; callers then simply never dedupe that
/// entry, the same "count once when we can, never guess" posture the rest of
/// this module uses for [`device_id`].
#[cfg(unix)]
fn hard_link_identity(metadata: &fs::Metadata) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt as _;

    if metadata.nlink() > 1 {
        Some((metadata.dev(), metadata.ino()))
    } else {
        None
    }
}

#[cfg(not(unix))]
fn hard_link_identity(_metadata: &fs::Metadata) -> Option<(u64, u64)> {
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
    use crate::cleaner::core::errors::ScanError;
    use crate::cleaner::core::fs::scan_root;
    use crate::cleaner::core::progress::{ProgressSink, ScanPhase, ScanProgress};
    use crate::cleaner::core::risk::RiskLevel;
    use crate::cleaner::core::scan_root::{AggregateMode, ScanRoot};

    use super::{same_filesystem, scan_matching_files, scan_matching_files_with_frame_observer};

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

    fn whole_root(path: std::path::PathBuf) -> ScanRoot {
        ScanRoot {
            path,
            max_depth: None,
            follow_symlinks: false,
            cross_filesystems: false,
            include_hidden: true,
            aggregate_mode: AggregateMode::WholeRoot,
            permission: None,
            risk: RiskLevel::SafeRecreatable,
        }
    }

    fn every_file_root(path: std::path::PathBuf) -> ScanRoot {
        ScanRoot {
            aggregate_mode: AggregateMode::EveryFile,
            ..whole_root(path)
        }
    }

    #[test]
    fn matching_files_streams_a_wide_tree_without_retaining_fresh_files() {
        const FRESH_FILE_COUNT: usize = 512;
        const CANDIDATE_SIZE: u64 = 101 * 1024 * 1024;

        let temp = std::env::temp_dir().join(format!(
            "dodo-cleaner-fs-matching-{}-{}",
            std::process::id(),
            line!()
        ));
        let wide = temp.join("wide");
        fs::create_dir_all(&wide).expect("creates wide directory");
        for index in 0..FRESH_FILE_COUNT {
            fs::write(wide.join(format!("fresh-{index}")), b"x").expect("writes fresh file");
        }
        let candidate = temp.join("sparse-large.bin");
        fs::File::create(&candidate)
            .and_then(|file| file.set_len(CANDIDATE_SIZE))
            .expect("creates sparse candidate");
        let root = every_file_root(temp.clone());

        let generic = scan_root(
            &root,
            CleanerCategory::LargeOldFiles,
            &RecordingSink(Arc::new(Mutex::new(Vec::new()))),
            &CancellationToken::new(),
        )
        .expect("generic scan still retains every file");
        assert_eq!(generic.scanned_entries, (FRESH_FILE_COUNT + 1) as u64);
        assert_eq!(generic.entries.len(), FRESH_FILE_COUNT + 1);

        let progress = Arc::new(Mutex::new(Vec::new()));
        let mut max_frame_count = 0;
        let matching = scan_matching_files_with_frame_observer(
            &root,
            CleanerCategory::LargeOldFiles,
            &RecordingSink(progress.clone()),
            &CancellationToken::new(),
            |size, _| size >= CANDIDATE_SIZE,
            |frame_count| max_frame_count = max_frame_count.max(frame_count),
        )
        .expect("streams matching files");

        assert_eq!(matching.scanned_entries, (FRESH_FILE_COUNT + 1) as u64);
        assert_eq!(matching.entries.len(), 1);
        assert_eq!(matching.entries[0].path, candidate);
        assert_eq!(max_frame_count, 2, "one frame per directory depth");
        let completed = progress
            .lock()
            .expect("lock poisoned")
            .iter()
            .find(|event| event.phase == ScanPhase::Completed)
            .cloned()
            .expect("reports completion");
        assert_eq!(completed.scanned_entries, (FRESH_FILE_COUNT + 1) as u64);
        assert_eq!(completed.discovered_items, (FRESH_FILE_COUNT + 1) as u64);
        assert_eq!(
            completed.discovered_bytes,
            CANDIDATE_SIZE + FRESH_FILE_COUNT as u64
        );

        let cancellation = CancellationToken::new();
        cancellation.cancel();
        assert!(matches!(
            scan_matching_files(
                &root,
                CleanerCategory::LargeOldFiles,
                &RecordingSink(Arc::new(Mutex::new(Vec::new()))),
                &cancellation,
                |size, _| size >= CANDIDATE_SIZE,
            ),
            Err(ScanError::Cancelled)
        ));

        fs::remove_dir_all(&temp).expect("removes temp tree");
    }

    #[test]
    #[cfg(unix)]
    fn a_hard_linked_file_is_only_counted_once() {
        let temp =
            std::env::temp_dir().join(format!("dodo-cleaner-fs-hardlink-{}", std::process::id()));
        fs::create_dir_all(&temp).expect("creates root");
        let original = temp.join("original.bin");
        fs::write(&original, vec![0u8; 100]).expect("writes original");
        std::fs::hard_link(&original, temp.join("linked.bin")).expect("creates a hard link");
        // An ordinary, non-linked file must still be counted normally.
        fs::write(temp.join("unrelated.bin"), vec![0u8; 50]).expect("writes unrelated file");

        let result = scan_root(
            &whole_root(temp.clone()),
            CleanerCategory::UserCache,
            &RecordingSink(Arc::new(Mutex::new(Vec::new()))),
            &CancellationToken::new(),
        )
        .expect("scans");

        assert_eq!(result.entries.len(), 1);
        assert_eq!(result.entries[0].logical_size, 150);
        assert_eq!(result.scanned_entries, 2);

        let matching = scan_matching_files(
            &every_file_root(temp.clone()),
            CleanerCategory::UserCache,
            &RecordingSink(Arc::new(Mutex::new(Vec::new()))),
            &CancellationToken::new(),
            |_, _| true,
        )
        .expect("scans matching files");
        assert_eq!(matching.entries.len(), 2);
        assert_eq!(matching.scanned_entries, 2);
        assert_eq!(
            matching
                .entries
                .iter()
                .map(|entry| entry.logical_size)
                .sum::<u64>(),
            150
        );

        fs::remove_dir_all(&temp).expect("removes temp tree");
    }

    #[test]
    fn unicode_and_space_containing_names_are_scanned_like_any_other() {
        let temp =
            std::env::temp_dir().join(format!("dodo-cleaner-fs-unicode-{}", std::process::id()));
        fs::create_dir_all(&temp).expect("creates root");
        fs::write(temp.join("café ☕ résumé.txt"), vec![0u8; 12]).expect("writes unicode file");

        let result = scan_root(
            &whole_root(temp.clone()),
            CleanerCategory::UserCache,
            &RecordingSink(Arc::new(Mutex::new(Vec::new()))),
            &CancellationToken::new(),
        )
        .expect("scans a unicode filename without error");

        assert_eq!(result.entries[0].logical_size, 12);
        fs::remove_dir_all(&temp).expect("removes temp tree");
    }

    #[test]
    #[cfg(unix)]
    fn newline_containing_names_are_scanned_like_any_other() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt as _;

        let temp =
            std::env::temp_dir().join(format!("dodo-cleaner-fs-newline-{}", std::process::id()));
        fs::create_dir_all(&temp).expect("creates root");
        let name = OsStr::from_bytes(b"weird\nname.txt");
        fs::write(temp.join(name), vec![0u8; 7]).expect("writes newline-named file");

        let result = scan_root(
            &whole_root(temp.clone()),
            CleanerCategory::UserCache,
            &RecordingSink(Arc::new(Mutex::new(Vec::new()))),
            &CancellationToken::new(),
        )
        .expect("scans a newline-containing filename without error");

        assert_eq!(result.entries[0].logical_size, 7);
        fs::remove_dir_all(&temp).expect("removes temp tree");
    }

    #[test]
    fn same_filesystem_allows_crossing_only_when_explicitly_enabled() {
        // No device information (as on a platform where `device_id` cannot
        // determine one) must never block traversal by itself.
        assert!(same_filesystem(None, Some(1), false));
        assert!(same_filesystem(Some(1), None, false));
        // Same device: always fine, regardless of the flag.
        assert!(same_filesystem(Some(1), Some(1), false));
        // Different device: blocked unless `cross_filesystems` opts in.
        assert!(!same_filesystem(Some(1), Some(2), false));
        assert!(same_filesystem(Some(1), Some(2), true));
    }
}
