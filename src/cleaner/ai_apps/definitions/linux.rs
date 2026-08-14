//! Linux path table — inferred, not confirmed on a real installation.
//!
//! Ollama exposes only its scan-only model root because journal output is not
//! a filesystem cache. LM Studio is bounded to exact Electron-generated
//! children below its XDG directories; no whole config/cache root is cleaned.

use std::path::{Path, PathBuf};

use crate::cleaner::ai_apps::definitions::AiAppEnvironment;
use crate::cleaner::core::ai_app_provider::{
    AiAppDefinition, AiAppLocation, AiAppPathEvidence, AiAppRole, AiAppTarget,
};

const INFERRED: AiAppPathEvidence = AiAppPathEvidence::Inferred;
const OLLAMA_ACTIVITY_IDS: &[&str] = &["ollama"];
const LM_STUDIO_ACTIVITY_IDS: &[&str] = &["lm-studio", "LM Studio"];

pub(crate) fn definitions(environment: &AiAppEnvironment) -> Vec<AiAppDefinition> {
    vec![ollama(environment), lm_studio(environment)]
}

fn ollama(environment: &AiAppEnvironment) -> AiAppDefinition {
    let locations = environment
        .ollama_models
        .clone()
        .or_else(|| {
            environment
                .home
                .as_ref()
                .map(|home| home.join(".ollama/models"))
        })
        .into_iter()
        .map(|path| {
            location(
                AiAppRole::Models,
                path,
                "Ollama models",
                AiAppTarget::DirectorySummary,
            )
        })
        .collect();
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
            home.join(".lmstudio/models"),
            "LM Studio models",
            AiAppTarget::DirectorySummary,
        ));
    }
    if let Some(cache_home) = cache_home(environment) {
        locations.push(location(
            AiAppRole::Models,
            cache_home.join("lm-studio/models"),
            "LM Studio models",
            AiAppTarget::DirectorySummary,
        ));
        electron_locations(&mut locations, &cache_home.join("LM Studio"));
    }
    if let Some(config_home) = config_home(environment) {
        electron_locations(&mut locations, &config_home.join("LM Studio"));
    }
    AiAppDefinition {
        id: "lm-studio",
        display_name: "LM Studio",
        activity_ids: LM_STUDIO_ACTIVITY_IDS,
        locations,
    }
}

fn cache_home(environment: &AiAppEnvironment) -> Option<PathBuf> {
    environment
        .xdg_cache_home
        .clone()
        .or_else(|| environment.home.as_ref().map(|home| home.join(".cache")))
}

fn config_home(environment: &AiAppEnvironment) -> Option<PathBuf> {
    environment
        .xdg_config_home
        .clone()
        .or_else(|| environment.home.as_ref().map(|home| home.join(".config")))
}

fn electron_locations(locations: &mut Vec<AiAppLocation>, root: &Path) {
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
    path: PathBuf,
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
    use crate::cleaner::ai_apps::definitions::AiAppEnvironment;
    use crate::cleaner::core::ai_app_provider::{AiAppPathEvidence, AiAppRole, AiAppTarget};
    use crate::paths::HostOs;

    #[test]
    fn injected_xdg_environment_resolves_only_exact_generated_children() {
        let mut environment = AiAppEnvironment::fixture(HostOs::Unix, "/home/captain");
        environment.xdg_cache_home = Some(PathBuf::from("/mnt/cache"));
        environment.xdg_config_home = Some(PathBuf::from("/mnt/config"));
        environment.ollama_models = Some(PathBuf::from("/mnt/models/ollama"));

        let apps = definitions(&environment);
        let ollama = apps.iter().find(|app| app.id == "ollama").expect("Ollama");
        assert_eq!(
            ollama.locations[0].path,
            PathBuf::from("/mnt/models/ollama")
        );
        assert_eq!(ollama.locations[0].target, AiAppTarget::DirectorySummary);

        let lm_studio = apps
            .iter()
            .find(|app| app.id == "lm-studio")
            .expect("LM Studio");
        assert!(lm_studio.locations.iter().any(|location| {
            location.path.as_path() == Path::new("/mnt/config/LM Studio/GPUCache")
                && location.target == AiAppTarget::DirectoryContents
        }));
        assert!(lm_studio.locations.iter().any(|location| {
            location.path.as_path() == Path::new("/mnt/cache/lm-studio/models")
                && location.role == AiAppRole::Models
        }));
        let cleanup_roots = crate::cleaner::ai_apps::cleanup_allowed_roots(&environment);
        assert!(!cleanup_roots.contains(&PathBuf::from("/mnt/models/ollama")));
        assert!(!cleanup_roots.contains(&PathBuf::from("/mnt/cache/lm-studio/models")));
        assert!(apps.iter().flat_map(|app| &app.locations).all(|location| {
            location.evidence == AiAppPathEvidence::Inferred
                && !(location.role.allow_cleanup()
                    && location.target == AiAppTarget::DirectorySummary)
        }));
    }
}
