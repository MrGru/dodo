//! Yarn Classic (Yarn 1.x)'s global download cache.
//!
//! Standard location on macOS is `~/Library/Caches/Yarn`, overridable with
//! `YARN_CACHE_FOLDER`. This provider does not add a separate logs location:
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

pub(crate) struct YarnClassicProvider;

/// Yarn Classic's cache root: `YARN_CACHE_FOLDER` if set, else
/// `~/Library/Caches/Yarn`.
fn resolve_cache_root(env_override: Option<&Path>, home: Option<&Path>) -> Option<PathBuf> {
    if let Some(path) = env_override {
        return Some(path.to_path_buf());
    }
    home.map(|home| home.join("Library").join("Caches").join("Yarn"))
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
            environment.home.as_deref(),
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
            Some(Path::new("/Users/example")),
        );
        assert_eq!(resolved, Some(PathBuf::from("/custom/yarn-cache")));
    }

    #[test]
    fn resolve_cache_root_falls_back_to_the_default_location() {
        let resolved = resolve_cache_root(None, Some(Path::new("/Users/example")));
        assert_eq!(
            resolved,
            Some(PathBuf::from("/Users/example/Library/Caches/Yarn"))
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
