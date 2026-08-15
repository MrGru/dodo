//! Bun's package install cache — the one Bun-owned directory this phase can
//! name with confidence.
//!
//! The ticket asks this provider to separate package cache, installation
//! cache, logs, temporary data, and project-local dependencies. Bun's real,
//! documented on-disk convention only gives this phase one directory to
//! stand behind: `<BUN_INSTALL>/install/cache` (default `~/.bun/install/
//! cache`, overridable directly with `BUN_INSTALL_CACHE_DIR`) — the same
//! directory `bun pm cache` reports, holding both downloaded package
//! tarballs and Bun's own installation bookkeeping for them. This provider
//! does not fabricate a second, distinct "installation cache" subfolder
//! beyond it, and does not claim a stable logs directory or a temporary-data
//! directory exists under `~/.bun` — none of the three has a documented,
//! version-stable location this phase can point at with the same confidence
//! as the install cache. See `docs/cleaner/known-limitations.md`.
//!
//! Project-local dependencies (a project's own `node_modules`, or any
//! Bun-specific per-project cache) are out of scope for discovery for the
//! same reason Yarn Berry's project-local `.yarn/cache` is: finding them
//! would mean crawling the home directory for arbitrary project checkouts,
//! which the ticket rules out. `node_modules` is never touched by this
//! provider, full stop — the shared "never delete `node_modules`
//! automatically" rule applies regardless of scope.
//!
//! Never touches Bun's own executable, `~/.bun/bin`, or `~/.bunfig.toml`.

use std::path::{Path, PathBuf};

use crate::core::node_tool_provider::{
    NodeCacheLocation, NodeCacheScope, NodeToolCacheProvider, NodeToolEnvironment,
};
use crate::core::risk::{RiskLevel, SelectionPolicy};
use crate::core::scan_root::AggregateMode;

pub(crate) struct BunProvider;

/// Bun's install cache: direct override, Bun's own answer, then the configured
/// or default install root.
fn resolve_cache_root(
    bun_install: Option<&Path>,
    cache_override: Option<&Path>,
    command: Option<&Path>,
    home: Option<&Path>,
) -> Option<PathBuf> {
    if let Some(path) = cache_override.or(command) {
        return Some(path.to_path_buf());
    }
    let install_root = match bun_install {
        Some(path) => Some(path.to_path_buf()),
        None => home.map(|home| home.join(".bun")),
    };
    install_root.map(|root| root.join("install").join("cache"))
}

impl NodeToolCacheProvider for BunProvider {
    fn id(&self) -> &'static str {
        "bun"
    }

    fn display_name(&self) -> &'static str {
        "Bun"
    }

    fn discover(&self, environment: &NodeToolEnvironment) -> Vec<NodeCacheLocation> {
        let Some(cache_root) = resolve_cache_root(
            environment.bun_install.as_deref(),
            environment.bun_install_cache_dir.as_deref(),
            environment.bun_command_cache.as_deref(),
            environment.home.as_deref(),
        ) else {
            return Vec::new();
        };
        if !cache_root.is_dir() {
            return Vec::new();
        }

        vec![NodeCacheLocation {
            path: cache_root,
            group: "Bun install cache".to_string(),
            scope: NodeCacheScope::Global,
            risk: RiskLevel::SafeRecreatable,
            selection_policy: SelectionPolicy::SelectedByDefault,
            allow_cleanup: true,
            aggregate_mode: AggregateMode::ImmediateChildren,
            explanation: "Bun's package install cache; Bun re-downloads a package into it \
                again if it is needed. Never includes Bun's executable, ~/.bun/bin, \
                ~/.bunfig.toml, or any project's node_modules."
                .to_string(),
        }]
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn resolve_cache_root_prefers_the_direct_cache_override() {
        let resolved = resolve_cache_root(
            Some(Path::new("/custom/bun-install")),
            Some(Path::new("/custom/bun-cache")),
            Some(Path::new("/queried/bun-cache")),
            Some(Path::new("/Users/example")),
        );
        assert_eq!(resolved, Some(PathBuf::from("/custom/bun-cache")));
    }

    #[test]
    fn resolve_cache_root_derives_from_a_configured_install_root() {
        let resolved = resolve_cache_root(
            Some(Path::new("/custom/bun-install")),
            None,
            None,
            Some(Path::new("/Users/example")),
        );
        assert_eq!(
            resolved,
            Some(PathBuf::from("/custom/bun-install/install/cache"))
        );
    }

    #[test]
    fn resolve_cache_root_uses_buns_own_answer_before_install_root() {
        let resolved = resolve_cache_root(
            Some(Path::new("/custom/bun-install")),
            None,
            Some(Path::new("/queried/bun-cache")),
            Some(Path::new("/Users/example")),
        );
        assert_eq!(resolved, Some(PathBuf::from("/queried/bun-cache")));
    }

    #[test]
    fn resolve_cache_root_falls_back_to_the_default_dot_bun() {
        let resolved = resolve_cache_root(None, None, None, Some(Path::new("/Users/example")));
        assert_eq!(
            resolved,
            Some(PathBuf::from("/Users/example/.bun/install/cache"))
        );
    }

    #[test]
    fn discover_reports_the_install_cache_when_present() {
        let temp = std::env::temp_dir().join(format!("dodo-cleaner-bun-{}", std::process::id()));
        fs::create_dir_all(
            temp.join(".bun")
                .join("install")
                .join("cache")
                .join("left-pad"),
        )
        .expect("creates bun install cache dir");

        let provider = BunProvider;
        let environment = NodeToolEnvironment {
            home: Some(temp.clone()),
            ..Default::default()
        };
        let locations = provider.discover(&environment);

        assert_eq!(locations.len(), 1);
        assert_eq!(locations[0].group, "Bun install cache");
        assert!(locations[0].allow_cleanup);
        assert_eq!(locations[0].risk, RiskLevel::SafeRecreatable);

        fs::remove_dir_all(&temp).expect("removes temp tree");
    }

    #[test]
    fn discover_reports_nothing_when_bun_was_never_used() {
        let temp =
            std::env::temp_dir().join(format!("dodo-cleaner-bun-missing-{}", std::process::id()));

        let provider = BunProvider;
        let environment = NodeToolEnvironment {
            home: Some(temp),
            ..Default::default()
        };
        assert!(provider.discover(&environment).is_empty());
    }
}
