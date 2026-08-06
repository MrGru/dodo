//! `CleanerCategory::XcodeJunk` (Phase 11): Xcode/CoreSimulator developer
//! caches under `~/Library/Developer` and `~/Library/org.swift.swiftpm`.
//!
//! Eight fixed roots, each tagged with a [`RootKind`] that drives risk,
//! selection policy and capabilities — see [`risk_for`], [`selection_policy_for`]
//! and [`capabilities_for`]. Only three roots are "normally recreatable" per
//! the ticket (DerivedData, the SwiftUI preview cache, and CoreSimulator's
//! own `Caches`); everything else is `ReviewRecommended`/not-selected and
//! never gets [`ItemCapability::MoveToTrash`] at all, because
//! `macos::cleanup::policy_for` never allow-lists it either — see
//! [`cleanup_allowed_roots`], which is the single list both this scanner's
//! "safe by default" set and cleanup's allow-list are built from, so the two
//! can never drift apart. `docs/cleaner/known-limitations.md` records why
//! Archives, iOS DeviceSupport, CoreSimulator Devices, XCTestDevices and the
//! SwiftPM cache all stay scan-only this phase.
//!
//! # Grouping DerivedData by project
//!
//! Xcode names each `DerivedData` subfolder `<ProjectName>-<hash>`. Each
//! [`CleanableItem::group`] built for a DerivedData entry strips a
//! plausible trailing `-<hash>` (see [`derived_data_project_name`]) so the UI
//! can group by project once it renders `group` — the ticket asks for this
//! explicitly ("Group DerivedData by project when possible").
//!
//! # Warning when Xcode is running
//!
//! `scan()` calls `platform::is_xcode_running()` once — a read-only
//! `NSRunningApplication` check, see `macos::platform::xcode` — and threads
//! the resulting `bool` into [`build_item`], which attaches an
//! [`ItemWarning`] to every DerivedData item when it is `true`. A category
//! level [`ScanWarning`] is also pushed so the warning is visible even before
//! per-item warnings are wired into the view. This warns rather than blocks:
//! the ticket does not ask Cleaner to refuse a DerivedData scan just because
//! Xcode happens to be open.
//!
//! # Full Disk Access
//!
//! None of these roots need it — `~/Library/Developer` is an ordinary user
//! location, unlike `~/Library/Mail` or `~/Library/Containers` — so
//! `required_permissions()` returns an empty slice, the same as most
//! scanners here.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use crate::cleaner::core::cancellation::CancellationToken;
use crate::cleaner::core::category::CleanerCategory;
use crate::cleaner::core::errors::ScanError;
use crate::cleaner::core::fs::{AggregatedEntry, scan_root};
use crate::cleaner::core::item::{CleanableItem, CleanableItemId, ItemMetadata, ItemWarning};
use crate::cleaner::core::permissions::MacPermission;
use crate::cleaner::core::progress::{ProgressSink, ScanPhase, ScanProgress};
use crate::cleaner::core::report::{
    CategoryScanResult, PartialScanReason, ScanCompleteness, ScanWarning,
};
use crate::cleaner::core::risk::{ItemCapability, RiskLevel, SelectionPolicy};
use crate::cleaner::core::scan_context::ScanContext;
use crate::cleaner::core::scan_root::{AggregateMode, ScanRoot};
use crate::cleaner::core::scanner::CleanerScanner;
use crate::cleaner::macos::platform;

const XCODE_RUNNING_ITEM_WARNING: &str =
    "Xcode is currently running; this project's DerivedData may be in active use.";
const XCODE_RUNNING_CATEGORY_WARNING: &str = "Xcode is currently running. DerivedData may be in active use — quit Xcode before cleaning it up.";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum RootKind {
    /// `~/Library/Developer/Xcode/DerivedData` — normally recreatable.
    DerivedData,
    /// `~/Library/Developer/Xcode/Archives` — review required, never
    /// auto-select; an archive may be needed for App Store resubmission.
    Archives,
    /// `~/Library/Developer/Xcode/iOS DeviceSupport` — review required;
    /// often reclaimable but not classified as junk.
    IosDeviceSupport,
    /// `~/Library/Developer/Xcode/UserData/Previews` — normally recreatable
    /// SwiftUI preview cache.
    Previews,
    /// `~/Library/Developer/CoreSimulator/Caches` — normally recreatable.
    SimulatorCaches,
    /// `~/Library/Developer/CoreSimulator/Devices` — review required; never
    /// delete directly while CoreSimulator is active, prefer `simctl`.
    SimulatorDevices,
    /// `~/Library/XCTestDevices` — review required, same simulator-adjacent
    /// caution as [`RootKind::SimulatorDevices`].
    XcTestDevices,
    /// `~/Library/org.swift.swiftpm` — review required; the on-disk layout
    /// has changed across Xcode versions, so this phase does not attempt to
    /// tell a recreatable cache apart from checked-out package sources.
    SwiftPmCache,
}

struct XcodeRoot {
    kind: RootKind,
    /// `~`-prefixed path, resolved against `ScanContext::user_home` the same
    /// way every other scanner here resolves its default roots.
    path: PathBuf,
}

pub struct XcodeJunkScanner {
    roots: Vec<XcodeRoot>,
}

impl XcodeJunkScanner {
    pub fn new() -> Self {
        Self {
            roots: default_roots(),
        }
    }

    #[cfg(test)]
    fn with_roots(roots: Vec<(RootKind, PathBuf)>) -> Self {
        Self {
            roots: roots
                .into_iter()
                .map(|(kind, path)| XcodeRoot { kind, path })
                .collect(),
        }
    }
}

/// The exact sub-paths this scanner treats as "normally recreatable" — the
/// only three `CleanerCategory::XcodeJunk` locations
/// `macos::cleanup::policy_for` allow-lists for deletion. Archives, iOS
/// DeviceSupport, CoreSimulator Devices, XCTestDevices and the SwiftPM cache
/// are deliberately absent, so cleanup rejects them with
/// `SafetyError::OutsideAllowedRoot` even if a future UI bug ever let one
/// through selected.
pub(crate) fn cleanup_allowed_roots(home: &Path) -> Vec<PathBuf> {
    vec![
        home.join("Library")
            .join("Developer")
            .join("Xcode")
            .join("DerivedData"),
        home.join("Library")
            .join("Developer")
            .join("Xcode")
            .join("UserData")
            .join("Previews"),
        home.join("Library")
            .join("Developer")
            .join("CoreSimulator")
            .join("Caches"),
    ]
}

impl CleanerScanner for XcodeJunkScanner {
    fn category(&self) -> CleanerCategory {
        CleanerCategory::XcodeJunk
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
            category: CleanerCategory::XcodeJunk,
            phase: ScanPhase::Preparing,
            current_path: None,
            scanned_entries: 0,
            discovered_items: 0,
            discovered_bytes: 0,
        });

        let xcode_running = platform::is_xcode_running();
        let mut items = Vec::new();
        let mut warnings = Vec::new();
        let mut scanned_entries = 0;
        let mut skipped_roots = Vec::new();
        let mut saw_derived_data_root = false;

        for root in resolve_roots(&self.roots, context.user_home.as_deref()) {
            if cancellation.is_cancelled() {
                return Err(ScanError::Cancelled);
            }
            if root.kind == RootKind::DerivedData {
                saw_derived_data_root = true;
            }
            let scan_spec = ScanRoot {
                path: root.path.clone(),
                max_depth: None,
                follow_symlinks: false,
                cross_filesystems: false,
                include_hidden: true,
                aggregate_mode: AggregateMode::ImmediateChildren,
                permission: None,
                risk: risk_for(root.kind),
            };
            match scan_root(
                &scan_spec,
                CleanerCategory::XcodeJunk,
                progress,
                cancellation,
            ) {
                Ok(result) => {
                    scanned_entries += result.scanned_entries;
                    warnings.extend(result.warnings);
                    for entry in result.entries {
                        if entry.logical_size == 0 {
                            continue;
                        }
                        items.push(build_item(root.kind, entry, xcode_running));
                    }
                }
                Err(ScanError::RootUnavailable(_)) => skipped_roots.push(root.path.clone()),
                Err(err @ ScanError::Cancelled) => return Err(err),
                Err(error) => warnings.push(ScanWarning {
                    message: format!("{}: {error:?}", root.path.display()),
                }),
            }
        }

        if xcode_running && saw_derived_data_root {
            warnings.push(ScanWarning {
                message: XCODE_RUNNING_CATEGORY_WARNING.into(),
            });
        }

        items.sort_by_key(|item| std::cmp::Reverse(item.logical_size));
        let estimated_reclaimable_bytes = items.iter().map(|item| item.logical_size).sum();
        Ok(CategoryScanResult {
            category: CleanerCategory::XcodeJunk,
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

fn default_roots() -> Vec<XcodeRoot> {
    vec![
        XcodeRoot {
            kind: RootKind::DerivedData,
            path: PathBuf::from("~/Library/Developer/Xcode/DerivedData"),
        },
        XcodeRoot {
            kind: RootKind::Archives,
            path: PathBuf::from("~/Library/Developer/Xcode/Archives"),
        },
        XcodeRoot {
            kind: RootKind::IosDeviceSupport,
            path: PathBuf::from("~/Library/Developer/Xcode/iOS DeviceSupport"),
        },
        XcodeRoot {
            kind: RootKind::Previews,
            path: PathBuf::from("~/Library/Developer/Xcode/UserData/Previews"),
        },
        XcodeRoot {
            kind: RootKind::SimulatorCaches,
            path: PathBuf::from("~/Library/Developer/CoreSimulator/Caches"),
        },
        XcodeRoot {
            kind: RootKind::SimulatorDevices,
            path: PathBuf::from("~/Library/Developer/CoreSimulator/Devices"),
        },
        XcodeRoot {
            kind: RootKind::XcTestDevices,
            path: PathBuf::from("~/Library/Developer/XCTestDevices"),
        },
        XcodeRoot {
            kind: RootKind::SwiftPmCache,
            path: PathBuf::from("~/Library/org.swift.swiftpm"),
        },
    ]
}

fn resolve_roots(roots: &[XcodeRoot], home: Option<&Path>) -> Vec<XcodeRoot> {
    roots
        .iter()
        .filter_map(|root| {
            let Some(path) = root.path.to_str() else {
                return Some(XcodeRoot {
                    kind: root.kind,
                    path: root.path.clone(),
                });
            };
            if !path.starts_with("~/") {
                return Some(XcodeRoot {
                    kind: root.kind,
                    path: root.path.clone(),
                });
            }
            let home = home?;
            Some(XcodeRoot {
                kind: root.kind,
                path: home.join(&path[2..]),
            })
        })
        .collect()
}

fn risk_for(kind: RootKind) -> RiskLevel {
    match kind {
        RootKind::DerivedData | RootKind::Previews | RootKind::SimulatorCaches => {
            RiskLevel::SafeRecreatable
        }
        RootKind::Archives
        | RootKind::IosDeviceSupport
        | RootKind::SimulatorDevices
        | RootKind::XcTestDevices
        | RootKind::SwiftPmCache => RiskLevel::ReviewRecommended,
    }
}

fn selection_policy_for(kind: RootKind) -> SelectionPolicy {
    match kind {
        RootKind::DerivedData | RootKind::Previews | RootKind::SimulatorCaches => {
            SelectionPolicy::SelectedByDefault
        }
        RootKind::Archives
        | RootKind::IosDeviceSupport
        | RootKind::SimulatorDevices
        | RootKind::XcTestDevices
        | RootKind::SwiftPmCache => SelectionPolicy::NotSelectedByDefault,
    }
}

/// Only the three "normally recreatable" kinds get
/// [`ItemCapability::MoveToTrash`] — matching [`cleanup_allowed_roots`]
/// exactly, so the UI never offers a Trash action `macos::cleanup::policy_for`
/// would reject anyway.
fn capabilities_for(kind: RootKind) -> Vec<ItemCapability> {
    match kind {
        RootKind::DerivedData | RootKind::Previews | RootKind::SimulatorCaches => vec![
            ItemCapability::MoveToTrash,
            ItemCapability::RevealInFinder,
            ItemCapability::CopyPath,
        ],
        RootKind::Archives
        | RootKind::IosDeviceSupport
        | RootKind::SimulatorDevices
        | RootKind::XcTestDevices
        | RootKind::SwiftPmCache => {
            vec![ItemCapability::RevealInFinder, ItemCapability::CopyPath]
        }
    }
}

fn explanation_for(kind: RootKind) -> &'static str {
    match kind {
        RootKind::DerivedData => {
            "Derived build data Xcode regenerates automatically on the next build."
        }
        RootKind::Archives => {
            "Xcode Archive build product. May be needed for App Store resubmission or crash \
             symbolication — review it in Xcode's Organizer before removing it manually."
        }
        RootKind::IosDeviceSupport => {
            "Debug symbol data for one iOS version. Often reclaimable if you no longer debug on \
             that OS version, but not classified as junk — review before removing."
        }
        RootKind::Previews => "SwiftUI preview cache Xcode regenerates automatically.",
        RootKind::SimulatorCaches => {
            "Simulator cache data CoreSimulator regenerates automatically."
        }
        RootKind::SimulatorDevices => {
            "Simulator device data. Do not delete this directly while CoreSimulator is active — \
             use `xcrun simctl delete` instead. Cleaner does not remove these yet."
        }
        RootKind::XcTestDevices => {
            "XCTest simulator device data; the same simulator-adjacent caution as CoreSimulator \
             Devices applies. Cleaner does not remove these yet."
        }
        RootKind::SwiftPmCache => {
            "Swift Package Manager data under ~/Library/org.swift.swiftpm. Its on-disk layout has \
             changed across Xcode versions, so this is treated as review-only rather than guessed \
             at as a safe-to-recreate cache."
        }
    }
}

fn group_for(kind: RootKind, entry_path: &Path) -> String {
    match kind {
        RootKind::DerivedData => derived_data_project_name(entry_path),
        RootKind::Archives => "Xcode Archives".to_string(),
        RootKind::IosDeviceSupport => "iOS Device Support".to_string(),
        RootKind::Previews => "SwiftUI Previews".to_string(),
        RootKind::SimulatorCaches => "Simulator Caches".to_string(),
        RootKind::SimulatorDevices => "Simulator Devices".to_string(),
        RootKind::XcTestDevices => "XCTest Devices".to_string(),
        RootKind::SwiftPmCache => "SwiftPM Cache".to_string(),
    }
}

/// Strips a plausible trailing `-<hash>` from a DerivedData subfolder name
/// (Xcode names them `<ProjectName>-<hash>`) so items from the same project
/// group together. `is_plausible_hash` intentionally stays loose (ASCII
/// alphanumeric, at least six characters) rather than matching Xcode's exact
/// hash alphabet: a false-positive strip just means a slightly shorter group
/// label, never a wrong path or a lost item.
fn derived_data_project_name(entry_path: &Path) -> String {
    let name = entry_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("Project");
    match name.rsplit_once('-') {
        Some((project, hash)) if !project.is_empty() && is_plausible_hash(hash) => {
            project.to_string()
        }
        _ => name.to_string(),
    }
}

fn is_plausible_hash(candidate: &str) -> bool {
    candidate.len() >= 6 && candidate.chars().all(|ch| ch.is_ascii_alphanumeric())
}

fn build_item(kind: RootKind, entry: AggregatedEntry, xcode_running: bool) -> CleanableItem {
    let mut warnings = Vec::new();
    if kind == RootKind::DerivedData && xcode_running {
        warnings.push(ItemWarning {
            message: XCODE_RUNNING_ITEM_WARNING.to_string(),
        });
    }
    CleanableItem {
        id: item_id(entry.path.as_path()),
        category: CleanerCategory::XcodeJunk,
        group: Some(group_for(kind, entry.path.as_path())),
        display_name: item_name(entry.path.as_path()),
        path: entry.path,
        logical_size: entry.logical_size,
        allocated_size: None,
        modified_at: entry.modified_at,
        last_accessed_at: None,
        risk: risk_for(kind),
        selection_policy: selection_policy_for(kind),
        capabilities: capabilities_for(kind),
        explanation: explanation_for(kind).to_string(),
        warnings,
        metadata: ItemMetadata::Generic,
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

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use crate::cleaner::core::cancellation::CancellationToken;
    use crate::cleaner::core::progress::{ProgressSink, ScanProgress};
    use crate::cleaner::core::risk::{ItemCapability, RiskLevel, SelectionPolicy};
    use crate::cleaner::core::scan_context::ScanContext;
    use crate::cleaner::core::scanner::CleanerScanner;
    use crate::cleaner::macos::scanners::xcode_junk::{RootKind, XcodeJunkScanner};

    struct RecordingSink;
    impl ProgressSink for RecordingSink {
        fn report(&self, _progress: ScanProgress) {}
    }

    fn context(home: PathBuf) -> ScanContext {
        ScanContext {
            started_at: std::time::SystemTime::now(),
            user_home: Some(home),
        }
    }

    #[test]
    fn derived_data_groups_by_project_and_defaults_selected() {
        use super::derived_data_project_name;
        assert_eq!(
            derived_data_project_name(std::path::Path::new("MyApp-fjbvqrxyzabcdefghijklmnop")),
            "MyApp"
        );
        assert_eq!(
            derived_data_project_name(std::path::Path::new("no-hash-here")),
            "no-hash-here"
        );
        assert_eq!(
            derived_data_project_name(std::path::Path::new("Solo")),
            "Solo"
        );
    }

    #[test]
    fn derived_data_scan_marks_items_safe_and_selected_by_default() {
        let temp =
            std::env::temp_dir().join(format!("dodo-cleaner-xcode-dd-{}", std::process::id()));
        let derived_data = temp
            .join("Library/Developer/Xcode/DerivedData")
            .join("MyApp-abcdefghijklmnop123456");
        fs::create_dir_all(&derived_data).expect("creates derived data dir");
        fs::write(derived_data.join("build.o"), vec![0u8; 32]).expect("writes build artifact");

        let scanner = XcodeJunkScanner::with_roots(vec![(
            RootKind::DerivedData,
            PathBuf::from("~/Library/Developer/Xcode/DerivedData"),
        )]);
        let result = scanner
            .scan(
                &context(temp.clone()),
                &RecordingSink,
                &CancellationToken::new(),
            )
            .expect("scans DerivedData");

        assert_eq!(result.items.len(), 1);
        let item = &result.items[0];
        assert_eq!(item.risk, RiskLevel::SafeRecreatable);
        assert_eq!(item.selection_policy, SelectionPolicy::SelectedByDefault);
        assert!(item.capabilities.contains(&ItemCapability::MoveToTrash));
        assert_eq!(item.group.as_deref(), Some("MyApp"));

        fs::remove_dir_all(&temp).expect("removes temp tree");
    }

    #[test]
    fn archives_are_review_only_and_never_get_move_to_trash() {
        let temp = std::env::temp_dir().join(format!(
            "dodo-cleaner-xcode-archives-{}",
            std::process::id()
        ));
        let dated = temp
            .join("Library/Developer/Xcode/Archives")
            .join("2024-01-15");
        fs::create_dir_all(&dated).expect("creates dated archive dir");
        fs::write(dated.join("App.xcarchive"), vec![0u8; 16]).expect("writes archive stub");

        let scanner = XcodeJunkScanner::with_roots(vec![(
            RootKind::Archives,
            PathBuf::from("~/Library/Developer/Xcode/Archives"),
        )]);
        let result = scanner
            .scan(
                &context(temp.clone()),
                &RecordingSink,
                &CancellationToken::new(),
            )
            .expect("scans Archives");

        assert_eq!(result.items.len(), 1);
        let item = &result.items[0];
        assert_eq!(item.risk, RiskLevel::ReviewRecommended);
        assert_eq!(item.selection_policy, SelectionPolicy::NotSelectedByDefault);
        assert!(!item.capabilities.contains(&ItemCapability::MoveToTrash));

        fs::remove_dir_all(&temp).expect("removes temp tree");
    }

    #[test]
    fn simulator_devices_are_never_move_to_trash_capable() {
        let temp = std::env::temp_dir().join(format!(
            "dodo-cleaner-xcode-simdevices-{}",
            std::process::id()
        ));
        let device = temp
            .join("Library/Developer/CoreSimulator/Devices")
            .join("00000000-0000-0000-0000-000000000000");
        fs::create_dir_all(&device).expect("creates device dir");
        fs::write(device.join("device.plist"), vec![0u8; 8]).expect("writes device plist");

        let scanner = XcodeJunkScanner::with_roots(vec![(
            RootKind::SimulatorDevices,
            PathBuf::from("~/Library/Developer/CoreSimulator/Devices"),
        )]);
        let result = scanner
            .scan(
                &context(temp.clone()),
                &RecordingSink,
                &CancellationToken::new(),
            )
            .expect("scans CoreSimulator Devices");

        assert_eq!(result.items.len(), 1);
        assert!(
            !result.items[0]
                .capabilities
                .contains(&ItemCapability::MoveToTrash)
        );

        fs::remove_dir_all(&temp).expect("removes temp tree");
    }

    #[test]
    fn cleanup_allowed_roots_cover_exactly_the_three_recreatable_kinds() {
        use super::cleanup_allowed_roots;

        let home = std::path::Path::new("/Users/example");
        let roots = cleanup_allowed_roots(home);
        assert_eq!(roots.len(), 3);
        assert!(roots.iter().any(|root| root.ends_with("DerivedData")));
        assert!(roots.iter().any(|root| root.ends_with("Previews")));
        assert!(
            roots
                .iter()
                .any(|root| root.ends_with("CoreSimulator/Caches"))
        );
    }
}
