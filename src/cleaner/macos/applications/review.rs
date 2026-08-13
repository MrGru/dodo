//! Builds an uninstall review for one installed application (Phase 9).
//!
//! This is the seam between the pure identity/confidence/location logic in
//! this module and the rest of Cleaner: it takes the already-scanned
//! `.app` [`CleanableItem`] (from
//! [`crate::cleaner::macos::scanners::installed_apps`]) plus the identities
//! of every *other* installed app, and returns a fully-typed
//! [`UninstallReview`] the view can render and, on confirmation, hand to the
//! existing [`crate::cleaner::macos::cleanup::cleanup_items`] pipeline. It
//! never deletes anything itself — matching the ticket's "scan and clean must
//! be separate" principle even for this one-app-at-a-time workflow.

use std::path::Path;

use crate::cleaner::core::category::CleanerCategory;
use crate::cleaner::core::fs::measure_size;
use crate::cleaner::core::item::{CleanableItem, CleanableItemId, ItemMetadata, ItemWarning};
use crate::cleaner::core::risk::{ItemCapability, RiskLevel, SelectionPolicy};

use super::confidence::{self, MatchConfidence, NameMatchKind, confidence_label};
use super::identity::AppIdentity;
use super::locations::{self, LeftoverLocation, LeftoverMatch, LocationScope, location_label};

/// Why [`build_uninstall_review`] refused to build a review at all — as
/// opposed to a review full of low-confidence items, which is not an error.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UninstallReviewError {
    /// The item is `RiskLevel::Protected` (a system app, or otherwise marked
    /// non-uninstallable). The ticket is explicit: "System apps must never
    /// be uninstallable" and "Refuse protected apps".
    ProtectedApplication,
    /// The item is not an application bundle at all (wrong category or
    /// metadata shape) — defensive; the UI only offers this action for
    /// `InstalledApps` items.
    NotAnApplication,
}

/// One candidate leftover file or directory, with the confidence and
/// location context the review dialog shows.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct UninstallCandidate {
    pub item: CleanableItem,
    pub confidence: MatchConfidence,
    pub location: LeftoverLocation,
    pub scope: LocationScope,
}

/// The full uninstall review for one app: the app bundle itself, plus every
/// leftover candidate found across the fixed location list.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct UninstallReview {
    pub app: CleanableItem,
    pub candidates: Vec<UninstallCandidate>,
}

/// Builds the review. Pure orchestration plus read-only filesystem lookups
/// (directory listings and size measurement) — no writes, no deletion.
///
/// `other_apps` should be every *other* installed app's identity (built from
/// the same `InstalledApps` scan result the caller already has), so
/// ambiguous leftovers can be flagged and downgraded per the ticket's
/// "another installed app also matches" penalty.
pub fn build_uninstall_review(
    app: &CleanableItem,
    other_apps: &[AppIdentity],
    home: Option<&Path>,
) -> Result<UninstallReview, UninstallReviewError> {
    if app.risk == RiskLevel::Protected {
        return Err(UninstallReviewError::ProtectedApplication);
    }
    let ItemMetadata::Application(metadata) = &app.metadata else {
        return Err(UninstallReviewError::NotAnApplication);
    };

    let identity = AppIdentity::new(
        metadata.bundle_id.as_deref(),
        &app.display_name,
        metadata.team_id.clone(),
    );

    let matches = locations::find_leftovers(&identity, home, other_apps);
    let candidates = matches.into_iter().map(build_candidate).collect();

    let app_item = CleanableItem {
        logical_size: measure_size(
            app.path.as_path(),
            CleanerCategory::InstalledApps,
            RiskLevel::ReviewRecommended,
        ),
        capabilities: vec![
            ItemCapability::UninstallApplication,
            ItemCapability::MoveToTrash,
            ItemCapability::RevealInFinder,
            ItemCapability::CopyPath,
        ],
        selection_policy: SelectionPolicy::SelectedByDefault,
        ..app.clone()
    };

    Ok(UninstallReview {
        app: app_item,
        candidates,
    })
}

/// Builds an [`AppIdentity`] from an already-scanned `CleanableItem`, for
/// callers assembling the `other_apps` list `build_uninstall_review` needs.
/// Returns `None` for anything that is not an `Application`-metadata item.
pub fn identity_for(item: &CleanableItem) -> Option<AppIdentity> {
    let ItemMetadata::Application(metadata) = &item.metadata else {
        return None;
    };
    Some(AppIdentity::new(
        metadata.bundle_id.as_deref(),
        &item.display_name,
        metadata.team_id.clone(),
    ))
}

fn build_candidate(candidate: LeftoverMatch) -> UninstallCandidate {
    let is_system = candidate.scope == LocationScope::System;
    let score = confidence::total_score(
        candidate.base_kind,
        candidate.matched_by_other_app,
        is_system,
    );
    let confidence = confidence::classify(score);

    let selection_policy = match confidence {
        MatchConfidence::Confirmed if !is_system => SelectionPolicy::SelectedByDefault,
        MatchConfidence::SharedOrUnsafe => SelectionPolicy::NeverBulkSelect,
        _ => SelectionPolicy::NotSelectedByDefault,
    };
    let risk = if is_system {
        RiskLevel::Protected
    } else if confidence == MatchConfidence::SharedOrUnsafe {
        RiskLevel::UserData
    } else {
        RiskLevel::ReviewRecommended
    };

    let mut capabilities = vec![ItemCapability::RevealInFinder, ItemCapability::CopyPath];
    if !is_system {
        capabilities.push(ItemCapability::MoveToTrash);
    }

    let mut warnings = Vec::new();
    if is_system {
        warnings.push(ItemWarning {
            message: "System-owned location: scan-only until a privileged helper exists."
                .to_string(),
        });
    }
    if candidate.matched_by_other_app {
        warnings.push(ItemWarning {
            message: "Another installed app also matches this path.".to_string(),
        });
    }

    let display_name = candidate
        .path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("Leftover item")
        .to_string();
    let logical_size = if is_system {
        0
    } else {
        measure_size(
            candidate.path.as_path(),
            CleanerCategory::InstalledApps,
            RiskLevel::ReviewRecommended,
        )
    };

    let item = CleanableItem {
        id: item_id(candidate.path.as_path()),
        category: CleanerCategory::InstalledApps,
        group: Some(location_label(candidate.location).to_string()),
        display_name,
        path: candidate.path,
        logical_size,
        allocated_size: None,
        modified_at: None,
        last_accessed_at: None,
        risk,
        selection_policy,
        capabilities,
        explanation: explanation_for(candidate.base_kind, confidence),
        warnings,
        metadata: ItemMetadata::Generic,
    };

    UninstallCandidate {
        item,
        confidence,
        location: candidate.location,
        scope: candidate.scope,
    }
}

fn explanation_for(kind: NameMatchKind, confidence: MatchConfidence) -> String {
    format!(
        "{} confidence: {}.",
        confidence_label(confidence),
        signal_label(kind)
    )
}

fn signal_label(kind: NameMatchKind) -> &'static str {
    match kind {
        NameMatchKind::ExactBundleIdentifier => "exact bundle identifier match",
        NameMatchKind::ExactSandboxContainer => "exact sandbox container match",
        NameMatchKind::ExactApplicationSupportDirectory => {
            "exact Application Support directory match"
        }
        NameMatchKind::ExactGroupContainerEntitlement => "app-specific group container match",
        NameMatchKind::ExactSavedStateIdentifier => "exact saved-state identifier match",
        NameMatchKind::ExactPreferenceIdentifier => "exact preference identifier match",
        NameMatchKind::ExactNormalizedAppName => "exact normalized app name match",
        NameMatchKind::KnownAppSpecificRule => "known app-specific rule",
        NameMatchKind::VendorAndAppNameCombination => "vendor and app name combination",
        NameMatchKind::PartialAppNameMatch => "partial app name match",
        NameMatchKind::VendorOnlyMatch => "vendor-only match",
        NameMatchKind::SharedContainer => "shared container",
        NameMatchKind::KnownSharedVendorDirectory => "known shared vendor directory",
    }
}

fn item_id(path: &Path) -> CleanableItemId {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    CleanableItemId(hasher.finish())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use super::*;
    use crate::cleaner::core::item::{ApplicationMetadata, CleanableItemId as ItemId};

    fn app_item(risk: RiskLevel, bundle_id: Option<&str>, path: PathBuf) -> CleanableItem {
        CleanableItem {
            id: ItemId(1),
            category: CleanerCategory::InstalledApps,
            group: None,
            display_name: "Notes".to_string(),
            path,
            logical_size: 0,
            allocated_size: None,
            modified_at: None,
            last_accessed_at: None,
            risk,
            selection_policy: SelectionPolicy::NeverBulkSelect,
            capabilities: vec![ItemCapability::RevealInFinder, ItemCapability::CopyPath],
            explanation: String::new(),
            warnings: Vec::new(),
            metadata: ItemMetadata::Application(ApplicationMetadata {
                bundle_id: bundle_id.map(ToOwned::to_owned),
                team_id: None,
                version: None,
                executable: None,
                icon: None,
            }),
        }
    }

    #[test]
    fn protected_apps_are_refused() {
        let item = app_item(
            RiskLevel::Protected,
            Some("com.apple.Notes"),
            PathBuf::from("/System/Applications/Notes.app"),
        );
        let result = build_uninstall_review(&item, &[], None);
        assert_eq!(result, Err(UninstallReviewError::ProtectedApplication));
    }

    #[test]
    fn review_includes_the_app_bundle_and_its_leftovers() {
        let temp = std::env::temp_dir().join(format!("dodo-cleaner-review-{}", std::process::id()));
        let app_path = temp.join("Applications").join("Notes.app");
        fs::create_dir_all(&app_path).expect("creates app bundle dir");
        let support = temp
            .join("Library")
            .join("Application Support")
            .join("Notes");
        fs::create_dir_all(&support).expect("creates app support dir");

        let item = app_item(
            RiskLevel::ReviewRecommended,
            Some("com.smallwidgets.Notes"),
            app_path,
        );
        let review = build_uninstall_review(&item, &[], Some(temp.as_path()))
            .expect("builds a review for a non-protected app");

        assert!(
            review
                .app
                .capabilities
                .contains(&ItemCapability::UninstallApplication)
        );
        assert!(
            review
                .candidates
                .iter()
                .any(|candidate| candidate.location == LeftoverLocation::ApplicationSupport)
        );

        fs::remove_dir_all(&temp).expect("removes temp tree");
    }

    #[test]
    fn shared_or_unsafe_candidates_are_never_selected_by_default() {
        let temp =
            std::env::temp_dir().join(format!("dodo-cleaner-review-shared-{}", std::process::id()));
        let group = temp
            .join("Library")
            .join("Group Containers")
            .join("ABCDE12345.smallwidgets.shared");
        fs::create_dir_all(&group).expect("creates shared group container");

        let item = app_item(
            RiskLevel::ReviewRecommended,
            Some("com.smallwidgets.Notes"),
            temp.join("Applications").join("Notes.app"),
        );
        let review =
            build_uninstall_review(&item, &[], Some(temp.as_path())).expect("builds a review");

        let shared = review
            .candidates
            .iter()
            .find(|candidate| candidate.location == LeftoverLocation::GroupContainers)
            .expect("finds the shared group container candidate");
        assert_eq!(shared.confidence, MatchConfidence::SharedOrUnsafe);
        assert_eq!(
            shared.item.selection_policy,
            SelectionPolicy::NeverBulkSelect
        );

        fs::remove_dir_all(&temp).expect("removes temp tree");
    }
}
