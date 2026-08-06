use std::path::PathBuf;
use std::time::SystemTime;

use crate::cleaner::core::category::CleanerCategory;
use crate::cleaner::core::risk::{ItemCapability, RiskLevel, SelectionPolicy};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct CleanableItemId(pub u64);

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CleanableItem {
    pub id: CleanableItemId,
    pub category: CleanerCategory,
    pub group: Option<String>,
    pub display_name: String,
    pub path: PathBuf,
    pub logical_size: u64,
    pub allocated_size: Option<u64>,
    pub modified_at: Option<SystemTime>,
    pub last_accessed_at: Option<SystemTime>,
    pub risk: RiskLevel,
    pub selection_policy: SelectionPolicy,
    pub capabilities: Vec<ItemCapability>,
    pub explanation: String,
    pub warnings: Vec<ItemWarning>,
    pub metadata: ItemMetadata,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ItemWarning {
    pub message: String,
}

/// Per-category detail hung off an item. The mock scanners produce `Generic`
/// only; each remaining variant is constructed by the real scanner for its
/// category. `#[allow(dead_code)]` comes off as those land.
#[derive(Clone, PartialEq, Eq, Debug)]
#[allow(dead_code)]
pub enum ItemMetadata {
    Generic,
    Application(ApplicationMetadata),
    MailFile(MailFileMetadata),
    LargeFile(LargeFileMetadata),
    Docker(DockerItemMetadata),
    NodeTool(NodeToolMetadata),
    UniversalBinary(UniversalBinaryMetadata),
    Language(LanguageMetadata),
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ApplicationMetadata {
    pub bundle_id: Option<String>,
    pub team_id: Option<String>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MailFileMetadata {
    pub account_hint: Option<String>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LargeFileMetadata {
    pub extension: Option<String>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DockerItemMetadata {
    pub object_type: String,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct NodeToolMetadata {
    pub provider: String,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct UniversalBinaryMetadata {
    pub architectures: Vec<String>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LanguageMetadata {
    pub language_code: String,
}
