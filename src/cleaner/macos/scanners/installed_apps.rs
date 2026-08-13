use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use crate::cleaner::core::cancellation::CancellationToken;
use crate::cleaner::core::category::CleanerCategory;
use crate::cleaner::core::errors::ScanError;
use crate::cleaner::core::fs::measure_size;
use crate::cleaner::core::item::{
    ApplicationMetadata, CleanableItem, CleanableItemId, ItemMetadata,
};
use crate::cleaner::core::permissions::MacPermission;
use crate::cleaner::core::progress::{ProgressSink, ScanPhase, ScanProgress};
use crate::cleaner::core::report::{
    CategoryScanResult, PartialScanReason, ScanCompleteness, ScanWarning,
};
use crate::cleaner::core::risk::{ItemCapability, RiskLevel, SelectionPolicy};
use crate::cleaner::core::scan_context::ScanContext;
use crate::cleaner::core::scanner::CleanerScanner;
use crate::cleaner::macos::applications::bundle::parse_bundle;
use crate::cleaner::macos::applications::identity::AppIdentity;
use crate::cleaner::macos::platform::application_icon;

/// The standard top-level application roots this scanner indexes, and the
/// same roots Phase 10's [`installed_app_identities`] builds its
/// installed-app index from — one list, so the two never drift apart on
/// where "installed" means.
const DEFAULT_APP_ROOTS: &[&str] = &[
    "/Applications",
    "~/Applications",
    "/System/Applications",
    "/System/Applications/Utilities",
];

pub struct InstalledAppsScanner {
    roots: Vec<PathBuf>,
}

impl InstalledAppsScanner {
    pub fn new() -> Self {
        Self {
            roots: DEFAULT_APP_ROOTS.iter().map(PathBuf::from).collect(),
        }
    }

    #[cfg(test)]
    fn with_roots(roots: Vec<PathBuf>) -> Self {
        Self { roots }
    }
}

/// Builds the [`AppIdentity`] index Phase 10's orphan detector needs, using
/// the same fixed root list and `Info.plist` parsing this scanner uses to
/// list installed apps.
///
/// A dedicated function rather than routing through a full
/// [`InstalledAppsScanner::scan`] and re-deriving identities from its
/// `CleanableItem`s: orphan detection runs as its own category (see
/// `crate::cleaner::macos::scanners::orphaned_files`) and needs nothing from
/// a `CleanableItem` — no id, no risk classification, no capabilities — that
/// this does not already produce more directly. `team_id` is always `None`
/// here for the same reason it is in the scanner itself: reading it needs
/// code-signing inspection this phase does not add (see
/// `docs/cleaner/known-limitations.md`).
pub fn installed_app_identities(home: Option<&Path>) -> Vec<AppIdentity> {
    let roots = resolve_roots(
        &DEFAULT_APP_ROOTS
            .iter()
            .map(PathBuf::from)
            .collect::<Vec<_>>(),
        home,
    );
    identities_from_roots(&roots)
}

/// Same work [`installed_app_identities`] does, but over an explicit root
/// list rather than [`DEFAULT_APP_ROOTS`] resolved against `home`.
///
/// `pub(crate)` — not `#[cfg(test)]` — because
/// `crate::cleaner::macos::scanners::orphaned_files`'s own tests need to
/// build an index from a temporary directory too, the same reason
/// [`InstalledAppsScanner::with_roots`] exists: a test must never depend on
/// the real `/Applications`, `~/Applications` or `/System/Applications`.
pub(crate) fn identities_from_roots(roots: &[PathBuf]) -> Vec<AppIdentity> {
    let mut identities = Vec::new();
    for root in roots {
        let Ok(read_dir) = fs::read_dir(root) else {
            continue;
        };
        for entry in read_dir.flatten() {
            let path = entry.path();
            if !path.extension().is_some_and(|extension| extension == "app") {
                continue;
            }
            if let Ok(bundle) = parse_bundle(path.as_path()) {
                identities.push(AppIdentity::new(
                    bundle.bundle_id.as_deref(),
                    &bundle.display_name,
                    None,
                ));
            }
        }
    }
    identities
}

impl CleanerScanner for InstalledAppsScanner {
    fn category(&self) -> CleanerCategory {
        CleanerCategory::InstalledApps
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
            category: CleanerCategory::InstalledApps,
            phase: ScanPhase::Preparing,
            current_path: None,
            scanned_entries: 0,
            discovered_items: 0,
            discovered_bytes: 0,
        });

        let roots = resolve_roots(&self.roots, context.user_home.as_deref());
        let mut items = Vec::new();
        let mut warnings = Vec::new();
        let mut skipped_roots = Vec::new();
        let mut scanned_entries = 0;
        // Carried rather than re-summed. The report below runs once per
        // bundle, and `items.iter().map(..).sum()` there made the whole scan
        // quadratic in the number of applications for a number that only ever
        // grows by one item at a time.
        let mut discovered_bytes = 0u64;

        for root in roots {
            if cancellation.is_cancelled() {
                return Err(ScanError::Cancelled);
            }
            let read_dir = match fs::read_dir(&root) {
                Ok(read_dir) => read_dir,
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                    skipped_roots.push(root.clone());
                    continue;
                }
                Err(err) => {
                    warnings.push(ScanWarning {
                        message: format!("{}: {err}", root.display()),
                    });
                    skipped_roots.push(root.clone());
                    continue;
                }
            };
            for entry in read_dir.flatten() {
                if cancellation.is_cancelled() {
                    return Err(ScanError::Cancelled);
                }
                let path = entry.path();
                if !path.extension().is_some_and(|extension| extension == "app") {
                    continue;
                }
                scanned_entries += 1;
                progress.report(ScanProgress {
                    category: CleanerCategory::InstalledApps,
                    phase: ScanPhase::Classifying,
                    current_path: Some(path.clone()),
                    scanned_entries,
                    discovered_items: items.len() as u64,
                    discovered_bytes,
                });
                match parse_bundle(path.as_path()) {
                    Ok(bundle) => {
                        let risk = if bundle.is_system_app {
                            RiskLevel::Protected
                        } else {
                            RiskLevel::ReviewRecommended
                        };
                        // The bundle's own on-disk size — a directory walk,
                        // not free, so it runs once per app here rather than
                        // being left at the round-1 placeholder of `0`. Shares
                        // `core::fs::measure_size` with the uninstall review
                        // workflow and orphan detection rather than a third
                        // copy of the same best-effort traversal.
                        let logical_size =
                            measure_size(path.as_path(), CleanerCategory::InstalledApps, risk);
                        let icon = application_icon(path.as_path());
                        discovered_bytes += logical_size;
                        items.push(CleanableItem {
                            id: item_id(path.as_path()),
                            category: CleanerCategory::InstalledApps,
                            group: Some(root_label(root.as_path())),
                            display_name: bundle.display_name,
                            path,
                            logical_size,
                            allocated_size: None,
                            modified_at: bundle.modified_at,
                            last_accessed_at: None,
                            risk,
                            selection_policy: SelectionPolicy::NeverBulkSelect,
                            capabilities: if bundle.is_system_app {
                                // "System apps must never be uninstallable" —
                                // no `UninstallApplication` capability at
                                // all, so the review view has nothing to
                                // gate a button on rather than needing to
                                // remember to check risk.
                                vec![ItemCapability::RevealInFinder, ItemCapability::CopyPath]
                            } else {
                                vec![
                                    ItemCapability::RevealInFinder,
                                    ItemCapability::CopyPath,
                                    ItemCapability::UninstallApplication,
                                ]
                            },
                            explanation: bundle.explanation,
                            warnings: Vec::new(),
                            metadata: ItemMetadata::Application(ApplicationMetadata {
                                bundle_id: bundle.bundle_id,
                                team_id: None,
                                version: bundle.version,
                                executable: bundle.executable,
                                icon,
                            }),
                        });
                    }
                    Err(message) => warnings.push(ScanWarning {
                        message: format!("{}: {message}", path.display()),
                    }),
                }
            }
        }

        items.sort_by(|left, right| left.display_name.cmp(&right.display_name));
        Ok(CategoryScanResult {
            category: CleanerCategory::InstalledApps,
            items,
            scanned_entries,
            estimated_reclaimable_bytes: 0,
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

fn resolve_roots(roots: &[PathBuf], home: Option<&Path>) -> Vec<PathBuf> {
    roots
        .iter()
        .filter_map(|root| {
            let Some(path) = root.to_str() else {
                return Some(root.clone());
            };
            if !path.starts_with("~/") {
                return Some(root.clone());
            }
            home.map(|home| home.join(&path[2..]))
        })
        .collect()
}

fn root_label(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("Applications")
        .to_string()
}

fn item_id(path: &Path) -> CleanableItemId {
    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    CleanableItemId(hasher.finish())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use crate::cleaner::core::cancellation::CancellationToken;
    use crate::cleaner::core::progress::{ProgressSink, ScanProgress};
    use crate::cleaner::core::scan_context::ScanContext;
    use crate::cleaner::core::scanner::CleanerScanner;
    use crate::cleaner::macos::scanners::installed_apps::InstalledAppsScanner;

    struct RecordingSink;
    impl ProgressSink for RecordingSink {
        fn report(&self, _progress: ScanProgress) {}
    }

    #[test]
    fn scanner_reads_basic_app_metadata() {
        let temp = std::env::temp_dir().join(format!(
            "dodo-cleaner-installed-apps-{}",
            std::process::id()
        ));
        let apps = temp.join("Applications");
        let app = apps.join("Example.app").join("Contents");
        fs::create_dir_all(&app).expect("creates app bundle");
        fs::write(
            app.join("Info.plist"),
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>com.example.app</string>
<key>CFBundleName</key><string>Example</string>
<key>CFBundleShortVersionString</key><string>1.2.3</string>
<key>CFBundleExecutable</key><string>Example</string>
</dict></plist>"#,
        )
        .expect("writes plist");

        let scanner = InstalledAppsScanner::with_roots(vec![apps.clone()]);
        let result = scanner
            .scan(
                &ScanContext {
                    started_at: std::time::SystemTime::now(),
                    user_home: Some(temp.clone()),
                },
                &RecordingSink,
                &CancellationToken::new(),
            )
            .expect("scans installed apps");

        assert_eq!(result.items.len(), 1);
        assert_eq!(result.items[0].display_name, "Example");
        fs::remove_dir_all(&temp).expect("removes temp tree");
    }

    #[test]
    fn identities_from_roots_builds_an_identity_per_app_bundle() {
        use super::identities_from_roots;

        let temp = std::env::temp_dir().join(format!(
            "dodo-cleaner-installed-apps-identities-{}",
            std::process::id()
        ));
        let app = temp.join("Example.app").join("Contents");
        fs::create_dir_all(&app).expect("creates app bundle");
        fs::write(
            app.join("Info.plist"),
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>com.example.app</string>
<key>CFBundleName</key><string>Example</string>
</dict></plist>"#,
        )
        .expect("writes plist");

        let identities = identities_from_roots(std::slice::from_ref(&temp));
        assert_eq!(identities.len(), 1);
        assert_eq!(identities[0].bundle_id.as_deref(), Some("com.example.app"));

        fs::remove_dir_all(&temp).expect("removes temp tree");
    }

    #[test]
    fn identities_from_roots_ignores_a_missing_root_rather_than_erroring() {
        use super::identities_from_roots;

        let missing = std::env::temp_dir().join(format!(
            "dodo-cleaner-installed-apps-missing-{}",
            std::process::id()
        ));
        assert_eq!(identities_from_roots(&[missing]), Vec::new());
    }
}
