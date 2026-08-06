//! The seam a Node.js package-manager cache provider plugs into
//! (`CleanerCategory::NodeToolingCache`, Phase 11).
//!
//! Six providers (npm, Yarn Classic, Yarn Berry, pnpm, Bun, Nub) implement
//! [`NodeToolCacheProvider`] under `macos::scanners::node_tooling`; the
//! single scanner that drives all six is
//! `macos::scanners::node_tooling_cache::NodeToolingCacheScanner`. The trait
//! and its data types live here, in `core`, rather than under `macos`,
//! because none of the six providers make a macOS API call or import GPUI —
//! every one of them is env-var reads plus filesystem-existence checks, the
//! same reasoning that puts [`crate::cleaner::core::scanner::CleanerScanner`]
//! itself in `core` while its macOS implementations live under `macos`.
//!
//! # Deviating from the ticket's suggested signature
//!
//! The ticket suggests `fn discover(&self, environment: &EnvironmentSnapshot)
//! -> Result<Vec<NodeCacheLocation>, NodeCacheError>`. [`discover`] is
//! infallible here instead: nothing a provider does can fail in a way worth
//! reporting separately from "not installed". Reading an environment
//! variable never errors — a missing one is just `None` — and checking
//! whether a candidate directory exists *is* the "is this tool installed"
//! question every provider already has to answer by returning an empty
//! `Vec` rather than a location. Adding an error type would give every
//! provider an unused `Err` arm to write and every caller an unused arm to
//! handle.
//!
//! [`NodeToolCacheProvider::discover`]: NodeToolCacheProvider::discover

use std::path::PathBuf;

use crate::cleaner::core::risk::{RiskLevel, SelectionPolicy};
use crate::cleaner::core::scan_root::AggregateMode;

/// Every environment variable any of the six providers read, snapshotted
/// once per scan by
/// `macos::scanners::node_tooling_cache::snapshot_environment` — the
/// ticket's "snapshot environment variables once per scan" applied
/// literally, rather than each provider calling `std::env::var_os` on its
/// own, once per provider, every time `discover` runs.
///
/// Every field is a resolved, non-empty [`PathBuf`] or `None` — never a raw
/// `OsString` — so a provider never has to repeat the
/// "is this set to something non-empty" check `resolve_cache_root`-style
/// helpers throughout this codebase already do once, centrally.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct NodeToolEnvironment {
    /// The user's home directory, i.e. `ScanContext::user_home` — every
    /// provider's *default* (unconfigured) location is derived from this.
    pub home: Option<PathBuf>,
    /// npm's `npm_config_cache` — npm's own convention for exposing its
    /// configured cache directory as an environment variable.
    pub npm_config_cache: Option<PathBuf>,
    /// Yarn Classic's `YARN_CACHE_FOLDER` override.
    pub yarn_cache_folder: Option<PathBuf>,
    /// pnpm's `PNPM_HOME`, one of two candidate overrides for its store
    /// location (see `macos::scanners::node_tooling::pnpm`).
    pub pnpm_home: Option<PathBuf>,
    /// pnpm's `npm_config_store_dir`, the other candidate override for its
    /// store location — the literal env-var form of `pnpm config get
    /// store-dir`.
    pub npm_config_store_dir: Option<PathBuf>,
    /// Bun's `BUN_INSTALL`, the base install directory its cache nests
    /// under by default.
    pub bun_install: Option<PathBuf>,
    /// Bun's `BUN_INSTALL_CACHE_DIR` override.
    pub bun_install_cache_dir: Option<PathBuf>,
    /// A defensive, generically-named `NUB_HOME` override, checked only by
    /// the Nub provider — see `macos::scanners::node_tooling::nub` for why
    /// it is read but never turned into a reported location.
    pub nub_home: Option<PathBuf>,
}

/// Whether a discovered cache location belongs to a single project checkout
/// or is shared/global across every project on the machine.
///
/// No provider in this phase ever constructs [`NodeCacheScope::ProjectLocal`]
/// — Yarn Berry's project-local `.yarn/cache` and Bun's project-local
/// dependencies are both out of scope for discovery this phase (see
/// `docs/cleaner/known-limitations.md`) — but the ticket asks each location
/// to carry this distinction explicitly, and a future provider that *can*
/// bound a project-local scan will have this variant already wired into the
/// scanner and UI.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NodeCacheScope {
    Global,
    ProjectLocal,
}

/// One directory a [`NodeToolCacheProvider`] wants scanned, plus everything
/// `macos::scanners::node_tooling_cache::NodeToolingCacheScanner` needs to
/// turn its contents into [`crate::cleaner::core::item::CleanableItem`]s
/// without asking the provider again: a group label, a risk/selection
/// verdict already made by the provider (not derived centrally, since the
/// six tools have genuinely different shapes), and whether the location may
/// ever be allow-listed for `MoveToTrash` cleanup at all.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct NodeCacheLocation {
    /// Absolute path to the directory to scan. Its *immediate children*
    /// become individual items (see `aggregate_mode`) — never the directory
    /// itself, so this path is safe to allow-list directly without ever
    /// colliding with an item's own path.
    pub path: PathBuf,
    /// UI group label, e.g. `"npm cache"` or `"pnpm store"`.
    pub group: String,
    pub scope: NodeCacheScope,
    pub risk: RiskLevel,
    pub selection_policy: SelectionPolicy,
    /// Whether this exact `path` may be allow-listed for `MoveToTrash`
    /// cleanup. `false` for pnpm's store (shared, never auto-pruned per the
    /// ticket) and for anything else a future provider marks scan-only.
    /// `macos::scanners::node_tooling_cache::cleanup_allowed_roots` only
    /// ever returns locations with `allow_cleanup: true`.
    pub allow_cleanup: bool,
    /// How `path`'s contents are aggregated into items. Every location this
    /// phase discovers uses `AggregateMode::ImmediateChildren` — see the
    /// scanner's module doc for why an `allow_cleanup: true` location must
    /// never use `AggregateMode::WholeRoot`: that would make one item's path
    /// equal its own allow-listed root, which `macos::safety::validate_path`
    /// rejects outright (`SafetyError::RootDeletionRejected`).
    pub aggregate_mode: AggregateMode,
    /// Shown on every item this location produces, verbatim.
    pub explanation: String,
}

/// Implemented once per Node.js tool
/// (`macos::scanners::node_tooling::{npm, yarn_classic, yarn_berry, pnpm,
/// bun, nub}`). A provider whose tool is not installed — no configured
/// override and no directory at the default location — returns an empty
/// `Vec` from `discover`, never an error and never a placeholder location;
/// see this module's doc comment for why `discover` has no `Result`.
pub trait NodeToolCacheProvider: Send + Sync {
    /// Stable machine identifier, stored on every item this provider
    /// produces via `ItemMetadata::NodeTool(NodeToolMetadata { provider })`.
    fn id(&self) -> &'static str;
    /// Human-readable name, for a future UI that lists providers by name.
    fn display_name(&self) -> &'static str;
    /// Returns every cache location this provider can positively identify
    /// as present, given `environment`. Never touches `std::env` itself —
    /// everything it needs is already on `environment`.
    fn discover(&self, environment: &NodeToolEnvironment) -> Vec<NodeCacheLocation>;
}
