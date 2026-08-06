use std::path::PathBuf;

use crate::cleaner::core::permissions::MacPermission;
use crate::cleaner::core::risk::RiskLevel;

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ScanRoot {
    pub path: PathBuf,
    pub max_depth: Option<usize>,
    pub follow_symlinks: bool,
    pub cross_filesystems: bool,
    pub include_hidden: bool,
    pub aggregate_mode: AggregateMode,
    pub permission: Option<MacPermission>,
    pub risk: RiskLevel,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AggregateMode {
    EveryFile,
    ImmediateChildren,
    TopLevelDirectory,
    WholeRoot,
}
