//! pnpm's content-addressable store (kept scan-only) and its separate
//! registry-metadata cache (safe to recreate).
//!
//! pnpm keeps two genuinely different things on disk: a content-addressable
//! *store* every pnpm project on the machine can share package contents
//! from, and a smaller *cache* of registry metadata. The ticket is explicit
//! that the store "may be shared across many projects" and must never be
//! "label[ed]... the whole store as harmless junk", and that pruning it is a
//! *future* explicit action with its own preview — "Never run prune
//! automatically". This provider honors that literally: the store location
//! is reported with `RiskLevel::UserData` and `allow_cleanup: false`, so
//! `macos::scanners::node_tooling_cache::cleanup_allowed_roots` never
//! includes it — there is no code path in this phase that can move it to
//! Trash, regardless of what a future UI bug might let a user select. The
//! cache is treated like any other tool's cache: `SafeRecreatable`,
//! selected by default, allow-listed for cleanup.
//!
//! The store's location is checked in the order the ticket suggests:
//! `npm_config_store_dir` (the literal environment-variable form of `pnpm
//! config get store-dir`) first, then `PNPM_HOME`, then the documented
//! macOS default `~/Library/pnpm/store`. This provider does not shell out to
//! `pnpm config get store-dir` itself — see
//! `docs/cleaner/known-limitations.md` for the same "avoid an extra process
//! call" reasoning `homebrew_cache` used for `brew --cache`.

use std::path::{Path, PathBuf};

use crate::cleaner::core::node_tool_provider::{
    NodeCacheLocation, NodeCacheScope, NodeToolCacheProvider, NodeToolEnvironment,
};
use crate::cleaner::core::risk::{RiskLevel, SelectionPolicy};
use crate::cleaner::core::scan_root::AggregateMode;

pub(crate) struct PnpmProvider;

/// pnpm's store root: `npm_config_store_dir` if set, else `PNPM_HOME`, else
/// the documented macOS default `~/Library/pnpm/store`.
fn resolve_store_root(
    store_dir_override: Option<&Path>,
    pnpm_home_override: Option<&Path>,
    home: Option<&Path>,
) -> Option<PathBuf> {
    if let Some(path) = store_dir_override {
        return Some(path.to_path_buf());
    }
    if let Some(path) = pnpm_home_override {
        return Some(path.to_path_buf());
    }
    home.map(|home| home.join("Library").join("pnpm").join("store"))
}

/// pnpm's separate registry-metadata cache: `~/Library/Caches/pnpm` on
/// macOS. No environment-variable override is documented for this one
/// specifically (unlike the store), so this always resolves from `home`.
fn resolve_cache_root(home: Option<&Path>) -> Option<PathBuf> {
    home.map(|home| home.join("Library").join("Caches").join("pnpm"))
}

impl NodeToolCacheProvider for PnpmProvider {
    fn id(&self) -> &'static str {
        "pnpm"
    }

    fn display_name(&self) -> &'static str {
        "pnpm"
    }

    fn discover(&self, environment: &NodeToolEnvironment) -> Vec<NodeCacheLocation> {
        let mut locations = Vec::new();

        if let Some(store_root) = resolve_store_root(
            environment.npm_config_store_dir.as_deref(),
            environment.pnpm_home.as_deref(),
            environment.home.as_deref(),
        ) && store_root.is_dir()
        {
            locations.push(NodeCacheLocation {
                path: store_root,
                group: "pnpm store".to_string(),
                scope: NodeCacheScope::Global,
                risk: RiskLevel::UserData,
                selection_policy: SelectionPolicy::NotSelectedByDefault,
                allow_cleanup: false,
                aggregate_mode: AggregateMode::ImmediateChildren,
                explanation: "pnpm's content-addressable package store. It may be shared by \
                    every pnpm project on this machine, so it is never treated as ordinary \
                    cache junk and this phase cannot delete it — a future explicit \"pnpm \
                    store prune\" action with its own preview is the intended way to reclaim \
                    it."
                .to_string(),
            });
        }

        if let Some(cache_root) = resolve_cache_root(environment.home.as_deref())
            && cache_root.is_dir()
        {
            locations.push(NodeCacheLocation {
                path: cache_root,
                group: "pnpm cache".to_string(),
                scope: NodeCacheScope::Global,
                risk: RiskLevel::SafeRecreatable,
                selection_policy: SelectionPolicy::SelectedByDefault,
                allow_cleanup: true,
                aggregate_mode: AggregateMode::ImmediateChildren,
                explanation: "pnpm's registry-metadata cache, distinct from its \
                    content-addressable store; pnpm re-fetches it again if it is needed."
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
    fn resolve_store_root_prefers_the_store_dir_override_over_pnpm_home() {
        let resolved = resolve_store_root(
            Some(Path::new("/custom/store-dir")),
            Some(Path::new("/custom/pnpm-home")),
            Some(Path::new("/Users/example")),
        );
        assert_eq!(resolved, Some(PathBuf::from("/custom/store-dir")));
    }

    #[test]
    fn resolve_store_root_falls_back_to_pnpm_home_then_the_default() {
        let via_pnpm_home = resolve_store_root(
            None,
            Some(Path::new("/custom/pnpm-home")),
            Some(Path::new("/Users/example")),
        );
        assert_eq!(via_pnpm_home, Some(PathBuf::from("/custom/pnpm-home")));

        let via_default = resolve_store_root(None, None, Some(Path::new("/Users/example")));
        assert_eq!(
            via_default,
            Some(PathBuf::from("/Users/example/Library/pnpm/store"))
        );
    }

    #[test]
    fn store_is_scan_only_and_cache_is_selected_by_default() {
        let temp = std::env::temp_dir().join(format!("dodo-cleaner-pnpm-{}", std::process::id()));
        fs::create_dir_all(temp.join("Library").join("pnpm").join("store").join("v3"))
            .expect("creates store dir");
        fs::create_dir_all(
            temp.join("Library")
                .join("Caches")
                .join("pnpm")
                .join("metadata"),
        )
        .expect("creates cache dir");

        let provider = PnpmProvider;
        let environment = NodeToolEnvironment {
            home: Some(temp.clone()),
            ..Default::default()
        };
        let locations = provider.discover(&environment);

        assert_eq!(locations.len(), 2);
        let store = locations
            .iter()
            .find(|location| location.group == "pnpm store")
            .expect("has a store location");
        assert!(
            !store.allow_cleanup,
            "the pnpm store must never be allow-listed for cleanup"
        );
        assert_eq!(store.risk, RiskLevel::UserData);
        assert_eq!(
            store.selection_policy,
            SelectionPolicy::NotSelectedByDefault
        );

        let cache = locations
            .iter()
            .find(|location| location.group == "pnpm cache")
            .expect("has a cache location");
        assert!(cache.allow_cleanup);
        assert_eq!(cache.risk, RiskLevel::SafeRecreatable);
        assert_eq!(cache.selection_policy, SelectionPolicy::SelectedByDefault);

        fs::remove_dir_all(&temp).expect("removes temp tree");
    }

    #[test]
    fn discover_reports_nothing_when_pnpm_was_never_used() {
        let temp =
            std::env::temp_dir().join(format!("dodo-cleaner-pnpm-missing-{}", std::process::id()));

        let provider = PnpmProvider;
        let environment = NodeToolEnvironment {
            home: Some(temp),
            ..Default::default()
        };
        assert!(provider.discover(&environment).is_empty());
    }
}
