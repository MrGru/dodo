//! Confidence scoring for a candidate leftover-file match (Phase 9).
//!
//! Pure domain logic. [`NameMatchKind`] is the structural signal a location
//! matcher found (see [`super::locations`]); [`total_score`] adds the two
//! situational penalties from the ticket's model that are not tied to a
//! specific location (ambiguity with another installed app, and a system
//! path that cleanup cannot touch yet); [`classify`] buckets the final score
//! into the five-value [`MatchConfidence`] the UI and selection policy read.

/// How confidently a leftover path belongs to one specific app.
///
/// Only [`MatchConfidence::Confirmed`] defaults to selected in the uninstall
/// review dialog. Every other value — including `High` — starts unselected;
/// the ticket's "carefully validated high-confidence matches" carve-out is
/// intentionally not implemented yet, so `High` stays conservative until a
/// concrete validation rule exists to tell it apart from `Confirmed`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MatchConfidence {
    Confirmed,
    High,
    Medium,
    Low,
    SharedOrUnsafe,
}

/// A named structural signal from the ticket's confidence table. Each variant
/// carries the point value from that table; [`total_score`] applies the two
/// situational penalties ("another installed app also matches", "protected
/// system path") on top, since those depend on context a location matcher
/// does not have.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NameMatchKind {
    ExactBundleIdentifier,
    ExactSandboxContainer,
    ExactApplicationSupportDirectory,
    ExactGroupContainerEntitlement,
    ExactSavedStateIdentifier,
    ExactPreferenceIdentifier,
    ExactNormalizedAppName,
    KnownAppSpecificRule,
    VendorAndAppNameCombination,
    PartialAppNameMatch,
    VendorOnlyMatch,
    SharedContainer,
    KnownSharedVendorDirectory,
}

impl NameMatchKind {
    /// The base point value from the ticket's suggested confidence model.
    pub fn base_points(self) -> i32 {
        match self {
            NameMatchKind::ExactBundleIdentifier => 100,
            NameMatchKind::ExactSandboxContainer => 100,
            NameMatchKind::ExactApplicationSupportDirectory => 90,
            NameMatchKind::ExactGroupContainerEntitlement => 90,
            NameMatchKind::ExactSavedStateIdentifier => 85,
            NameMatchKind::ExactPreferenceIdentifier => 80,
            NameMatchKind::ExactNormalizedAppName => 65,
            NameMatchKind::KnownAppSpecificRule => 60,
            NameMatchKind::VendorAndAppNameCombination => 45,
            NameMatchKind::PartialAppNameMatch => 20,
            NameMatchKind::VendorOnlyMatch => 10,
            NameMatchKind::SharedContainer => -80,
            NameMatchKind::KnownSharedVendorDirectory => -70,
        }
    }
}

/// The two situational penalties, applied on top of a [`NameMatchKind`]'s
/// base points.
const AMBIGUOUS_WITH_ANOTHER_APP_PENALTY: i32 = -70;
const PROTECTED_SYSTEM_PATH_PENALTY: i32 = -100;

/// Adds the situational penalties to a base signal's points.
pub fn total_score(
    base: NameMatchKind,
    matched_by_other_app: bool,
    is_protected_system_path: bool,
) -> i32 {
    let mut score = base.base_points();
    if matched_by_other_app {
        score += AMBIGUOUS_WITH_ANOTHER_APP_PENALTY;
    }
    if is_protected_system_path {
        score += PROTECTED_SYSTEM_PATH_PENALTY;
    }
    score
}

/// Buckets a total score into a [`MatchConfidence`].
///
/// Thresholds line up with the ticket's own point values so every named
/// signal lands where its row implies: `ExactPreferenceIdentifier` (80) is
/// the lowest `High`, `VendorAndAppNameCombination` (45) is the lowest
/// `Medium`, and anything at or below zero — including every negative
/// signal — is `SharedOrUnsafe`.
pub fn classify(score: i32) -> MatchConfidence {
    if score >= 100 {
        MatchConfidence::Confirmed
    } else if score >= 80 {
        MatchConfidence::High
    } else if score >= 45 {
        MatchConfidence::Medium
    } else if score > 0 {
        MatchConfidence::Low
    } else {
        MatchConfidence::SharedOrUnsafe
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_bundle_identifier_is_confirmed() {
        let score = total_score(NameMatchKind::ExactBundleIdentifier, false, false);
        assert_eq!(classify(score), MatchConfidence::Confirmed);
    }

    #[test]
    fn application_support_and_group_container_land_in_high_not_confirmed() {
        assert_eq!(
            classify(total_score(
                NameMatchKind::ExactApplicationSupportDirectory,
                false,
                false
            )),
            MatchConfidence::High
        );
        assert_eq!(
            classify(total_score(
                NameMatchKind::ExactGroupContainerEntitlement,
                false,
                false
            )),
            MatchConfidence::High
        );
    }

    #[test]
    fn versioned_name_match_is_medium() {
        assert_eq!(
            classify(total_score(
                NameMatchKind::ExactNormalizedAppName,
                false,
                false
            )),
            MatchConfidence::Medium
        );
    }

    #[test]
    fn similar_name_partial_match_is_low() {
        assert_eq!(
            classify(total_score(
                NameMatchKind::PartialAppNameMatch,
                false,
                false
            )),
            MatchConfidence::Low
        );
    }

    #[test]
    fn vendor_only_match_is_low_not_shared() {
        assert_eq!(
            classify(total_score(NameMatchKind::VendorOnlyMatch, false, false)),
            MatchConfidence::Low
        );
    }

    #[test]
    fn shared_container_is_shared_or_unsafe() {
        assert_eq!(
            classify(total_score(NameMatchKind::SharedContainer, false, false)),
            MatchConfidence::SharedOrUnsafe
        );
    }

    #[test]
    fn ambiguous_match_is_downgraded() {
        let unambiguous = total_score(
            NameMatchKind::ExactApplicationSupportDirectory,
            false,
            false,
        );
        let ambiguous = total_score(NameMatchKind::ExactApplicationSupportDirectory, true, false);
        assert!(ambiguous < unambiguous);
        assert_eq!(classify(ambiguous), MatchConfidence::Low);
    }

    #[test]
    fn protected_system_path_is_always_shared_or_unsafe() {
        let score = total_score(NameMatchKind::ExactBundleIdentifier, false, true);
        assert_eq!(classify(score), MatchConfidence::SharedOrUnsafe);
    }
}
