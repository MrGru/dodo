//! macOS path table retained from the original AI Apps scanner.
//!
//! Ollama's roots and LM Studio's support/log/cache roots are existing verified
//! definitions. LM Studio's two candidate model roots remain explicitly
//! inferred. Their overlap with the Application Support summary is an open
//! captain product decision recorded in the wide-scan backlog; this move does
//! not change that attribution.

use std::path::PathBuf;

use crate::cleaner::ai_apps::definitions::AiAppEnvironment;
use crate::cleaner::core::ai_app_provider::{
    AiAppDefinition, AiAppLocation, AiAppPathEvidence, AiAppRole, AiAppTarget,
};

const OLLAMA_ACTIVITY_IDS: &[&str] = &["com.ollama.ollama", "ai.ollama.ollama"];
const LM_STUDIO_ACTIVITY_IDS: &[&str] = &["com.lmstudio.LMStudio", "com.electron.lmstudio"];

pub(crate) fn definitions(environment: &AiAppEnvironment) -> Vec<AiAppDefinition> {
    vec![ollama(environment), lm_studio(environment)]
}

fn ollama(environment: &AiAppEnvironment) -> AiAppDefinition {
    let mut locations = Vec::new();
    if let Some(models) = environment.ollama_models.clone().or_else(|| {
        environment
            .home
            .as_ref()
            .map(|home| home.join(".ollama/models"))
    }) {
        locations.push(location(
            AiAppRole::Models,
            models,
            "Ollama models",
            AiAppTarget::DirectorySummary,
            AiAppPathEvidence::Verified,
        ));
    }
    if let Some(home) = environment.home.as_ref() {
        locations.extend([
            location(
                AiAppRole::ApplicationSupport,
                home.join("Library/Application Support/Ollama"),
                "Ollama application support",
                AiAppTarget::DirectorySummary,
                AiAppPathEvidence::Verified,
            ),
            location(
                AiAppRole::Logs,
                home.join("Library/Logs/Ollama"),
                "Ollama logs",
                AiAppTarget::DirectoryContents,
                AiAppPathEvidence::Verified,
            ),
            location(
                AiAppRole::Cache,
                home.join("Library/Caches/Ollama"),
                "Ollama cache",
                AiAppTarget::DirectoryContents,
                AiAppPathEvidence::Verified,
            ),
        ]);
    }
    AiAppDefinition {
        id: "ollama",
        display_name: "Ollama",
        activity_ids: OLLAMA_ACTIVITY_IDS,
        locations,
    }
}

fn lm_studio(environment: &AiAppEnvironment) -> AiAppDefinition {
    let mut locations = Vec::new();
    if let Some(home) = environment.home.as_ref() {
        locations.extend([
            location(
                AiAppRole::Models,
                home.join(".cache/lm-studio/models"),
                "LM Studio models",
                AiAppTarget::DirectorySummary,
                AiAppPathEvidence::Inferred,
            ),
            location(
                AiAppRole::Models,
                home.join("Library/Application Support/LM Studio/models"),
                "LM Studio models",
                AiAppTarget::DirectorySummary,
                AiAppPathEvidence::Inferred,
            ),
            location(
                AiAppRole::ApplicationSupport,
                home.join("Library/Application Support/LM Studio"),
                "LM Studio application support",
                AiAppTarget::DirectorySummary,
                AiAppPathEvidence::Verified,
            ),
            location(
                AiAppRole::Logs,
                home.join("Library/Application Support/LM Studio/logs"),
                "LM Studio logs",
                AiAppTarget::DirectoryContents,
                AiAppPathEvidence::Verified,
            ),
            location(
                AiAppRole::Cache,
                home.join("Library/Caches/LM Studio"),
                "LM Studio cache",
                AiAppTarget::DirectoryContents,
                AiAppPathEvidence::Verified,
            ),
        ]);
    }
    AiAppDefinition {
        id: "lm-studio",
        display_name: "LM Studio",
        activity_ids: LM_STUDIO_ACTIVITY_IDS,
        locations,
    }
}

fn location(
    role: AiAppRole,
    path: PathBuf,
    group: &'static str,
    target: AiAppTarget,
    evidence: AiAppPathEvidence,
) -> AiAppLocation {
    AiAppLocation {
        role,
        path,
        group,
        target,
        evidence,
    }
}

#[cfg(test)]
mod tests {
    use super::definitions;
    use crate::cleaner::ai_apps::definitions::AiAppEnvironment;
    use crate::cleaner::core::ai_app_provider::{AiAppPathEvidence, AiAppRole};
    use crate::paths::HostOs;

    #[test]
    fn preserves_both_providers_and_marks_only_uncertain_model_candidates_inferred() {
        let apps = definitions(&AiAppEnvironment::fixture(HostOs::MacOs, "/Users/captain"));
        assert_eq!(apps.len(), 2);
        assert!(apps.iter().any(|app| app.id == "ollama"));
        assert!(apps.iter().any(|app| app.id == "lm-studio"));

        let lm_studio = apps
            .iter()
            .find(|app| app.id == "lm-studio")
            .expect("LM Studio");
        assert_eq!(
            lm_studio
                .locations
                .iter()
                .filter(|location| location.role == AiAppRole::Models)
                .count(),
            2
        );
        assert!(lm_studio.locations.iter().all(|location| {
            (location.role == AiAppRole::Models)
                == (location.evidence == AiAppPathEvidence::Inferred)
        }));
    }
}
