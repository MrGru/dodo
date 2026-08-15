//! Shared AI Apps scanner for macOS, Windows and Linux.
//!
//! The original role policy was cross-platform, but the rest of its seam was
//! quietly macOS-shaped: static `~/Library` strings, bundle identifiers in the
//! core definition, and a direct `NSRunningApplication` call in the scanner.
//! The scanner now consumes resolved [`AiAppLocation`] values and one injected
//! activity function. Host path tables remain isolated under [`definitions`].
//!
//! Models, chats and settings are never cleanup roots. Cleanable directories
//! produce immediate-child items; exact log files produce one exact item; and
//! user-data directories remain one scan-only summary. Running or unknown app
//! activity suppresses default selection, while the existing deletion boundary
//! still validates every manual cleanup immediately before Trash.

mod definitions;

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use crate::ai_apps::definitions::{AiAppEnvironment, default_ai_apps, snapshot_environment};
use crate::core::ai_app_provider::{
    AiAppActivity, AiAppDefinition, AiAppLocation, AiAppRole, AiAppTarget,
};
use crate::core::cancellation::CancellationToken;
use crate::core::category::CleanerCategory;
use crate::core::errors::ScanError;
use crate::core::fs::{AggregatedEntry, scan_root};
use crate::core::item::{AiAppMetadata, CleanableItem, CleanableItemId, ItemMetadata, ItemWarning};
use crate::core::permissions::MacPermission;
use crate::core::progress::{ProgressSink, ScanPhase, ScanProgress};
use crate::core::report::{CategoryScanResult, PartialScanReason, ScanCompleteness, ScanWarning};
use crate::core::risk::{ItemCapability, SelectionPolicy};
use crate::core::scan_context::ScanContext;
use crate::core::scan_root::{AggregateMode, ScanRoot};
use crate::core::scanner::CleanerScanner;
use crate::paths::HostOs;

pub(crate) type AiAppActivityProbe = fn(&[&str]) -> AiAppActivity;

pub struct AiAppsScanner {
    host: HostOs,
    activity_probe: AiAppActivityProbe,
    #[cfg(test)]
    apps: Option<Vec<AiAppDefinition>>,
}

impl AiAppsScanner {
    pub(crate) fn new(host: HostOs, activity_probe: AiAppActivityProbe) -> Self {
        Self {
            host,
            activity_probe,
            #[cfg(test)]
            apps: None,
        }
    }

    #[cfg(test)]
    fn with_apps(apps: Vec<AiAppDefinition>, activity_probe: AiAppActivityProbe) -> Self {
        Self {
            host: HostOs::MacOs,
            activity_probe,
            apps: Some(apps),
        }
    }
}

pub(crate) fn environment(host: HostOs, home: Option<&Path>) -> AiAppEnvironment {
    snapshot_environment(host, home)
}

/// Scanner-derived cleanup boundaries for the existing deletion policy.
/// Exact files authorize only their parent directory; directory summaries are
/// always scan-only, even if a future definition accidentally gives one a
/// cleanable role.
pub(crate) fn cleanup_allowed_roots(environment: &AiAppEnvironment) -> Vec<PathBuf> {
    default_ai_apps(environment)
        .into_iter()
        .flat_map(|app| app.locations)
        .filter(|location| location.role.allow_cleanup())
        .filter_map(|location| match location.target {
            AiAppTarget::ExactFile => location.path.parent().map(Path::to_path_buf),
            AiAppTarget::DirectoryContents => Some(location.path),
            AiAppTarget::DirectorySummary => None,
        })
        .collect()
}

impl CleanerScanner for AiAppsScanner {
    fn category(&self) -> CleanerCategory {
        CleanerCategory::AiApps
    }

    fn required_permissions(&self) -> &[MacPermission] {
        &[]
    }

    fn scan(
        &self,
        context: &ScanContext,
        progress: &dyn ProgressSink,
        cancellation: &CancellationToken,
    ) -> Result<CategoryScanResult, ScanError> {
        progress.report(ScanProgress {
            category: CleanerCategory::AiApps,
            phase: ScanPhase::Preparing,
            current_path: None,
            scanned_entries: 0,
            discovered_items: 0,
            discovered_bytes: 0,
        });

        let environment = snapshot_environment(self.host, context.user_home.as_deref());
        #[cfg(test)]
        let apps = self
            .apps
            .clone()
            .unwrap_or_else(|| default_ai_apps(&environment));
        #[cfg(not(test))]
        let apps = default_ai_apps(&environment);

        let mut items = Vec::new();
        let mut warnings = Vec::new();
        let mut skipped_roots = Vec::new();
        let mut scanned_entries = 0;

        for app in &apps {
            let activity = (self.activity_probe)(app.activity_ids);
            let mut saw_any_location = false;

            for location in &app.locations {
                if cancellation.is_cancelled() {
                    return Err(ScanError::Cancelled);
                }
                let scan_spec = ScanRoot {
                    path: location.path.clone(),
                    max_depth: None,
                    follow_symlinks: false,
                    cross_filesystems: false,
                    include_hidden: true,
                    aggregate_mode: aggregate_mode_for(location.target),
                    permission: None,
                    risk: location.role.risk(),
                };
                match scan_root(&scan_spec, CleanerCategory::AiApps, progress, cancellation) {
                    Ok(result) => {
                        saw_any_location = true;
                        scanned_entries += result.scanned_entries;
                        warnings.extend(result.warnings);
                        items.extend(
                            result
                                .entries
                                .into_iter()
                                .filter(|entry| entry.logical_size > 0)
                                .map(|entry| build_item(app, location, entry, activity)),
                        );
                    }
                    Err(ScanError::RootUnavailable(_)) => skipped_roots.push(location.path.clone()),
                    Err(err @ ScanError::Cancelled) => return Err(err),
                    Err(error) => warnings.push(ScanWarning {
                        message: format!("{}: {error:?}", location.path.display()),
                    }),
                }
            }

            if saw_any_location && let Some(message) = activity_warning(app.display_name, activity)
            {
                warnings.push(ScanWarning { message });
            }
        }

        items.sort_by_key(|item| std::cmp::Reverse(item.logical_size));
        let estimated_reclaimable_bytes = items.iter().map(|item| item.logical_size).sum();
        Ok(CategoryScanResult {
            category: CleanerCategory::AiApps,
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

fn aggregate_mode_for(target: AiAppTarget) -> AggregateMode {
    match target {
        AiAppTarget::ExactFile | AiAppTarget::DirectorySummary => AggregateMode::WholeRoot,
        AiAppTarget::DirectoryContents => AggregateMode::ImmediateChildren,
    }
}

fn selection_policy_for(role: AiAppRole, activity: AiAppActivity) -> SelectionPolicy {
    match (role.selection_policy(), activity) {
        (SelectionPolicy::SelectedByDefault, AiAppActivity::Running | AiAppActivity::Unknown) => {
            SelectionPolicy::NotSelectedByDefault
        }
        (selection, _) => selection,
    }
}

fn activity_warning(display_name: &str, activity: AiAppActivity) -> Option<String> {
    match activity {
        AiAppActivity::Running => Some(format!(
            "{display_name} is currently running. Its data may be in active use."
        )),
        AiAppActivity::Unknown => Some(format!(
            "Could not determine whether {display_name} is running. Review before cleaning."
        )),
        AiAppActivity::NotRunning => None,
    }
}

fn item_warning(display_name: &str, activity: AiAppActivity) -> Option<ItemWarning> {
    activity_warning(display_name, activity).map(|message| ItemWarning { message })
}

fn model_names_for(app: &AiAppDefinition, location: &AiAppLocation, path: &Path) -> Vec<String> {
    if app.id == "ollama" && location.role == AiAppRole::Models {
        collect_ollama_model_names(&path.join("manifests"))
    } else {
        Vec::new()
    }
}

fn capabilities_for(role: AiAppRole) -> Vec<ItemCapability> {
    let mut capabilities = vec![ItemCapability::RevealInFinder, ItemCapability::CopyPath];
    if role.allow_cleanup() {
        capabilities.push(ItemCapability::MoveToTrash);
    }
    capabilities
}

fn explanation_for(app: &AiAppDefinition, role: AiAppRole) -> String {
    match role {
        AiAppRole::Logs => format!(
            "{}'s log files. Regenerated automatically.",
            app.display_name
        ),
        AiAppRole::Cache => format!(
            "{}'s cache data. Re-created automatically if it is needed again.",
            app.display_name
        ),
        AiAppRole::TemporaryDownloads => format!("{}'s temporary downloads.", app.display_name),
        AiAppRole::Models => format!(
            "{}'s downloaded model files. These are user-managed assets, not cache — removing one means downloading it again, which can be large. Review before removing.",
            app.display_name
        ),
        AiAppRole::ApplicationSupport => {
            format!("{}'s application support data.", app.display_name)
        }
        AiAppRole::ChatHistory => format!(
            "{}'s chat or prompt history. Only removed if you explicitly choose to.",
            app.display_name
        ),
    }
}

fn build_item(
    app: &AiAppDefinition,
    location: &AiAppLocation,
    entry: AggregatedEntry,
    activity: AiAppActivity,
) -> CleanableItem {
    let model_names = model_names_for(app, location, entry.path.as_path());
    CleanableItem {
        id: item_id(entry.path.as_path()),
        category: CleanerCategory::AiApps,
        group: Some(location.group.to_string()),
        display_name: item_name(entry.path.as_path()),
        path: entry.path,
        logical_size: entry.logical_size,
        allocated_size: None,
        modified_at: entry.modified_at,
        last_accessed_at: None,
        risk: location.role.risk(),
        selection_policy: selection_policy_for(location.role, activity),
        capabilities: capabilities_for(location.role),
        explanation: explanation_for(app, location.role),
        warnings: item_warning(app.display_name, activity)
            .into_iter()
            .collect(),
        metadata: ItemMetadata::AiApp(AiAppMetadata {
            app_id: app.id.to_string(),
            role: location.role,
            model_names,
        }),
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

/// Best-effort Ollama model names from manifest directory/file names only.
/// No manifest body or model weight is opened.
pub(crate) fn collect_ollama_model_names(manifests_dir: &Path) -> Vec<String> {
    let mut names = Vec::new();
    let Ok(hosts) = std::fs::read_dir(manifests_dir) else {
        return names;
    };
    for host in hosts.flatten().filter(|entry| entry.path().is_dir()) {
        let Ok(namespaces) = std::fs::read_dir(host.path()) else {
            continue;
        };
        for namespace in namespaces.flatten().filter(|entry| entry.path().is_dir()) {
            let namespace_name = namespace.file_name().to_string_lossy().into_owned();
            let Ok(models) = std::fs::read_dir(namespace.path()) else {
                continue;
            };
            for model in models.flatten().filter(|entry| entry.path().is_dir()) {
                let model_name = model.file_name().to_string_lossy().into_owned();
                let Ok(tags) = std::fs::read_dir(model.path()) else {
                    continue;
                };
                for tag in tags.flatten().filter(|entry| entry.path().is_file()) {
                    let tag_name = tag.file_name().to_string_lossy().into_owned();
                    names.push(if namespace_name == "library" {
                        format!("{model_name}:{tag_name}")
                    } else {
                        format!("{namespace_name}/{model_name}:{tag_name}")
                    });
                }
            }
        }
    }
    names.sort();
    names
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use super::{AiAppsScanner, cleanup_allowed_roots};
    use crate::ai_apps::definitions::{AiAppEnvironment, default_ai_apps};
    use crate::core::ai_app_provider::{
        AiAppActivity, AiAppDefinition, AiAppLocation, AiAppPathEvidence, AiAppRole, AiAppTarget,
    };
    use crate::core::cancellation::CancellationToken;
    use crate::core::progress::{ProgressSink, ScanProgress};
    use crate::core::risk::{ItemCapability, RiskLevel, SelectionPolicy};
    use crate::core::scan_context::ScanContext;
    use crate::core::scanner::CleanerScanner;
    use crate::paths::HostOs;

    struct RecordingSink;
    impl ProgressSink for RecordingSink {
        fn report(&self, _progress: ScanProgress) {}
    }

    fn activity_not_running(_: &[&str]) -> AiAppActivity {
        AiAppActivity::NotRunning
    }

    fn activity_running(_: &[&str]) -> AiAppActivity {
        AiAppActivity::Running
    }

    fn activity_unknown(_: &[&str]) -> AiAppActivity {
        AiAppActivity::Unknown
    }

    fn context(home: PathBuf) -> ScanContext {
        ScanContext {
            started_at: std::time::SystemTime::now(),
            user_home: Some(home),
        }
    }

    fn app(locations: Vec<AiAppLocation>) -> AiAppDefinition {
        AiAppDefinition {
            id: "test-app",
            display_name: "Test App",
            activity_ids: &["test-app"],
            locations,
        }
    }

    fn location(role: AiAppRole, path: PathBuf, target: AiAppTarget) -> AiAppLocation {
        AiAppLocation {
            role,
            path,
            group: "Test App data",
            target,
            evidence: AiAppPathEvidence::Verified,
        }
    }

    fn scan(
        app: AiAppDefinition,
        probe: fn(&[&str]) -> AiAppActivity,
    ) -> crate::core::report::CategoryScanResult {
        AiAppsScanner::with_apps(vec![app], probe)
            .scan(
                &context(std::env::temp_dir()),
                &RecordingSink,
                &CancellationToken::new(),
            )
            .expect("scans")
    }

    #[test]
    fn cache_items_are_safe_and_selected_by_default() {
        let temp =
            std::env::temp_dir().join(format!("dodo-cleaner-ai-cache-{}", std::process::id()));
        fs::create_dir_all(&temp).expect("creates cache");
        fs::write(temp.join("blob.bin"), [0_u8; 32]).expect("writes cache file");

        let result = scan(
            app(vec![location(
                AiAppRole::Cache,
                temp.clone(),
                AiAppTarget::DirectoryContents,
            )]),
            activity_not_running,
        );
        let cache = result
            .items
            .iter()
            .find(|item| item.display_name == "blob.bin")
            .expect("cache item");
        assert_eq!(cache.risk, RiskLevel::SafeRecreatable);
        assert_eq!(cache.selection_policy, SelectionPolicy::SelectedByDefault);
        assert!(cache.capabilities.contains(&ItemCapability::MoveToTrash));

        fs::remove_dir_all(temp).expect("removes temp tree");
    }

    #[test]
    fn model_items_are_never_selected_by_default_or_move_to_trash_capable() {
        let temp =
            std::env::temp_dir().join(format!("dodo-cleaner-ai-models-{}", std::process::id()));
        fs::create_dir_all(&temp).expect("creates models");
        fs::write(temp.join("weights.bin"), [0_u8; 64]).expect("writes model file");

        let result = scan(
            app(vec![location(
                AiAppRole::Models,
                temp.clone(),
                AiAppTarget::DirectorySummary,
            )]),
            activity_not_running,
        );
        let model = result.items.first().expect("model summary");
        assert_ne!(model.selection_policy, SelectionPolicy::SelectedByDefault);
        assert!(!model.capabilities.contains(&ItemCapability::MoveToTrash));

        fs::remove_dir_all(temp).expect("removes temp tree");
    }

    #[test]
    fn chat_history_is_never_bulk_selected() {
        let temp =
            std::env::temp_dir().join(format!("dodo-cleaner-ai-history-{}", std::process::id()));
        fs::create_dir_all(&temp).expect("creates history");
        fs::write(temp.join("chat.json"), [0_u8; 8]).expect("writes chat history");

        let result = scan(
            app(vec![location(
                AiAppRole::ChatHistory,
                temp.clone(),
                AiAppTarget::DirectorySummary,
            )]),
            activity_not_running,
        );
        assert_eq!(
            result
                .items
                .first()
                .expect("history summary")
                .selection_policy,
            SelectionPolicy::NeverBulkSelect
        );

        fs::remove_dir_all(temp).expect("removes temp tree");
    }

    #[test]
    fn cleanup_allowed_roots_include_only_logs_and_cache_roles() {
        let environment = AiAppEnvironment::fixture(HostOs::MacOs, "/Users/example");
        let mut actual = cleanup_allowed_roots(&environment);
        let mut expected: Vec<PathBuf> = default_ai_apps(&environment)
            .into_iter()
            .flat_map(|app| app.locations)
            .filter(|location| matches!(location.role, AiAppRole::Logs | AiAppRole::Cache))
            .filter(|location| location.target == AiAppTarget::DirectoryContents)
            .map(|location| location.path)
            .collect();
        actual.sort();
        expected.sort();

        assert_eq!(actual, expected);
        assert!(actual.iter().all(|root| !root.ends_with("models")));
    }

    #[test]
    fn exact_files_directory_contents_and_directory_summaries_stay_distinct() {
        let temp =
            std::env::temp_dir().join(format!("dodo-cleaner-ai-targets-{}", std::process::id()));
        let log_file = temp.join("logs/app.log");
        let cache = temp.join("cache");
        let models = temp.join("models");
        fs::create_dir_all(log_file.parent().expect("log parent")).expect("creates logs");
        fs::create_dir_all(&cache).expect("creates cache");
        fs::create_dir_all(&models).expect("creates models");
        fs::write(&log_file, b"log").expect("writes exact log");
        fs::write(cache.join("cache.bin"), b"cache").expect("writes cache");
        fs::write(models.join("weights.bin"), b"models").expect("writes models");

        let result = scan(
            app(vec![
                location(AiAppRole::Logs, log_file.clone(), AiAppTarget::ExactFile),
                location(
                    AiAppRole::Cache,
                    cache.clone(),
                    AiAppTarget::DirectoryContents,
                ),
                location(
                    AiAppRole::Models,
                    models.clone(),
                    AiAppTarget::DirectorySummary,
                ),
            ]),
            activity_not_running,
        );
        assert!(result.items.iter().any(|item| item.path == log_file));
        assert!(
            result
                .items
                .iter()
                .any(|item| item.path == cache.join("cache.bin"))
        );
        let model = result
            .items
            .iter()
            .find(|item| item.path == models)
            .expect("one whole model summary");
        assert_eq!(model.selection_policy, SelectionPolicy::NeverBulkSelect);
        assert!(!model.capabilities.contains(&ItemCapability::MoveToTrash));

        fs::remove_dir_all(temp).expect("removes temp tree");
    }

    #[test]
    fn running_and_unknown_activity_suppress_default_selection() {
        let temp =
            std::env::temp_dir().join(format!("dodo-cleaner-ai-activity-{}", std::process::id()));
        fs::create_dir_all(&temp).expect("creates temp tree");
        fs::write(temp.join("cache.bin"), b"cache").expect("writes cache");
        let definition = app(vec![location(
            AiAppRole::Cache,
            temp.clone(),
            AiAppTarget::DirectoryContents,
        )]);

        for probe in [
            activity_running as fn(&[&str]) -> AiAppActivity,
            activity_unknown,
        ] {
            let result = scan(definition.clone(), probe);
            assert_eq!(
                result.items[0].selection_policy,
                SelectionPolicy::NotSelectedByDefault
            );
            assert!(!result.items[0].warnings.is_empty());
            assert!(!result.warnings.is_empty());
        }
        let inactive = scan(definition, activity_not_running);
        assert_eq!(
            inactive.items[0].selection_policy,
            SelectionPolicy::SelectedByDefault
        );
        assert!(inactive.items[0].warnings.is_empty());

        fs::remove_dir_all(temp).expect("removes temp tree");
    }

    #[test]
    fn collect_ollama_model_names_reads_the_manifest_tree_structure_only() {
        let temp = std::env::temp_dir().join(format!(
            "dodo-cleaner-ollama-manifests-{}",
            std::process::id()
        ));
        let manifests = temp.join("manifests");
        let official = manifests.join("registry.ollama.ai/library/llama3");
        fs::create_dir_all(&official).expect("creates official model tree");
        fs::write(official.join("8b"), b"not json and never read").expect("writes manifest");

        let published = manifests.join("registry.ollama.ai/someuser/custom-model");
        fs::create_dir_all(&published).expect("creates published model tree");
        fs::write(published.join("latest"), b"also never read").expect("writes manifest");

        assert_eq!(
            super::collect_ollama_model_names(&manifests),
            vec!["llama3:8b", "someuser/custom-model:latest"]
        );
        fs::remove_dir_all(temp).expect("removes temp tree");
    }

    #[test]
    fn collect_ollama_model_names_is_empty_for_a_missing_manifests_dir() {
        let missing = std::env::temp_dir().join(format!(
            "dodo-cleaner-ollama-manifests-missing-{}",
            std::process::id()
        ));
        assert!(super::collect_ollama_model_names(&missing).is_empty());
    }
}
