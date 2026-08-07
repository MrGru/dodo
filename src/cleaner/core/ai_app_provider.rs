//! The seam a local AI desktop app plugs into (`CleanerCategory::AiApps`,
//! Phase 12). Two providers today (Ollama, LM Studio) under
//! `macos::scanners::ai_app_providers`, one scanner
//! (`macos::scanners::ai_apps::AiAppsScanner`) that drives both.
//!
//! # Why this is `AiAppRole`-driven rather than per-location, unlike Node
//! Tooling
//!
//! `core::node_tool_provider::NodeCacheLocation` carries its own
//! `risk`/`selection_policy`/`allow_cleanup` fields because Node Tooling's six
//! providers are genuinely different tools with genuinely different judgment
//! calls per location (pnpm's shared store needed a bespoke "never allow
//! cleanup" call that no other provider needed). The ticket's six AI-app
//! sub-categories are not like that: "Logs and recreatable caches may be
//! safe", "Models are never selected by default", "Chat history requires
//! explicit opt-in" are rules about the *role* a location plays, stated once,
//! meant to apply identically to every current and future provider. So
//! [`AiAppRoot`] carries only a `role`, and [`AiAppRole::risk`],
//! [`AiAppRole::selection_policy`] and [`AiAppRole::allow_cleanup`] derive the
//! rest centrally — which is also what makes "add a third provider" mean
//! "add one [`AiAppDefinition`] naming existing roles", never "decide a new
//! provider's risk posture" or "touch the scanner".
//!
//! # `allow_cleanup` is deliberately narrower than the ticket's per-role
//! rules alone would suggest
//!
//! Only [`AiAppRole::Logs`] and [`AiAppRole::Cache`] return `true` from
//! [`AiAppRole::allow_cleanup`]. Models, Application support, Chat history
//! and Temporary downloads are scan-only this phase — reviewable and
//! individually revealable, but never wired into
//! `macos::cleanup::policy_for`'s allow-list — the same "scan-only until a
//! more deliberate workflow exists" posture Phase 11 used for pnpm's store
//! and Xcode's Archives. See `docs/cleaner/known-limitations.md`.
//!
//! # Deviating from the ticket's suggested signature
//!
//! The ticket's suggested `AiAppDefinition` has `bundle_ids: &'static
//! [&'static str]`. Every registered app lists more than one candidate
//! bundle identifier on purpose: neither Ollama's nor LM Studio's exact
//! macOS bundle identifier is confidently known at authorship time (see
//! `docs/cleaner/known-limitations.md`), so
//! `macos::platform::running_apps::is_any_bundle_running` checks every
//! candidate and a wrong guess just means the running-process warning never
//! fires — a safe failure mode, since nothing in this phase deletes
//! anything automatically.

use crate::cleaner::core::risk::{RiskLevel, SelectionPolicy};

/// One of the six sub-categories the ticket names explicitly for AI apps:
/// "Detect separately: Logs, Temporary downloads, Caches, Models,
/// Application support, Chat or prompt history."
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
    /// The risk tier every location tagged with this role is scanned at,
    /// regardless of which provider it came from.
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

    /// Whether this role's items are ever bulk-selected, individually
    /// pre-checked, or excluded from both — see
    /// `core::selection::selected_by_default_ids` for how
    /// `SelectionPolicy::SelectedByDefault` is the *only* variant "Select
    /// safe items" ever acts on, which is what makes `NeverBulkSelect` here
    /// the actual enforcement of the ticket's "Chat history requires
    /// explicit opt-in" — not a second, separately-checked flag.
    pub fn selection_policy(self) -> SelectionPolicy {
        match self {
            AiAppRole::Logs | AiAppRole::Cache => SelectionPolicy::SelectedByDefault,
            AiAppRole::TemporaryDownloads | AiAppRole::ApplicationSupport => {
                SelectionPolicy::NotSelectedByDefault
            }
            AiAppRole::Models | AiAppRole::ChatHistory => SelectionPolicy::NeverBulkSelect,
        }
    }

    /// Whether a location tagged with this role may ever be allow-listed for
    /// `MoveToTrash` cleanup. See this module's doc comment for why only
    /// Logs and Cache qualify this phase.
    pub fn allow_cleanup(self) -> bool {
        matches!(self, AiAppRole::Logs | AiAppRole::Cache)
    }
}

/// One directory a provider wants scanned under its own role.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AiAppRoot {
    pub role: AiAppRole,
    /// Either `~`-prefixed (resolved against `ScanContext::user_home`, the
    /// same convention `macos::scanners::xcode_junk::resolve_roots` uses) or
    /// an absolute path.
    pub path: &'static str,
    /// UI group label, e.g. `"Ollama models"` or `"LM Studio logs"`.
    pub group: &'static str,
}

/// One AI app provider — the ticket's suggested shape, verbatim aside from
/// doc comments. `id` is stored on every item this provider produces via
/// `ItemMetadata::AiApp(AiAppMetadata { app_id, .. })`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AiAppDefinition {
    pub id: &'static str,
    pub display_name: &'static str,
    pub bundle_ids: &'static [&'static str],
    pub roots: &'static [AiAppRoot],
}

#[cfg(test)]
mod tests {
    use super::*;

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
        // `core::selection::selected_by_default_ids` only ever includes
        // `SelectionPolicy::SelectedByDefault` items — this is what actually
        // enforces "Chat history requires explicit opt-in" for the bulk
        // "Select safe items" action; this test pins that both roles resolve
        // to a policy other than `SelectedByDefault` so that enforcement
        // keeps covering them.
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
}
