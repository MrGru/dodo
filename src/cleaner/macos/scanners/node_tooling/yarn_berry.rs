//! Yarn Berry (Yarn 2+)'s global cache only.
//!
//! The ticket asks this provider to consider three things: project-local
//! `.yarn/cache`, the global cache, and Plug'n'Play (`.pnp.cjs`/
//! `.pnp.data.json`) files. This provider implements exactly one of them.
//!
//! Project-local `.yarn/cache` and PnP files both require knowing where a
//! Yarn Berry *project* lives on disk — there is no fixed, home-relative
//! convention for either, unlike a tool's own global cache. Finding one
//! would mean crawling the home directory for arbitrary project checkouts,
//! which the ticket rules out for normal cleanup ("Do not scan the entire
//! home directory"). Both stay out of scope for discovery; see
//! `docs/cleaner/known-limitations.md`. Because this provider never surfaces
//! either, the ticket's "must be shown separately and not selected by
//! default" requirement for project-local caches is satisfied by
//! construction — nothing project-local ever appears at all.
//!
//! The global cache location is Yarn Berry's own convention
//! (`enableGlobalCache`/`globalFolder`), separate from Yarn Classic's. This
//! provider defaults to `~/Library/Caches/Yarn/Berry` and deliberately does
//! *not* honor `YARN_CACHE_FOLDER`: that variable's documented semantics are
//! Yarn Classic's, and applying it here too would risk resolving to the
//! exact same directory as Yarn Classic's own location whenever a user sets
//! it, double-reporting one cache under two provider names. Since the
//! default path nests *inside* `~/Library/Caches/Yarn` — the same directory
//! Yarn Classic scans — `macos::scanners::node_tooling_cache` excludes any
//! immediate child that is also another provider's own location, the same
//! technique `homebrew_cache` uses for its `Cask`/`Logs` subdirectories.

use std::path::{Path, PathBuf};

use crate::cleaner::core::node_tool_provider::{
    NodeCacheLocation, NodeCacheScope, NodeToolCacheProvider, NodeToolEnvironment,
};
use crate::cleaner::core::risk::{RiskLevel, SelectionPolicy};
use crate::cleaner::core::scan_root::AggregateMode;

pub(crate) struct YarnBerryProvider;

/// Yarn Berry's default global cache root. No environment-variable override
/// is honored here — see this module's doc comment for why.
fn resolve_global_cache_root(home: Option<&Path>) -> Option<PathBuf> {
    home.map(|home| {
        home.join("Library")
            .join("Caches")
            .join("Yarn")
            .join("Berry")
    })
}

impl NodeToolCacheProvider for YarnBerryProvider {
    fn id(&self) -> &'static str {
        "yarn-berry"
    }

    fn display_name(&self) -> &'static str {
        "Yarn Berry"
    }

    fn discover(&self, environment: &NodeToolEnvironment) -> Vec<NodeCacheLocation> {
        let Some(cache_root) = resolve_global_cache_root(environment.home.as_deref()) else {
            return Vec::new();
        };
        if !cache_root.is_dir() {
            return Vec::new();
        }

        vec![NodeCacheLocation {
            path: cache_root,
            group: "Yarn Berry global cache".to_string(),
            scope: NodeCacheScope::Global,
            risk: RiskLevel::SafeRecreatable,
            selection_policy: SelectionPolicy::SelectedByDefault,
            allow_cleanup: true,
            aggregate_mode: AggregateMode::ImmediateChildren,
            explanation: "Yarn Berry's global package cache (used when a project enables \
                Yarn's global cache); Yarn re-downloads a package into it again if it is \
                needed. Project-local .yarn/cache and Plug'n'Play files are not discovered by \
                this scanner."
                .to_string(),
        }]
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn resolve_global_cache_root_nests_under_yarn_classics_own_cache_directory() {
        let resolved = resolve_global_cache_root(Some(Path::new("/Users/example")));
        assert_eq!(
            resolved,
            Some(PathBuf::from("/Users/example/Library/Caches/Yarn/Berry"))
        );
    }

    #[test]
    fn discover_reports_the_global_cache_when_present() {
        let home_root =
            std::env::temp_dir().join(format!("dodo-cleaner-yarn-berry-{}", std::process::id()));
        let berry_root = home_root
            .join("Library")
            .join("Caches")
            .join("Yarn")
            .join("Berry");
        fs::create_dir_all(&berry_root).expect("creates berry cache dir");
        fs::write(berry_root.join("cached-package.zip"), vec![0u8; 16])
            .expect("writes a cached archive");

        let provider = YarnBerryProvider;
        let environment = NodeToolEnvironment {
            home: Some(home_root.clone()),
            ..Default::default()
        };
        let locations = provider.discover(&environment);

        assert_eq!(locations.len(), 1);
        assert_eq!(locations[0].group, "Yarn Berry global cache");
        assert_eq!(locations[0].path, berry_root);
        assert!(locations[0].allow_cleanup);

        fs::remove_dir_all(&home_root).expect("removes temp tree");
    }

    #[test]
    fn discover_reports_nothing_when_yarn_berry_was_never_used() {
        let temp = std::env::temp_dir().join(format!(
            "dodo-cleaner-yarn-berry-missing-{}",
            std::process::id()
        ));

        let provider = YarnBerryProvider;
        let environment = NodeToolEnvironment {
            home: Some(temp),
            ..Default::default()
        };
        assert!(provider.discover(&environment).is_empty());
    }
}
