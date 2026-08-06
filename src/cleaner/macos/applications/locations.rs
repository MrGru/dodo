//! Leftover-location candidates for the uninstall review workflow (Phase 9).
//!
//! [`find_leftovers`] is the only impure function here: it reads directory
//! listings (never file contents) under the fixed set of locations the
//! ticket names, and classifies each candidate entry against an
//! [`AppIdentity`] using the pure matchers in this module and in
//! [`super::confidence`]. Everything else — [`classify_name_match`],
//! [`classify_group_container`] — is pure and unit-tested directly.
//!
//! System-scope locations (`/Library/...`) are scanned for transparency (the
//! review dialog shows what was found) but are never turned into an
//! [`crate::cleaner::core::safety::AllowedRoot`] entry — see
//! `src/cleaner/macos/cleanup.rs`'s `policy_for` — so they can be surfaced
//! without becoming cleanable, matching the ticket's "system locations remain
//! scan-only until a secure helper exists".

use std::fs;
use std::path::{Path, PathBuf};

use super::confidence::NameMatchKind;
use super::identity::{AppIdentity, normalize_app_name, strip_helper_suffix};

/// Where a candidate directory entry was found.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LeftoverLocation {
    ApplicationSupport,
    Caches,
    Preferences,
    Containers,
    GroupContainers,
    Logs,
    SavedApplicationState,
    LaunchAgents,
    WebKit,
    HttpStorages,
    Cookies,
    Services,
    AutosaveInformation,
    SystemApplicationSupport,
    SystemCaches,
    SystemPreferences,
    SystemLaunchAgents,
    SystemLaunchDaemons,
    SystemPrivilegedHelperTools,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LocationScope {
    User,
    System,
}

/// One candidate leftover path, with the structural signal that matched it.
/// [`super::confidence::total_score`] still needs `matched_by_other_app` and
/// whether the scope is `System` (both applied by the caller, since
/// `find_leftovers` already has that context) to reach a final score.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LeftoverMatch {
    pub path: PathBuf,
    pub location: LeftoverLocation,
    pub scope: LocationScope,
    pub base_kind: NameMatchKind,
    pub matched_by_other_app: bool,
}

/// A data-driven, testable escape hatch for an app whose leftover naming
/// generic heuristics would miss or under-score. Empty by default: no app
/// needs one yet, but the mechanism exists and is exercised by
/// `known_app_rule_is_data_driven` below rather than by editing match logic.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct KnownAppRule {
    pub bundle_id: &'static str,
    pub extra_leftover_names: &'static [&'static str],
}

/// The registry `find_leftovers` uses in production. Add an entry here (not
/// new matching code) when a specific app's leftovers need it.
pub const KNOWN_APP_RULES: &[KnownAppRule] = &[];

/// `~/Library/<name>` subdirectories the ticket names as user-scope leftover
/// locations, in the shape [`user_scope_leftover_roots`] returns them.
pub const USER_LEFTOVER_SUBDIRS: &[&str] = &[
    "Application Support",
    "Caches",
    "Preferences",
    "Containers",
    "Group Containers",
    "Logs",
    "Saved Application State",
    "LaunchAgents",
    "WebKit",
    "HTTPStorages",
    "Cookies",
    "Services",
    "Autosave Information",
];

/// The absolute `~/Library/<name>` paths cleanup's allow-list can offer for
/// `CleanerCategory::InstalledApps`. Kept separate from `find_leftovers` so
/// the cleanup policy does not have to re-derive it from the scan logic.
pub fn user_scope_leftover_roots(home: &Path) -> Vec<PathBuf> {
    let library = home.join("Library");
    USER_LEFTOVER_SUBDIRS
        .iter()
        .map(|name| library.join(name))
        .collect()
}

/// Known vendor directories that are conventionally shared by several
/// products from the same vendor (e.g. a single top-level `Google` folder
/// under Application Support). A vendor-only match against one of these
/// scores as [`NameMatchKind::KnownSharedVendorDirectory`] instead of the
/// weaker but still-positive [`NameMatchKind::VendorOnlyMatch`].
const KNOWN_SHARED_VENDOR_DIRECTORIES: &[&str] =
    &["google", "microsoft", "adobe", "mozilla", "jetbrains"];

/// Finds every leftover candidate for `identity` across the fixed location
/// list, flags entries that also match another installed app, and returns
/// them unscored (call [`super::confidence::total_score`] with
/// `base_kind` and `matched_by_other_app`, and whether `scope` is `System`,
/// to get a final score).
pub fn find_leftovers(
    identity: &AppIdentity,
    home: Option<&Path>,
    other_apps: &[AppIdentity],
) -> Vec<LeftoverMatch> {
    find_leftovers_from(identity, home, other_apps, Path::new("/Library"))
}

/// Same as [`find_leftovers`], but with the system-scope `/Library` root
/// injected rather than hardcoded, so tests never touch the real one — the
/// ticket is explicit that scanner tests must not read real `/Library`.
/// `system_library` should be `/Library` itself in production and a
/// temporary directory laid out the same way in tests.
fn find_leftovers_from(
    identity: &AppIdentity,
    home: Option<&Path>,
    other_apps: &[AppIdentity],
    system_library: &Path,
) -> Vec<LeftoverMatch> {
    let mut matches = Vec::new();

    if let Some(home) = home {
        let library = home.join("Library");
        push_identifier_exact(
            &mut matches,
            library.join("Containers"),
            identity,
            LeftoverLocation::Containers,
            LocationScope::User,
            NameMatchKind::ExactSandboxContainer,
            IdentifierShape::Directory,
        );
        push_identifier_exact(
            &mut matches,
            library.join("Saved Application State"),
            identity,
            LeftoverLocation::SavedApplicationState,
            LocationScope::User,
            NameMatchKind::ExactSavedStateIdentifier,
            IdentifierShape::Suffixed(".savedState"),
        );
        push_identifier_exact(
            &mut matches,
            library.join("Preferences"),
            identity,
            LeftoverLocation::Preferences,
            LocationScope::User,
            NameMatchKind::ExactPreferenceIdentifier,
            IdentifierShape::Suffixed(".plist"),
        );
        push_identifier_exact(
            &mut matches,
            library.join("LaunchAgents"),
            identity,
            LeftoverLocation::LaunchAgents,
            LocationScope::User,
            NameMatchKind::ExactPreferenceIdentifier,
            IdentifierShape::Suffixed(".plist"),
        );
        push_group_container_scan(
            &mut matches,
            library.join("Group Containers"),
            identity,
            LocationScope::User,
        );

        for (name, location, upgrade) in [
            (
                "Application Support",
                LeftoverLocation::ApplicationSupport,
                Some(NameMatchKind::ExactApplicationSupportDirectory),
            ),
            ("Caches", LeftoverLocation::Caches, None),
            ("Logs", LeftoverLocation::Logs, None),
            ("WebKit", LeftoverLocation::WebKit, None),
            ("HTTPStorages", LeftoverLocation::HttpStorages, None),
            ("Cookies", LeftoverLocation::Cookies, None),
            ("Services", LeftoverLocation::Services, None),
            (
                "Autosave Information",
                LeftoverLocation::AutosaveInformation,
                None,
            ),
        ] {
            push_generic_scan(
                &mut matches,
                library.join(name),
                identity,
                location,
                LocationScope::User,
                upgrade,
            );
        }
    }

    for (name, location) in [
        (
            "Application Support",
            LeftoverLocation::SystemApplicationSupport,
        ),
        ("Caches", LeftoverLocation::SystemCaches),
        ("Preferences", LeftoverLocation::SystemPreferences),
        (
            "PrivilegedHelperTools",
            LeftoverLocation::SystemPrivilegedHelperTools,
        ),
    ] {
        push_generic_scan(
            &mut matches,
            system_library.join(name),
            identity,
            location,
            LocationScope::System,
            None,
        );
    }
    for (name, location) in [
        ("LaunchAgents", LeftoverLocation::SystemLaunchAgents),
        ("LaunchDaemons", LeftoverLocation::SystemLaunchDaemons),
    ] {
        push_identifier_exact(
            &mut matches,
            system_library.join(name),
            identity,
            location,
            LocationScope::System,
            NameMatchKind::ExactPreferenceIdentifier,
            IdentifierShape::Suffixed(".plist"),
        );
    }

    for candidate in &mut matches {
        candidate.matched_by_other_app =
            is_ambiguous_with_other_apps(candidate.path.as_path(), identity, other_apps);
    }

    matches
}

#[derive(Clone, Copy)]
enum IdentifierShape {
    Directory,
    Suffixed(&'static str),
}

fn candidate_identifiers(identity: &AppIdentity) -> Vec<&str> {
    let mut ids = Vec::new();
    if let Some(id) = identity.bundle_id.as_deref() {
        ids.push(id);
    }
    if let Some(id) = identity.bundle_id_without_helper_suffix.as_deref()
        && !ids.contains(&id)
    {
        ids.push(id);
    }
    ids
}

fn push_identifier_exact(
    matches: &mut Vec<LeftoverMatch>,
    root: PathBuf,
    identity: &AppIdentity,
    location: LeftoverLocation,
    scope: LocationScope,
    kind: NameMatchKind,
    shape: IdentifierShape,
) {
    if !root.is_dir() {
        return;
    }
    for id in candidate_identifiers(identity) {
        let name = match shape {
            IdentifierShape::Directory => id.to_string(),
            IdentifierShape::Suffixed(suffix) => format!("{id}{suffix}"),
        };
        let candidate = root.join(&name);
        if fs::symlink_metadata(&candidate).is_ok() {
            matches.push(LeftoverMatch {
                path: candidate,
                location,
                scope,
                base_kind: kind,
                matched_by_other_app: false,
            });
        }
    }
}

fn push_generic_scan(
    matches: &mut Vec<LeftoverMatch>,
    root: PathBuf,
    identity: &AppIdentity,
    location: LeftoverLocation,
    scope: LocationScope,
    upgrade_exact_name: Option<NameMatchKind>,
) {
    let Ok(entries) = fs::read_dir(&root) else {
        return;
    };
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let Some(name) = file_name.to_str() else {
            continue;
        };
        let Some(kind) = classify_name_match(name, identity, KNOWN_APP_RULES) else {
            continue;
        };
        let kind = match (kind, upgrade_exact_name) {
            (NameMatchKind::ExactNormalizedAppName, Some(upgrade)) => upgrade,
            _ => kind,
        };
        matches.push(LeftoverMatch {
            path: entry.path(),
            location,
            scope,
            base_kind: kind,
            matched_by_other_app: false,
        });
    }
}

fn push_group_container_scan(
    matches: &mut Vec<LeftoverMatch>,
    root: PathBuf,
    identity: &AppIdentity,
    scope: LocationScope,
) {
    let Ok(entries) = fs::read_dir(&root) else {
        return;
    };
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let Some(name) = file_name.to_str() else {
            continue;
        };
        let Some(kind) = classify_group_container(name, identity) else {
            continue;
        };
        matches.push(LeftoverMatch {
            path: entry.path(),
            location: LeftoverLocation::GroupContainers,
            scope,
            base_kind: kind,
            matched_by_other_app: false,
        });
    }
}

fn strip_known_extension(name: &str) -> &str {
    for ext in [".plist", ".savedState"] {
        if let Some(stripped) = name.strip_suffix(ext) {
            return stripped;
        }
    }
    name
}

/// Classifies a single directory-entry name found under a generically
/// scanned leftover root (Application Support, Caches, Logs, WebKit,
/// HTTPStorages, Cookies, Services, Autosave Information, and the
/// system-scope roots) against `identity`.
///
/// `known_rules` lets a specific app override generic name matching; pass
/// [`KNOWN_APP_RULES`] in production.
pub fn classify_name_match(
    entry_name: &str,
    identity: &AppIdentity,
    known_rules: &[KnownAppRule],
) -> Option<NameMatchKind> {
    if let Some(bundle_id) = identity.bundle_id.as_deref()
        && let Some(rule) = known_rules.iter().find(|rule| rule.bundle_id == bundle_id)
        && rule
            .extra_leftover_names
            .iter()
            .any(|name| name.eq_ignore_ascii_case(entry_name))
    {
        return Some(NameMatchKind::KnownAppSpecificRule);
    }

    let stem = strip_known_extension(entry_name);
    if let Some(bundle_id) = identity.bundle_id.as_deref() {
        let stripped = strip_helper_suffix(stem);
        let candidate = stripped.as_deref().unwrap_or(stem);
        if stem.eq_ignore_ascii_case(bundle_id) || candidate.eq_ignore_ascii_case(bundle_id) {
            return Some(NameMatchKind::ExactBundleIdentifier);
        }
    }

    let normalized_entry = normalize_app_name(stem);
    if normalized_entry.is_empty() {
        return None;
    }
    if !identity.normalized_name.is_empty() && normalized_entry == identity.normalized_name {
        return Some(NameMatchKind::ExactNormalizedAppName);
    }

    let vendor = identity.vendor.as_deref();
    let contains_name = !identity.normalized_name.is_empty()
        && normalized_entry.contains(identity.normalized_name.as_str());
    let contains_vendor = vendor.is_some_and(|vendor| normalized_entry.contains(vendor));

    match (contains_vendor, contains_name) {
        (true, true) => Some(NameMatchKind::VendorAndAppNameCombination),
        (false, true) => Some(NameMatchKind::PartialAppNameMatch),
        (true, false) => {
            let vendor = vendor.unwrap_or_default();
            if normalized_entry == vendor {
                if KNOWN_SHARED_VENDOR_DIRECTORIES.contains(&vendor) {
                    Some(NameMatchKind::KnownSharedVendorDirectory)
                } else {
                    Some(NameMatchKind::VendorOnlyMatch)
                }
            } else {
                Some(NameMatchKind::PartialAppNameMatch)
            }
        }
        (false, false) => None,
    }
}

/// Classifies a `~/Library/Group Containers` entry name.
///
/// Group container identifiers are developer-chosen (`<team>.<group>`) and
/// not required to relate to any single app's bundle id, so this needs both
/// an app-specific signal (the final bundle component, or the normalized
/// name) *and* an ownership signal (the team id prefix, or the vendor) to
/// call it [`NameMatchKind::ExactGroupContainerEntitlement`]. A directory
/// that only carries the ownership signal — a team- or vendor-wide group with
/// no app-specific text — is exactly the "shared across a vendor's apps"
/// case the ticket's `SharedContainer` penalty exists for.
pub fn classify_group_container(entry_name: &str, identity: &AppIdentity) -> Option<NameMatchKind> {
    let lower = entry_name.to_ascii_lowercase();
    let normalized = normalize_app_name(&lower);
    let has_component = identity
        .final_bundle_component
        .as_deref()
        .is_some_and(|component| lower.contains(component))
        || (!identity.normalized_name.is_empty() && normalized.contains(&identity.normalized_name));
    let has_vendor = identity
        .vendor
        .as_deref()
        .is_some_and(|vendor| lower.contains(vendor));
    let has_team_prefix = identity
        .team_id
        .as_deref()
        .is_some_and(|team_id| lower.starts_with(&team_id.to_ascii_lowercase()));

    if has_component && (has_team_prefix || has_vendor) {
        Some(NameMatchKind::ExactGroupContainerEntitlement)
    } else if has_vendor || has_team_prefix {
        Some(NameMatchKind::SharedContainer)
    } else {
        None
    }
}

/// The human-readable label for a leftover location, used by both the
/// uninstall review workflow ([`super::review`]) and orphan detection
/// ([`super::orphans`]) so the two features describe the same locations the
/// same way.
pub fn location_label(location: LeftoverLocation) -> &'static str {
    match location {
        LeftoverLocation::ApplicationSupport => "Application Support",
        LeftoverLocation::Caches => "Caches",
        LeftoverLocation::Preferences => "Preferences",
        LeftoverLocation::Containers => "Containers",
        LeftoverLocation::GroupContainers => "Group Containers",
        LeftoverLocation::Logs => "Logs",
        LeftoverLocation::SavedApplicationState => "Saved Application State",
        LeftoverLocation::LaunchAgents => "LaunchAgents",
        LeftoverLocation::WebKit => "WebKit",
        LeftoverLocation::HttpStorages => "HTTPStorages",
        LeftoverLocation::Cookies => "Cookies",
        LeftoverLocation::Services => "Services",
        LeftoverLocation::AutosaveInformation => "Autosave Information",
        LeftoverLocation::SystemApplicationSupport => "/Library/Application Support",
        LeftoverLocation::SystemCaches => "/Library/Caches",
        LeftoverLocation::SystemPreferences => "/Library/Preferences",
        LeftoverLocation::SystemLaunchAgents => "/Library/LaunchAgents",
        LeftoverLocation::SystemLaunchDaemons => "/Library/LaunchDaemons",
        LeftoverLocation::SystemPrivilegedHelperTools => "/Library/PrivilegedHelperTools",
    }
}

fn is_ambiguous_with_other_apps(
    path: &Path,
    identity: &AppIdentity,
    other_apps: &[AppIdentity],
) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    other_apps.iter().any(|other| {
        other.bundle_id != identity.bundle_id && classify_name_match(name, other, &[]).is_some()
    })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn temp_home(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "dodo-cleaner-leftovers-{label}-{}-{}",
            std::process::id(),
            label.len()
        ))
    }

    fn notes_identity() -> AppIdentity {
        AppIdentity::new(Some("com.smallwidgets.Notes"), "Notes", None)
    }

    #[test]
    fn exact_bundle_id_sandbox_container_is_found() {
        let home = temp_home("exact-bundle-id");
        let containers = home
            .join("Library")
            .join("Containers")
            .join("com.smallwidgets.Notes");
        fs::create_dir_all(&containers).expect("creates sandbox container");

        let identity = notes_identity();
        let matches = find_leftovers(&identity, Some(home.as_path()), &[]);

        assert!(matches.iter().any(|candidate| {
            candidate.location == LeftoverLocation::Containers
                && candidate.base_kind == NameMatchKind::ExactSandboxContainer
        }));

        fs::remove_dir_all(&home).expect("removes temp home");
    }

    #[test]
    fn helper_bundle_id_leftover_still_resolves_to_the_main_app() {
        let identity = notes_identity();
        let kind = classify_name_match("com.smallwidgets.Notes.helper", &identity, &[]);
        assert_eq!(kind, Some(NameMatchKind::ExactBundleIdentifier));
    }

    #[test]
    fn team_id_presence_enables_a_team_scoped_group_container_match() {
        let without_team = AppIdentity::new(Some("com.smallwidgets.Notes"), "Notes", None);
        let with_team = AppIdentity::new(
            Some("com.smallwidgets.Notes"),
            "Notes",
            Some("ABCDE12345".into()),
        );

        // Named after the team only, plus the app's own final component — no
        // vendor text at all, so only the team-id signal can explain it.
        let entry = "ABCDE12345.notes";
        assert_eq!(classify_group_container(entry, &without_team), None);
        assert_eq!(
            classify_group_container(entry, &with_team),
            Some(NameMatchKind::ExactGroupContainerEntitlement)
        );
    }

    #[test]
    fn group_container_is_found_end_to_end() {
        let home = temp_home("group-container");
        let group = home
            .join("Library")
            .join("Group Containers")
            .join("ABCDE12345.smallwidgets.notes");
        fs::create_dir_all(&group).expect("creates group container");

        let identity = AppIdentity::new(
            Some("com.smallwidgets.Notes"),
            "Notes",
            Some("ABCDE12345".into()),
        );
        let matches = find_leftovers(&identity, Some(home.as_path()), &[]);

        assert!(matches.iter().any(|candidate| {
            candidate.location == LeftoverLocation::GroupContainers
                && candidate.base_kind == NameMatchKind::ExactGroupContainerEntitlement
        }));

        fs::remove_dir_all(&home).expect("removes temp home");
    }

    #[test]
    fn vendor_only_group_container_is_shared_not_confirmed() {
        let identity = AppIdentity::new(
            Some("com.smallwidgets.Notes"),
            "Notes",
            Some("ABCDE12345".into()),
        );
        let kind = classify_group_container("ABCDE12345.smallwidgets.shared", &identity);
        assert_eq!(kind, Some(NameMatchKind::SharedContainer));
    }

    #[test]
    fn similar_name_is_a_partial_match_not_exact() {
        let identity = notes_identity();
        let kind = classify_name_match("Notes Pro", &identity, &[]);
        assert_eq!(kind, Some(NameMatchKind::PartialAppNameMatch));
    }

    #[test]
    fn vendor_only_directory_is_a_weak_positive_match() {
        let identity = notes_identity();
        let kind = classify_name_match("SmallWidgets", &identity, &[]);
        assert_eq!(kind, Some(NameMatchKind::VendorOnlyMatch));
    }

    #[test]
    fn known_shared_vendor_directory_outranks_plain_vendor_only() {
        let identity = AppIdentity::new(Some("com.google.Chrome"), "Chrome", None);
        let kind = classify_name_match("Google", &identity, &[]);
        assert_eq!(kind, Some(NameMatchKind::KnownSharedVendorDirectory));
    }

    #[test]
    fn versioned_display_name_still_matches_an_unversioned_leftover_directory() {
        let identity = AppIdentity::new(Some("com.smallwidgets.Notes"), "Notes 2", None);
        let kind = classify_name_match("Notes", &identity, &[]);
        assert_eq!(kind, Some(NameMatchKind::ExactNormalizedAppName));
    }

    #[test]
    fn application_support_upgrades_exact_name_match() {
        let home = temp_home("app-support");
        let app_support = home
            .join("Library")
            .join("Application Support")
            .join("Notes");
        fs::create_dir_all(&app_support).expect("creates app support dir");

        let identity = notes_identity();
        let matches = find_leftovers(&identity, Some(home.as_path()), &[]);

        assert!(matches.iter().any(|candidate| {
            candidate.location == LeftoverLocation::ApplicationSupport
                && candidate.base_kind == NameMatchKind::ExactApplicationSupportDirectory
        }));

        fs::remove_dir_all(&home).expect("removes temp home");
    }

    #[test]
    fn known_app_rule_is_data_driven() {
        let identity = notes_identity();
        // A name generic matching cannot explain at all (no vendor, no app
        // name substring) — only the data-driven rule should recognize it.
        let rules = [KnownAppRule {
            bundle_id: "com.smallwidgets.Notes",
            extra_leftover_names: &["legacy-storage-blob"],
        }];

        assert_eq!(
            classify_name_match("legacy-storage-blob", &identity, &rules),
            Some(NameMatchKind::KnownAppSpecificRule)
        );
        assert_eq!(
            classify_name_match("legacy-storage-blob", &identity, &[]),
            None
        );
    }

    #[test]
    fn ambiguous_match_with_another_installed_app_is_flagged() {
        let home = temp_home("ambiguous");
        let app_support = home
            .join("Library")
            .join("Application Support")
            .join("Notes");
        fs::create_dir_all(&app_support).expect("creates app support dir");

        let identity = notes_identity();
        let other = AppIdentity::new(Some("com.otherco.Notes"), "Notes", None);
        let matches = find_leftovers(&identity, Some(home.as_path()), &[other]);

        let candidate = matches
            .iter()
            .find(|candidate| candidate.location == LeftoverLocation::ApplicationSupport)
            .expect("finds the Application Support match");
        assert!(candidate.matched_by_other_app);

        fs::remove_dir_all(&home).expect("removes temp home");
    }

    #[test]
    fn system_scope_locations_are_found_and_tagged_without_touching_real_library() {
        // `find_leftovers` (the production entry point) reads the real
        // `/Library`; tests must not depend on that, so this drives the
        // injectable `find_leftovers_from` with a fake system root instead —
        // never the real `/Library`, `~/Library`, `/Applications` or
        // `/System` the ticket forbids testing against.
        let home = temp_home("system-scope-home");
        let system_library = temp_home("system-scope-library");
        let launch_daemon_dir = system_library.join("LaunchDaemons");
        fs::create_dir_all(&launch_daemon_dir).expect("creates fake system LaunchDaemons");
        fs::write(launch_daemon_dir.join("com.smallwidgets.Notes.plist"), b"")
            .expect("writes fake launch daemon plist");

        let identity = notes_identity();
        let matches = find_leftovers_from(
            &identity,
            Some(home.as_path()),
            &[],
            system_library.as_path(),
        );

        let candidate = matches
            .iter()
            .find(|candidate| candidate.location == LeftoverLocation::SystemLaunchDaemons)
            .expect("finds the system-scope LaunchDaemons match");
        assert_eq!(candidate.scope, LocationScope::System);
        assert_eq!(
            candidate.base_kind,
            NameMatchKind::ExactPreferenceIdentifier
        );

        fs::remove_dir_all(&system_library).expect("removes fake system library");
    }
}
