//! Windows Installed Apps inventory.
//!
//! Discovery is intentionally split three ways: all four registry
//! hive/view pairs, the current user's MSIX packages, and direct children of
//! `%LOCALAPPDATA%\Programs`. Registry uninstall command strings are never
//! read, parsed or executed. Every uninstall action opens Windows Installed
//! Apps settings; portable directories are inventory-only and are never
//! deleted by dodo.

use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;

use crate::cleaner::core::item::{CleanableItem, CleanableItemId, ItemMetadata};
use crate::cleaner::core::risk::{ItemCapability, RiskLevel, SelectionPolicy};
use crate::cleaner::core::safety::{contains_path, is_direct_child};
use crate::paths::HostOs;

#[cfg(target_os = "windows")]
use crate::cleaner::core::cancellation::CancellationToken;
#[cfg(target_os = "windows")]
use crate::cleaner::core::category::CleanerCategory;
#[cfg(target_os = "windows")]
use crate::cleaner::core::errors::ScanError;
#[cfg(target_os = "windows")]
use crate::cleaner::core::permissions::MacPermission;
#[cfg(target_os = "windows")]
use crate::cleaner::core::progress::{ProgressSink, ScanPhase, ScanProgress};
#[cfg(target_os = "windows")]
use crate::cleaner::core::report::{
    CategoryScanResult, PartialScanReason, ScanCompleteness, ScanWarning,
};
#[cfg(target_os = "windows")]
use crate::cleaner::core::scan_context::ScanContext;
#[cfg(target_os = "windows")]
use crate::cleaner::core::scanner::CleanerScanner;

#[cfg(target_os = "windows")]
const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall";
const MSIX_QUERY: &str = "Get-AppxPackage | Select-Object Name,PackageFullName,PackageFamilyName,Version,InstallLocation,Publisher,IsFramework,IsResourcePackage,NonRemovable,SignatureKind | ConvertTo-Json -Compress";
const POWERSHELL_ARGS: &[&str] = &[
    "-NoLogo",
    "-NoProfile",
    "-NonInteractive",
    "-Command",
    MSIX_QUERY,
];
const SETTINGS_ARGS: &[&str] = &["ms-settings:appsfeatures"];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct CommandSpec {
    pub program: &'static str,
    pub args: &'static [&'static str],
}

fn msix_command() -> CommandSpec {
    CommandSpec {
        program: "powershell.exe",
        args: POWERSHELL_ARGS,
    }
}

pub(crate) fn installed_apps_settings_command() -> CommandSpec {
    CommandSpec {
        program: "explorer.exe",
        args: SETTINGS_ARGS,
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
enum RegistryHive {
    Machine,
    CurrentUser,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
enum RegistryView {
    Registry64,
    Registry32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
struct RegistryScope {
    hive: RegistryHive,
    view: RegistryView,
}

/// Both views are named explicitly for both hives. Do not collapse this into
/// the process-default registry view: a 64-bit process would then miss every
/// 32-bit registration.
const REGISTRY_SCOPES: [RegistryScope; 4] = [
    RegistryScope {
        hive: RegistryHive::Machine,
        view: RegistryView::Registry64,
    },
    RegistryScope {
        hive: RegistryHive::Machine,
        view: RegistryView::Registry32,
    },
    RegistryScope {
        hive: RegistryHive::CurrentUser,
        view: RegistryView::Registry64,
    },
    RegistryScope {
        hive: RegistryHive::CurrentUser,
        view: RegistryView::Registry32,
    },
];

#[derive(Clone, PartialEq, Eq, Debug)]
struct RegistryRecord {
    scope: RegistryScope,
    key_name: String,
    display_name: Option<String>,
    version: Option<String>,
    publisher: Option<String>,
    install_location: Option<PathBuf>,
    estimated_size_kib: Option<u64>,
    system_component: bool,
    no_remove: bool,
    parent_key_name: Option<String>,
    release_type: Option<String>,
}

#[derive(Clone, PartialEq, Eq, Debug, Deserialize)]
struct MsixRecord {
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "PackageFullName", default)]
    package_full_name: Option<String>,
    #[serde(rename = "PackageFamilyName", default)]
    package_family_name: Option<String>,
    #[serde(
        rename = "Version",
        default,
        deserialize_with = "deserialize_msix_version"
    )]
    version: Option<String>,
    #[serde(rename = "InstallLocation", default)]
    install_location: Option<PathBuf>,
    #[serde(rename = "Publisher", default)]
    publisher: Option<String>,
    #[serde(rename = "IsFramework", default)]
    is_framework: bool,
    #[serde(rename = "IsResourcePackage", default)]
    is_resource_package: bool,
    #[serde(rename = "NonRemovable", default)]
    non_removable: bool,
    #[serde(rename = "SignatureKind", default)]
    signature_kind: Value,
}

#[derive(Clone, PartialEq, Eq, Debug)]
struct PortableRecord {
    programs_root: PathBuf,
    location: PathBuf,
    display_name: String,
    direct_executable: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum InventorySource {
    Registry,
    Msix,
    Portable,
}

#[derive(Clone, PartialEq, Eq, Debug, Hash)]
enum SourceIdentity {
    MsiProductCode(String),
    RegistryEntry(RegistryScope, String),
    MsixPackage(String),
    PortableLocation(String),
}

#[derive(Clone, PartialEq, Eq, Debug)]
struct InventoryApp {
    identity: SourceIdentity,
    source: InventorySource,
    display_name: String,
    version: Option<String>,
    publisher: Option<String>,
    location: Option<PathBuf>,
    logical_size: u64,
    shared_location: bool,
}

fn inventory_from_records(
    registry: Vec<RegistryRecord>,
    msix: Vec<MsixRecord>,
    portable: Vec<PortableRecord>,
) -> Vec<InventoryApp> {
    let records = registry
        .into_iter()
        .filter_map(registry_app)
        .chain(msix.into_iter().filter_map(msix_app))
        .chain(portable.into_iter().filter_map(portable_app));

    // ponytail: installed-app inventories are small; index identities and
    // locations only if this linear de-duplication becomes measurable.
    let mut apps: Vec<InventoryApp> = Vec::new();
    for app in records {
        if let Some(existing) = apps
            .iter_mut()
            .find(|existing| existing.identity == app.identity)
        {
            merge_missing(existing, &app);
        } else {
            apps.push(app);
        }
    }

    let mut pending = apps;
    let mut deduplicated = Vec::new();
    while let Some(app) = pending.pop() {
        let Some(location) = app.location.clone() else {
            deduplicated.push(app);
            continue;
        };

        let mut group = vec![app];
        let mut index = 0;
        while index < pending.len() {
            if pending[index]
                .location
                .as_deref()
                .is_some_and(|other| same_windows_location(&location, other))
            {
                group.push(pending.swap_remove(index));
            } else {
                index += 1;
            }
        }

        let mut owners: Vec<InventoryApp> = Vec::new();
        for candidate in group {
            if let Some(existing) = owners
                .iter_mut()
                .find(|existing| same_owner(existing, &candidate))
            {
                if source_priority(candidate.source) < source_priority(existing.source) {
                    let mut preferred = candidate;
                    merge_missing(&mut preferred, existing);
                    *existing = preferred;
                } else {
                    merge_missing(existing, &candidate);
                }
            } else {
                owners.push(candidate);
            }
        }

        if owners.len() > 1 {
            for owner in &mut owners {
                owner.shared_location = true;
            }
        }
        deduplicated.extend(owners);
    }

    deduplicated
}

fn registry_app(record: RegistryRecord) -> Option<InventoryApp> {
    let display_name = clean_text(record.display_name?)?;
    if record.system_component
        || record.no_remove
        || record.parent_key_name.as_deref().is_some_and(has_text)
        || record
            .release_type
            .as_deref()
            .is_some_and(is_update_release)
        || component_name(&display_name)
    {
        return None;
    }

    let identity = msi_product_code(&record.key_name)
        .map(SourceIdentity::MsiProductCode)
        .unwrap_or_else(|| SourceIdentity::RegistryEntry(record.scope, record.key_name));
    Some(InventoryApp {
        identity,
        source: InventorySource::Registry,
        display_name,
        version: record.version.and_then(clean_text),
        publisher: record.publisher.and_then(clean_text),
        location: clean_path(record.install_location),
        logical_size: record.estimated_size_kib.unwrap_or(0).saturating_mul(1024),
        shared_location: false,
    })
}

fn msix_app(record: MsixRecord) -> Option<InventoryApp> {
    if protected_msix(&record) {
        return None;
    }
    let display_name = clean_text(record.name)?;
    let package_identity = record
        .package_family_name
        .as_deref()
        .and_then(nonempty)
        .or_else(|| record.package_full_name.as_deref().and_then(nonempty))?
        .to_ascii_lowercase();
    Some(InventoryApp {
        identity: SourceIdentity::MsixPackage(package_identity),
        source: InventorySource::Msix,
        display_name,
        version: record.version.and_then(clean_text),
        publisher: record.publisher.and_then(clean_text),
        location: clean_path(record.install_location),
        logical_size: 0,
        shared_location: false,
    })
}

fn portable_app(record: PortableRecord) -> Option<InventoryApp> {
    if !record.direct_executable
        || !is_direct_child(HostOs::Windows, &record.programs_root, &record.location)
    {
        return None;
    }
    let display_name = clean_text(record.display_name)?;
    Some(InventoryApp {
        identity: SourceIdentity::PortableLocation(windows_location_key(&record.location)),
        source: InventorySource::Portable,
        display_name,
        version: None,
        publisher: None,
        location: Some(record.location),
        logical_size: 0,
        shared_location: false,
    })
}

fn protected_msix(record: &MsixRecord) -> bool {
    record.is_framework
        || record.is_resource_package
        || record.non_removable
        || signature_is_system(&record.signature_kind)
        || msix_component_name(&record.name)
}

fn deserialize_msix_version<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<Value>::deserialize(deserializer)?;
    Ok(value.and_then(msix_version))
}

fn msix_version(value: Value) -> Option<String> {
    match value {
        Value::String(version) => clean_text(version),
        Value::Number(version) => Some(version.to_string()),
        Value::Object(parts) => {
            let numbers: Option<Vec<u64>> = ["Major", "Minor", "Build", "Revision"]
                .into_iter()
                .map(|part| parts.get(part).and_then(Value::as_u64))
                .collect();
            numbers.map(|numbers| {
                numbers
                    .into_iter()
                    .map(|number| number.to_string())
                    .collect::<Vec<_>>()
                    .join(".")
            })
        }
        _ => None,
    }
}

fn signature_is_system(value: &Value) -> bool {
    value
        .as_str()
        .is_some_and(|value| value.eq_ignore_ascii_case("system"))
        || value.as_u64() == Some(4)
}

fn msix_component_name(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    ["vclibs", ".net.native.", "microsoft.ui.xaml", ".resources"]
        .iter()
        .any(|component| name.contains(component))
}

fn component_name(name: &str) -> bool {
    let name = name.trim().to_ascii_lowercase();
    [
        "update for ",
        "security update for ",
        "hotfix for ",
        "service pack for ",
    ]
    .iter()
    .any(|prefix| name.starts_with(prefix))
        || [
            " redistributable",
            " runtime",
            " framework",
            " language pack",
            " resource pack",
        ]
        .iter()
        .any(|component| name.contains(component))
}

fn is_update_release(release_type: &str) -> bool {
    matches!(
        release_type.trim().to_ascii_lowercase().as_str(),
        "update" | "hotfix" | "security update" | "update rollup" | "service pack" | "driver"
    )
}

fn msi_product_code(key_name: &str) -> Option<String> {
    let key_name = key_name.trim();
    let bytes = key_name.as_bytes();
    if bytes.len() != 38 || bytes[0] != b'{' || bytes[37] != b'}' {
        return None;
    }
    for (index, byte) in bytes.iter().enumerate().skip(1).take(36) {
        if matches!(index, 9 | 14 | 19 | 24) {
            if *byte != b'-' {
                return None;
            }
        } else if !byte.is_ascii_hexdigit() {
            return None;
        }
    }
    Some(key_name.to_ascii_uppercase())
}

fn merge_missing(target: &mut InventoryApp, other: &InventoryApp) {
    if target.version.is_none() {
        target.version.clone_from(&other.version);
    }
    if target.publisher.is_none() {
        target.publisher.clone_from(&other.publisher);
    }
    if target.location.is_none() {
        target.location.clone_from(&other.location);
    }
    target.logical_size = target.logical_size.max(other.logical_size);
}

fn source_priority(source: InventorySource) -> u8 {
    match source {
        InventorySource::Registry => 0,
        InventorySource::Msix => 1,
        InventorySource::Portable => 2,
    }
}

fn same_owner(left: &InventoryApp, right: &InventoryApp) -> bool {
    left.display_name.eq_ignore_ascii_case(&right.display_name)
        && match (left.publisher.as_deref(), right.publisher.as_deref()) {
            (Some(left), Some(right)) => left.eq_ignore_ascii_case(right),
            _ => true,
        }
}

fn same_windows_location(left: &Path, right: &Path) -> bool {
    contains_path(HostOs::Windows, left, right) && contains_path(HostOs::Windows, right, left)
}

fn windows_location_key(path: &Path) -> String {
    path.to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_ascii_lowercase()
}

fn clean_text(value: String) -> Option<String> {
    nonempty(&value).map(ToOwned::to_owned)
}

fn nonempty(value: &str) -> Option<&str> {
    let value = value.trim();
    (!value.is_empty()).then_some(value)
}

fn has_text(value: &str) -> bool {
    nonempty(value).is_some()
}

fn clean_path(path: Option<PathBuf>) -> Option<PathBuf> {
    let path = path?;
    let path = PathBuf::from(path.to_string_lossy().trim());
    if path.as_os_str().is_empty() {
        return None;
    }
    Some(std::fs::canonicalize(&path).unwrap_or(path))
}

impl InventoryApp {
    fn into_item(self) -> CleanableItem {
        let can_uninstall = !self.shared_location && self.source != InventorySource::Portable;
        let mut capabilities = Vec::new();
        if self.location.is_some() {
            capabilities.extend([ItemCapability::RevealInFinder, ItemCapability::CopyPath]);
        }
        if can_uninstall {
            capabilities.push(ItemCapability::UninstallApplication);
        }

        let path = self
            .location
            .clone()
            .unwrap_or_else(|| match &self.identity {
                SourceIdentity::MsiProductCode(code) => PathBuf::from(code),
                SourceIdentity::RegistryEntry(_, key) => PathBuf::from(format!("Registry: {key}")),
                SourceIdentity::MsixPackage(package) => PathBuf::from(package),
                SourceIdentity::PortableLocation(location) => PathBuf::from(location),
            });
        let explanation = if self.shared_location {
            "Multiple installed applications reference this location; uninstall is disabled."
        } else {
            match self.source {
                InventorySource::Registry => {
                    "Win32 application. Uninstall is delegated to Windows Installed Apps; registry commands are never run."
                }
                InventorySource::Msix => {
                    "Store/MSIX application. Uninstall is delegated to Windows Installed Apps."
                }
                InventorySource::Portable => {
                    "Bounded portable-app candidate. Dodo will not delete its directory."
                }
            }
        }
        .to_string();
        let group = match self.source {
            InventorySource::Registry => "Win32 / MSI",
            InventorySource::Msix => "Microsoft Store / MSIX",
            InventorySource::Portable => "Portable",
        };
        let id = item_id(&self.identity);

        CleanableItem {
            id,
            category: crate::cleaner::core::category::CleanerCategory::InstalledApps,
            group: Some(group.to_string()),
            display_name: self.display_name,
            path,
            logical_size: self.logical_size,
            allocated_size: None,
            modified_at: None,
            last_accessed_at: None,
            risk: if self.shared_location {
                RiskLevel::Protected
            } else {
                RiskLevel::ReviewRecommended
            },
            selection_policy: SelectionPolicy::NeverBulkSelect,
            capabilities,
            explanation,
            warnings: Vec::new(),
            metadata: ItemMetadata::Generic,
        }
    }
}

fn item_id(identity: &SourceIdentity) -> CleanableItemId {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    identity.hash(&mut hasher);
    CleanableItemId(hasher.finish())
}

fn parse_msix_json(output: &str) -> Result<Vec<MsixRecord>, String> {
    if output.trim().is_empty() {
        return Ok(Vec::new());
    }
    let value: Value = serde_json::from_str(output).map_err(|error| error.to_string())?;
    match value {
        Value::Null => Ok(Vec::new()),
        Value::Array(records) => records
            .into_iter()
            .map(|record| serde_json::from_value(record).map_err(|error| error.to_string()))
            .collect(),
        record @ Value::Object(_) => serde_json::from_value(record)
            .map(|record| vec![record])
            .map_err(|error| error.to_string()),
        _ => Err("PowerShell returned an unexpected MSIX inventory shape".to_string()),
    }
}

fn discover_portable_records(
    programs_root: &Path,
    host: HostOs,
) -> Result<Vec<PortableRecord>, String> {
    let read_dir = match std::fs::read_dir(programs_root) {
        Ok(read_dir) => read_dir,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.to_string()),
    };
    let resolved_root = std::fs::canonicalize(programs_root).map_err(|error| error.to_string())?;
    let mut records = Vec::new();
    for entry in read_dir.flatten() {
        let path = entry.path();
        let Ok(metadata) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            continue;
        }
        let Ok(resolved) = std::fs::canonicalize(&path) else {
            continue;
        };
        if !is_direct_child(host, &resolved_root, &resolved) {
            continue;
        }
        let direct_executable = std::fs::read_dir(&path)
            .ok()
            .into_iter()
            .flatten()
            .flatten()
            .any(|entry| probable_app_executable(entry.path().as_path()));
        if !direct_executable {
            continue;
        }
        records.push(PortableRecord {
            programs_root: resolved_root.clone(),
            location: resolved,
            display_name: entry.file_name().to_string_lossy().into_owned(),
            direct_executable,
        });
    }
    Ok(records)
}

fn probable_app_executable(path: &Path) -> bool {
    let Ok(metadata) = std::fs::symlink_metadata(path) else {
        return false;
    };
    if !metadata.is_file()
        || !path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
    {
        return false;
    }
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    !["setup", "update", "updater", "installer"].contains(&stem.as_str())
        && !stem.starts_with("unins")
}

#[cfg(target_os = "windows")]
#[derive(Default)]
pub struct InstalledAppsScanner;

#[cfg(target_os = "windows")]
impl InstalledAppsScanner {
    pub fn new() -> Self {
        Self
    }
}

#[cfg(target_os = "windows")]
impl CleanerScanner for InstalledAppsScanner {
    fn category(&self) -> CleanerCategory {
        CleanerCategory::InstalledApps
    }

    fn required_permissions(&self) -> &[MacPermission] {
        const NONE: &[MacPermission] = &[];
        NONE
    }

    fn scan(
        &self,
        _context: &ScanContext,
        progress: &dyn ProgressSink,
        cancellation: &CancellationToken,
    ) -> Result<CategoryScanResult, ScanError> {
        progress.report(ScanProgress {
            category: CleanerCategory::InstalledApps,
            phase: ScanPhase::Preparing,
            current_path: None,
            scanned_entries: 0,
            discovered_items: 0,
            discovered_bytes: 0,
        });

        let mut warnings = Vec::new();
        let mut skipped_sources = Vec::new();
        let (registry, registry_scanned) =
            read_registry_records(&mut warnings, &mut skipped_sources);
        if cancellation.is_cancelled() {
            return Err(ScanError::Cancelled);
        }

        let msix = match query_msix_records() {
            Ok(records) => records,
            Err(error) => {
                warnings.push(ScanWarning {
                    message: format!("MSIX inventory: {error}"),
                });
                skipped_sources.push(PathBuf::from("MSIX packages"));
                Vec::new()
            }
        };
        if cancellation.is_cancelled() {
            return Err(ScanError::Cancelled);
        }

        let portable = match std::env::var_os("LOCALAPPDATA") {
            Some(local_app_data) => {
                let root = PathBuf::from(local_app_data).join("Programs");
                match discover_portable_records(&root, HostOs::Windows) {
                    Ok(records) => records,
                    Err(error) => {
                        warnings.push(ScanWarning {
                            message: format!("{}: {error}", root.display()),
                        });
                        skipped_sources.push(root);
                        Vec::new()
                    }
                }
            }
            None => {
                warnings.push(ScanWarning {
                    message: "%LOCALAPPDATA% is unavailable; portable apps were skipped."
                        .to_string(),
                });
                skipped_sources.push(PathBuf::from("%LOCALAPPDATA%\\Programs"));
                Vec::new()
            }
        };

        let scanned_entries = registry_scanned + msix.len() as u64 + portable.len() as u64;
        let mut items: Vec<CleanableItem> = inventory_from_records(registry, msix, portable)
            .into_iter()
            .map(InventoryApp::into_item)
            .collect();
        items.sort_by(|left, right| {
            left.display_name
                .to_ascii_lowercase()
                .cmp(&right.display_name.to_ascii_lowercase())
        });
        progress.report(ScanProgress {
            category: CleanerCategory::InstalledApps,
            phase: ScanPhase::Completed,
            current_path: None,
            scanned_entries,
            discovered_items: items.len() as u64,
            discovered_bytes: 0,
        });

        Ok(CategoryScanResult {
            category: CleanerCategory::InstalledApps,
            items,
            scanned_entries,
            estimated_reclaimable_bytes: 0,
            warnings,
            completeness: if skipped_sources.is_empty() {
                ScanCompleteness::Complete
            } else {
                ScanCompleteness::Partial {
                    skipped_roots: skipped_sources,
                    reason: PartialScanReason::UnsupportedEnvironment,
                }
            },
        })
    }
}

#[cfg(target_os = "windows")]
fn query_msix_records() -> Result<Vec<MsixRecord>, String> {
    let command = msix_command();
    let output = std::process::Command::new(command.program)
        .args(command.args)
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    parse_msix_json(&String::from_utf8_lossy(&output.stdout))
}

#[cfg(target_os = "windows")]
fn read_registry_records(
    warnings: &mut Vec<ScanWarning>,
    skipped_sources: &mut Vec<PathBuf>,
) -> (Vec<RegistryRecord>, u64) {
    let mut records = Vec::new();
    let mut scanned = 0;
    for scope in REGISTRY_SCOPES {
        match enumerate_registry_scope(scope) {
            Ok(scope_records) => {
                scanned += scope_records.len() as u64;
                records.extend(scope_records);
            }
            Err(error) => {
                let label = registry_scope_label(scope);
                warnings.push(ScanWarning {
                    message: format!("{label}: Windows registry error {error}"),
                });
                skipped_sources.push(PathBuf::from(label));
            }
        }
    }
    (records, scanned)
}

#[cfg(target_os = "windows")]
fn enumerate_registry_scope(scope: RegistryScope) -> Result<Vec<RegistryRecord>, u32> {
    use windows_sys::Win32::Foundation::{
        ERROR_FILE_NOT_FOUND, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS,
    };
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY,
        RegCloseKey, RegEnumKeyExW, RegOpenKeyExW,
    };

    struct Key(HKEY);
    impl Drop for Key {
        fn drop(&mut self) {
            unsafe {
                RegCloseKey(self.0);
            }
        }
    }

    let hive = match scope.hive {
        RegistryHive::Machine => HKEY_LOCAL_MACHINE,
        RegistryHive::CurrentUser => HKEY_CURRENT_USER,
    };
    let view = match scope.view {
        RegistryView::Registry64 => KEY_WOW64_64KEY,
        RegistryView::Registry32 => KEY_WOW64_32KEY,
    };
    let uninstall_key = wide(UNINSTALL_KEY);
    let mut raw_key: HKEY = std::ptr::null_mut();
    let status = unsafe {
        RegOpenKeyExW(
            hive,
            uninstall_key.as_ptr(),
            0,
            KEY_READ | view,
            &mut raw_key,
        )
    };
    if status == ERROR_FILE_NOT_FOUND {
        return Ok(Vec::new());
    }
    if status != ERROR_SUCCESS {
        return Err(status);
    }
    let parent = Key(raw_key);

    let mut records = Vec::new();
    let mut index = 0;
    loop {
        let mut name = [0u16; 256];
        let mut name_len = name.len() as u32;
        let status = unsafe {
            RegEnumKeyExW(
                parent.0,
                index,
                name.as_mut_ptr(),
                &mut name_len,
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        if status == ERROR_NO_MORE_ITEMS {
            break;
        }
        index += 1;
        if status != ERROR_SUCCESS {
            return Err(status);
        }

        let key_name = String::from_utf16_lossy(&name[..name_len as usize]);
        let key_name_wide = wide(&key_name);
        let mut raw_child: HKEY = std::ptr::null_mut();
        if unsafe {
            RegOpenKeyExW(
                parent.0,
                key_name_wide.as_ptr(),
                0,
                KEY_READ | view,
                &mut raw_child,
            )
        } != ERROR_SUCCESS
        {
            continue;
        }
        let child = Key(raw_child);
        records.push(RegistryRecord {
            scope,
            key_name,
            display_name: registry_string(child.0, "DisplayName"),
            version: registry_string(child.0, "DisplayVersion"),
            publisher: registry_string(child.0, "Publisher"),
            install_location: registry_string(child.0, "InstallLocation").map(PathBuf::from),
            estimated_size_kib: registry_dword(child.0, "EstimatedSize").map(u64::from),
            system_component: registry_flag(child.0, "SystemComponent"),
            no_remove: registry_flag(child.0, "NoRemove"),
            parent_key_name: registry_string(child.0, "ParentKeyName"),
            release_type: registry_string(child.0, "ReleaseType"),
        });
    }
    Ok(records)
}

#[cfg(target_os = "windows")]
fn registry_string(key: windows_sys::Win32::System::Registry::HKEY, name: &str) -> Option<String> {
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ, RRF_ZEROONFAILURE, RegGetValueW,
    };

    let name = wide(name);
    let flags = RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ | RRF_ZEROONFAILURE;
    let mut byte_count = 0;
    if unsafe {
        RegGetValueW(
            key,
            std::ptr::null(),
            name.as_ptr(),
            flags,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut byte_count,
        )
    } != ERROR_SUCCESS
        || byte_count == 0
    {
        return None;
    }
    let mut value = vec![0u16; byte_count.div_ceil(2) as usize];
    if unsafe {
        RegGetValueW(
            key,
            std::ptr::null(),
            name.as_ptr(),
            flags,
            std::ptr::null_mut(),
            value.as_mut_ptr().cast(),
            &mut byte_count,
        )
    } != ERROR_SUCCESS
    {
        return None;
    }
    let length = (byte_count as usize / 2).min(value.len());
    let end = value[..length]
        .iter()
        .position(|character| *character == 0)
        .unwrap_or(length);
    Some(String::from_utf16_lossy(&value[..end]))
}

#[cfg(target_os = "windows")]
fn registry_flag(key: windows_sys::Win32::System::Registry::HKEY, name: &str) -> bool {
    registry_dword(key, name) == Some(1)
        || registry_string(key, name).is_some_and(|value| value.trim() == "1")
}

#[cfg(target_os = "windows")]
fn registry_dword(key: windows_sys::Win32::System::Registry::HKEY, name: &str) -> Option<u32> {
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{RRF_RT_REG_DWORD, RegGetValueW};

    let name = wide(name);
    let mut value = 0u32;
    let mut byte_count = std::mem::size_of::<u32>() as u32;
    (unsafe {
        RegGetValueW(
            key,
            std::ptr::null(),
            name.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            (&mut value as *mut u32).cast(),
            &mut byte_count,
        )
    } == ERROR_SUCCESS)
        .then_some(value)
}

#[cfg(target_os = "windows")]
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(target_os = "windows")]
fn registry_scope_label(scope: RegistryScope) -> &'static str {
    match (scope.hive, scope.view) {
        (RegistryHive::Machine, RegistryView::Registry64) => "HKLM 64-bit uninstall entries",
        (RegistryHive::Machine, RegistryView::Registry32) => "HKLM 32-bit uninstall entries",
        (RegistryHive::CurrentUser, RegistryView::Registry64) => "HKCU 64-bit uninstall entries",
        (RegistryHive::CurrentUser, RegistryView::Registry32) => "HKCU 32-bit uninstall entries",
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::cleaner::core::risk::ItemCapability;

    fn registry_record(scope: RegistryScope, name: &str, location: &str) -> RegistryRecord {
        RegistryRecord {
            scope,
            key_name: name.to_string(),
            display_name: Some(name.to_string()),
            version: Some("1.0".to_string()),
            publisher: Some("Example Co".to_string()),
            install_location: Some(PathBuf::from(location)),
            estimated_size_kib: Some(10),
            system_component: false,
            no_remove: false,
            parent_key_name: None,
            release_type: None,
        }
    }

    fn msix_record(name: &str, family: &str) -> MsixRecord {
        MsixRecord {
            name: name.to_string(),
            package_full_name: Some(format!("{name}_1.0_x64")),
            package_family_name: Some(family.to_string()),
            version: Some("1.0".to_string()),
            install_location: Some(PathBuf::from(format!(
                r"C:\Program Files\WindowsApps\{name}"
            ))),
            publisher: Some("CN=Example".to_string()),
            is_framework: false,
            is_resource_package: false,
            non_removable: false,
            signature_kind: Value::String("Store".to_string()),
        }
    }

    #[test]
    fn registry_inventory_covers_both_views_and_both_hives() {
        assert_eq!(
            REGISTRY_SCOPES,
            [
                RegistryScope {
                    hive: RegistryHive::Machine,
                    view: RegistryView::Registry64,
                },
                RegistryScope {
                    hive: RegistryHive::Machine,
                    view: RegistryView::Registry32,
                },
                RegistryScope {
                    hive: RegistryHive::CurrentUser,
                    view: RegistryView::Registry64,
                },
                RegistryScope {
                    hive: RegistryHive::CurrentUser,
                    view: RegistryView::Registry32,
                },
            ]
        );
        let records = REGISTRY_SCOPES
            .into_iter()
            .enumerate()
            .map(|(index, scope)| {
                registry_record(
                    scope,
                    &format!("App {index}"),
                    &format!(r"C:\Apps\App{index}"),
                )
            })
            .collect();
        assert_eq!(inventory_from_records(records, vec![], vec![]).len(), 4);
    }

    #[test]
    fn registry_components_updates_and_nonremovable_entries_are_filtered() {
        let scope = REGISTRY_SCOPES[0];
        let mut records = vec![registry_record(scope, "Visible App", r"C:\Apps\Visible")];
        let mut system = registry_record(scope, "System Component", r"C:\Windows\Component");
        system.system_component = true;
        records.push(system);
        let mut update = registry_record(scope, "Security Update for Windows", r"C:\Windows");
        update.release_type = Some("Security Update".to_string());
        records.push(update);
        let mut child = registry_record(scope, "App Language Pack", r"C:\Apps\Visible\lang");
        child.parent_key_name = Some("Visible App".to_string());
        records.push(child);
        let mut no_remove = registry_record(scope, "Managed App", r"C:\Apps\Managed");
        no_remove.no_remove = true;
        records.push(no_remove);
        records.push(registry_record(
            scope,
            "Example Runtime",
            r"C:\Apps\Runtime",
        ));

        let inventory = inventory_from_records(records, vec![], vec![]);
        assert_eq!(inventory.len(), 1);
        assert_eq!(inventory[0].display_name, "Visible App");
        let item = inventory
            .into_iter()
            .next()
            .expect("visible app")
            .into_item();
        assert!(
            item.capabilities
                .contains(&ItemCapability::UninstallApplication)
        );
        assert!(
            !item.capabilities.contains(&ItemCapability::MoveToTrash),
            "a registry install location is never a deletion target"
        );
    }

    #[test]
    fn typed_identities_and_matching_locations_deduplicate_sources() {
        let product_code = "{12345678-1234-1234-1234-1234567890ab}";
        let mut machine = registry_record(REGISTRY_SCOPES[0], product_code, r"C:\Apps\One");
        machine.display_name = Some("One".to_string());
        let mut user = registry_record(REGISTRY_SCOPES[3], product_code, r"c:\apps\ONE\");
        user.display_name = Some("One".to_string());
        let portable = PortableRecord {
            programs_root: PathBuf::from(r"C:\Apps"),
            location: PathBuf::from(r"C:\Apps\One"),
            display_name: "One".to_string(),
            direct_executable: true,
        };
        let store_v1 = msix_record("StoreApp", "Example.Store_abc");
        let mut store_v2 = msix_record("StoreApp", "example.store_ABC");
        store_v2.version = Some("2.0".to_string());

        let inventory = inventory_from_records(
            vec![machine, user],
            vec![store_v1, store_v2],
            vec![portable],
        );
        assert_eq!(inventory.len(), 2);
        assert_eq!(
            inventory
                .iter()
                .filter(|app| app.display_name == "One")
                .count(),
            1
        );
        assert_eq!(
            inventory
                .iter()
                .filter(|app| app.display_name == "StoreApp")
                .count(),
            1
        );
    }

    #[test]
    fn protected_msix_packages_are_excluded_and_store_apps_use_native_settings() {
        let mut framework = msix_record("Example.Framework", "framework_abc");
        framework.is_framework = true;
        let mut resource = msix_record("Example.Resources", "resources_abc");
        resource.is_resource_package = true;
        let mut non_removable = msix_record("Example.System", "system_abc");
        non_removable.non_removable = true;
        let mut system_signature = msix_record("Example.Shell", "shell_abc");
        system_signature.signature_kind = Value::from(4);
        let normal = msix_record("Example.App", "app_abc");

        let inventory = inventory_from_records(
            vec![],
            vec![framework, resource, non_removable, system_signature, normal],
            vec![],
        );
        assert_eq!(inventory.len(), 1);
        let item = inventory
            .into_iter()
            .next()
            .expect("normal package")
            .into_item();
        assert!(
            item.capabilities
                .contains(&ItemCapability::UninstallApplication)
        );
        assert!(!item.capabilities.contains(&ItemCapability::MoveToTrash));
    }

    #[test]
    fn a_shared_install_location_vetoes_every_uninstall_action() {
        let scope = REGISTRY_SCOPES[0];
        let records = vec![
            registry_record(scope, "Suite Editor", r"C:\Apps\SharedSuite"),
            registry_record(scope, "Suite Viewer", r"c:\apps\sharedsuite\"),
        ];
        let items: Vec<_> = inventory_from_records(records, vec![], vec![])
            .into_iter()
            .map(InventoryApp::into_item)
            .collect();

        assert_eq!(items.len(), 2);
        assert!(items.iter().all(|item| item.risk == RiskLevel::Protected));
        assert!(items.iter().all(|item| {
            !item
                .capabilities
                .contains(&ItemCapability::UninstallApplication)
        }));
        assert!(
            items
                .iter()
                .all(|item| !item.capabilities.contains(&ItemCapability::MoveToTrash))
        );
    }

    #[test]
    fn portable_records_must_be_direct_children_with_a_direct_executable() {
        let root = PathBuf::from(r"C:\Users\Ada\AppData\Local\Programs");
        let records = vec![
            PortableRecord {
                programs_root: root.clone(),
                location: root.join("Portable One"),
                display_name: "Portable One".to_string(),
                direct_executable: true,
            },
            PortableRecord {
                programs_root: root.clone(),
                location: root.join("Nested").join("Portable Two"),
                display_name: "Portable Two".to_string(),
                direct_executable: true,
            },
            PortableRecord {
                programs_root: root.clone(),
                location: root.join("No Executable"),
                display_name: "No Executable".to_string(),
                direct_executable: false,
            },
        ];

        let inventory = inventory_from_records(vec![], vec![], records);
        assert_eq!(inventory.len(), 1);
        let item = inventory
            .into_iter()
            .next()
            .expect("portable app")
            .into_item();
        assert_eq!(item.display_name, "Portable One");
        assert!(
            !item
                .capabilities
                .contains(&ItemCapability::UninstallApplication)
        );
        assert!(!item.capabilities.contains(&ItemCapability::MoveToTrash));
    }

    #[test]
    fn portable_filesystem_discovery_never_recurses() {
        let temp = std::env::temp_dir().join(format!(
            "dodo-cleaner-portable-apps-{}-{}",
            std::process::id(),
            line!()
        ));
        let direct = temp.join("Direct App");
        let nested = temp.join("Nested App").join("bin");
        let installer_only = temp.join("Installer Only");
        fs::create_dir_all(&direct).expect("creates direct app");
        fs::create_dir_all(&nested).expect("creates nested app");
        fs::create_dir_all(&installer_only).expect("creates installer folder");
        fs::write(direct.join("Direct App.EXE"), b"exe").expect("writes direct executable");
        fs::write(nested.join("Nested App.exe"), b"exe").expect("writes nested executable");
        fs::write(installer_only.join("setup.exe"), b"exe").expect("writes setup executable");

        let records = discover_portable_records(&temp, HostOs::MacOs).expect("scans bounded root");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].display_name, "Direct App");

        fs::remove_dir_all(&temp).expect("removes temp tree");
    }

    #[test]
    fn msix_json_accepts_a_single_record_and_an_array() {
        let single = r#"{"Name":"Example.App","PackageFullName":"Example.App_1","PackageFamilyName":"Example.App_abc","Version":{"Major":1,"Minor":2,"Build":3,"Revision":4},"IsFramework":false,"IsResourcePackage":false,"NonRemovable":false,"SignatureKind":"Store"}"#;
        let parsed = parse_msix_json(single).expect("single package");
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].version.as_deref(), Some("1.2.3.4"));
        assert_eq!(
            parse_msix_json(&format!("[{single},{single}]"))
                .expect("package array")
                .len(),
            2
        );
    }

    #[test]
    fn process_invocations_are_fixed_and_argument_separated() {
        assert_eq!(
            msix_command(),
            CommandSpec {
                program: "powershell.exe",
                args: &[
                    "-NoLogo",
                    "-NoProfile",
                    "-NonInteractive",
                    "-Command",
                    "Get-AppxPackage | Select-Object Name,PackageFullName,PackageFamilyName,Version,InstallLocation,Publisher,IsFramework,IsResourcePackage,NonRemovable,SignatureKind | ConvertTo-Json -Compress",
                ],
            }
        );
        assert_eq!(
            installed_apps_settings_command(),
            CommandSpec {
                program: "explorer.exe",
                args: &["ms-settings:appsfeatures"],
            }
        );
    }
}
