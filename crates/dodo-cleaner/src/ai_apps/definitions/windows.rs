//! Windows path table — inferred, not confirmed on a real installation.
//!
//! Only exact Ollama log files and exact Electron cache/log subdirectories for
//! LM Studio are cleanable. Models are separate scan-only summaries. Never
//! replace these with either application's whole AppData directory.

use crate::ai_apps::definitions::AiAppEnvironment;
use crate::core::ai_app_provider::{
    AiAppDefinition, AiAppLocation, AiAppPathEvidence, AiAppRole, AiAppTarget,
};

const INFERRED: AiAppPathEvidence = AiAppPathEvidence::Inferred;
const OLLAMA_ACTIVITY_IDS: &[&str] = &["ollama.exe", "ollama app.exe"];
const LM_STUDIO_ACTIVITY_IDS: &[&str] = &["LM Studio.exe"];

pub(crate) fn definitions(environment: &AiAppEnvironment) -> Vec<AiAppDefinition> {
    vec![ollama(environment), lm_studio(environment)]
}

fn ollama(environment: &AiAppEnvironment) -> AiAppDefinition {
    let mut locations = Vec::new();
    if let Some(models) = environment.ollama_models.clone().or_else(|| {
        environment
            .home
            .as_ref()
            .map(|home| home.join(".ollama").join("models"))
    }) {
        locations.push(location(
            AiAppRole::Models,
            models,
            "Ollama models",
            AiAppTarget::DirectorySummary,
        ));
    }
    if let Some(root) = environment
        .local_app_data
        .as_ref()
        .map(|path| path.join("Ollama"))
    {
        for file in ["server.log", "app.log"] {
            locations.push(location(
                AiAppRole::Logs,
                root.join(file),
                "Ollama logs",
                AiAppTarget::ExactFile,
            ));
        }
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
        locations.push(location(
            AiAppRole::Models,
            home.join(".lmstudio").join("models"),
            "LM Studio models",
            AiAppTarget::DirectorySummary,
        ));
    }
    if let Some(root) = environment
        .roaming_app_data
        .as_ref()
        .map(|path| path.join("LM Studio"))
    {
        electron_locations(&mut locations, &root);
    }
    AiAppDefinition {
        id: "lm-studio",
        display_name: "LM Studio",
        activity_ids: LM_STUDIO_ACTIVITY_IDS,
        locations,
    }
}

fn electron_locations(locations: &mut Vec<AiAppLocation>, root: &std::path::Path) {
    for directory in ["Cache", "Code Cache", "GPUCache"] {
        locations.push(location(
            AiAppRole::Cache,
            root.join(directory),
            "LM Studio cache",
            AiAppTarget::DirectoryContents,
        ));
    }
    locations.push(location(
        AiAppRole::Logs,
        root.join("logs"),
        "LM Studio logs",
        AiAppTarget::DirectoryContents,
    ));
}

fn location(
    role: AiAppRole,
    path: std::path::PathBuf,
    group: &'static str,
    target: AiAppTarget,
) -> AiAppLocation {
    AiAppLocation {
        role,
        path,
        group,
        target,
        evidence: INFERRED,
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::definitions;
    use crate::ai_apps::definitions::AiAppEnvironment;
    use crate::core::ai_app_provider::{AiAppPathEvidence, AiAppRole, AiAppTarget};
    use crate::paths::HostOs;

    #[test]
    fn injected_windows_environment_resolves_only_bounded_targets() {
        let mut environment = AiAppEnvironment::fixture(HostOs::Windows, r"C:\Users\captain");
        environment.local_app_data = Some(PathBuf::from(r"C:\Users\captain\AppData\Local"));
        environment.roaming_app_data = Some(PathBuf::from(r"C:\Users\captain\AppData\Roaming"));
        environment.ollama_models = Some(PathBuf::from(r"D:\Models\Ollama"));

        let apps = definitions(&environment);
        let ollama = apps.iter().find(|app| app.id == "ollama").expect("Ollama");
        assert!(ollama.locations.iter().any(|location| {
            location.path.as_path() == Path::new(r"D:\Models\Ollama")
                && location.role == AiAppRole::Models
                && location.target == AiAppTarget::DirectorySummary
        }));
        assert_eq!(
            ollama
                .locations
                .iter()
                .filter(|location| location.target == AiAppTarget::ExactFile)
                .count(),
            2
        );

        let lm_studio = apps
            .iter()
            .find(|app| app.id == "lm-studio")
            .expect("LM Studio");
        assert_eq!(
            lm_studio
                .locations
                .iter()
                .filter(|location| location.target == AiAppTarget::DirectoryContents)
                .count(),
            4
        );
        let cleanup_roots = crate::ai_apps::cleanup_allowed_roots(&environment);
        assert!(
            cleanup_roots
                .contains(&PathBuf::from(r"C:\Users\captain\AppData\Local").join("Ollama"))
        );
        assert!(!cleanup_roots.contains(&PathBuf::from(r"D:\Models\Ollama")));
        assert!(apps.iter().flat_map(|app| &app.locations).all(|location| {
            location.evidence == AiAppPathEvidence::Inferred
                && !(location.role.allow_cleanup()
                    && location.target == AiAppTarget::DirectorySummary)
        }));
    }
}
