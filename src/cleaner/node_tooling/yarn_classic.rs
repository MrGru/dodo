//! Yarn Classic (Yarn 1.x)'s global download cache.
//!
//! The default follows each host's cache convention and is overridable with
//! `YARN_CACHE_FOLDER` or Yarn's own `yarn cache dir` answer. This provider
//! does not add a separate logs location:
//! Yarn Classic has no documented, stable log directory distinct from its
//! cache the way npm's `_logs` is — inventing one would mean guessing at a
//! path this phase cannot justify. See `docs/cleaner/known-limitations.md`.
//! Never touches project `.yarnrc` or `package.json`.

use std::path::{Path, PathBuf};

use crate::cleaner::core::node_tool_provider::{
    NodeCacheLocation, NodeCacheScope, NodeToolCacheProvider, NodeToolEnvironment,
};
use crate::cleaner::core::risk::{RiskLevel, SelectionPolicy};
use crate::cleaner::core::scan_root::AggregateMode;
use crate::paths::HostOs;

pub(crate) struct YarnClassicProvider;

fn resolve_cache_root(
    env_override: Option<&Path>,
    command: Option<&Path>,
    host: HostOs,
    cache_home: Option<&Path>,
) -> Option<PathBuf> {
    env_override.or(command).map(Path::to_path_buf).or_else(|| {
        cache_home.map(|root| match host {
            HostOs::Windows => root.join("Yarn").join("Cache"),
            HostOs::MacOs => root.join("Yarn"),
            HostOs::Unix => root.join("yarn"),
        })
    })
}

impl NodeToolCacheProvider for YarnClassicProvider {
    fn id(&self) -> &'static str {
        "yarn-classic"
    }

    fn display_name(&self) -> &'static str {
        "Yarn Classic"
    }

    fn discover(&self, environment: &NodeToolEnvironment) -> Vec<NodeCacheLocation> {
        let Some(cache_root) = resolve_cache_root(
            environment.yarn_cache_folder.as_deref(),
            environment.yarn_classic_command_cache.as_deref(),
            environment.host,
            environment.cache_home.as_deref(),
        ) else {
            return Vec::new();
        };
        if !cache_root.is_dir() {
            return Vec::new();
        }

        vec![NodeCacheLocation {
            path: cache_root,
            group: "Yarn Classic cache".to_string(),
            scope: NodeCacheScope::Global,
            risk: RiskLevel::SafeRecreatable,
            selection_policy: SelectionPolicy::SelectedByDefault,
            allow_cleanup: true,
            aggregate_mode: AggregateMode::ImmediateChildren,
            explanation: "Yarn Classic's global package download cache; Yarn re-downloads a \
                package into it again if it is needed."
                .to_string(),
        }]
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn resolve_cache_root_prefers_the_environment_override() {
        let resolved = resolve_cache_root(
            Some(Path::new("/custom/yarn-cache")),
            Some(Path::new("/queried/yarn-cache")),
            HostOs::MacOs,
            Some(Path::new("/Users/example/Library/Caches")),
        );
        assert_eq!(resolved, Some(PathBuf::from("/custom/yarn-cache")));
    }

    #[test]
    fn resolve_cache_root_falls_back_to_the_default_location() {
        let resolved = resolve_cache_root(
            None,
            None,
            HostOs::Unix,
            Some(Path::new("/home/example/.cache")),
        );
        assert_eq!(resolved, Some(PathBuf::from("/home/example/.cache/yarn")));

        let windows = Path::new(r"C:\Users\example\AppData\Local");
        assert_eq!(
            resolve_cache_root(None, None, HostOs::Windows, Some(windows)),
            Some(windows.join("Yarn").join("Cache"))
        );
    }

    #[test]
    fn discover_reports_the_cache_when_present() {
        let temp =
            std::env::temp_dir().join(format!("dodo-cleaner-yarn-classic-{}", std::process::id()));
        fs::create_dir_all(temp.join("v6").join("npm-left-pad-1.0.0")).expect("creates cache tree");

        let provider = YarnClassicProvider;
        let environment = NodeToolEnvironment {
            yarn_cache_folder: Some(temp.clone()),
            ..Default::default()
        };
        let locations = provider.discover(&environment);

        assert_eq!(locations.len(), 1);
        assert_eq!(locations[0].group, "Yarn Classic cache");
        assert!(locations[0].allow_cleanup);
        assert_eq!(
            locations[0].selection_policy,
            SelectionPolicy::SelectedByDefault
        );

        fs::remove_dir_all(&temp).expect("removes temp tree");
    }

    #[test]
    fn discover_reports_nothing_when_yarn_classic_was_never_used() {
        let temp = std::env::temp_dir().join(format!(
            "dodo-cleaner-yarn-classic-missing-{}",
            std::process::id()
        ));

        let provider = YarnClassicProvider;
        let environment = NodeToolEnvironment {
            home: Some(temp),
            ..Default::default()
        };
        assert!(provider.discover(&environment).is_empty());
    }
}
