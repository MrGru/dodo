use std::path::PathBuf;
use std::time::SystemTime;

use crate::cleaner::core::ai_app_provider::AiAppRole;
use crate::cleaner::core::category::CleanerCategory;
use crate::cleaner::core::icon::IconRaster;
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
    AiApp(AiAppMetadata),
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
    /// This application's own Finder icon, rasterised small and shared —
    /// never the platform's full representation ladder. [`IconRaster`]'s
    /// module doc carries what that cost when it was, and why the bound is a
    /// type rather than a comment.
    pub icon: Option<IconRaster>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MailFileMetadata {
    pub account_hint: Option<String>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LargeFileMetadata {
    pub extension: Option<String>,
}

/// Which Docker engine object a `CleanerCategory::DockerCache` item names.
/// See [`crate::cleaner::docker_cache`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DockerObjectKind {
    Image,
    Container,
    Volume,
    Network,
}

/// Attached to every item [`crate::cleaner::docker_cache::DockerCacheScanner`]
/// produces. `engine_id` is whatever the `docker` CLI identifies the object
/// by — an image/container id, a volume name, a network id — exactly what
/// `docker_cache::prune_items` passes back to `docker rmi`/`rm`/`volume
/// rm`/`network rm`. `CleanableItem::path` for these items is a synthetic
/// `docker://<kind>/<id>` string for display and `CopyPath` only; it is never
/// resolved against the filesystem (see `docker_cache`'s module doc comment).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DockerItemMetadata {
    pub kind: DockerObjectKind,
    pub engine_id: String,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct NodeToolMetadata {
    pub provider: String,
}

/// Attached to every item `cleaner::ai_apps::AiAppsScanner` produces.
/// `role` is what the six per-item risk/selection/capability derivations
/// (`AiAppRole::risk`, `AiAppRole::selection_policy`,
/// `AiAppRole::allow_cleanup`) are keyed on — carrying it here too lets a
/// future UI filter or explain a result by role without re-deriving it from
/// the item's group label. `model_names` is non-empty only for a
/// [`AiAppRole::Models`] item where a provider could cheaply extract names
/// from on-disk *structure* (directory/file names) without reading any
/// model's content — currently only Ollama's manifest tree; see
/// `cleaner::ai_apps::collect_ollama_model_names`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct AiAppMetadata {
    pub app_id: String,
    pub role: AiAppRole,
    pub model_names: Vec<String>,
}

/// Attached to every item
/// `macos::scanners::universal_binaries::UniversalBinariesScanner` produces
/// (Phase 14, analysis-only — nothing yet reads `capabilities` to offer a
/// `RemoveArchitecture` action).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct UniversalBinaryMetadata {
    /// Every architecture this bundle's main executable was built for, in
    /// Mach-O slice order (e.g. `["arm64", "x86_64"]`).
    pub architectures: Vec<String>,
    /// This machine's own architecture (`"arm64"` or `"x86_64"`), so the UI
    /// can show which slice a future removal would keep.
    pub current_architecture: String,
    /// The combined byte size of every slice that is *not*
    /// `current_architecture` — an estimate of what a future thinning action
    /// could reclaim, never actually removed this phase.
    pub estimated_removable_bytes: u64,
    /// Whether `codesign --verify` accepted the bundle at scan time. `None`
    /// when the check itself could not run (`codesign` missing) rather than
    /// a real signing verdict.
    pub signed: Option<bool>,
    /// This bundle's own Finder icon, on the same terms as
    /// [`ApplicationMetadata::icon`].
    pub icon: Option<IconRaster>,
}

/// Attached to every item
/// `macos::scanners::language_files::LanguageFilesScanner` produces (Phase
/// 15, analysis-only — nothing yet reads `capabilities` to offer a
/// `RemoveLocalization` action).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LanguageMetadata {
    /// The `.lproj` folder's own name minus the extension (e.g. `"en"`,
    /// `"zh-Hans"`, `"Base"`).
    pub language_code: String,
    /// Why this specific `.lproj` is protected rather than an ordinary
    /// removal candidate — `None` for an ordinary, ordinary-risk language.
    pub protection_reason: Option<LanguageProtectionReason>,
}

/// Ticket-named reasons a `.lproj` must never be presented as safe to
/// remove, even before any removal exists. See `LanguageMetadata`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LanguageProtectionReason {
    /// `Base.lproj` — storyboard/XIB strings with no dedicated translation.
    BaseLocalization,
    /// Matches one of `NSGlobalDomain`'s `AppleLanguages`, this machine's
    /// ordered preferred-language list.
    PreferredLanguage,
    /// The bundle's own `CFBundleDevelopmentRegion`.
    DevelopmentRegion,
    /// English, kept regardless of the preferred-language list as the
    /// ticket's own explicit fallback.
    EnglishFallback,
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
