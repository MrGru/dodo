//! `CleanerCategory::AiApps` (Phase 12): local AI desktop apps — Ollama and
//! LM Studio today, more later via `ai_app_providers::default_ai_apps`.
//!
//! This scanner is deliberately thin: almost every judgment call (risk,
//! selection policy, whether a location may ever be allow-listed for
//! cleanup) lives on [`AiAppRole`] in `core::ai_app_provider`, keyed by the
//! *role* a location plays (Logs, Cache, Models, Application support) rather
//! than by which provider it came from. Adding a third provider means adding
//! one `AiAppDefinition` in `ai_app_providers` — never touching this file.
//!
//! # Aggregation: `WholeRoot` for anything never allow-listed, `ImmediateChildren` otherwise
//!
//! `core::node_tool_provider`'s doc comment warns against `AggregateMode::
//! WholeRoot` because it makes an item's path equal its cleanup-allow-listed
//! root, which `core::safety::validate_path` rejects as
//! `SafetyError::RootDeletionRejected`. That risk only exists for roles this
//! scanner ever grants [`ItemCapability::MoveToTrash`] — [`AiAppRole::Logs`]
//! and [`AiAppRole::Cache`], scanned as `ImmediateChildren` so each log file
//! or cache entry is its own item. Every other role
//! (`Models`/`ApplicationSupport`/`TemporaryDownloads`/`ChatHistory`) never
//! gets that capability at all — see [`AiAppRole::allow_cleanup`] — so
//! representing the whole location as one `WholeRoot` item is both safe and
//! more useful: a model directory split into its internal `blobs`/
//! `manifests` subfolders would not read as anything a user recognizes.
//!
//! # Model names, never model content
//!
//! For Ollama's `Models` root specifically, [`AiAppMetadata::model_names`] is
//! populated by walking the manifest tree's directory and file *names* only
//! (`ai_app_providers::collect_ollama_model_names`) — never opening a
//! manifest's JSON body or any model weight file. LM Studio has no
//! confidently-known equivalent convention this phase, so its `Models` items
//! always carry an empty `model_names`; see
//! `docs/cleaner/known-limitations.md`.
//!
//! # Warning when an app is running
//!
//! `scan()` calls `platform::is_any_bundle_running` once per app (a read-only
//! `NSRunningApplication` check — see `macos::platform::running_apps`) and
//! attaches an [`ItemWarning`] to every item that app produced, plus one
//! category-level [`ScanWarning`] per running app. This warns rather than
//! blocks, the same posture `xcode_junk` established for Xcode.
//!
//! # Full Disk Access
//!
//! None of these roots need it — `~/.ollama`, `~/.cache/lm-studio` and
//! `~/Library/{Application Support,Caches,Logs}/<App>` are ordinary
//! user-writable locations, unlike `~/Library/Mail` or `~/Library/
//! Containers` — so `required_permissions()` returns an empty slice.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use crate::cleaner::core::ai_app_provider::{AiAppDefinition, AiAppRole, AiAppRoot};
use crate::cleaner::core::cancellation::CancellationToken;
use crate::cleaner::core::category::CleanerCategory;
use crate::cleaner::core::errors::ScanError;
use crate::cleaner::core::fs::scan_root;
use crate::cleaner::core::item::{
    AiAppMetadata, CleanableItem, CleanableItemId, ItemMetadata, ItemWarning,
};
use crate::cleaner::core::permissions::MacPermission;
use crate::cleaner::core::progress::{ProgressSink, ScanPhase, ScanProgress};
use crate::cleaner::core::report::{
    CategoryScanResult, PartialScanReason, ScanCompleteness, ScanWarning,
};
use crate::cleaner::core::risk::ItemCapability;
use crate::cleaner::core::scan_context::ScanContext;
use crate::cleaner::core::scan_root::{AggregateMode, ScanRoot};
use crate::cleaner::core::scanner::CleanerScanner;
use crate::cleaner::macos::platform;
use crate::cleaner::macos::scanners::ai_app_providers::{
    collect_ollama_model_names, default_ai_apps,
};

pub struct AiAppsScanner {
    apps: Vec<AiAppDefinition>,
}

impl AiAppsScanner {
    pub fn new() -> Self {
        Self {
            apps: default_ai_apps(),
        }
    }

    #[cfg(test)]
    fn with_apps(apps: Vec<AiAppDefinition>) -> Self {
        Self { apps }
    }
}

/// Every location `AiAppRole::allow_cleanup` permits, across every
/// registered provider, resolved against `home` — the single list both this
/// scanner's `MoveToTrash` capability grants and `macos::cleanup::policy_for`
/// allow-list from, so the two can never drift apart. Mirrors
/// `xcode_junk::cleanup_allowed_roots`.
pub(crate) fn cleanup_allowed_roots(home: &Path) -> Vec<PathBuf> {
    default_ai_apps()
        .into_iter()
        .flat_map(|app| app.roots.to_vec())
        .filter(|root| root.role.allow_cleanup())
        .filter_map(|root| resolve_root_path(root.path, Some(home)))
        .collect()
}

impl CleanerScanner for AiAppsScanner {
    fn category(&self) -> CleanerCategory {
        CleanerCategory::AiApps
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
            category: CleanerCategory::AiApps,
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

        for app in &self.apps {
            let app_running = platform::is_any_bundle_running(app.bundle_ids);
            let mut saw_any_root_for_app = false;

            for root in app.roots {
                if cancellation.is_cancelled() {
                    return Err(ScanError::Cancelled);
                }
                let Some(path) = resolve_root_path(root.path, context.user_home.as_deref()) else {
                    continue;
                };

                let scan_spec = ScanRoot {
                    path: path.clone(),
                    max_depth: None,
                    follow_symlinks: false,
                    cross_filesystems: false,
                    include_hidden: true,
                    aggregate_mode: aggregate_mode_for(root.role),
                    permission: None,
                    risk: root.role.risk(),
                };
                match scan_root(&scan_spec, CleanerCategory::AiApps, progress, cancellation) {
                    Ok(result) => {
                        saw_any_root_for_app = true;
                        scanned_entries += result.scanned_entries;
                        warnings.extend(result.warnings);
                        for entry_path in result.entries.into_iter().filter_map(|entry| {
                            if entry.logical_size == 0 {
                                None
                            } else {
                                Some(entry)
                            }
                        }) {
                            items.push(build_item(app, root, entry_path, app_running));
                        }
                    }
                    Err(ScanError::RootUnavailable(_)) => skipped_roots.push(path),
                    Err(err @ ScanError::Cancelled) => return Err(err),
                    Err(error) => warnings.push(ScanWarning {
                        message: format!("{}: {error:?}", path.display()),
                    }),
                }
            }

            if app_running && saw_any_root_for_app {
                warnings.push(ScanWarning {
                    message: format!(
                        "{} is currently running. Its data may be in active use.",
                        app.display_name
                    ),
                });
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

fn resolve_root_path(path: &str, home: Option<&Path>) -> Option<PathBuf> {
    match path.strip_prefix("~/") {
        Some(rest) => home.map(|home| home.join(rest)),
        None => Some(PathBuf::from(path)),
    }
}

fn aggregate_mode_for(role: AiAppRole) -> AggregateMode {
    if role.allow_cleanup() {
        AggregateMode::ImmediateChildren
    } else {
        AggregateMode::WholeRoot
    }
}

fn model_names_for(app: &AiAppDefinition, root: &AiAppRoot, entry_path: &Path) -> Vec<String> {
    if app.id == "ollama" && root.role == AiAppRole::Models {
        collect_ollama_model_names(&entry_path.join("manifests"))
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
        AiAppRole::TemporaryDownloads => {
            format!("{}'s temporary downloads.", app.display_name)
        }
        AiAppRole::Models => format!(
            "{}'s downloaded model files. These are user-managed assets, not cache — removing \
             one means downloading it again, which can be large. Review before removing.",
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
    root: &AiAppRoot,
    entry: crate::cleaner::core::fs::AggregatedEntry,
    app_running: bool,
) -> CleanableItem {
    let mut warnings = Vec::new();
    if app_running {
        warnings.push(ItemWarning {
            message: format!(
                "{} is currently running; this data may be in active use.",
                app.display_name
            ),
        });
    }
    let model_names = model_names_for(app, root, entry.path.as_path());
    CleanableItem {
        id: item_id(entry.path.as_path()),
        category: CleanerCategory::AiApps,
        group: Some(root.group.to_string()),
        display_name: item_name(entry.path.as_path()),
        path: entry.path,
        logical_size: entry.logical_size,
        allocated_size: None,
        modified_at: entry.modified_at,
        last_accessed_at: None,
        risk: root.role.risk(),
        selection_policy: root.role.selection_policy(),
        capabilities: capabilities_for(root.role),
        explanation: explanation_for(app, root.role),
        warnings,
        metadata: ItemMetadata::AiApp(AiAppMetadata {
            app_id: app.id.to_string(),
            role: root.role,
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

#[cfg(test)]
mod tests {
    use std::fs;

    use crate::cleaner::core::ai_app_provider::{AiAppDefinition, AiAppRole, AiAppRoot};
    use crate::cleaner::core::cancellation::CancellationToken;
    use crate::cleaner::core::item::ItemMetadata;
    use crate::cleaner::core::progress::{ProgressSink, ScanProgress};
    use crate::cleaner::core::risk::{ItemCapability, RiskLevel, SelectionPolicy};
    use crate::cleaner::core::scan_context::ScanContext;
    use crate::cleaner::core::scanner::CleanerScanner;

    use super::AiAppsScanner;

    struct RecordingSink;
    impl ProgressSink for RecordingSink {
        fn report(&self, _progress: ScanProgress) {}
    }

    fn context(home: std::path::PathBuf) -> ScanContext {
        ScanContext {
            started_at: std::time::SystemTime::now(),
            user_home: Some(home),
        }
    }

    const TEST_APP: AiAppDefinition = AiAppDefinition {
        id: "test-app",
        display_name: "Test App",
        bundle_ids: &["com.example.test-app"],
        roots: &[
            AiAppRoot {
                role: AiAppRole::Cache,
                path: "~/Library/Caches/TestApp",
                group: "Test App cache",
            },
            AiAppRoot {
                role: AiAppRole::Models,
                path: "~/.test-app/models",
                group: "Test App models",
            },
            AiAppRoot {
                role: AiAppRole::ChatHistory,
                path: "~/.test-app/history",
                group: "Test App history",
            },
        ],
    };

    #[test]
    fn cache_items_are_safe_and_selected_by_default() {
        let temp =
            std::env::temp_dir().join(format!("dodo-cleaner-ai-cache-{}", std::process::id()));
        let cache = temp.join("Library/Caches/TestApp");
        fs::create_dir_all(&cache).expect("creates cache dir");
        fs::write(cache.join("blob.bin"), vec![0u8; 32]).expect("writes cache file");

        let scanner = AiAppsScanner::with_apps(vec![TEST_APP]);
        let result = scanner
            .scan(
                &context(temp.clone()),
                &RecordingSink,
                &CancellationToken::new(),
            )
            .expect("scans");

        let cache_item = result
            .items
            .iter()
            .find(|item| item.display_name == "blob.bin")
            .expect("finds the cache file item");
        assert_eq!(cache_item.risk, RiskLevel::SafeRecreatable);
        assert_eq!(
            cache_item.selection_policy,
            SelectionPolicy::SelectedByDefault
        );
        assert!(
            cache_item
                .capabilities
                .contains(&ItemCapability::MoveToTrash)
        );

        fs::remove_dir_all(&temp).expect("removes temp tree");
    }

    #[test]
    fn model_items_are_never_selected_by_default_or_move_to_trash_capable() {
        let temp =
            std::env::temp_dir().join(format!("dodo-cleaner-ai-models-{}", std::process::id()));
        let models = temp.join(".test-app").join("models");
        fs::create_dir_all(&models).expect("creates models dir");
        fs::write(models.join("weights.bin"), vec![0u8; 64]).expect("writes model file");

        let scanner = AiAppsScanner::with_apps(vec![TEST_APP]);
        let result = scanner
            .scan(
                &context(temp.clone()),
                &RecordingSink,
                &CancellationToken::new(),
            )
            .expect("scans");

        let model_item = result
            .items
            .iter()
            .find(|item| item.group.as_deref() == Some("Test App models"))
            .expect("finds the whole-root models item");
        assert_ne!(
            model_item.selection_policy,
            SelectionPolicy::SelectedByDefault
        );
        assert!(
            !model_item
                .capabilities
                .contains(&ItemCapability::MoveToTrash)
        );
        assert!(matches!(model_item.metadata, ItemMetadata::AiApp(_)));

        fs::remove_dir_all(&temp).expect("removes temp tree");
    }

    #[test]
    fn chat_history_is_never_bulk_selected() {
        let temp =
            std::env::temp_dir().join(format!("dodo-cleaner-ai-history-{}", std::process::id()));
        let history = temp.join(".test-app").join("history");
        fs::create_dir_all(&history).expect("creates history dir");
        fs::write(history.join("chat.json"), vec![0u8; 8]).expect("writes history file");

        let scanner = AiAppsScanner::with_apps(vec![TEST_APP]);
        let result = scanner
            .scan(
                &context(temp.clone()),
                &RecordingSink,
                &CancellationToken::new(),
            )
            .expect("scans");

        let history_item = result
            .items
            .iter()
            .find(|item| item.group.as_deref() == Some("Test App history"))
            .expect("finds the whole-root history item");
        assert_eq!(
            history_item.selection_policy,
            SelectionPolicy::NeverBulkSelect
        );

        fs::remove_dir_all(&temp).expect("removes temp tree");
    }

    #[test]
    fn cleanup_allowed_roots_include_only_logs_and_cache_roles() {
        use super::cleanup_allowed_roots;

        let home = std::path::Path::new("/Users/example");
        let roots = cleanup_allowed_roots(home);
        assert!(roots.iter().any(|root| root.ends_with("Ollama")));
        assert!(!roots.iter().any(|root| root.ends_with("models")));
    }
}
