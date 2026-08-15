//! Platform-neutral AI-app policy and resolved location values.
//!
//! Role policy is genuinely shared: models, application support and chat
//! history are user data everywhere, while only logs and generated caches may
//! be cleaned. The old seam was not otherwise platform-neutral: it carried
//! macOS bundle identifiers and static `~/Library` strings. Definitions now
//! resolve each host's environment into [`AiAppLocation`] values before the
//! shared scanner sees them.

use std::path::PathBuf;

use crate::core::risk::{RiskLevel, SelectionPolicy};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AiAppRole {
    Logs,
    TemporaryDownloads,
    Cache,
    Models,
    ApplicationSupport,
    ChatHistory,
}

impl AiAppRole {
    pub fn risk(self) -> RiskLevel {
        match self {
            AiAppRole::Logs | AiAppRole::TemporaryDownloads | AiAppRole::Cache => {
                RiskLevel::SafeRecreatable
            }
            AiAppRole::Models | AiAppRole::ApplicationSupport | AiAppRole::ChatHistory => {
                RiskLevel::UserData
            }
        }
    }

    pub fn selection_policy(self) -> SelectionPolicy {
        match self {
            AiAppRole::Logs | AiAppRole::Cache => SelectionPolicy::SelectedByDefault,
            AiAppRole::TemporaryDownloads | AiAppRole::ApplicationSupport => {
                SelectionPolicy::NotSelectedByDefault
            }
            AiAppRole::Models | AiAppRole::ChatHistory => SelectionPolicy::NeverBulkSelect,
        }
    }

    /// Models, support data, chat history and temporary downloads remain
    /// scan-only even if a provider later registers a location for them.
    pub fn allow_cleanup(self) -> bool {
        matches!(self, AiAppRole::Logs | AiAppRole::Cache)
    }
}

/// Whether the location has been confirmed against a real installation or is
/// inferred from a documented platform convention pending captain validation.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AiAppPathEvidence {
    Verified,
    Inferred,
}

/// What the path names. This avoids treating an exact Windows log file like a
/// directory, or splitting a model store into implementation-detail children.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AiAppTarget {
    ExactFile,
    DirectoryContents,
    DirectorySummary,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct AiAppLocation {
    pub role: AiAppRole,
    pub path: PathBuf,
    pub group: &'static str,
    pub target: AiAppTarget,
    pub evidence: AiAppPathEvidence,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct AiAppDefinition {
    pub id: &'static str,
    pub display_name: &'static str,
    /// Bundle identifiers on macOS and executable/process names elsewhere.
    /// The platform activity probe alone interprets these values.
    pub activity_ids: &'static [&'static str],
    pub locations: Vec<AiAppLocation>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AiAppActivity {
    Running,
    NotRunning,
    Unknown,
}

#[cfg(test)]
mod tests {
    use super::AiAppRole;
    use crate::core::risk::SelectionPolicy;

    #[test]
    fn logs_and_cache_are_the_only_roles_ever_allow_listed() {
        let allow_listed: Vec<AiAppRole> = [
            AiAppRole::Logs,
            AiAppRole::TemporaryDownloads,
            AiAppRole::Cache,
            AiAppRole::Models,
            AiAppRole::ApplicationSupport,
            AiAppRole::ChatHistory,
        ]
        .into_iter()
        .filter(|role| role.allow_cleanup())
        .collect();

        assert_eq!(allow_listed, vec![AiAppRole::Logs, AiAppRole::Cache]);
    }

    #[test]
    fn models_and_chat_history_are_never_selected_by_default() {
        for role in [AiAppRole::Models, AiAppRole::ChatHistory] {
            assert_ne!(role.selection_policy(), SelectionPolicy::SelectedByDefault);
        }
    }

    #[test]
    fn models_and_chat_history_are_excluded_from_bulk_select() {
        for role in [AiAppRole::Models, AiAppRole::ChatHistory] {
            assert_eq!(role.selection_policy(), SelectionPolicy::NeverBulkSelect);
        }
    }

    #[test]
    fn every_role_has_an_explicit_risk_and_selection_policy() {
        for role in [
            AiAppRole::Logs,
            AiAppRole::TemporaryDownloads,
            AiAppRole::Cache,
            AiAppRole::Models,
            AiAppRole::ApplicationSupport,
            AiAppRole::ChatHistory,
        ] {
            let _ = role.risk();
            let _ = role.selection_policy();
        }
    }

    #[test]
    fn models_and_chat_history_are_user_data_never_bulk_selected() {
        for role in [AiAppRole::Models, AiAppRole::ChatHistory] {
            assert_eq!(role.risk(), crate::core::risk::RiskLevel::UserData);
            assert_eq!(role.selection_policy(), SelectionPolicy::NeverBulkSelect);
        }
    }
}
