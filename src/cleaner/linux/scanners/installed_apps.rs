//! Linux installed desktop-application inventory.
//!
//! Desktop entries are the user-facing evidence; package adapters attach
//! ownership and metadata without ever executing an entry's `Exec` text.
//! Native dpkg, RPM and pacman packages plus system Flatpaks and Snaps are
//! inventory-only. Only user Flatpaks and bounded, user-owned AppImages carry
//! an uninstall action. Package-managed directories are never deletion roots.

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use crate::cleaner::core::cancellation::CancellationToken;
use crate::cleaner::core::category::CleanerCategory;
use crate::cleaner::core::errors::ScanError;
use crate::cleaner::core::item::{
    CleanableItem, CleanableItemId, InstalledAppAction, InstalledAppMetadata, InstalledAppScope,
    ItemMetadata,
};
use crate::cleaner::core::permissions::MacPermission;
use crate::cleaner::core::progress::{ProgressSink, ScanPhase, ScanProgress};
use crate::cleaner::core::report::{
    CategoryScanResult, PartialScanReason, ScanCompleteness, ScanWarning,
};
use crate::cleaner::core::risk::{ItemCapability, RiskLevel, SelectionPolicy};
use crate::cleaner::core::safety::{contains_path, is_direct_child};
use crate::cleaner::core::scan_context::ScanContext;
use crate::cleaner::core::scanner::CleanerScanner;
use crate::paths::HostOs;

const DPKG_FORMAT: &str = "${binary:Package}\t${Version}\t${Maintainer}\t${Installed-Size}\\n";
const RPM_FORMAT: &str = "%{NAME}\t%{VERSION}-%{RELEASE}\t%{VENDOR}\t%{SIZE}\\n";
const FLATPAK_COLUMNS: &str = "application,name,version,size";

#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct CommandSpec {
    pub program: &'static str,
    pub args: Vec<OsString>,
}

#[derive(Debug)]
struct CommandOutput {
    success: bool,
    stdout: String,
    stderr: String,
}

#[derive(Debug)]
enum AdapterError {
    Unavailable,
    Failed(String),
}

trait CommandRunner {
    fn run(&self, command: &CommandSpec) -> Result<CommandOutput, AdapterError>;
}

struct ProcessRunner;

impl CommandRunner for ProcessRunner {
    fn run(&self, command: &CommandSpec) -> Result<CommandOutput, AdapterError> {
        let output = std::process::Command::new(command.program)
            .args(&command.args)
            .env("LC_ALL", "C")
            .env("LANG", "C")
            .output()
            .map_err(|error| {
                if error.kind() == std::io::ErrorKind::NotFound {
                    AdapterError::Unavailable
                } else {
                    AdapterError::Failed(error.to_string())
                }
            })?;
        Ok(CommandOutput {
            success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
enum Source {
    Dpkg,
    Rpm,
    Pacman,
    Flatpak,
    Snap,
    AppImage,
}

impl Source {
    fn label(self) -> &'static str {
        match self {
            Self::Dpkg => "dpkg",
            Self::Rpm => "RPM",
            Self::Pacman => "pacman",
            Self::Flatpak => "Flatpak",
            Self::Snap => "Snap",
            Self::AppImage => "AppImage",
        }
    }

    fn group(self, scope: InstalledAppScope) -> &'static str {
        match (self, scope) {
            (Self::Dpkg, _) => "Debian / Ubuntu (dpkg)",
            (Self::Rpm, _) => "Fedora (RPM)",
            (Self::Pacman, _) => "Arch Linux (pacman)",
            (Self::Flatpak, InstalledAppScope::User) => "Flatpak · User",
            (Self::Flatpak, InstalledAppScope::System) => "Flatpak · System",
            (Self::Snap, _) => "Snap",
            (Self::AppImage, _) => "AppImage",
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug, Hash)]
struct SourceIdentity {
    source: Source,
    scope: InstalledAppScope,
    identifier: String,
}

#[derive(Clone, PartialEq, Eq, Debug)]
struct InventoryApp {
    identity: SourceIdentity,
    display_name: String,
    version: Option<String>,
    publisher: Option<String>,
    location: Option<PathBuf>,
    logical_size: u64,
    action: Option<InstalledAppAction>,
    shared_location: bool,
}

#[derive(Clone, PartialEq, Eq, Debug)]
struct DesktopEntry {
    id: String,
    path: PathBuf,
    scope: InstalledAppScope,
    name: String,
    exec_target: Option<PathBuf>,
    snap_instance: Option<String>,
    visible: bool,
}

#[derive(Clone, PartialEq, Eq, Debug)]
struct DesktopRoot {
    path: PathBuf,
    scope: InstalledAppScope,
}

#[derive(Clone, PartialEq, Eq, Debug)]
struct PackageMetadata {
    identifier: String,
    version: Option<String>,
    publisher: Option<String>,
    logical_size: u64,
}

trait PackageAdapter {
    fn collect(
        &self,
        desktops: &[DesktopEntry],
        runner: &dyn CommandRunner,
    ) -> Result<Vec<InventoryApp>, AdapterError>;
}

struct DpkgAdapter;
struct RpmAdapter;
struct PacmanAdapter;

impl PackageAdapter for DpkgAdapter {
    fn collect(
        &self,
        desktops: &[DesktopEntry],
        runner: &dyn CommandRunner,
    ) -> Result<Vec<InventoryApp>, AdapterError> {
        let desktops = system_desktops(desktops);
        if desktops.is_empty() {
            return Ok(Vec::new());
        }

        let mut owner_args = vec![OsString::from("-S")];
        owner_args.extend(desktops.iter().map(|entry| entry.path.as_os_str().into()));
        let owner_output = runner.run(&CommandSpec {
            program: "dpkg-query",
            args: owner_args,
        })?;
        let owners = parse_dpkg_owners(&owner_output.stdout);
        if owners.is_empty() {
            return Ok(Vec::new());
        }

        let packages: HashSet<String> = owners.values().cloned().collect();
        let mut metadata_args = vec![
            OsString::from("-W"),
            OsString::from(format!("-f={DPKG_FORMAT}")),
        ];
        metadata_args.extend(packages.iter().map(OsString::from));
        let metadata_output = runner.run(&CommandSpec {
            program: "dpkg-query",
            args: metadata_args,
        })?;
        let metadata = parse_dpkg_metadata(&metadata_output.stdout);
        if !metadata_output.success && metadata.is_empty() {
            return Err(AdapterError::Failed(command_message(&metadata_output)));
        }

        Ok(desktops
            .into_iter()
            .filter_map(|desktop| {
                let package = owners.get(&desktop.path)?;
                let metadata = metadata.get(package)?;
                Some(native_app(Source::Dpkg, desktop, metadata))
            })
            .collect())
    }
}

impl PackageAdapter for RpmAdapter {
    fn collect(
        &self,
        desktops: &[DesktopEntry],
        runner: &dyn CommandRunner,
    ) -> Result<Vec<InventoryApp>, AdapterError> {
        let mut apps = Vec::new();
        for desktop in system_desktops(desktops) {
            let output = runner.run(&CommandSpec {
                program: "rpm",
                args: vec![
                    OsString::from("-qf"),
                    OsString::from("--queryformat"),
                    OsString::from(RPM_FORMAT),
                    desktop.path.as_os_str().into(),
                ],
            })?;
            if !output.success {
                continue;
            }
            let Some(metadata) = parse_rpm_metadata(&output.stdout) else {
                continue;
            };
            apps.push(native_app(Source::Rpm, desktop, &metadata));
        }
        Ok(apps)
    }
}

impl PackageAdapter for PacmanAdapter {
    fn collect(
        &self,
        desktops: &[DesktopEntry],
        runner: &dyn CommandRunner,
    ) -> Result<Vec<InventoryApp>, AdapterError> {
        let desktops = system_desktops(desktops);
        if desktops.is_empty() {
            return Ok(Vec::new());
        }

        let mut owner_args = vec![OsString::from("-Qo")];
        owner_args.extend(desktops.iter().map(|entry| entry.path.as_os_str().into()));
        let owner_output = runner.run(&CommandSpec {
            program: "pacman",
            args: owner_args,
        })?;
        let owners = parse_pacman_owners(&owner_output.stdout);
        if owners.is_empty() {
            return Ok(Vec::new());
        }

        let packages: HashSet<String> = owners.values().cloned().collect();
        let mut metadata_args = vec![OsString::from("-Qi")];
        metadata_args.extend(packages.iter().map(OsString::from));
        let metadata_output = runner.run(&CommandSpec {
            program: "pacman",
            args: metadata_args,
        })?;
        let metadata = parse_pacman_metadata(&metadata_output.stdout);
        if !metadata_output.success && metadata.is_empty() {
            return Err(AdapterError::Failed(command_message(&metadata_output)));
        }

        Ok(desktops
            .into_iter()
            .filter_map(|desktop| {
                let package = owners.get(&desktop.path)?;
                let metadata = metadata.get(package)?;
                Some(native_app(Source::Pacman, desktop, metadata))
            })
            .collect())
    }
}

fn system_desktops(desktops: &[DesktopEntry]) -> Vec<&DesktopEntry> {
    desktops
        .iter()
        .filter(|entry| entry.scope == InstalledAppScope::System)
        .collect()
}

fn native_app(source: Source, desktop: &DesktopEntry, metadata: &PackageMetadata) -> InventoryApp {
    InventoryApp {
        identity: SourceIdentity {
            source,
            scope: InstalledAppScope::System,
            identifier: metadata.identifier.clone(),
        },
        display_name: desktop.name.clone(),
        version: metadata.version.clone(),
        publisher: metadata.publisher.clone(),
        location: desktop.exec_target.clone(),
        logical_size: metadata.logical_size,
        action: None,
        shared_location: false,
    }
}

fn command_message(output: &CommandOutput) -> String {
    if output.stderr.is_empty() {
        "package manager returned an unsuccessful status".to_string()
    } else {
        output.stderr.clone()
    }
}

fn parse_dpkg_owners(output: &str) -> HashMap<PathBuf, String> {
    output
        .lines()
        .filter_map(|line| {
            let (packages, path) = line.rsplit_once(": ")?;
            if packages.contains(',') {
                return None;
            }
            let package = packages.trim();
            valid_native_identifier(package)
                .then(|| (PathBuf::from(path.trim()), package.to_string()))
        })
        .collect()
}

fn parse_dpkg_metadata(output: &str) -> HashMap<String, PackageMetadata> {
    output
        .lines()
        .filter_map(|line| {
            let mut columns = line.split('\t');
            let identifier = columns.next()?.trim();
            if !valid_native_identifier(identifier) {
                return None;
            }
            let version = clean(columns.next().unwrap_or_default());
            let publisher = clean(columns.next().unwrap_or_default());
            let logical_size = columns
                .next()
                .and_then(|size| size.trim().parse::<u64>().ok())
                .unwrap_or(0)
                .saturating_mul(1024);
            Some((
                identifier.to_string(),
                PackageMetadata {
                    identifier: identifier.to_string(),
                    version,
                    publisher,
                    logical_size,
                },
            ))
        })
        .collect()
}

fn parse_rpm_metadata(output: &str) -> Option<PackageMetadata> {
    let mut columns = output
        .lines()
        .find(|line| !line.trim().is_empty())?
        .split('\t');
    let identifier = columns.next()?.trim();
    if !valid_native_identifier(identifier) {
        return None;
    }
    Some(PackageMetadata {
        identifier: identifier.to_string(),
        version: clean(columns.next().unwrap_or_default()),
        publisher: clean(columns.next().unwrap_or_default()),
        logical_size: columns
            .next()
            .and_then(|size| size.trim().parse().ok())
            .unwrap_or(0),
    })
}

fn parse_pacman_owners(output: &str) -> HashMap<PathBuf, String> {
    output
        .lines()
        .filter_map(|line| {
            let (path, owner) = line.rsplit_once(" is owned by ")?;
            let package = owner.split_whitespace().next()?;
            valid_native_identifier(package)
                .then(|| (PathBuf::from(path.trim()), package.to_string()))
        })
        .collect()
}

fn parse_pacman_metadata(output: &str) -> HashMap<String, PackageMetadata> {
    output
        .split("\n\n")
        .filter_map(|block| {
            let fields: HashMap<&str, &str> = block
                .lines()
                .filter_map(|line| line.split_once(':'))
                .map(|(key, value)| (key.trim(), value.trim()))
                .collect();
            let identifier = *fields.get("Name")?;
            if !valid_native_identifier(identifier) {
                return None;
            }
            Some((
                identifier.to_string(),
                PackageMetadata {
                    identifier: identifier.to_string(),
                    version: fields.get("Version").and_then(|value| clean(value)),
                    publisher: fields.get("Packager").and_then(|value| clean(value)),
                    logical_size: fields
                        .get("Installed Size")
                        .and_then(|value| parse_human_size(value))
                        .unwrap_or(0),
                },
            ))
        })
        .collect()
}

fn valid_native_identifier(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('-')
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"+._:-".contains(&byte))
}

fn flatpak_list_command(scope: InstalledAppScope) -> CommandSpec {
    let scope = match scope {
        InstalledAppScope::User => "--user",
        InstalledAppScope::System => "--system",
    };
    CommandSpec {
        program: "flatpak",
        args: [
            scope,
            "list",
            "--app",
            &format!("--columns={FLATPAK_COLUMNS}"),
        ]
        .into_iter()
        .map(OsString::from)
        .collect(),
    }
}

pub(crate) fn flatpak_uninstall_command(application_id: &str) -> Option<CommandSpec> {
    valid_flatpak_id(application_id).then(|| CommandSpec {
        program: "flatpak",
        args: [
            "--user",
            "uninstall",
            "--app",
            "--noninteractive",
            application_id,
        ]
        .into_iter()
        .map(OsString::from)
        .collect(),
    })
}

#[cfg(target_os = "linux")]
pub(crate) fn uninstall_user_flatpak(application_id: &str) -> Result<(), String> {
    let command = flatpak_uninstall_command(application_id)
        .ok_or_else(|| "invalid Flatpak application identifier".to_string())?;
    let output = ProcessRunner.run(&command).map_err(|error| match error {
        AdapterError::Unavailable => "flatpak is unavailable".to_string(),
        AdapterError::Failed(message) => message,
    })?;
    if output.success {
        Ok(())
    } else {
        Err(command_message(&output))
    }
}

fn collect_flatpak(
    scope: InstalledAppScope,
    desktops: &[DesktopEntry],
    runner: &dyn CommandRunner,
) -> Result<Vec<InventoryApp>, AdapterError> {
    let output = runner.run(&flatpak_list_command(scope))?;
    let records = parse_flatpak_output(&output.stdout, scope, desktops);
    if !output.success && records.is_empty() {
        return Err(AdapterError::Failed(command_message(&output)));
    }
    Ok(records)
}

fn parse_flatpak_output(
    output: &str,
    scope: InstalledAppScope,
    desktops: &[DesktopEntry],
) -> Vec<InventoryApp> {
    output
        .lines()
        .filter_map(|line| {
            let mut columns = line.split('\t');
            let identifier = columns.next()?.trim();
            if !valid_flatpak_id(identifier) {
                return None;
            }
            let listed_name = clean(columns.next().unwrap_or_default());
            let version = clean(columns.next().unwrap_or_default());
            let logical_size = columns.next().and_then(parse_human_size).unwrap_or(0);
            let desktop = desktops.iter().find(|entry| entry.id == identifier);
            let display_name = desktop
                .map(|entry| entry.name.clone())
                .or(listed_name)
                .unwrap_or_else(|| identifier.to_string());
            Some(InventoryApp {
                identity: SourceIdentity {
                    source: Source::Flatpak,
                    scope,
                    identifier: identifier.to_string(),
                },
                display_name,
                version,
                publisher: None,
                location: None,
                logical_size,
                action: (scope == InstalledAppScope::User).then(|| {
                    InstalledAppAction::FlatpakUser {
                        application_id: identifier.to_string(),
                    }
                }),
                shared_location: false,
            })
        })
        .collect()
}

fn valid_flatpak_id(value: &str) -> bool {
    value.len() <= 255
        && value.split('.').count() >= 2
        && value.split('.').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        })
}

fn snap_list_command() -> CommandSpec {
    CommandSpec {
        program: "snap",
        args: vec![OsString::from("list")],
    }
}

fn collect_snap(
    desktops: &[DesktopEntry],
    runner: &dyn CommandRunner,
) -> Result<Vec<InventoryApp>, AdapterError> {
    let output = runner.run(&snap_list_command())?;
    let records = parse_snap_output(&output.stdout, desktops);
    if !output.success && records.is_empty() {
        return Err(AdapterError::Failed(command_message(&output)));
    }
    Ok(records)
}

fn parse_snap_output(output: &str, desktops: &[DesktopEntry]) -> Vec<InventoryApp> {
    output
        .lines()
        .skip_while(|line| !line.trim_start().starts_with("Name "))
        .skip(1)
        .filter_map(|line| {
            let columns: Vec<&str> = line.split_whitespace().collect();
            let identifier = *columns.first()?;
            if !valid_snap_name(identifier) {
                return None;
            }
            let desktop = desktops.iter().find(|entry| {
                entry.snap_instance.as_deref() == Some(identifier)
                    || entry
                        .id
                        .strip_prefix(identifier)
                        .is_some_and(|rest| rest.starts_with('_'))
            })?;
            Some(InventoryApp {
                identity: SourceIdentity {
                    source: Source::Snap,
                    scope: InstalledAppScope::System,
                    identifier: identifier.to_string(),
                },
                display_name: desktop.name.clone(),
                version: columns.get(1).and_then(|value| clean(value)),
                publisher: columns.get(4).and_then(|value| clean(value)),
                location: desktop.exec_target.clone(),
                logical_size: 0,
                action: None,
                shared_location: false,
            })
        })
        .collect()
}

fn valid_snap_name(value: &str) -> bool {
    value.len() <= 40
        && !value.starts_with('-')
        && !value.ends_with('-')
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn parse_human_size(value: &str) -> Option<u64> {
    let mut parts = value.split_whitespace();
    let amount = parts.next()?.replace(',', ".").parse::<f64>().ok()?;
    let unit = parts.next().unwrap_or("B").to_ascii_lowercase();
    let multiplier = match unit.as_str() {
        "b" | "bytes" => 1f64,
        "kb" => 1_000f64,
        "mb" => 1_000_000f64,
        "gb" => 1_000_000_000f64,
        "kib" => 1024f64,
        "mib" => 1024f64.powi(2),
        "gib" => 1024f64.powi(3),
        _ => return None,
    };
    (amount.is_finite() && amount >= 0.0).then_some((amount * multiplier) as u64)
}

fn parse_desktop_entry(
    path: PathBuf,
    scope: InstalledAppScope,
    input: &str,
) -> Option<DesktopEntry> {
    let id = path
        .file_name()?
        .to_str()?
        .strip_suffix(".desktop")?
        .to_string();
    let mut in_desktop_group = false;
    let mut fields = HashMap::new();
    for raw_line in input.lines() {
        let line = raw_line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            in_desktop_group = line == "[Desktop Entry]";
            continue;
        }
        if !in_desktop_group || line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            fields.entry(key.trim()).or_insert(value.trim());
        }
    }
    if fields.get("Type").copied().unwrap_or("Application") != "Application" {
        return None;
    }
    let name = desktop_unescape(fields.get("Name")?)?;
    let hidden = desktop_bool(fields.get("Hidden").copied());
    let no_display = desktop_bool(fields.get("NoDisplay").copied());
    Some(DesktopEntry {
        id,
        path,
        scope,
        name,
        exec_target: fields
            .get("Exec")
            .and_then(|exec| desktop_exec_target(exec)),
        snap_instance: fields
            .get("X-SnapInstanceName")
            .and_then(|value| clean(value)),
        visible: !hidden && !no_display,
    })
}

fn desktop_bool(value: Option<&str>) -> bool {
    value.is_some_and(|value| value.eq_ignore_ascii_case("true"))
}

fn desktop_unescape(value: &str) -> Option<String> {
    let mut result = String::new();
    let mut characters = value.chars();
    while let Some(character) = characters.next() {
        if character != '\\' {
            result.push(character);
            continue;
        }
        result.push(match characters.next()? {
            's' => ' ',
            'n' => '\n',
            't' => '\t',
            'r' => '\r',
            '\\' => '\\',
            _ => return None,
        });
    }
    clean(&result)
}

fn desktop_exec_target(exec: &str) -> Option<PathBuf> {
    let token = first_exec_token(exec)?;
    if token.contains('%') {
        return None;
    }
    let path = PathBuf::from(token);
    path.is_absolute().then_some(path)
}

fn first_exec_token(exec: &str) -> Option<String> {
    let mut characters = exec.trim().chars().peekable();
    let quoted = characters.peek() == Some(&'"');
    if quoted {
        characters.next();
    }
    let mut token = String::new();
    let mut escaped = false;
    for character in characters {
        if escaped {
            token.push(character);
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else if quoted && character == '"' {
            return (!token.is_empty()).then_some(token);
        } else if !quoted && character.is_whitespace() {
            break;
        } else {
            token.push(character);
        }
    }
    (!escaped && !token.is_empty() && !quoted).then_some(token)
}

fn desktop_roots(home: Option<&Path>) -> Vec<DesktopRoot> {
    let mut roots = Vec::new();
    if let Some(home) = home {
        let data_home = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".local/share"));
        roots.push(DesktopRoot {
            path: data_home.join("applications"),
            scope: InstalledAppScope::User,
        });
    }

    let data_dirs: Vec<PathBuf> = std::env::var_os("XDG_DATA_DIRS")
        .map(|value| std::env::split_paths(&value).collect())
        .unwrap_or_else(|| {
            vec![
                PathBuf::from("/usr/local/share"),
                PathBuf::from("/usr/share"),
            ]
        });
    roots.extend(data_dirs.into_iter().map(|root| DesktopRoot {
        path: root.join("applications"),
        scope: InstalledAppScope::System,
    }));
    roots.push(DesktopRoot {
        path: PathBuf::from("/var/lib/snapd/desktop/applications"),
        scope: InstalledAppScope::System,
    });

    let mut seen = HashSet::new();
    roots.retain(|root| seen.insert(root.path.clone()));
    roots
}

fn discover_desktop_entries(
    roots: &[DesktopRoot],
) -> (Vec<DesktopEntry>, Vec<(PathBuf, String)>, u64) {
    let mut entries = Vec::new();
    let mut failures = Vec::new();
    let mut seen_ids = HashSet::new();
    let mut scanned = 0;
    for root in roots {
        let read_dir = match std::fs::read_dir(&root.path) {
            Ok(read_dir) => read_dir,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                failures.push((root.path.clone(), error.to_string()));
                continue;
            }
        };
        for file in read_dir.flatten() {
            let path = file.path();
            if path.extension().and_then(|value| value.to_str()) != Some("desktop") {
                continue;
            }
            scanned += 1;
            let Ok(input) = std::fs::read_to_string(&path) else {
                continue;
            };
            let Some(entry) = parse_desktop_entry(path, root.scope, &input) else {
                continue;
            };
            if !seen_ids.insert(entry.id.clone()) {
                continue;
            }
            if entry.visible {
                entries.push(entry);
            }
        }
    }
    (entries, failures, scanned)
}

fn discover_appimages(
    home: &Path,
    applications_root: &Path,
    desktops: &[DesktopEntry],
) -> Result<Vec<InventoryApp>, String> {
    let resolved_home = std::fs::canonicalize(home).map_err(|error| error.to_string())?;
    let mut records = Vec::new();

    for desktop in desktops
        .iter()
        .filter(|entry| entry.scope == InstalledAppScope::User)
    {
        let Some(path) = desktop.exec_target.as_deref() else {
            continue;
        };
        let Some(resolved) = safe_appimage(path) else {
            continue;
        };
        if resolved == resolved_home || !contains_path(HostOs::Unix, &resolved_home, &resolved) {
            continue;
        }
        records.push(appimage_record(resolved, desktop.name.clone()));
    }

    let read_dir = match std::fs::read_dir(applications_root) {
        Ok(read_dir) => read_dir,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(normalize_inventory(records));
        }
        Err(error) => return Err(error.to_string()),
    };
    let resolved_root =
        std::fs::canonicalize(applications_root).map_err(|error| error.to_string())?;
    if resolved_root == resolved_home
        || !contains_path(HostOs::Unix, &resolved_home, &resolved_root)
    {
        return Ok(normalize_inventory(records));
    }
    for entry in read_dir.flatten() {
        let Some(resolved) = safe_appimage(&entry.path()) else {
            continue;
        };
        if !is_direct_child(HostOs::Unix, &resolved_root, &resolved) {
            continue;
        }
        let display_name = resolved
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or("AppImage")
            .to_string();
        records.push(appimage_record(resolved, display_name));
    }
    Ok(normalize_inventory(records))
}

fn safe_appimage(path: &Path) -> Option<PathBuf> {
    if !path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("AppImage"))
    {
        return None;
    }
    let metadata = std::fs::symlink_metadata(path).ok()?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || !is_executable(&metadata) {
        return None;
    }
    std::fs::canonicalize(path).ok()
}

#[cfg(unix)]
fn is_executable(metadata: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt as _;
    metadata.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_executable(_metadata: &std::fs::Metadata) -> bool {
    false
}

fn appimage_record(path: PathBuf, display_name: String) -> InventoryApp {
    let logical_size = std::fs::metadata(&path)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    InventoryApp {
        identity: SourceIdentity {
            source: Source::AppImage,
            scope: InstalledAppScope::User,
            identifier: path.to_string_lossy().into_owned(),
        },
        display_name,
        version: None,
        publisher: None,
        location: Some(path),
        logical_size,
        action: Some(InstalledAppAction::AppImage),
        shared_location: false,
    }
}

fn normalize_inventory(records: Vec<InventoryApp>) -> Vec<InventoryApp> {
    // Desktop entries are evidence consumed by the package adapters, not a
    // second inventory source, so a package and its launcher become one row.
    let mut apps: Vec<InventoryApp> = Vec::new();
    for record in records {
        if let Some(existing) = apps
            .iter_mut()
            .find(|existing| existing.identity == record.identity)
        {
            merge_inventory(existing, &record);
        } else {
            apps.push(record);
        }
    }

    // ponytail: desktop inventories are small; index locations only if this
    // pairwise shared-location veto becomes measurable.
    for left in 0..apps.len() {
        for right in left + 1..apps.len() {
            if apps[left].identity != apps[right].identity
                && same_location(
                    apps[left].location.as_deref(),
                    apps[right].location.as_deref(),
                )
            {
                apps[left].shared_location = true;
                apps[right].shared_location = true;
            }
        }
    }
    apps
}

fn merge_inventory(target: &mut InventoryApp, other: &InventoryApp) {
    if target.version.is_none() {
        target.version.clone_from(&other.version);
    }
    if target.publisher.is_none() {
        target.publisher.clone_from(&other.publisher);
    }
    if target.location.is_none() {
        target.location.clone_from(&other.location);
    }
    if target.action.is_none() {
        target.action.clone_from(&other.action);
    }
    target.logical_size = target.logical_size.max(other.logical_size);
}

fn same_location(left: Option<&Path>, right: Option<&Path>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => {
            contains_path(HostOs::Unix, left, right) && contains_path(HostOs::Unix, right, left)
        }
        _ => false,
    }
}

fn clean(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty() && value != "(none)").then(|| value.to_string())
}

impl InventoryApp {
    fn into_item(self) -> CleanableItem {
        let action = (!self.shared_location).then_some(self.action).flatten();
        let mut capabilities = Vec::new();
        if self.location.is_some() {
            capabilities.extend([ItemCapability::RevealInFinder, ItemCapability::CopyPath]);
        }
        if action.is_some() {
            capabilities.push(ItemCapability::UninstallApplication);
        }

        let path = self.location.clone().unwrap_or_else(|| {
            PathBuf::from(format!(
                "{}://{}/{}",
                self.identity.source.label().to_ascii_lowercase(),
                match self.identity.scope {
                    InstalledAppScope::User => "user",
                    InstalledAppScope::System => "system",
                },
                self.identity.identifier
            ))
        });
        let explanation = if self.shared_location {
            "Another installed package references this exact location; removal is disabled."
        } else {
            match (&self.identity.source, &action) {
                (Source::Flatpak, Some(InstalledAppAction::FlatpakUser { .. })) => {
                    "User Flatpak. Uninstall uses fixed Flatpak arguments and keeps application data."
                }
                (Source::AppImage, Some(InstalledAppAction::AppImage)) => {
                    "Bounded AppImage. Uninstall moves only this file to Trash."
                }
                (Source::Flatpak, _) => "System Flatpak. Inventory only; no privilege helper is used.",
                (Source::Snap, _) => "Snap application. Inventory only; no privileged removal is attempted.",
                (Source::Dpkg | Source::Rpm | Source::Pacman, _) => {
                    "Native package. Inventory only; package-managed files are never deleted by dodo."
                }
                (Source::AppImage, _) => "AppImage removal is disabled because ownership is uncertain.",
            }
        }
        .to_string();
        let id = item_id(&self.identity);

        CleanableItem {
            id,
            category: CleanerCategory::InstalledApps,
            group: Some(self.identity.source.group(self.identity.scope).to_string()),
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
            metadata: ItemMetadata::InstalledApp(InstalledAppMetadata {
                source: self.identity.source.label().to_string(),
                identifier: self.identity.identifier,
                scope: self.identity.scope,
                action,
            }),
        }
    }
}

fn item_id(identity: &SourceIdentity) -> CleanableItemId {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    identity.hash(&mut hasher);
    CleanableItemId(hasher.finish())
}

fn native_adapter() -> Option<(&'static str, Box<dyn PackageAdapter>)> {
    if Path::new("/var/lib/dpkg/status").is_file() {
        Some(("dpkg", Box::new(DpkgAdapter)))
    } else if Path::new("/usr/lib/sysimage/rpm").exists() || Path::new("/var/lib/rpm").exists() {
        Some(("RPM", Box::new(RpmAdapter)))
    } else if Path::new("/var/lib/pacman/local").is_dir() {
        Some(("pacman", Box::new(PacmanAdapter)))
    } else {
        None
    }
}

#[derive(Default)]
pub struct InstalledAppsScanner;

impl InstalledAppsScanner {
    pub fn new() -> Self {
        Self
    }
}

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

        let home = std::env::var_os("HOME").map(PathBuf::from);
        let roots = desktop_roots(home.as_deref());
        let (desktops, desktop_failures, mut scanned_entries) = discover_desktop_entries(&roots);
        let mut warnings: Vec<ScanWarning> = desktop_failures
            .iter()
            .map(|(path, error)| ScanWarning {
                message: format!("{}: {error}", path.display()),
            })
            .collect();
        let mut skipped_roots: Vec<PathBuf> =
            desktop_failures.into_iter().map(|(path, _)| path).collect();
        if cancellation.is_cancelled() {
            return Err(ScanError::Cancelled);
        }

        let runner = ProcessRunner;
        let mut records = Vec::new();
        if let Some((label, adapter)) = native_adapter() {
            match adapter.collect(&desktops, &runner) {
                Ok(apps) => {
                    scanned_entries += apps.len() as u64;
                    records.extend(apps);
                }
                Err(AdapterError::Unavailable) => {
                    warnings.push(ScanWarning {
                        message: format!("{label} metadata command is unavailable."),
                    });
                    skipped_roots.push(PathBuf::from(label));
                }
                Err(AdapterError::Failed(error)) => {
                    warnings.push(ScanWarning {
                        message: format!("{label} metadata: {error}"),
                    });
                    skipped_roots.push(PathBuf::from(label));
                }
            }
        }

        for scope in [InstalledAppScope::User, InstalledAppScope::System] {
            match collect_flatpak(scope, &desktops, &runner) {
                Ok(apps) => {
                    scanned_entries += apps.len() as u64;
                    records.extend(apps);
                }
                Err(AdapterError::Unavailable) => break,
                Err(AdapterError::Failed(error)) => {
                    let label = match scope {
                        InstalledAppScope::User => "user Flatpak",
                        InstalledAppScope::System => "system Flatpak",
                    };
                    warnings.push(ScanWarning {
                        message: format!("{label} inventory: {error}"),
                    });
                    skipped_roots.push(PathBuf::from(label));
                }
            }
        }
        if cancellation.is_cancelled() {
            return Err(ScanError::Cancelled);
        }

        match collect_snap(&desktops, &runner) {
            Ok(apps) => {
                scanned_entries += apps.len() as u64;
                records.extend(apps);
            }
            Err(AdapterError::Unavailable) => {}
            Err(AdapterError::Failed(error)) => {
                warnings.push(ScanWarning {
                    message: format!("Snap inventory: {error}"),
                });
                skipped_roots.push(PathBuf::from("Snap"));
            }
        }

        if let Some(home) = home.as_deref() {
            let appimage_root = home.join("Applications");
            match discover_appimages(home, &appimage_root, &desktops) {
                Ok(apps) => {
                    scanned_entries += apps.len() as u64;
                    records.extend(apps);
                }
                Err(error) => {
                    warnings.push(ScanWarning {
                        message: format!("{}: {error}", appimage_root.display()),
                    });
                    skipped_roots.push(appimage_root);
                }
            }
        }

        let mut items: Vec<CleanableItem> = normalize_inventory(records)
            .into_iter()
            .map(InventoryApp::into_item)
            .collect();
        items.sort_by(|left, right| {
            left.display_name
                .to_ascii_lowercase()
                .cmp(&right.display_name.to_ascii_lowercase())
                .then_with(|| left.path.cmp(&right.path))
        });
        progress.report(ScanProgress {
            category: CleanerCategory::InstalledApps,
            phase: ScanPhase::Completed,
            current_path: None,
            scanned_entries,
            discovered_items: items.len() as u64,
            discovered_bytes: items.iter().map(|item| item.logical_size).sum(),
        });

        Ok(CategoryScanResult {
            category: CleanerCategory::InstalledApps,
            items,
            scanned_entries,
            estimated_reclaimable_bytes: 0,
            warnings,
            completeness: if skipped_roots.is_empty() {
                ScanCompleteness::Complete
            } else {
                ScanCompleteness::Partial {
                    skipped_roots,
                    reason: PartialScanReason::UnsupportedEnvironment,
                }
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::{PermissionsExt as _, symlink};

    use super::*;

    fn desktop(id: &str, name: &str, path: &str, scope: InstalledAppScope) -> DesktopEntry {
        DesktopEntry {
            id: id.to_string(),
            path: PathBuf::from(format!("/usr/share/applications/{id}.desktop")),
            scope,
            name: name.to_string(),
            exec_target: Some(PathBuf::from(path)),
            snap_instance: None,
            visible: true,
        }
    }

    fn app(
        source: Source,
        scope: InstalledAppScope,
        id: &str,
        name: &str,
        path: Option<&str>,
        action: Option<InstalledAppAction>,
    ) -> InventoryApp {
        InventoryApp {
            identity: SourceIdentity {
                source,
                scope,
                identifier: id.to_string(),
            },
            display_name: name.to_string(),
            version: None,
            publisher: None,
            location: path.map(PathBuf::from),
            logical_size: 0,
            action,
            shared_location: false,
        }
    }

    #[test]
    fn desktop_fixture_uses_the_unlocalized_name_and_never_runs_exec_text() {
        let input = r#"[Desktop Entry]
Type=Application
Name=Example\sEditor
Name[vi]=Trình sửa
Exec="/home/ada/Applications/Example Editor.AppImage" %U
X-SnapInstanceName=example-editor
"#;
        let entry = parse_desktop_entry(
            PathBuf::from("/home/ada/.local/share/applications/example.desktop"),
            InstalledAppScope::User,
            input,
        )
        .expect("desktop entry");
        assert_eq!(entry.name, "Example Editor");
        assert_eq!(
            entry.exec_target,
            Some(PathBuf::from(
                "/home/ada/Applications/Example Editor.AppImage"
            ))
        );
        assert_eq!(entry.snap_instance.as_deref(), Some("example-editor"));

        let shell = parse_desktop_entry(
            PathBuf::from("unsafe.desktop"),
            InstalledAppScope::User,
            "[Desktop Entry]\nName=Unsafe\nExec=sh -c '/tmp/App.AppImage'\n",
        )
        .expect("desktop entry");
        assert_eq!(shell.exec_target, None, "command text is not path evidence");
    }

    #[test]
    fn hidden_nodisplay_and_non_application_desktop_entries_are_excluded() {
        for flag in ["Hidden=true", "NoDisplay=true"] {
            let entry = parse_desktop_entry(
                PathBuf::from("hidden.desktop"),
                InstalledAppScope::System,
                &format!("[Desktop Entry]\nType=Application\nName=Hidden\n{flag}\n"),
            )
            .expect("valid but hidden entry");
            assert!(!entry.visible);
        }
        assert!(
            parse_desktop_entry(
                PathBuf::from("link.desktop"),
                InstalledAppScope::System,
                "[Desktop Entry]\nType=Link\nName=Link\n",
            )
            .is_none()
        );
    }

    #[test]
    fn dpkg_owner_and_metadata_fixtures_form_one_desktop_app() {
        let owners = parse_dpkg_owners(
            "firefox: /usr/share/applications/firefox.desktop\nshared-a, shared-b: /usr/share/applications/shared.desktop\n",
        );
        assert_eq!(
            owners.get(Path::new("/usr/share/applications/firefox.desktop")),
            Some(&"firefox".to_string())
        );
        assert!(!owners.contains_key(Path::new("/usr/share/applications/shared.desktop")));

        let metadata = parse_dpkg_metadata("firefox\t128.0+build1\tUbuntu Mozilla Team\t245760\n");
        let package = metadata.get("firefox").expect("package metadata");
        let entry = desktop(
            "firefox",
            "Firefox",
            "/usr/lib/firefox/firefox",
            InstalledAppScope::System,
        );
        let inventory = normalize_inventory(vec![native_app(Source::Dpkg, &entry, package)]);
        assert_eq!(inventory.len(), 1);
        assert_eq!(inventory[0].display_name, "Firefox");
        assert_eq!(inventory[0].logical_size, 245760 * 1024);
        assert!(inventory[0].action.is_none());
    }

    #[test]
    fn rpm_metadata_fixture_is_parsed_without_a_shell() {
        let metadata = parse_rpm_metadata("firefox\t128.0-2.fc40\tFedora Project\t251658240\n")
            .expect("rpm record");
        assert_eq!(metadata.identifier, "firefox");
        assert_eq!(metadata.version.as_deref(), Some("128.0-2.fc40"));
        assert_eq!(metadata.publisher.as_deref(), Some("Fedora Project"));
        assert_eq!(metadata.logical_size, 251658240);
    }

    #[test]
    fn pacman_owner_and_metadata_fixtures_are_parsed() {
        let owners = parse_pacman_owners(
            "/usr/share/applications/firefox.desktop is owned by firefox 128.0-1\n",
        );
        assert_eq!(
            owners.get(Path::new("/usr/share/applications/firefox.desktop")),
            Some(&"firefox".to_string())
        );
        let metadata = parse_pacman_metadata(
            "Name            : firefox\nVersion         : 128.0-1\nPackager        : Arch Linux\nInstalled Size  : 245.5 MiB\n\n",
        );
        let package = metadata.get("firefox").expect("pacman metadata");
        assert_eq!(package.version.as_deref(), Some("128.0-1"));
        assert_eq!(package.logical_size, (245.5 * 1024.0 * 1024.0) as u64);
    }

    #[test]
    fn flatpak_fixture_keeps_user_and_system_scopes_distinct() {
        let fixture = "org.example.Editor\tExample Editor\t2.4\t120.5 MB\n";
        let mut records = parse_flatpak_output(fixture, InstalledAppScope::User, &[]);
        records.extend(parse_flatpak_output(
            fixture,
            InstalledAppScope::System,
            &[],
        ));
        let inventory = normalize_inventory(records);
        assert_eq!(inventory.len(), 2);
        let user = inventory
            .iter()
            .find(|app| app.identity.scope == InstalledAppScope::User)
            .expect("user installation");
        let system = inventory
            .iter()
            .find(|app| app.identity.scope == InstalledAppScope::System)
            .expect("system installation");
        assert!(matches!(
            user.action,
            Some(InstalledAppAction::FlatpakUser { .. })
        ));
        assert!(system.action.is_none());
        assert_eq!(user.logical_size, 120_500_000);
    }

    #[test]
    fn malformed_flatpak_identifiers_never_become_actions() {
        for id in ["--delete-data", "onepart", "org.example/App", "org..App"] {
            assert!(
                parse_flatpak_output(
                    &format!("{id}\tBad\t1\t1 B\n"),
                    InstalledAppScope::User,
                    &[]
                )
                .is_empty()
            );
            assert!(flatpak_uninstall_command(id).is_none());
        }
    }

    #[test]
    fn snap_fixture_requires_a_matching_desktop_application() {
        let mut snap_desktop = desktop(
            "code_code",
            "Visual Studio Code",
            "/snap/bin/code",
            InstalledAppScope::System,
        );
        snap_desktop.snap_instance = Some("code".to_string());
        let fixture = "Name  Version  Rev  Tracking  Publisher  Notes\ncode  1.92.0  100  latest/stable  vscode**  classic\ncore22  1  1  latest/stable  canonical**  base\n";
        let records = parse_snap_output(fixture, &[snap_desktop]);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].identity.identifier, "code");
        assert!(records[0].action.is_none());
    }

    #[test]
    fn duplicate_identities_merge_but_names_alone_never_do() {
        let records = vec![
            app(
                Source::Flatpak,
                InstalledAppScope::User,
                "org.example.App",
                "Example",
                None,
                Some(InstalledAppAction::FlatpakUser {
                    application_id: "org.example.App".to_string(),
                }),
            ),
            app(
                Source::Flatpak,
                InstalledAppScope::User,
                "org.example.App",
                "Example launcher",
                None,
                None,
            ),
            app(
                Source::Snap,
                InstalledAppScope::System,
                "example",
                "Example",
                Some("/snap/bin/example"),
                None,
            ),
        ];
        let inventory = normalize_inventory(records);
        assert_eq!(inventory.len(), 2);
        assert!(
            inventory
                .iter()
                .any(|app| matches!(app.action, Some(InstalledAppAction::FlatpakUser { .. })))
        );
    }

    #[test]
    fn a_shared_location_vetoes_every_removal_action() {
        let records = vec![
            app(
                Source::AppImage,
                InstalledAppScope::User,
                "/home/ada/Applications/shared.AppImage",
                "Portable",
                Some("/home/ada/Applications/shared.AppImage"),
                Some(InstalledAppAction::AppImage),
            ),
            app(
                Source::Dpkg,
                InstalledAppScope::System,
                "managed-portable",
                "Managed",
                Some("/home/ada/Applications/shared.AppImage"),
                None,
            ),
        ];
        let items: Vec<_> = normalize_inventory(records)
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
        assert!(items.iter().all(|item| {
            let ItemMetadata::InstalledApp(metadata) = &item.metadata else {
                panic!("installed metadata")
            };
            metadata.action.is_none()
        }));
    }

    #[test]
    fn exact_flatpak_action_is_argument_separated_and_never_deletes_data() {
        let command = flatpak_uninstall_command("org.example.Editor").expect("valid action");
        assert_eq!(command.program, "flatpak");
        assert_eq!(
            command.args,
            [
                "--user",
                "uninstall",
                "--app",
                "--noninteractive",
                "org.example.Editor"
            ]
            .into_iter()
            .map(OsString::from)
            .collect::<Vec<_>>()
        );
        assert!(!command.args.iter().any(|arg| arg == "--delete-data"));
    }

    #[test]
    fn appimage_action_names_only_the_scanned_file() {
        let item = app(
            Source::AppImage,
            InstalledAppScope::User,
            "/home/ada/Applications/Editor.AppImage",
            "Editor",
            Some("/home/ada/Applications/Editor.AppImage"),
            Some(InstalledAppAction::AppImage),
        )
        .into_item();
        assert_eq!(
            item.path,
            Path::new("/home/ada/Applications/Editor.AppImage")
        );
        let ItemMetadata::InstalledApp(metadata) = item.metadata else {
            panic!("installed metadata")
        };
        assert_eq!(metadata.action, Some(InstalledAppAction::AppImage));
        assert!(
            item.capabilities
                .contains(&ItemCapability::UninstallApplication)
        );
        assert!(!item.capabilities.contains(&ItemCapability::MoveToTrash));
    }

    #[test]
    fn appimage_discovery_is_bounded_and_never_recurses() {
        let temp = std::env::temp_dir().join(format!(
            "dodo-linux-installed-apps-{}-{}",
            std::process::id(),
            line!()
        ));
        let applications = temp.join("Applications");
        let nested = applications.join("Nested");
        fs::create_dir_all(&nested).expect("creates fixture");
        let direct = applications.join("Direct.AppImage");
        let nested_app = nested.join("Nested.AppImage");
        let outside = temp.join("Outside.AppImage");
        for path in [&direct, &nested_app, &outside] {
            fs::write(path, b"appimage").expect("writes fixture");
            fs::set_permissions(path, fs::Permissions::from_mode(0o755)).expect("marks executable");
        }
        let desktop = DesktopEntry {
            id: "outside".to_string(),
            path: temp.join("outside.desktop"),
            scope: InstalledAppScope::User,
            name: "Outside Desktop".to_string(),
            exec_target: Some(outside.clone()),
            snap_instance: None,
            visible: true,
        };

        let records = discover_appimages(&temp, &applications, &[desktop]).expect("inventory");
        let direct = fs::canonicalize(direct).expect("canonical direct app");
        let outside = fs::canonicalize(outside).expect("canonical desktop app");
        let nested_app = fs::canonicalize(nested_app).expect("canonical nested app");
        assert_eq!(records.len(), 2);
        assert!(
            records
                .iter()
                .any(|record| record.location.as_deref() == Some(direct.as_path()))
        );
        assert!(
            records
                .iter()
                .any(|record| record.location.as_deref() == Some(outside.as_path()))
        );
        assert!(
            !records
                .iter()
                .any(|record| record.location.as_deref() == Some(nested_app.as_path()))
        );

        fs::remove_dir_all(&temp).expect("removes fixture");
    }

    #[test]
    fn an_applications_symlink_outside_home_is_not_inventory() {
        let temp = std::env::temp_dir().join(format!(
            "dodo-linux-appimage-root-{}-{}",
            std::process::id(),
            line!()
        ));
        let home = temp.join("home");
        let outside = temp.join("outside");
        fs::create_dir_all(&home).expect("creates home");
        fs::create_dir_all(&outside).expect("creates outside root");
        let app = outside.join("Outside.AppImage");
        fs::write(&app, b"appimage").expect("writes fixture");
        fs::set_permissions(&app, fs::Permissions::from_mode(0o755)).expect("marks executable");
        let applications = home.join("Applications");
        symlink(&outside, &applications).expect("links outside home");

        assert!(
            discover_appimages(&home, &applications, &[])
                .expect("safe empty inventory")
                .is_empty()
        );

        fs::remove_dir_all(&temp).expect("removes fixture");
    }

    #[test]
    fn package_list_commands_are_fixed_and_scope_specific() {
        assert_eq!(
            InstalledAppsScanner::new().category(),
            CleanerCategory::InstalledApps
        );
        assert_eq!(
            flatpak_list_command(InstalledAppScope::User).args,
            [
                "--user",
                "list",
                "--app",
                "--columns=application,name,version,size"
            ]
            .into_iter()
            .map(OsString::from)
            .collect::<Vec<_>>()
        );
        assert_eq!(
            flatpak_list_command(InstalledAppScope::System).args[0],
            OsString::from("--system")
        );
        assert_eq!(snap_list_command().args, vec![OsString::from("list")]);
    }
}
