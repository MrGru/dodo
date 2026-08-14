//! pnpm's registry-metadata cache. Its content-addressable store is explicitly
//! denied: it is shared by linked projects, is not disposable cache, never
//! becomes a result row and is never allow-listed for deletion.
//!
//! `PNPM_HOME` is intentionally not used for store discovery. It is pnpm's
//! executable directory, not the content-addressable store; the old macOS-only
//! provider quietly treated it as a store override. Store identity now comes
//! only from `npm_config_store_dir` or `pnpm store path` in the shared snapshot.

use std::path::{Path, PathBuf};

use crate::cleaner::core::node_tool_provider::{
    NodeCacheLocation, NodeCacheScope, NodeToolCacheProvider, NodeToolEnvironment,
};
use crate::cleaner::core::risk::{RiskLevel, SelectionPolicy};
use crate::cleaner::core::safety::contains_path;
use crate::cleaner::core::scan_root::AggregateMode;
use crate::paths::HostOs;

pub(crate) struct PnpmProvider;

fn resolve_cache_root(
    command: Option<&Path>,
    host: HostOs,
    cache_home: Option<&Path>,
) -> Option<PathBuf> {
    command.map(Path::to_path_buf).or_else(|| {
        cache_home.map(|root| match host {
            HostOs::Windows => root.join("pnpm-cache"),
            HostOs::MacOs | HostOs::Unix => root.join("pnpm"),
        })
    })
}

fn overlaps(host: HostOs, left: &Path, right: &Path) -> bool {
    contains_path(host, left, right) || contains_path(host, right, left)
}

impl NodeToolCacheProvider for PnpmProvider {
    fn id(&self) -> &'static str {
        "pnpm"
    }

    fn display_name(&self) -> &'static str {
        "pnpm"
    }

    fn discover(&self, environment: &NodeToolEnvironment) -> Vec<NodeCacheLocation> {
        let Some(cache_root) = resolve_cache_root(
            environment.pnpm_command_cache.as_deref(),
            environment.host,
            environment.cache_home.as_deref(),
        ) else {
            return Vec::new();
        };
        if !cache_root.is_dir()
            || environment
                .pnpm_store
                .as_deref()
                .is_some_and(|store| overlaps(environment.host, &cache_root, store))
        {
            return Vec::new();
        }

        vec![NodeCacheLocation {
            path: cache_root,
            group: "pnpm cache".to_string(),
            scope: NodeCacheScope::Global,
            risk: RiskLevel::SafeRecreatable,
            selection_policy: SelectionPolicy::SelectedByDefault,
            allow_cleanup: true,
            aggregate_mode: AggregateMode::ImmediateChildren,
            explanation: "pnpm's registry-metadata cache, distinct from its shared package store; pnpm re-fetches it when needed.".to_string(),
        }]
    }

    fn denied_roots(&self, environment: &NodeToolEnvironment) -> Vec<PathBuf> {
        environment.pnpm_store.iter().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn defaults_follow_each_platform_cache_home() {
        assert_eq!(
            resolve_cache_root(None, HostOs::Unix, Some(Path::new("/home/example/.cache"))),
            Some(PathBuf::from("/home/example/.cache/pnpm"))
        );
        let local = Path::new(r"C:\Users\example\AppData\Local");
        assert_eq!(
            resolve_cache_root(None, HostOs::Windows, Some(local)),
            Some(local.join("pnpm-cache"))
        );
    }

    #[test]
    fn store_is_denied_and_never_reported() {
        let temp = std::env::temp_dir().join(format!("dodo-cleaner-pnpm-{}", std::process::id()));
        let cache = temp.join("cache");
        let store = temp.join("store");
        fs::create_dir_all(cache.join("metadata")).expect("creates cache");
        fs::create_dir_all(store.join("v3")).expect("creates store");

        let provider = PnpmProvider;
        let environment = NodeToolEnvironment {
            host: HostOs::Unix,
            pnpm_command_cache: Some(cache.clone()),
            pnpm_store: Some(store.clone()),
            ..Default::default()
        };
        let locations = provider.discover(&environment);

        assert_eq!(locations.len(), 1);
        assert_eq!(locations[0].path, cache);
        assert!(locations[0].allow_cleanup);
        assert_eq!(provider.denied_roots(&environment), vec![store.clone()]);
        assert!(locations.iter().all(|location| location.path != store));

        fs::remove_dir_all(&temp).expect("removes temp tree");
    }

    #[test]
    fn a_cache_overlapping_the_store_is_refused() {
        let temp =
            std::env::temp_dir().join(format!("dodo-cleaner-pnpm-overlap-{}", std::process::id()));
        let store = temp.join("store");
        fs::create_dir_all(store.join("metadata")).expect("creates overlap tree");

        let provider = PnpmProvider;
        let environment = NodeToolEnvironment {
            host: HostOs::Unix,
            pnpm_command_cache: Some(store.join("metadata")),
            pnpm_store: Some(store.clone()),
            ..Default::default()
        };
        assert!(provider.discover(&environment).is_empty());
        assert_eq!(provider.denied_roots(&environment), vec![store]);

        fs::remove_dir_all(&temp).expect("removes temp tree");
    }

    #[test]
    fn discover_reports_nothing_when_pnpm_was_never_used() {
        let temp =
            std::env::temp_dir().join(format!("dodo-cleaner-pnpm-missing-{}", std::process::id()));
        let provider = PnpmProvider;
        let environment = NodeToolEnvironment {
            host: HostOs::Unix,
            cache_home: Some(temp),
            ..Default::default()
        };
        assert!(provider.discover(&environment).is_empty());
    }
}
