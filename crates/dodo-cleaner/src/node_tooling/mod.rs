//! Shared `NodeToolCacheProvider` implementations for npm, Yarn Classic,
//! Yarn Berry, pnpm, Bun and the deliberately empty Nub placeholder. See
//! `cleaner::node_tooling_cache` for discovery, de-duplication and cleanup
//! policy.

mod bun;
mod npm;
mod nub;
mod pnpm;
mod yarn_berry;
mod yarn_classic;

use std::sync::Arc;

use bun::BunProvider;
use npm::NpmProvider;
use nub::NubProvider;
use pnpm::PnpmProvider;
use yarn_berry::YarnBerryProvider;
use yarn_classic::YarnClassicProvider;

use crate::core::node_tool_provider::NodeToolCacheProvider;

/// One instance per tool, in a fixed, deterministic order — the same order
/// every scan and every `cleanup_allowed_roots` call iterates providers in,
/// which matters only for which provider "wins" a path collision (see
/// `node_tooling_cache`'s duplicate-counting guard).
pub(crate) fn default_providers() -> Vec<Arc<dyn NodeToolCacheProvider>> {
    vec![
        Arc::new(NpmProvider),
        Arc::new(YarnClassicProvider),
        Arc::new(YarnBerryProvider),
        Arc::new(PnpmProvider),
        Arc::new(BunProvider),
        Arc::new(NubProvider),
    ]
}
