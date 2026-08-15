//! npm's own cache, split from its own log output.
//!
//! npm's real on-disk cache subfolder is always `_cacache`, regardless of
//! whether the cache root is the host default (`~/.npm` on macOS/Linux or
//! `%LOCALAPPDATA%\\npm-cache` on Windows) or a configured location
//! (`npm_config_cache` or `npm config get cache`) —
//! npm writes its content-addressable cache to `<cache root>/_cacache`
//! either way. `<cache root>/_logs` is npm's own log output, reported as a
//! separate group rather than folded into the cache group, per the ticket
//! ("separate cache from logs"). `~/.npmrc` and
//! `<cache root>/_update-notifier-last-checked` are never scanned — this
//! provider only ever names `_cacache` and `_logs`, nothing else under the
//! cache root.

use std::path::{Path, PathBuf};

use crate::core::node_tool_provider::{
    NodeCacheLocation, NodeCacheScope, NodeToolCacheProvider, NodeToolEnvironment,
};
use crate::core::risk::{RiskLevel, SelectionPolicy};
use crate::core::scan_root::AggregateMode;
use crate::paths::HostOs;

pub(crate) struct NpmProvider;

/// npm's cache root: explicit environment override, npm's own answer, then
/// the platform default.
fn resolve_cache_root(
    configured: Option<&Path>,
    command: Option<&Path>,
    host: HostOs,
    home: Option<&Path>,
    local_app_data: Option<&Path>,
) -> Option<PathBuf> {
    configured
        .or(command)
        .map(Path::to_path_buf)
        .or_else(|| match host {
            HostOs::Windows => local_app_data.map(|root| root.join("npm-cache")),
            HostOs::MacOs | HostOs::Unix => home.map(|home| home.join(".npm")),
        })
}

impl NodeToolCacheProvider for NpmProvider {
    fn id(&self) -> &'static str {
        "npm"
    }

    fn display_name(&self) -> &'static str {
        "npm"
    }

    fn discover(&self, environment: &NodeToolEnvironment) -> Vec<NodeCacheLocation> {
        let Some(cache_root) = resolve_cache_root(
            environment.npm_config_cache.as_deref(),
            environment.npm_command_cache.as_deref(),
            environment.host,
            environment.home.as_deref(),
            environment.local_app_data.as_deref(),
        ) else {
            return Vec::new();
        };

        let mut locations = Vec::new();

        let cacache = cache_root.join("_cacache");
        if cacache.is_dir() {
            locations.push(NodeCacheLocation {
                path: cacache,
                group: "npm cache".to_string(),
                scope: NodeCacheScope::Global,
                risk: RiskLevel::SafeRecreatable,
                selection_policy: SelectionPolicy::SelectedByDefault,
                allow_cleanup: true,
                aggregate_mode: AggregateMode::ImmediateChildren,
                explanation: "npm's content-addressable download cache; npm re-downloads a \
                    package into it again if it is needed."
                    .to_string(),
            });
        }

        let logs = cache_root.join("_logs");
        if logs.is_dir() {
            locations.push(NodeCacheLocation {
                path: logs,
                group: "npm logs".to_string(),
                scope: NodeCacheScope::Global,
                risk: RiskLevel::SafeRecreatable,
                selection_policy: SelectionPolicy::SelectedByDefault,
                allow_cleanup: true,
                aggregate_mode: AggregateMode::ImmediateChildren,
                explanation: "npm's own log output, kept separate from its download cache."
                    .to_string(),
            });
        }

        locations
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn resolve_cache_root_prefers_the_configured_override() {
        let resolved = resolve_cache_root(
            Some(Path::new("/custom/npm-cache")),
            Some(Path::new("/queried/npm-cache")),
            HostOs::MacOs,
            Some(Path::new("/Users/example")),
            None,
        );
        assert_eq!(resolved, Some(PathBuf::from("/custom/npm-cache")));
    }

    #[test]
    fn resolve_cache_root_falls_back_to_the_default_dot_npm() {
        let resolved = resolve_cache_root(
            None,
            None,
            HostOs::MacOs,
            Some(Path::new("/Users/example")),
            None,
        );
        assert_eq!(resolved, Some(PathBuf::from("/Users/example/.npm")));
    }

    #[test]
    fn resolve_cache_root_is_none_without_an_override_or_a_home() {
        assert_eq!(
            resolve_cache_root(None, None, HostOs::Unix, None, None),
            None
        );
    }

    #[test]
    fn windows_default_uses_local_app_data() {
        let local = Path::new(r"C:\Users\example\AppData\Local");
        assert_eq!(
            resolve_cache_root(None, None, HostOs::Windows, None, Some(local)),
            Some(local.join("npm-cache"))
        );
    }

    #[test]
    fn discover_reports_cache_and_logs_as_separate_groups_when_both_exist() {
        let temp = std::env::temp_dir().join(format!("dodo-cleaner-npm-{}", std::process::id()));
        fs::create_dir_all(temp.join("_cacache").join("content-v2")).expect("creates cacache");
        fs::create_dir_all(temp.join("_logs")).expect("creates logs dir");
        fs::write(
            temp.join("_logs")
                .join("2024-01-01T00_00_00_000Z-debug-0.log"),
            vec![0u8; 8],
        )
        .expect("writes a log file");

        let provider = NpmProvider;
        let environment = NodeToolEnvironment {
            npm_config_cache: Some(temp.clone()),
            ..Default::default()
        };
        let locations = provider.discover(&environment);

        assert_eq!(locations.len(), 2);
        assert!(
            locations
                .iter()
                .any(|location| location.group == "npm cache" && location.allow_cleanup)
        );
        assert!(
            locations
                .iter()
                .any(|location| location.group == "npm logs" && location.allow_cleanup)
        );
        assert!(
            locations
                .iter()
                .all(|location| location.selection_policy == SelectionPolicy::SelectedByDefault)
        );

        fs::remove_dir_all(&temp).expect("removes temp tree");
    }

    #[test]
    fn discover_reports_nothing_when_npm_was_never_used() {
        let temp =
            std::env::temp_dir().join(format!("dodo-cleaner-npm-missing-{}", std::process::id()));

        let provider = NpmProvider;
        let environment = NodeToolEnvironment {
            home: Some(temp),
            ..Default::default()
        };
        assert!(provider.discover(&environment).is_empty());
    }

    #[test]
    fn discover_never_reports_npmrc_or_the_update_notifier_marker() {
        let temp =
            std::env::temp_dir().join(format!("dodo-cleaner-npm-npmrc-{}", std::process::id()));
        fs::create_dir_all(temp.join("_cacache")).expect("creates cacache");
        fs::write(temp.join("_update-notifier-last-checked"), []).expect("writes marker file");

        let provider = NpmProvider;
        let environment = NodeToolEnvironment {
            npm_config_cache: Some(temp.clone()),
            ..Default::default()
        };
        let locations = provider.discover(&environment);

        assert!(locations.iter().all(
            |location| location.path.file_name().and_then(|n| n.to_str())
                != Some("_update-notifier-last-checked")
        ));

        fs::remove_dir_all(&temp).expect("removes temp tree");
    }
}
