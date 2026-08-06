//! `CleanerCategory::NodeToolingCache` (Phase 11): six independent Node
//! package-manager cache providers (`macos::scanners::node_tooling`) behind
//! one [`NodeToolCacheProvider`] trait (`core::node_tool_provider`), scanned
//! uniformly by [`NodeToolingCacheScanner`].
//!
//! # Why the trait lives in `core`, not here
//!
//! Every provider is env-var-plus-filesystem-path logic only — no macOS API,
//! no GPUI — so the trait, [`NodeCacheLocation`] and [`NodeToolEnvironment`]
//! live in `core::node_tool_provider`, next to `CleanerScanner` itself,
//! which is the same split (a plain trait in `core`, macOS implementations
//! under `macos`). Only this scanner and the six provider *implementations*
//! live under `macos`, per the ticket's suggested layout.
//!
//! # One environment snapshot per scan
//!
//! [`snapshot_environment`] reads every environment variable any provider
//! cares about exactly once per scan and hands the result to every
//! provider — the ticket's "snapshot environment variables once per scan"
//! applied literally, rather than each provider calling `std::env::var_os`
//! on its own. No test exercises it: every provider test and every test in
//! this module builds a [`NodeToolEnvironment`] by hand, so no test result
//! depends on whatever the machine running it happens to have exported —
//! the same reasoning `HomebrewCacheScanner`'s tests give for not exercising
//! its own real-`HOMEBREW_CACHE` path.
//!
//! # Avoiding duplicate counting across providers
//!
//! Two hazards, both handled once here rather than per-provider:
//!
//! - Two providers could resolve to the exact same path (an unlikely but
//!   possible environment-variable coincidence). [`NodeToolingCacheScanner::
//!   scan`] drops any location whose path it has already seen from an
//!   earlier provider before scanning anything.
//! - Yarn Berry's default global cache (`~/Library/Caches/Yarn/Berry`) nests
//!   *inside* Yarn Classic's own cache root (`~/Library/Caches/Yarn`), so
//!   Yarn Classic's `ImmediateChildren` scan would otherwise also enumerate
//!   `Berry` as one opaque child of its own cache. `scan` excludes any
//!   immediate-child entry whose path equals another provider's own
//!   location — the same technique `homebrew_cache` uses to exclude its
//!   `Cask`/`Logs` subdirectories from its top-level "Download cache" group,
//!   generalized across all providers rather than hard-coded to the one
//!   pair known to collide today.
//!
//! # Cleanup allow-listing
//!
//! [`cleanup_allowed_roots`] reruns the same provider discovery and keeps
//! only locations marked `allow_cleanup: true`, so `macos::cleanup::
//! policy_for` can never allow-list a root this scan did not itself
//! produce — the same one-function-shared-by-scan-and-cleanup pattern
//! `homebrew_cache::resolve_cache_root` and `xcode_junk::
//! cleanup_allowed_roots` use. Every `allow_cleanup: true` location is
//! `AggregateMode::ImmediateChildren` (enforced by convention in every
//! provider, not by a runtime check): a `WholeRoot` item's path would equal
//! its own allow-listed root, which `macos::safety::validate_path` rejects
//! outright as `SafetyError::RootDeletionRejected`.
//!
//! # Scope cuts
//!
//! Recorded in full in `docs/cleaner/known-limitations.md`: no CLI
//! invocation of `npm`/`yarn`/`pnpm`/`bun`/`pnpm config` for this phase;
//! pnpm's store is scan-only and never allow-listed; Yarn Berry's
//! project-local `.yarn/cache` and Plug'n'Play files, and Bun's
//! project-local dependencies, are out of scope for discovery (no
//! home-directory crawl); `node_modules` is never touched by any provider;
//! Nub falls back to reporting nothing at all rather than guessing at a
//! directory layout this phase has no confident knowledge of.

use std::collections::HashSet;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use crate::cleaner::core::cancellation::CancellationToken;
use crate::cleaner::core::category::CleanerCategory;
use crate::cleaner::core::errors::ScanError;
use crate::cleaner::core::fs::scan_root;
use crate::cleaner::core::item::{CleanableItem, CleanableItemId, ItemMetadata, NodeToolMetadata};
use crate::cleaner::core::node_tool_provider::{
    NodeCacheLocation, NodeToolCacheProvider, NodeToolEnvironment,
};
use crate::cleaner::core::permissions::MacPermission;
use crate::cleaner::core::progress::{ProgressSink, ScanPhase, ScanProgress};
use crate::cleaner::core::report::{
    CategoryScanResult, PartialScanReason, ScanCompleteness, ScanWarning,
};
use crate::cleaner::core::risk::ItemCapability;
use crate::cleaner::core::scan_context::ScanContext;
use crate::cleaner::core::scan_root::ScanRoot;
use crate::cleaner::core::scanner::CleanerScanner;
use crate::cleaner::macos::scanners::node_tooling::default_providers;

pub struct NodeToolingCacheScanner {
    providers: Vec<Arc<dyn NodeToolCacheProvider>>,
    /// Injected only by tests; a real scan always calls
    /// [`snapshot_environment`] at scan time so it observes the real
    /// environment and the scan's own `user_home`, exactly like every other
    /// scanner here resolves its roots from `ScanContext`.
    forced_environment: Option<NodeToolEnvironment>,
}

impl NodeToolingCacheScanner {
    pub fn new() -> Self {
        Self {
            providers: default_providers(),
            forced_environment: None,
        }
    }

    #[cfg(test)]
    fn with_providers_and_environment(
        providers: Vec<Arc<dyn NodeToolCacheProvider>>,
        environment: NodeToolEnvironment,
    ) -> Self {
        Self {
            providers,
            forced_environment: Some(environment),
        }
    }
}

/// Reads every environment variable any of the six providers care about,
/// exactly once. Shared by [`NodeToolingCacheScanner::scan`] and
/// `macos::cleanup::policy_for`'s [`cleanup_allowed_roots`] call, so both
/// observe the exact same snapshot shape.
pub(crate) fn snapshot_environment(home: Option<&Path>) -> NodeToolEnvironment {
    NodeToolEnvironment {
        home: home.map(Path::to_path_buf),
        npm_config_cache: non_empty_env("npm_config_cache"),
        yarn_cache_folder: non_empty_env("YARN_CACHE_FOLDER"),
        pnpm_home: non_empty_env("PNPM_HOME"),
        npm_config_store_dir: non_empty_env("npm_config_store_dir"),
        bun_install: non_empty_env("BUN_INSTALL"),
        bun_install_cache_dir: non_empty_env("BUN_INSTALL_CACHE_DIR"),
        nub_home: non_empty_env("NUB_HOME"),
    }
}

fn non_empty_env(key: &str) -> Option<PathBuf> {
    std::env::var_os(key)
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
}

/// Every location any provider marks `allow_cleanup: true`, for the given
/// environment — see this module's doc comment for why `macos::cleanup::
/// policy_for` calls this instead of building its own list.
pub(crate) fn cleanup_allowed_roots(environment: &NodeToolEnvironment) -> Vec<PathBuf> {
    default_providers()
        .iter()
        .flat_map(|provider| provider.discover(environment))
        .filter(|location| location.allow_cleanup)
        .map(|location| location.path)
        .collect()
}

impl CleanerScanner for NodeToolingCacheScanner {
    fn category(&self) -> CleanerCategory {
        CleanerCategory::NodeToolingCache
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
            category: CleanerCategory::NodeToolingCache,
            phase: ScanPhase::Preparing,
            current_path: None,
            scanned_entries: 0,
            discovered_items: 0,
            discovered_bytes: 0,
        });

        let environment = match &self.forced_environment {
            Some(environment) => environment.clone(),
            None => snapshot_environment(context.user_home.as_deref()),
        };

        let mut discovered: Vec<(Arc<dyn NodeToolCacheProvider>, NodeCacheLocation)> = Vec::new();
        let mut seen_paths = HashSet::new();
        for provider in &self.providers {
            for location in provider.discover(&environment) {
                if seen_paths.insert(location.path.clone()) {
                    discovered.push((Arc::clone(provider), location));
                }
            }
        }
        let all_location_paths: HashSet<PathBuf> = discovered
            .iter()
            .map(|(_, location)| location.path.clone())
            .collect();

        let mut items = Vec::new();
        let mut warnings = Vec::new();
        let mut scanned_entries = 0u64;
        let mut skipped_roots = Vec::new();

        for (provider, location) in &discovered {
            if cancellation.is_cancelled() {
                return Err(ScanError::Cancelled);
            }
            let scan_spec = ScanRoot {
                path: location.path.clone(),
                max_depth: None,
                follow_symlinks: false,
                cross_filesystems: false,
                include_hidden: true,
                aggregate_mode: location.aggregate_mode,
                permission: None,
                risk: location.risk,
            };
            match scan_root(
                &scan_spec,
                CleanerCategory::NodeToolingCache,
                progress,
                cancellation,
            ) {
                Ok(result) => {
                    scanned_entries += result.scanned_entries;
                    warnings.extend(result.warnings);
                    for entry in result.entries {
                        if cancellation.is_cancelled() {
                            return Err(ScanError::Cancelled);
                        }
                        if entry.logical_size == 0 {
                            continue;
                        }
                        if entry.path != location.path && all_location_paths.contains(&entry.path) {
                            // Scanned separately, under its own group, as
                            // another provider's own location — see this
                            // module's doc comment.
                            continue;
                        }
                        items.push(build_item(
                            provider.id(),
                            location,
                            entry.path,
                            entry.logical_size,
                            entry.modified_at,
                        ));
                    }
                }
                Err(ScanError::RootUnavailable(_)) => skipped_roots.push(location.path.clone()),
                Err(err @ ScanError::Cancelled) => return Err(err),
                Err(error) => warnings.push(ScanWarning {
                    message: format!("{}: {error:?}", location.path.display()),
                }),
            }
        }

        items.sort_by_key(|item| std::cmp::Reverse(item.logical_size));
        let estimated_reclaimable_bytes = items.iter().map(|item| item.logical_size).sum();
        Ok(CategoryScanResult {
            category: CleanerCategory::NodeToolingCache,
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

fn build_item(
    provider_id: &'static str,
    location: &NodeCacheLocation,
    path: PathBuf,
    logical_size: u64,
    modified_at: Option<SystemTime>,
) -> CleanableItem {
    CleanableItem {
        id: item_id(path.as_path()),
        category: CleanerCategory::NodeToolingCache,
        group: Some(location.group.clone()),
        display_name: item_name(path.as_path()),
        path,
        logical_size,
        allocated_size: None,
        modified_at,
        last_accessed_at: None,
        risk: location.risk,
        selection_policy: location.selection_policy,
        capabilities: capabilities_for(location.allow_cleanup),
        explanation: location.explanation.clone(),
        warnings: Vec::new(),
        metadata: ItemMetadata::NodeTool(NodeToolMetadata {
            provider: provider_id.to_string(),
        }),
    }
}

fn capabilities_for(allow_cleanup: bool) -> Vec<ItemCapability> {
    if allow_cleanup {
        vec![
            ItemCapability::MoveToTrash,
            ItemCapability::RevealInFinder,
            ItemCapability::CopyPath,
        ]
    } else {
        vec![ItemCapability::RevealInFinder, ItemCapability::CopyPath]
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

    use crate::cleaner::core::cancellation::CancellationToken;
    use crate::cleaner::core::node_tool_provider::{
        NodeCacheLocation, NodeCacheScope, NodeToolCacheProvider, NodeToolEnvironment,
    };
    use crate::cleaner::core::progress::{ProgressSink, ScanProgress};
    use crate::cleaner::core::risk::{ItemCapability, RiskLevel, SelectionPolicy};
    use crate::cleaner::core::scan_context::ScanContext;
    use crate::cleaner::core::scan_root::AggregateMode;
    use crate::cleaner::core::scanner::CleanerScanner;
    use crate::cleaner::macos::scanners::node_tooling_cache::NodeToolingCacheScanner;
    use std::sync::Arc;

    struct RecordingSink;
    impl ProgressSink for RecordingSink {
        fn report(&self, _progress: ScanProgress) {}
    }

    fn empty_context() -> ScanContext {
        ScanContext {
            started_at: std::time::SystemTime::now(),
            user_home: None,
        }
    }

    struct StubProvider {
        id: &'static str,
        locations: Vec<NodeCacheLocation>,
    }

    impl NodeToolCacheProvider for StubProvider {
        fn id(&self) -> &'static str {
            self.id
        }

        fn display_name(&self) -> &'static str {
            self.id
        }

        fn discover(&self, _environment: &NodeToolEnvironment) -> Vec<NodeCacheLocation> {
            self.locations.clone()
        }
    }

    fn allow_cleanup_location(path: std::path::PathBuf, group: &str) -> NodeCacheLocation {
        NodeCacheLocation {
            path,
            group: group.to_string(),
            scope: NodeCacheScope::Global,
            risk: RiskLevel::SafeRecreatable,
            selection_policy: SelectionPolicy::SelectedByDefault,
            allow_cleanup: true,
            aggregate_mode: AggregateMode::ImmediateChildren,
            explanation: "test location".to_string(),
        }
    }

    #[test]
    fn scan_reports_items_from_every_provider_with_move_to_trash() {
        let temp =
            std::env::temp_dir().join(format!("dodo-cleaner-node-tooling-{}", std::process::id()));
        let cache_a = temp.join("provider-a-cache");
        let cache_b = temp.join("provider-b-cache");
        fs::create_dir_all(cache_a.join("child")).expect("creates provider A cache");
        fs::write(cache_a.join("child").join("data.bin"), vec![0u8; 16])
            .expect("writes provider A data");
        fs::create_dir_all(cache_b.join("child")).expect("creates provider B cache");
        fs::write(cache_b.join("child").join("data.bin"), vec![0u8; 8])
            .expect("writes provider B data");

        let providers: Vec<Arc<dyn NodeToolCacheProvider>> = vec![
            Arc::new(StubProvider {
                id: "provider-a",
                locations: vec![allow_cleanup_location(cache_a.clone(), "Provider A cache")],
            }),
            Arc::new(StubProvider {
                id: "provider-b",
                locations: vec![allow_cleanup_location(cache_b.clone(), "Provider B cache")],
            }),
        ];
        let scanner = NodeToolingCacheScanner::with_providers_and_environment(
            providers,
            NodeToolEnvironment::default(),
        );
        let result = scanner
            .scan(&empty_context(), &RecordingSink, &CancellationToken::new())
            .expect("scans node tooling caches");

        assert_eq!(result.items.len(), 2);
        assert!(
            result
                .items
                .iter()
                .all(|item| item.capabilities.contains(&ItemCapability::MoveToTrash))
        );
        assert!(
            result
                .items
                .iter()
                .any(|item| item.group.as_deref() == Some("Provider A cache"))
        );
        assert!(
            result
                .items
                .iter()
                .any(|item| item.group.as_deref() == Some("Provider B cache"))
        );

        fs::remove_dir_all(&temp).expect("removes temp tree");
    }

    #[test]
    fn scan_never_double_counts_a_location_two_providers_both_resolved_to() {
        let temp = std::env::temp_dir().join(format!(
            "dodo-cleaner-node-tooling-dup-{}",
            std::process::id()
        ));
        let shared_cache = temp.join("shared-cache");
        fs::create_dir_all(shared_cache.join("child")).expect("creates shared cache dir");
        fs::write(shared_cache.join("child").join("data.bin"), vec![0u8; 16])
            .expect("writes shared cache data");

        let providers: Vec<Arc<dyn NodeToolCacheProvider>> = vec![
            Arc::new(StubProvider {
                id: "provider-a",
                locations: vec![allow_cleanup_location(shared_cache.clone(), "Shared cache")],
            }),
            Arc::new(StubProvider {
                id: "provider-b",
                locations: vec![allow_cleanup_location(shared_cache.clone(), "Shared cache")],
            }),
        ];
        let scanner = NodeToolingCacheScanner::with_providers_and_environment(
            providers,
            NodeToolEnvironment::default(),
        );
        let result = scanner
            .scan(&empty_context(), &RecordingSink, &CancellationToken::new())
            .expect("scans node tooling caches");

        assert_eq!(
            result.items.len(),
            1,
            "the same resolved path must never be scanned twice"
        );

        fs::remove_dir_all(&temp).expect("removes temp tree");
    }

    #[test]
    fn scan_excludes_a_nested_child_that_is_also_another_providers_own_location() {
        let temp = std::env::temp_dir().join(format!(
            "dodo-cleaner-node-tooling-nested-{}",
            std::process::id()
        ));
        let outer = temp.join("outer-cache");
        let inner = outer.join("inner-cache");
        fs::create_dir_all(inner.join("child")).expect("creates nested cache dirs");
        fs::write(inner.join("child").join("data.bin"), vec![0u8; 16])
            .expect("writes nested cache data");
        fs::create_dir_all(outer.join("sibling")).expect("creates a sibling of inner");
        fs::write(outer.join("sibling").join("data.bin"), vec![0u8; 4])
            .expect("writes sibling data");

        let providers: Vec<Arc<dyn NodeToolCacheProvider>> = vec![
            Arc::new(StubProvider {
                id: "outer-provider",
                locations: vec![allow_cleanup_location(outer.clone(), "Outer cache")],
            }),
            Arc::new(StubProvider {
                id: "inner-provider",
                locations: vec![allow_cleanup_location(inner.clone(), "Inner cache")],
            }),
        ];
        let scanner = NodeToolingCacheScanner::with_providers_and_environment(
            providers,
            NodeToolEnvironment::default(),
        );
        let result = scanner
            .scan(&empty_context(), &RecordingSink, &CancellationToken::new())
            .expect("scans node tooling caches");

        // "inner-cache" itself must never appear as an opaque item under the
        // "Outer cache" group — only its own group, plus the outer
        // provider's unrelated sibling.
        assert!(
            !result.items.iter().any(|item| item.path == inner),
            "the inner provider's own root must never appear as an item"
        );
        assert!(
            result
                .items
                .iter()
                .any(|item| item.group.as_deref() == Some("Outer cache")
                    && item.path == outer.join("sibling"))
        );
        assert!(
            result
                .items
                .iter()
                .any(|item| item.group.as_deref() == Some("Inner cache")
                    && item.path == inner.join("child"))
        );

        fs::remove_dir_all(&temp).expect("removes temp tree");
    }

    #[test]
    fn scan_only_locations_never_get_move_to_trash() {
        let temp = std::env::temp_dir().join(format!(
            "dodo-cleaner-node-tooling-scanonly-{}",
            std::process::id()
        ));
        let store = temp.join("store");
        fs::create_dir_all(store.join("child")).expect("creates store dir");
        fs::write(store.join("child").join("data.bin"), vec![0u8; 16]).expect("writes store data");

        let providers: Vec<Arc<dyn NodeToolCacheProvider>> = vec![Arc::new(StubProvider {
            id: "pnpm",
            locations: vec![NodeCacheLocation {
                path: store.clone(),
                group: "pnpm store".to_string(),
                scope: NodeCacheScope::Global,
                risk: RiskLevel::UserData,
                selection_policy: SelectionPolicy::NotSelectedByDefault,
                allow_cleanup: false,
                aggregate_mode: AggregateMode::ImmediateChildren,
                explanation: "test store".to_string(),
            }],
        })];
        let scanner = NodeToolingCacheScanner::with_providers_and_environment(
            providers,
            NodeToolEnvironment::default(),
        );
        let result = scanner
            .scan(&empty_context(), &RecordingSink, &CancellationToken::new())
            .expect("scans node tooling caches");

        assert_eq!(result.items.len(), 1);
        assert!(
            !result.items[0]
                .capabilities
                .contains(&ItemCapability::MoveToTrash)
        );
        assert_eq!(
            result.items[0].selection_policy,
            SelectionPolicy::NotSelectedByDefault
        );

        fs::remove_dir_all(&temp).expect("removes temp tree");
    }

    #[test]
    fn a_missing_location_is_a_partial_scan_not_an_error() {
        let temp = std::env::temp_dir().join(format!(
            "dodo-cleaner-node-tooling-missing-{}",
            std::process::id()
        ));

        let providers: Vec<Arc<dyn NodeToolCacheProvider>> = vec![Arc::new(StubProvider {
            id: "provider-a",
            locations: vec![allow_cleanup_location(
                temp.join("does-not-exist"),
                "Missing cache",
            )],
        })];
        let scanner = NodeToolingCacheScanner::with_providers_and_environment(
            providers,
            NodeToolEnvironment::default(),
        );
        let result = scanner
            .scan(&empty_context(), &RecordingSink, &CancellationToken::new())
            .expect("scan tolerates a missing location");

        assert!(result.items.is_empty());
        assert!(matches!(
            result.completeness,
            crate::cleaner::core::report::ScanCompleteness::Partial { .. }
        ));
    }

    #[test]
    fn cleanup_allowed_roots_excludes_scan_only_locations() {
        use crate::cleaner::macos::scanners::node_tooling_cache::cleanup_allowed_roots;

        let temp = std::env::temp_dir().join(format!(
            "dodo-cleaner-node-tooling-allowlist-{}",
            std::process::id()
        ));
        fs::create_dir_all(temp.join("Library").join("pnpm").join("store"))
            .expect("creates pnpm store dir");
        fs::create_dir_all(temp.join("Library").join("Caches").join("pnpm"))
            .expect("creates pnpm cache dir");

        let environment = NodeToolEnvironment {
            home: Some(temp.clone()),
            ..Default::default()
        };
        let roots = cleanup_allowed_roots(&environment);

        assert!(
            roots.iter().any(|root| root.ends_with("Caches/pnpm")),
            "pnpm's cache must be allow-listed"
        );
        assert!(
            !roots.iter().any(|root| root.ends_with("pnpm/store")),
            "pnpm's store must never be allow-listed"
        );

        fs::remove_dir_all(&temp).expect("removes temp tree");
    }

    // `NodeToolingCacheScanner::new()` reading the real environment is
    // deliberately not exercised here — see this module's doc comment.
}
