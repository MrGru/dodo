//! Platform-neutral provider seam for `CleanerCategory::NodeToolingCache`.
//!
//! Providers receive one resolved snapshot: platform directories, explicit
//! environment overrides and successful fixed-argv tool queries. They make no
//! OS calls and never read process-global environment themselves. The scanner
//! and provider implementations live in `cleaner::{node_tooling_cache,
//! node_tooling}` because all three desktop targets use them.

use std::path::PathBuf;

use crate::cleaner::core::risk::{RiskLevel, SelectionPolicy};
use crate::cleaner::core::scan_root::AggregateMode;
use crate::paths::HostOs;

/// Inputs used by the npm, Yarn, pnpm and Bun providers.
///
/// The `*_command_*` fields contain only successful, absolute, existing paths
/// parsed from each tool's own answer. Explicit environment overrides stay
/// separate so providers can preserve their documented precedence.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct NodeToolEnvironment {
    pub host: HostOs,
    pub home: Option<PathBuf>,
    /// macOS `~/Library/Caches`, Windows `%LOCALAPPDATA%`, or Linux's
    /// `$XDG_CACHE_HOME` (falling back to `~/.cache`).
    pub cache_home: Option<PathBuf>,
    /// `%LOCALAPPDATA%`; `None` off Windows.
    pub local_app_data: Option<PathBuf>,
    pub npm_config_cache: Option<PathBuf>,
    pub yarn_cache_folder: Option<PathBuf>,
    pub bun_install: Option<PathBuf>,
    pub bun_install_cache_dir: Option<PathBuf>,
    pub nub_home: Option<PathBuf>,
    pub npm_command_cache: Option<PathBuf>,
    pub yarn_classic_command_cache: Option<PathBuf>,
    pub yarn_berry_command_global_folder: Option<PathBuf>,
    /// pnpm's content-addressable store. It is a denied root, never a cache
    /// location, regardless of whether it came from config or `pnpm store path`.
    pub pnpm_store: Option<PathBuf>,
    pub pnpm_command_cache: Option<PathBuf>,
    pub bun_command_cache: Option<PathBuf>,
}

impl Default for NodeToolEnvironment {
    fn default() -> Self {
        Self {
            host: HostOs::MacOs,
            home: None,
            cache_home: None,
            local_app_data: None,
            npm_config_cache: None,
            yarn_cache_folder: None,
            bun_install: None,
            bun_install_cache_dir: None,
            nub_home: None,
            npm_command_cache: None,
            yarn_classic_command_cache: None,
            yarn_berry_command_global_folder: None,
            pnpm_store: None,
            pnpm_command_cache: None,
            bun_command_cache: None,
        }
    }
}

/// Whether a discovered location is global or belongs to one project.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NodeCacheScope {
    Global,
    ProjectLocal,
}

/// One positively identified cache directory and the policy used to build its
/// result rows. Providers return roots, while the shared scanner turns each
/// root's immediate children into items; the root itself is never deletable.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct NodeCacheLocation {
    pub path: PathBuf,
    pub group: String,
    pub scope: NodeCacheScope,
    pub risk: RiskLevel,
    pub selection_policy: SelectionPolicy,
    pub allow_cleanup: bool,
    pub aggregate_mode: AggregateMode,
    pub explanation: String,
}

pub trait NodeToolCacheProvider: Send + Sync {
    fn id(&self) -> &'static str;
    fn display_name(&self) -> &'static str;
    fn discover(&self, environment: &NodeToolEnvironment) -> Vec<NodeCacheLocation>;

    /// Roots this provider can identify but the Cleaner must neither show nor
    /// delete. pnpm uses this for its shared content-addressable store.
    fn denied_roots(&self, _environment: &NodeToolEnvironment) -> Vec<PathBuf> {
        Vec::new()
    }
}
