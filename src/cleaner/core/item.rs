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

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ItemMetadata {
    Generic,
    Application(ApplicationMetadata),
    MailFile(MailFileMetadata),
    LargeFile(LargeFileMetadata),
    Docker(DockerItemMetadata),
    NodeTool(NodeToolMetadata),
    UniversalBinary(UniversalBinaryMetadata),
    Language(LanguageMetadata),
    OrphanedFile(OrphanedFileMetadata),
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ApplicationMetadata {
    pub bundle_id: Option<String>,
    pub team_id: Option<String>,
    pub version: Option<String>,
    pub executable: Option<String>,
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

/// Why an orphan-detection candidate (Phase 10) is considered orphaned, per
/// the ticket's suggested enum. Attached to every
/// [`ItemMetadata::OrphanedFile`] item so a future UI or test can filter or
/// explain a result by reason rather than only by its free-text
/// `CleanableItem::explanation`.
///
/// Lives beside the other metadata structs in `core` rather than in
/// `macos::applications::orphans` — like `ApplicationMetadata`'s `bundle_id`
/// and `team_id`, this is plain data populated by macOS-only matching logic,
/// and `ItemMetadata` stays self-contained without depending on `macos`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OrphanReason {
    /// A `~/Library/Containers/<bundle id>` (or equivalent identifier-keyed)
    /// entry whose bundle id matches no installed app.
    BundleIdentifierNotInstalled,
    /// A generically-named leftover (Application Support, Caches, Logs,
    /// WebKit, HTTPStorages, Cookies, Services, Autosave Information) whose
    /// name does not resemble any installed app closely enough to explain it.
    AppNameNotInstalled,
    /// A `~/Library/LaunchAgents` (or `LaunchDaemons`) entry naming an app or
    /// bundle id that is not installed — something is supposed to launch it,
    /// and nothing can.
    MissingOwnerApplication,
    /// A `~/Library/Saved Application State/<bundle id>.savedState` entry
    /// whose bundle id matches no installed app.
    StaleSavedState,
    /// A `~/Library/Preferences/<bundle id>.plist` entry whose bundle id
    /// matches no installed app.
    StalePreference,
    /// A `~/Library/Group Containers` entry no installed app's identity
    /// claims. Ownership of a group container can never be confidently
    /// attributed to one specific missing app — see
    /// `macos::applications::orphans` — so this reason is always the most
    /// conservative confidence bucket.
    UnknownContainerOwner,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct OrphanedFileMetadata {
    pub reason: OrphanReason,
}
