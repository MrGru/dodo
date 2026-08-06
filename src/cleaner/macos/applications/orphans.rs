//! Reverse ownership matching: leftovers under the fixed location list that
//! no *currently installed* app can explain (Phase 10).
//!
//! [`super::locations::find_leftovers`] answers "what does *this* app own?"
//! for one [`AppIdentity`] at a time. [`find_orphans`] asks the opposite
//! question for the whole installed-app index at once: walking the same
//! fixed location list, is there *any* installed app whose identity explains
//! this entry? An entry no installed app explains becomes an
//! [`OrphanCandidate`], tagged with the [`OrphanReason`] appropriate to where
//! it was found and scored through the same [`super::confidence`] pipeline
//! Phase 9 uses — reusing the scheme (`NameMatchKind` base points, the
//! protected-system-path penalty, the five-value [`MatchConfidence`] buckets)
//! rather than inventing a second one, because the ticket's bar ("never call
//! uncertain leftovers safe by default") is the same bar in both directions.
//!
//! # Ownership check
//!
//! Every location already has an ownership predicate in [`super::locations`]:
//! [`locations::classify_name_match`] for every identifier-suffixed and
//! generically-scanned location, [`locations::classify_group_container`] for
//! Group Containers. An entry is "owned" when that predicate returns
//! `Some(_)` for *any* identity in the index — including a weak vendor-only
//! match, since a vendor folder a still-installed app of that vendor
//! plausibly created is not an orphan even though it is not a confident match
//! either. Only an entry no identity explains at all becomes an
//! [`OrphanCandidate`].
//!
//! There is no "ambiguous with another installed app" penalty here the way
//! [`super::review`] has one: that penalty downgrades a match *for one known
//! app* when a second app could also explain it. An orphan is, by definition,
//! explained by *no* app, so the situation the penalty exists for cannot
//! arise. The protected-system-path penalty still applies.
//!
//! # Confidence, by location shape
//!
//! Every location keeps the same base [`NameMatchKind`] Phase 9 assigned it
//! when checking a *known* identity's ownership, since that value already
//! reflects how strongly the location's naming convention pins an entry to a
//! single owner:
//!
//! | Location | [`OrphanReason`] | Base kind |
//! |---|---|---|
//! | Containers | `BundleIdentifierNotInstalled` | `ExactSandboxContainer` (100) |
//! | Saved Application State | `StaleSavedState` | `ExactSavedStateIdentifier` (85) |
//! | Preferences | `StalePreference` | `ExactPreferenceIdentifier` (80) |
//! | LaunchAgents / LaunchDaemons | `MissingOwnerApplication` | `ExactPreferenceIdentifier` (80) |
//! | Group Containers | `UnknownContainerOwner` | `SharedContainer` (-80), always |
//! | every generically-named location | `AppNameNotInstalled` | `PartialAppNameMatch` (20) |
//!
//! The generic-location row is deliberately the most conservative kind on the
//! table (other than the always-negative Group Containers row): unlike Phase
//! 9's per-app matching, there is no *matched* signal here to grade the
//! strength of — every generic-location entry that reaches this row failed to
//! match *any* installed app's name or vendor at all, and a name that
//! resembles nothing installed is still only weak evidence that whatever
//! created it is gone rather than simply not name-shaped. Group Containers is
//! always scored as the negative `SharedContainer` kind regardless of shape,
//! because attributing an unclaimed group container to one specific missing
//! app is never reliable — see "Detect shared containers" in the ticket.
//!
//! # Apple's own namespace is never flagged
//!
//! System-scope roots (`/Library/...`) hold hundreds of `com.apple.*`
//! daemons, caches and preference files that belong to macOS itself, not to
//! any `.app` bundle the installed-app index would ever see (the index comes
//! from `/Applications`, `~/Applications` and `/System/Applications`). dodo
//! has no way to tell a leftover Apple daemon from a live one, so any entry
//! whose name starts with `com.apple.` (case-insensitively) is skipped before
//! it is ever considered, in every scope. Flooding the review list with
//! hundreds of low-value, unconfirmable "orphans" would be worse than staying
//! silent about them; see `docs/cleaner/known-limitations.md`.
//!
//! # What is *not* here
//!
//! CLI tools with no `.app` bundle (Homebrew formulae, language toolchains,
//! anything a package manager installed into `/usr/local`, `/opt/homebrew` or
//! a dotfile) are not detected at all. The ticket hedges this requirement
//! with "where possible": there is no bundle identifier, no `Info.plist`, and
//! none of the fixed leftover-location conventions this module reverse-
//! matches against apply to them, so a detector here would have to invent
//! heuristics the ticket does not specify. Left as a documented gap rather
//! than a guess — see `docs/cleaner/known-limitations.md`.

use std::fs;
use std::path::{Path, PathBuf};

use crate::cleaner::core::item::OrphanReason;

use super::confidence::{self, MatchConfidence, NameMatchKind};
use super::identity::AppIdentity;
use super::locations::{self, KNOWN_APP_RULES, LeftoverLocation, LocationScope};

/// One leftover entry no installed app's identity explains.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct OrphanCandidate {
    pub path: PathBuf,
    pub location: LeftoverLocation,
    pub scope: LocationScope,
    pub reason: OrphanReason,
    pub confidence: MatchConfidence,
}

/// Finds every orphan candidate across the fixed location list, given the
/// whole installed-app index (see
/// `crate::cleaner::macos::scanners::installed_apps::installed_app_identities`).
pub fn find_orphans(index: &[AppIdentity], home: Option<&Path>) -> Vec<OrphanCandidate> {
    find_orphans_from(index, home, Path::new("/Library"))
}

/// Same as [`find_orphans`], but with the system-scope `/Library` root
/// injected rather than hardcoded, so tests never touch the real one — the
/// ticket is explicit that scanner tests must not read real `/Library`.
///
/// `pub(crate)` rather than private: `crate::cleaner::macos::scanners::orphaned_files`'s
/// own tests need the same injection for the same reason.
pub(crate) fn find_orphans_from(
    index: &[AppIdentity],
    home: Option<&Path>,
    system_library: &Path,
) -> Vec<OrphanCandidate> {
    let mut candidates = Vec::new();

    if let Some(home) = home {
        let library = home.join("Library");
        scan_location(
            &mut candidates,
            library.join("Containers"),
            index,
            LeftoverLocation::Containers,
            LocationScope::User,
            OrphanReason::BundleIdentifierNotInstalled,
            NameMatchKind::ExactSandboxContainer,
        );
        scan_location(
            &mut candidates,
            library.join("Saved Application State"),
            index,
            LeftoverLocation::SavedApplicationState,
            LocationScope::User,
            OrphanReason::StaleSavedState,
            NameMatchKind::ExactSavedStateIdentifier,
        );
        scan_location(
            &mut candidates,
            library.join("Preferences"),
            index,
            LeftoverLocation::Preferences,
            LocationScope::User,
            OrphanReason::StalePreference,
            NameMatchKind::ExactPreferenceIdentifier,
        );
        scan_location(
            &mut candidates,
            library.join("LaunchAgents"),
            index,
            LeftoverLocation::LaunchAgents,
            LocationScope::User,
            OrphanReason::MissingOwnerApplication,
            NameMatchKind::ExactPreferenceIdentifier,
        );
        scan_group_containers(
            &mut candidates,
            library.join("Group Containers"),
            index,
            LocationScope::User,
        );

        for (name, location) in [
            ("Application Support", LeftoverLocation::ApplicationSupport),
            ("Caches", LeftoverLocation::Caches),
            ("Logs", LeftoverLocation::Logs),
            ("WebKit", LeftoverLocation::WebKit),
            ("HTTPStorages", LeftoverLocation::HttpStorages),
            ("Cookies", LeftoverLocation::Cookies),
            ("Services", LeftoverLocation::Services),
            (
                "Autosave Information",
                LeftoverLocation::AutosaveInformation,
            ),
        ] {
            scan_location(
                &mut candidates,
                library.join(name),
                index,
                location,
                LocationScope::User,
                OrphanReason::AppNameNotInstalled,
                NameMatchKind::PartialAppNameMatch,
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
        scan_location(
            &mut candidates,
            system_library.join(name),
            index,
            location,
            LocationScope::System,
            OrphanReason::AppNameNotInstalled,
            NameMatchKind::PartialAppNameMatch,
        );
    }
    for (name, location) in [
        ("LaunchAgents", LeftoverLocation::SystemLaunchAgents),
        ("LaunchDaemons", LeftoverLocation::SystemLaunchDaemons),
    ] {
        scan_location(
            &mut candidates,
            system_library.join(name),
            index,
            location,
            LocationScope::System,
            OrphanReason::MissingOwnerApplication,
            NameMatchKind::ExactPreferenceIdentifier,
        );
    }

    candidates
}

/// Scans one directory-listed location, producing an [`OrphanCandidate`] for
/// every entry no identity in `index` claims via
/// [`locations::classify_name_match`].
fn scan_location(
    candidates: &mut Vec<OrphanCandidate>,
    root: PathBuf,
    index: &[AppIdentity],
    location: LeftoverLocation,
    scope: LocationScope,
    reason: OrphanReason,
    base_kind: NameMatchKind,
) {
    let Ok(entries) = fs::read_dir(&root) else {
        return;
    };
    let is_system = scope == LocationScope::System;
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let Some(name) = file_name.to_str() else {
            continue;
        };
        if is_apple_namespaced(name) {
            continue;
        }
        let owned = index.iter().any(|identity| {
            locations::classify_name_match(name, identity, KNOWN_APP_RULES).is_some()
        });
        if owned {
            continue;
        }
        let score = confidence::total_score(base_kind, false, is_system);
        candidates.push(OrphanCandidate {
            path: entry.path(),
            location,
            scope,
            reason,
            confidence: confidence::classify(score),
        });
    }
}

/// Scans `~/Library/Group Containers`, producing an [`OrphanCandidate`] for
/// every entry no identity in `index` claims via
/// [`locations::classify_group_container`]. Always scored via the negative
/// [`NameMatchKind::SharedContainer`] base — see the module doc.
fn scan_group_containers(
    candidates: &mut Vec<OrphanCandidate>,
    root: PathBuf,
    index: &[AppIdentity],
    scope: LocationScope,
) {
    let Ok(entries) = fs::read_dir(&root) else {
        return;
    };
    let is_system = scope == LocationScope::System;
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let Some(name) = file_name.to_str() else {
            continue;
        };
        if is_apple_namespaced(name) {
            continue;
        }
        let owned = index
            .iter()
            .any(|identity| locations::classify_group_container(name, identity).is_some());
        if owned {
            continue;
        }
        let score = confidence::total_score(NameMatchKind::SharedContainer, false, is_system);
        candidates.push(OrphanCandidate {
            path: entry.path(),
            location: LeftoverLocation::GroupContainers,
            scope,
            reason: OrphanReason::UnknownContainerOwner,
            confidence: confidence::classify(score),
        });
    }
}

/// Whether `entry_name` sits in Apple's own reverse-DNS namespace. See the
/// module doc's "Apple's own namespace is never flagged" section. A plain
/// prefix check on the raw name is enough — bundle ids in this namespace look
/// like `com.apple.something`, and a `.plist`/`.savedState` suffix (if any)
/// comes after that prefix, not before it.
fn is_apple_namespaced(entry_name: &str) -> bool {
    entry_name.to_ascii_lowercase().starts_with("com.apple.")
}

/// A short, stable label for an [`OrphanReason`] — used in
/// `macos::scanners::orphaned_files` to build each candidate's
/// `CleanableItem::explanation`. Plain English, like every other domain-level
/// explanation this deep in Cleaner (see `super::review::signal_label`): this
/// module has no GPUI dependency and does not localize through `Str`.
pub fn reason_label(reason: OrphanReason) -> &'static str {
    match reason {
        OrphanReason::BundleIdentifierNotInstalled => "no installed app has this bundle identifier",
        OrphanReason::AppNameNotInstalled => "no installed app's name or vendor matches this",
        OrphanReason::MissingOwnerApplication => {
            "no installed app owns this launch agent or daemon"
        }
        OrphanReason::StaleSavedState => "no installed app has this saved-state identifier",
        OrphanReason::StalePreference => "no installed app has this preference identifier",
        OrphanReason::UnknownContainerOwner => "no installed app's identity claims this container",
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn temp_home(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "dodo-cleaner-orphans-{label}-{}-{}",
            std::process::id(),
            label.len()
        ))
    }

    fn notes_identity() -> AppIdentity {
        AppIdentity::new(Some("com.smallwidgets.Notes"), "Notes", None)
    }

    /// A system-scope root that is guaranteed not to be the real `/Library`,
    /// for every test below that only cares about user-scope behavior. It is
    /// never created, so every system-scope location under it is simply
    /// missing — exactly like a `find_orphans_from` call against a real but
    /// otherwise-empty `/Library` would see, without ever touching the real
    /// one. The ticket is explicit that scanner tests must not read real
    /// `/Library`.
    fn no_system_library() -> PathBuf {
        temp_home("unused-system-library")
    }

    #[test]
    fn a_container_matching_an_installed_app_is_not_an_orphan() {
        let home = temp_home("owned-container");
        let containers = home
            .join("Library")
            .join("Containers")
            .join("com.smallwidgets.Notes");
        fs::create_dir_all(&containers).expect("creates sandbox container");

        let index = vec![notes_identity()];
        let orphans =
            find_orphans_from(&index, Some(home.as_path()), no_system_library().as_path());

        assert!(
            orphans.is_empty(),
            "an installed app's own container must not be flagged orphaned"
        );

        fs::remove_dir_all(&home).expect("removes temp home");
    }

    #[test]
    fn a_container_matching_no_installed_app_is_an_orphan() {
        let home = temp_home("orphan-container");
        let containers = home
            .join("Library")
            .join("Containers")
            .join("com.gonecorp.OldApp");
        fs::create_dir_all(&containers).expect("creates sandbox container");

        // The index has some *other* installed app, not the one that owns
        // this container.
        let index = vec![notes_identity()];
        let orphans =
            find_orphans_from(&index, Some(home.as_path()), no_system_library().as_path());

        let candidate = orphans
            .iter()
            .find(|candidate| candidate.location == LeftoverLocation::Containers)
            .expect("finds the orphaned container");
        assert_eq!(candidate.reason, OrphanReason::BundleIdentifierNotInstalled);
        assert_eq!(candidate.confidence, MatchConfidence::Confirmed);

        fs::remove_dir_all(&home).expect("removes temp home");
    }

    #[test]
    fn an_empty_installed_app_index_still_flags_every_identifier_location() {
        let home = temp_home("empty-index");
        let saved_state = home
            .join("Library")
            .join("Saved Application State")
            .join("com.gonecorp.OldApp.savedState");
        fs::create_dir_all(&saved_state).expect("creates saved state dir");

        let orphans = find_orphans_from(&[], Some(home.as_path()), no_system_library().as_path());

        let candidate = orphans
            .iter()
            .find(|candidate| candidate.location == LeftoverLocation::SavedApplicationState)
            .expect("finds the orphaned saved state");
        assert_eq!(candidate.reason, OrphanReason::StaleSavedState);
        assert_eq!(candidate.confidence, MatchConfidence::High);

        fs::remove_dir_all(&home).expect("removes temp home");
    }

    #[test]
    fn a_launch_agent_naming_an_uninstalled_app_is_missing_its_owner() {
        let home = temp_home("launch-agent");
        let launch_agents = home.join("Library").join("LaunchAgents");
        fs::create_dir_all(&launch_agents).expect("creates LaunchAgents dir");
        fs::write(launch_agents.join("com.gonecorp.helper.plist"), b"")
            .expect("writes launch agent plist");

        let orphans = find_orphans_from(&[], Some(home.as_path()), no_system_library().as_path());

        let candidate = orphans
            .iter()
            .find(|candidate| candidate.location == LeftoverLocation::LaunchAgents)
            .expect("finds the orphaned launch agent");
        assert_eq!(candidate.reason, OrphanReason::MissingOwnerApplication);

        fs::remove_dir_all(&home).expect("removes temp home");
    }

    #[test]
    fn a_generic_name_matching_no_installed_app_is_a_low_confidence_orphan() {
        let home = temp_home("generic-name");
        let app_support = home
            .join("Library")
            .join("Application Support")
            .join("SomeOldTool");
        fs::create_dir_all(&app_support).expect("creates app support dir");

        let index = vec![notes_identity()];
        let orphans =
            find_orphans_from(&index, Some(home.as_path()), no_system_library().as_path());

        let candidate = orphans
            .iter()
            .find(|candidate| candidate.location == LeftoverLocation::ApplicationSupport)
            .expect("finds the orphaned Application Support entry");
        assert_eq!(candidate.reason, OrphanReason::AppNameNotInstalled);
        assert_eq!(
            candidate.confidence,
            MatchConfidence::Low,
            "a generic-location orphan never matched anything, so it stays low confidence"
        );

        fs::remove_dir_all(&home).expect("removes temp home");
    }

    #[test]
    fn a_name_that_weakly_matches_an_installed_app_is_not_an_orphan() {
        let home = temp_home("weak-match-owned");
        // "SmallWidgets" is Notes's vendor — a vendor-only match still counts
        // as owned, the same way `find_leftovers` treats it as a match rather
        // than nothing at all.
        let vendor_dir = home
            .join("Library")
            .join("Application Support")
            .join("SmallWidgets");
        fs::create_dir_all(&vendor_dir).expect("creates vendor dir");

        let index = vec![notes_identity()];
        let orphans =
            find_orphans_from(&index, Some(home.as_path()), no_system_library().as_path());

        assert!(
            orphans.is_empty(),
            "a vendor-only match against an installed app's identity is owned, not orphaned"
        );

        fs::remove_dir_all(&home).expect("removes temp home");
    }

    #[test]
    fn an_unclaimed_group_container_is_unknown_container_owner_and_shared_or_unsafe() {
        let home = temp_home("group-container");
        let group = home
            .join("Library")
            .join("Group Containers")
            .join("ABCDE12345.gonecorp.shared");
        fs::create_dir_all(&group).expect("creates group container");

        let orphans = find_orphans_from(&[], Some(home.as_path()), no_system_library().as_path());

        let candidate = orphans
            .iter()
            .find(|candidate| candidate.location == LeftoverLocation::GroupContainers)
            .expect("finds the unclaimed group container");
        assert_eq!(candidate.reason, OrphanReason::UnknownContainerOwner);
        assert_eq!(candidate.confidence, MatchConfidence::SharedOrUnsafe);

        fs::remove_dir_all(&home).expect("removes temp home");
    }

    #[test]
    fn a_group_container_claimed_by_an_installed_app_is_not_an_orphan() {
        let home = temp_home("group-container-owned");
        let group = home
            .join("Library")
            .join("Group Containers")
            .join("ABCDE12345.smallwidgets.notes");
        fs::create_dir_all(&group).expect("creates group container");

        let index = vec![AppIdentity::new(
            Some("com.smallwidgets.Notes"),
            "Notes",
            Some("ABCDE12345".into()),
        )];
        let orphans =
            find_orphans_from(&index, Some(home.as_path()), no_system_library().as_path());

        assert!(
            orphans.is_empty(),
            "a group container an installed app's identity claims must not be flagged"
        );

        fs::remove_dir_all(&home).expect("removes temp home");
    }

    #[test]
    fn apples_own_namespace_is_never_flagged_even_with_an_empty_index() {
        let home = temp_home("apple-namespace");
        let launch_agents = home.join("Library").join("LaunchAgents");
        fs::create_dir_all(&launch_agents).expect("creates LaunchAgents dir");
        fs::write(launch_agents.join("com.apple.something.plist"), b"")
            .expect("writes an Apple launch agent plist");

        let orphans = find_orphans_from(&[], Some(home.as_path()), no_system_library().as_path());

        assert!(
            orphans.is_empty(),
            "com.apple.* entries must never be flagged as orphans"
        );

        fs::remove_dir_all(&home).expect("removes temp home");
    }

    #[test]
    fn system_scope_candidates_are_always_shared_or_unsafe() {
        let system_library = temp_home("system-scope-library");
        let launch_daemons = system_library.join("LaunchDaemons");
        fs::create_dir_all(&launch_daemons).expect("creates fake system LaunchDaemons");
        fs::write(launch_daemons.join("com.gonecorp.daemon.plist"), b"")
            .expect("writes fake launch daemon plist");

        let orphans = find_orphans_from(&[], None, system_library.as_path());

        let candidate = orphans
            .iter()
            .find(|candidate| candidate.location == LeftoverLocation::SystemLaunchDaemons)
            .expect("finds the system-scope orphan candidate");
        assert_eq!(candidate.scope, LocationScope::System);
        assert_eq!(
            candidate.confidence,
            MatchConfidence::SharedOrUnsafe,
            "the protected-system-path penalty must always win for system scope"
        );

        fs::remove_dir_all(&system_library).expect("removes fake system library");
    }

    #[test]
    fn every_reason_has_a_non_empty_label() {
        for reason in [
            OrphanReason::BundleIdentifierNotInstalled,
            OrphanReason::AppNameNotInstalled,
            OrphanReason::MissingOwnerApplication,
            OrphanReason::StaleSavedState,
            OrphanReason::StalePreference,
            OrphanReason::UnknownContainerOwner,
        ] {
            assert!(!reason_label(reason).is_empty());
        }
    }
}
