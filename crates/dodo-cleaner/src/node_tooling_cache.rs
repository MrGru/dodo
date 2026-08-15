//! Shared Node Tooling Cache scanner for macOS, Windows and Linux.
//!
//! The original provider trait and `NodeCacheLocation` were genuinely
//! platform-independent policy values, but their input snapshot was not: it
//! exposed only `home` and macOS-era overrides, while three providers embedded
//! `~/Library` defaults. The trait therefore stays; its snapshot now carries
//! resolved host cache directories and successful tool-query answers, and the
//! providers remain pure filesystem policy.
//!
//! Configured paths come from fixed argv calls (`npm config get cache`, Yarn's
//! Classic and Berry queries, `pnpm store path`/`config get cache-dir`, and
//! `bun pm cache`) behind an injectable runner. Output must be one absolute,
//! existing path. Explicit environment overrides win, then tool output, then a
//! documented platform default where one is safe.
//!
//! Duplicate roots use host-aware comparison. An item containing another
//! provider root is omitted rather than counted or deleted twice. Project
//! `node_modules` paths are excluded. pnpm's content-addressable store is a
//! denied root: it is neither shown nor deletable, and cleanup adds it to the
//! standing deletion policy's protected paths.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::SystemTime;

use crate::core::cancellation::CancellationToken;
use crate::core::category::CleanerCategory;
use crate::core::errors::ScanError;
use crate::core::fs::scan_root;
use crate::core::item::{CleanableItem, CleanableItemId, ItemMetadata, NodeToolMetadata};
use crate::core::node_tool_provider::{
    NodeCacheLocation, NodeToolCacheProvider, NodeToolEnvironment,
};
use crate::core::permissions::MacPermission;
use crate::core::progress::{ProgressSink, ScanPhase, ScanProgress};
use crate::core::report::{CategoryScanResult, PartialScanReason, ScanCompleteness, ScanWarning};
use crate::core::risk::ItemCapability;
use crate::core::safety::contains_path;
use crate::core::scan_context::ScanContext;
use crate::core::scan_root::ScanRoot;
use crate::core::scanner::CleanerScanner;
use crate::node_tooling::default_providers;
use crate::paths::HostOs;

struct NodeCommandOutput {
    stdout: Vec<u8>,
    exit_code: Option<i32>,
}

trait NodeCommandRunner: Send + Sync {
    fn run(&self, program: &str, args: &[&str]) -> io::Result<NodeCommandOutput>;
}

struct ProcessNodeCommandRunner;

impl NodeCommandRunner for ProcessNodeCommandRunner {
    fn run(&self, program: &str, args: &[&str]) -> io::Result<NodeCommandOutput> {
        let output = Command::new(program).args(args).output()?;
        Ok(NodeCommandOutput {
            stdout: output.stdout,
            exit_code: output.status.code(),
        })
    }
}

pub struct NodeToolingCacheScanner {
    providers: Vec<Arc<dyn NodeToolCacheProvider>>,
    runner: Arc<dyn NodeCommandRunner>,
    forced_environment: Option<NodeToolEnvironment>,
}

impl NodeToolingCacheScanner {
    pub fn new() -> Self {
        Self {
            providers: default_providers(),
            runner: Arc::new(ProcessNodeCommandRunner),
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
            runner: Arc::new(ProcessNodeCommandRunner),
            forced_environment: Some(environment),
        }
    }

    #[cfg(test)]
    fn with_runner(runner: Arc<dyn NodeCommandRunner>) -> Self {
        Self {
            providers: default_providers(),
            runner,
            forced_environment: None,
        }
    }
}

#[derive(Default)]
struct NodeEnvironmentVariables {
    local_app_data: Option<PathBuf>,
    xdg_cache_home: Option<PathBuf>,
    npm_config_cache: Option<PathBuf>,
    yarn_cache_folder: Option<PathBuf>,
    npm_config_store_dir: Option<PathBuf>,
    bun_install: Option<PathBuf>,
    bun_install_cache_dir: Option<PathBuf>,
    nub_home: Option<PathBuf>,
}

impl NodeEnvironmentVariables {
    fn from_env(host: HostOs) -> Self {
        let get = |key| {
            std::env::var_os(key)
                .map(PathBuf::from)
                .filter(|path| is_absolute_for(host, path))
        };
        Self {
            local_app_data: get("LOCALAPPDATA"),
            xdg_cache_home: get("XDG_CACHE_HOME"),
            npm_config_cache: get("npm_config_cache"),
            yarn_cache_folder: get("YARN_CACHE_FOLDER"),
            npm_config_store_dir: get("npm_config_store_dir"),
            bun_install: get("BUN_INSTALL"),
            bun_install_cache_dir: get("BUN_INSTALL_CACHE_DIR"),
            nub_home: get("NUB_HOME"),
        }
    }
}

pub(crate) fn snapshot_environment(host: HostOs, home: Option<&Path>) -> NodeToolEnvironment {
    snapshot_environment_with_runner(host, home, &ProcessNodeCommandRunner)
}

fn snapshot_environment_with_runner(
    host: HostOs,
    home: Option<&Path>,
    runner: &dyn NodeCommandRunner,
) -> NodeToolEnvironment {
    snapshot_environment_from(host, home, NodeEnvironmentVariables::from_env(host), runner)
}

fn snapshot_environment_from(
    host: HostOs,
    home: Option<&Path>,
    variables: NodeEnvironmentVariables,
    runner: &dyn NodeCommandRunner,
) -> NodeToolEnvironment {
    let valid = |path: Option<PathBuf>| path.filter(|path| is_absolute_for(host, path));
    let local_app_data = valid(variables.local_app_data);
    let xdg_cache_home = valid(variables.xdg_cache_home);
    let npm_config_cache = valid(variables.npm_config_cache);
    let yarn_cache_folder = valid(variables.yarn_cache_folder);
    let npm_config_store_dir = valid(variables.npm_config_store_dir);
    let bun_install = valid(variables.bun_install);
    let bun_install_cache_dir = valid(variables.bun_install_cache_dir);
    let nub_home = valid(variables.nub_home);
    let cache_home = match host {
        HostOs::MacOs => home.map(|home| home.join("Library").join("Caches")),
        HostOs::Windows => local_app_data.clone(),
        HostOs::Unix => xdg_cache_home.or_else(|| home.map(|home| home.join(".cache"))),
    };

    NodeToolEnvironment {
        host,
        home: home.map(Path::to_path_buf),
        cache_home,
        local_app_data,
        npm_command_cache: npm_config_cache
            .is_none()
            .then(|| {
                query_path(
                    runner,
                    host,
                    tool_program(host, "npm"),
                    &["config", "get", "cache"],
                )
            })
            .flatten(),
        yarn_classic_command_cache: yarn_cache_folder
            .is_none()
            .then(|| query_path(runner, host, tool_program(host, "yarn"), &["cache", "dir"]))
            .flatten(),
        yarn_berry_command_global_folder: query_path(
            runner,
            host,
            tool_program(host, "yarn"),
            &["config", "get", "globalFolder"],
        ),
        pnpm_store: npm_config_store_dir
            .or_else(|| query_path(runner, host, tool_program(host, "pnpm"), &["store", "path"])),
        pnpm_command_cache: query_path(
            runner,
            host,
            tool_program(host, "pnpm"),
            &["config", "get", "cache-dir"],
        ),
        bun_command_cache: bun_install_cache_dir
            .is_none()
            .then(|| query_path(runner, host, tool_program(host, "bun"), &["pm", "cache"]))
            .flatten(),
        npm_config_cache,
        yarn_cache_folder,
        bun_install,
        bun_install_cache_dir,
        nub_home,
    }
}

fn tool_program(host: HostOs, tool: &'static str) -> &'static str {
    match (host, tool) {
        (HostOs::Windows, "npm") => "npm.cmd",
        (HostOs::Windows, "yarn") => "yarn.cmd",
        (HostOs::Windows, "pnpm") => "pnpm.cmd",
        _ => tool,
    }
}

fn query_path(
    runner: &dyn NodeCommandRunner,
    host: HostOs,
    program: &str,
    args: &[&str],
) -> Option<PathBuf> {
    let output = runner.run(program, args).ok()?;
    if output.exit_code != Some(0) {
        return None;
    }
    parse_command_path(host, &output.stdout).filter(|path| path.is_dir())
}

fn parse_command_path(host: HostOs, stdout: &[u8]) -> Option<PathBuf> {
    let text = std::str::from_utf8(stdout).ok()?.trim();
    if text.is_empty() || text.lines().count() != 1 {
        return None;
    }
    let path = PathBuf::from(text);
    is_absolute_for(host, &path).then_some(path)
}

fn is_absolute_for(host: HostOs, path: &Path) -> bool {
    match host {
        HostOs::MacOs | HostOs::Unix => path.is_absolute(),
        HostOs::Windows => {
            let text = path.to_string_lossy().replace('/', "\\");
            let bytes = text.as_bytes();
            text.starts_with("\\\\")
                || (bytes.len() >= 3
                    && bytes[0].is_ascii_alphabetic()
                    && bytes[1] == b':'
                    && bytes[2] == b'\\')
        }
    }
}

pub(crate) fn cleanup_allowed_roots(environment: &NodeToolEnvironment) -> Vec<PathBuf> {
    default_providers()
        .iter()
        .flat_map(|provider| provider.discover(environment))
        .filter(|location| location.allow_cleanup && !contains_node_modules(&location.path))
        .map(|location| location.path)
        .collect()
}

pub(crate) fn cleanup_denied_roots(environment: &NodeToolEnvironment) -> Vec<PathBuf> {
    default_providers()
        .iter()
        .flat_map(|provider| provider.denied_roots(environment))
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
        if cancellation.is_cancelled() {
            return Err(ScanError::Cancelled);
        }

        let environment = match &self.forced_environment {
            Some(environment) => environment.clone(),
            None => snapshot_environment_with_runner(
                crate::paths::current(),
                context.user_home.as_deref(),
                self.runner.as_ref(),
            ),
        };
        let host = environment.host;
        let denied_roots: Vec<PathBuf> = self
            .providers
            .iter()
            .flat_map(|provider| provider.denied_roots(&environment))
            .collect();

        let mut discovered: Vec<(Arc<dyn NodeToolCacheProvider>, NodeCacheLocation)> = Vec::new();
        for provider in &self.providers {
            for location in provider.discover(&environment) {
                if contains_node_modules(&location.path)
                    || denied_roots
                        .iter()
                        .any(|denied| contains_path(host, denied, &location.path))
                    || discovered
                        .iter()
                        .any(|(_, existing)| same_path(host, &existing.path, &location.path))
                {
                    continue;
                }
                discovered.push((Arc::clone(provider), location));
            }
        }
        let all_location_paths: Vec<PathBuf> = discovered
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
                        if contains_node_modules_tree(&entry.path)
                            || denied_roots
                                .iter()
                                .any(|denied| paths_overlap(host, &entry.path, denied))
                            || all_location_paths.iter().any(|other| {
                                !same_path(host, &location.path, other)
                                    && contains_path(host, &entry.path, other)
                            })
                        {
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

fn same_path(host: HostOs, left: &Path, right: &Path) -> bool {
    contains_path(host, left, right) && contains_path(host, right, left)
}

fn paths_overlap(host: HostOs, left: &Path, right: &Path) -> bool {
    contains_path(host, left, right) || contains_path(host, right, left)
}

fn contains_node_modules(path: &Path) -> bool {
    path.to_string_lossy()
        .split(['/', '\\'])
        .any(|component| component.eq_ignore_ascii_case("node_modules"))
}

/// A cache entry is moved as one unit, so a nested `node_modules` directory
/// denies the whole entry rather than merely subtracting its measured bytes.
fn contains_node_modules_tree(path: &Path) -> bool {
    if contains_node_modules(path) {
        return true;
    }
    let Ok(metadata) = std::fs::symlink_metadata(path) else {
        return true;
    };
    if metadata.file_type().is_symlink() {
        return true;
    }
    if !metadata.is_dir() {
        return false;
    }
    let mut pending = vec![path.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(directory) else {
            return true;
        };
        for entry in entries {
            let Ok(entry) = entry else {
                return true;
            };
            if entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.eq_ignore_ascii_case("node_modules"))
            {
                return true;
            }
            let Ok(file_type) = entry.file_type() else {
                return true;
            };
            if file_type.is_dir() && !file_type.is_symlink() {
                pending.push(entry.path());
            }
        }
    }
    false
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
    use std::collections::BTreeMap;
    use std::fs;
    use std::io;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};

    use crate::core::cancellation::CancellationToken;
    use crate::core::node_tool_provider::{
        NodeCacheLocation, NodeCacheScope, NodeToolCacheProvider, NodeToolEnvironment,
    };
    use crate::core::progress::{ProgressSink, ScanProgress};
    use crate::core::risk::{ItemCapability, RiskLevel, SelectionPolicy};
    use crate::core::scan_context::ScanContext;
    use crate::core::scan_root::AggregateMode;
    use crate::core::scanner::CleanerScanner;
    use crate::node_tooling_cache::{
        NodeCommandOutput, NodeCommandRunner, NodeEnvironmentVariables, NodeToolingCacheScanner,
        cleanup_allowed_roots, cleanup_denied_roots, parse_command_path, snapshot_environment_from,
        tool_program,
    };
    use crate::paths::HostOs;

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

    struct DeniedProvider {
        path: PathBuf,
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

    impl NodeToolCacheProvider for DeniedProvider {
        fn id(&self) -> &'static str {
            "denied"
        }

        fn display_name(&self) -> &'static str {
            "Denied"
        }

        fn discover(&self, _environment: &NodeToolEnvironment) -> Vec<NodeCacheLocation> {
            Vec::new()
        }

        fn denied_roots(&self, _environment: &NodeToolEnvironment) -> Vec<PathBuf> {
            vec![self.path.clone()]
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
        let inner = outer.join("bucket").join("inner-cache");
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

        // The nested root is deeper than one immediate child. The outer
        // provider must still omit the containing bucket rather than count or
        // delete the inner provider's bytes twice.
        assert!(
            !result
                .items
                .iter()
                .any(|item| item.path == outer.join("bucket")),
            "an outer item containing another provider root must be omitted"
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
    fn denied_store_nested_in_another_cache_is_neither_counted_nor_deletable() {
        let temp = std::env::temp_dir().join(format!(
            "dodo-cleaner-node-tooling-denied-{}",
            std::process::id()
        ));
        let cache = temp.join("cache");
        let store = cache.join("pnpm-store");
        fs::create_dir_all(store.join("v3")).expect("creates denied store");
        fs::write(store.join("v3").join("package"), b"store").expect("writes store");
        fs::create_dir_all(cache.join("safe-cache")).expect("creates safe cache");
        fs::write(cache.join("safe-cache").join("archive"), b"cache").expect("writes cache");

        let providers: Vec<Arc<dyn NodeToolCacheProvider>> = vec![
            Arc::new(StubProvider {
                id: "outer",
                locations: vec![allow_cleanup_location(cache.clone(), "Outer cache")],
            }),
            Arc::new(DeniedProvider {
                path: store.clone(),
            }),
        ];
        let scanner = NodeToolingCacheScanner::with_providers_and_environment(
            providers,
            NodeToolEnvironment {
                host: HostOs::Unix,
                ..Default::default()
            },
        );
        let result = scanner
            .scan(&empty_context(), &RecordingSink, &CancellationToken::new())
            .expect("scans while denying pnpm store");

        assert_eq!(result.items.len(), 1);
        assert_eq!(result.items[0].path, cache.join("safe-cache"));
        assert!(result.items.iter().all(|item| item.path != store));

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
            crate::core::report::ScanCompleteness::Partial { .. }
        ));
    }

    #[test]
    fn pnpm_store_is_a_denied_root_not_a_scan_only_result() {
        let temp = std::env::temp_dir().join(format!(
            "dodo-cleaner-node-tooling-allowlist-{}",
            std::process::id()
        ));
        let cache = temp.join("pnpm-cache");
        let store = temp.join("pnpm-store");
        fs::create_dir_all(cache.join("metadata")).expect("creates pnpm cache");
        fs::create_dir_all(store.join("v3")).expect("creates pnpm store");

        let environment = NodeToolEnvironment {
            host: HostOs::Unix,
            pnpm_command_cache: Some(cache.clone()),
            pnpm_store: Some(store.clone()),
            ..Default::default()
        };
        assert_eq!(cleanup_allowed_roots(&environment), vec![cache]);
        assert_eq!(cleanup_denied_roots(&environment), vec![store]);

        fs::remove_dir_all(&temp).expect("removes temp tree");
    }

    #[test]
    fn node_modules_is_never_a_result_or_cleanup_root() {
        let temp = std::env::temp_dir().join(format!(
            "dodo-cleaner-node-tooling-node-modules-{}",
            std::process::id()
        ));
        let cache = temp.join("cache");
        fs::create_dir_all(cache.join("safe-cache")).expect("creates safe cache item");
        fs::write(cache.join("safe-cache").join("archive"), b"cache")
            .expect("writes safe cache item");
        fs::create_dir_all(
            cache
                .join("package-cache")
                .join("nested")
                .join("node_modules")
                .join("project-dependency"),
        )
        .expect("creates excluded dependencies");
        fs::write(
            cache
                .join("package-cache")
                .join("nested")
                .join("node_modules")
                .join("project-dependency")
                .join("index.js"),
            b"dependency",
        )
        .expect("writes excluded dependency");

        let providers: Vec<Arc<dyn NodeToolCacheProvider>> = vec![Arc::new(StubProvider {
            id: "provider",
            locations: vec![allow_cleanup_location(cache.clone(), "Cache")],
        })];
        let scanner = NodeToolingCacheScanner::with_providers_and_environment(
            providers,
            NodeToolEnvironment {
                host: HostOs::Unix,
                ..Default::default()
            },
        );
        let result = scanner
            .scan(&empty_context(), &RecordingSink, &CancellationToken::new())
            .expect("scans while excluding node_modules");

        assert_eq!(result.items.len(), 1);
        assert_eq!(result.items[0].path, cache.join("safe-cache"));
        assert!(
            result
                .items
                .iter()
                .all(|item| !item.path.to_string_lossy().contains("node_modules"))
        );

        let forbidden_root = temp.join("node_modules").join("cache");
        fs::create_dir_all(&forbidden_root).expect("creates forbidden root");
        let environment = NodeToolEnvironment {
            host: HostOs::Unix,
            npm_config_cache: Some(temp.join("node_modules")),
            ..Default::default()
        };
        assert!(cleanup_allowed_roots(&environment).is_empty());

        fs::remove_dir_all(&temp).expect("removes temp tree");
    }

    struct FixtureRunner {
        outputs: BTreeMap<String, PathBuf>,
        calls: Mutex<Vec<String>>,
    }

    impl NodeCommandRunner for FixtureRunner {
        fn run(&self, program: &str, args: &[&str]) -> io::Result<NodeCommandOutput> {
            let key = format!("{program} {}", args.join(" "));
            self.calls.lock().expect("calls lock").push(key.clone());
            let Some(path) = self.outputs.get(&key) else {
                return Err(io::Error::new(io::ErrorKind::NotFound, "fixture missing"));
            };
            Ok(NodeCommandOutput {
                stdout: format!("{}\n", path.display()).into_bytes(),
                exit_code: Some(0),
            })
        }
    }

    #[test]
    fn fixed_argv_queries_populate_every_tool_answer() {
        let temp = std::env::temp_dir().join(format!(
            "dodo-cleaner-node-tooling-commands-{}",
            std::process::id()
        ));
        let paths = [
            ("npm config get cache", temp.join("npm")),
            ("yarn cache dir", temp.join("yarn-classic")),
            (
                "yarn config get globalFolder",
                temp.join("yarn-berry-global"),
            ),
            ("pnpm store path", temp.join("pnpm-store")),
            ("pnpm config get cache-dir", temp.join("pnpm-cache")),
            ("bun pm cache", temp.join("bun-cache")),
        ];
        for (_, path) in &paths {
            fs::create_dir_all(path).expect("creates command fixture path");
        }
        let runner = Arc::new(FixtureRunner {
            outputs: paths
                .iter()
                .map(|(command, path)| (command.to_string(), path.clone()))
                .collect(),
            calls: Mutex::new(Vec::new()),
        });
        let _scanner = NodeToolingCacheScanner::with_runner(runner.clone());
        let environment = snapshot_environment_from(
            HostOs::Unix,
            Some(temp.as_path()),
            NodeEnvironmentVariables::default(),
            runner.as_ref(),
        );

        assert_eq!(environment.npm_command_cache, Some(temp.join("npm")));
        assert_eq!(
            environment.yarn_classic_command_cache,
            Some(temp.join("yarn-classic"))
        );
        assert_eq!(
            environment.yarn_berry_command_global_folder,
            Some(temp.join("yarn-berry-global"))
        );
        assert_eq!(environment.pnpm_store, Some(temp.join("pnpm-store")));
        assert_eq!(
            environment.pnpm_command_cache,
            Some(temp.join("pnpm-cache"))
        );
        assert_eq!(environment.bun_command_cache, Some(temp.join("bun-cache")));
        assert_eq!(runner.calls.lock().expect("calls lock").len(), 6);

        fs::remove_dir_all(&temp).expect("removes temp tree");
    }

    #[test]
    fn synthetic_windows_and_xdg_records_resolve_without_host_environment() {
        struct MissingRunner;
        impl NodeCommandRunner for MissingRunner {
            fn run(&self, _program: &str, _args: &[&str]) -> io::Result<NodeCommandOutput> {
                Err(io::Error::new(io::ErrorKind::NotFound, "not installed"))
            }
        }

        let local = PathBuf::from(r"C:\Users\Ada\AppData\Local");
        let windows = snapshot_environment_from(
            HostOs::Windows,
            Some(Path::new(r"C:\Users\Ada")),
            NodeEnvironmentVariables {
                local_app_data: Some(local.clone()),
                ..Default::default()
            },
            &MissingRunner,
        );
        assert_eq!(windows.cache_home, Some(local.clone()));
        assert_eq!(windows.local_app_data, Some(local));

        let linux = snapshot_environment_from(
            HostOs::Unix,
            Some(Path::new("/home/ada")),
            NodeEnvironmentVariables {
                xdg_cache_home: Some(PathBuf::from("/mnt/cache")),
                ..Default::default()
            },
            &MissingRunner,
        );
        assert_eq!(linux.cache_home, Some(PathBuf::from("/mnt/cache")));

        let invalid_xdg = snapshot_environment_from(
            HostOs::Unix,
            Some(Path::new("/home/ada")),
            NodeEnvironmentVariables {
                xdg_cache_home: Some(PathBuf::from("relative/cache")),
                ..Default::default()
            },
            &MissingRunner,
        );
        assert_eq!(
            invalid_xdg.cache_home,
            Some(PathBuf::from("/home/ada/.cache"))
        );
    }

    #[test]
    fn windows_uses_command_wrappers_without_a_shell_command_string() {
        assert_eq!(tool_program(HostOs::Windows, "npm"), "npm.cmd");
        assert_eq!(tool_program(HostOs::Windows, "yarn"), "yarn.cmd");
        assert_eq!(tool_program(HostOs::Windows, "pnpm"), "pnpm.cmd");
        assert_eq!(tool_program(HostOs::Windows, "bun"), "bun");
        assert_eq!(tool_program(HostOs::Unix, "npm"), "npm");
    }

    #[test]
    fn command_output_must_be_one_absolute_path() {
        assert_eq!(
            parse_command_path(
                HostOs::Windows,
                b"C:\\Users\\Ada\\AppData\\Local\\npm-cache\r\n"
            ),
            Some(PathBuf::from(r"C:\Users\Ada\AppData\Local\npm-cache"))
        );
        assert_eq!(
            parse_command_path(HostOs::Unix, b"/home/ada/.cache/yarn\n"),
            Some(PathBuf::from("/home/ada/.cache/yarn"))
        );
        for output in [
            b"relative/cache\n".as_slice(),
            b"/one/path\n/second/path\n".as_slice(),
            b"\xff".as_slice(),
        ] {
            assert_eq!(parse_command_path(HostOs::Unix, output), None);
        }
    }
}
