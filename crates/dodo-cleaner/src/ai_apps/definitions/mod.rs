//! Per-platform Ollama and LM Studio locations.
//!
//! These modules are intentionally compiled and tested on every host. The
//! Windows and Linux tables are inferred from platform convention and marked
//! as such on every location; edit only the matching module after a captain
//! captures a real installation. macOS preserves the existing table and marks
//! its already-documented uncertain LM Studio candidates as inferred.

pub(crate) mod linux;
pub(crate) mod macos;
pub(crate) mod windows;

use std::path::{Path, PathBuf};

use crate::core::ai_app_provider::AiAppDefinition;
use crate::core::safety::is_absolute_path;
use crate::paths::HostOs;

#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct AiAppEnvironment {
    pub host: HostOs,
    pub home: Option<PathBuf>,
    pub ollama_models: Option<PathBuf>,
    pub local_app_data: Option<PathBuf>,
    pub roaming_app_data: Option<PathBuf>,
    pub xdg_cache_home: Option<PathBuf>,
    pub xdg_config_home: Option<PathBuf>,
}

impl AiAppEnvironment {
    #[cfg(test)]
    pub(crate) fn fixture(host: HostOs, home: impl Into<PathBuf>) -> Self {
        Self {
            host,
            home: Some(home.into()),
            ollama_models: None,
            local_app_data: None,
            roaming_app_data: None,
            xdg_cache_home: None,
            xdg_config_home: None,
        }
    }
}

pub(crate) fn snapshot_environment(host: HostOs, home: Option<&Path>) -> AiAppEnvironment {
    let path_var = |name| {
        std::env::var_os(name)
            .map(PathBuf::from)
            .filter(|path| is_absolute_path(host, path))
    };
    AiAppEnvironment {
        host,
        home: home.map(Path::to_path_buf),
        ollama_models: path_var("OLLAMA_MODELS"),
        local_app_data: path_var("LOCALAPPDATA"),
        roaming_app_data: path_var("APPDATA"),
        xdg_cache_home: path_var("XDG_CACHE_HOME"),
        xdg_config_home: path_var("XDG_CONFIG_HOME"),
    }
}

pub(crate) fn default_ai_apps(environment: &AiAppEnvironment) -> Vec<AiAppDefinition> {
    match environment.host {
        HostOs::MacOs => macos::definitions(environment),
        HostOs::Windows => windows::definitions(environment),
        HostOs::Unix => linux::definitions(environment),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{AiAppEnvironment, default_ai_apps};
    use crate::core::ai_app_provider::{AiAppRole, AiAppTarget};
    use crate::core::safety::is_absolute_path;
    use crate::paths::HostOs;

    fn environments() -> [AiAppEnvironment; 3] {
        let macos = AiAppEnvironment::fixture(HostOs::MacOs, "/Users/captain");

        let mut windows = AiAppEnvironment::fixture(HostOs::Windows, r"C:\Users\captain");
        windows.local_app_data = Some(PathBuf::from(r"C:\Users\captain\AppData\Local"));
        windows.roaming_app_data = Some(PathBuf::from(r"C:\Users\captain\AppData\Roaming"));

        let mut linux = AiAppEnvironment::fixture(HostOs::Unix, "/home/captain");
        linux.xdg_cache_home = Some(PathBuf::from("/home/captain/.cache"));
        linux.xdg_config_home = Some(PathBuf::from("/home/captain/.config"));

        [macos, windows, linux]
    }

    #[test]
    fn default_ai_apps_registers_ollama_and_lm_studio() {
        for environment in environments() {
            let apps = default_ai_apps(&environment);
            assert_eq!(apps.len(), 2);
            assert!(apps.iter().any(|app| app.id == "ollama"));
            assert!(apps.iter().any(|app| app.id == "lm-studio"));
        }
    }

    #[test]
    fn every_registered_root_path_is_home_relative_or_absolute() {
        for environment in environments() {
            for location in default_ai_apps(&environment)
                .into_iter()
                .flat_map(|app| app.locations)
            {
                assert!(
                    is_absolute_path(environment.host, &location.path),
                    "{:?} location {:?} must resolve to an absolute path",
                    environment.host,
                    location.path
                );
            }
        }
    }

    #[test]
    fn neither_provider_registers_a_temporary_downloads_or_chat_history_root() {
        for environment in environments() {
            for app in default_ai_apps(&environment) {
                assert!(
                    app.locations.iter().all(|location| {
                        !matches!(
                            location.role,
                            AiAppRole::TemporaryDownloads | AiAppRole::ChatHistory
                        )
                    }),
                    "{} must not guess at temporary-download or chat-history paths",
                    app.id
                );
            }
        }
    }

    #[test]
    fn ollama_registers_a_distinct_models_root_never_merged_with_cache() {
        let environment = AiAppEnvironment::fixture(HostOs::MacOs, "/Users/captain");
        let ollama = default_ai_apps(&environment)
            .into_iter()
            .find(|app| app.id == "ollama")
            .expect("Ollama");
        let models = ollama
            .locations
            .iter()
            .find(|location| location.role == AiAppRole::Models)
            .expect("Ollama models");
        let cache = ollama
            .locations
            .iter()
            .find(|location| location.role == AiAppRole::Cache)
            .expect("Ollama cache");

        assert_ne!(models.path, cache.path);
        assert_ne!(models.group, cache.group);
        assert_eq!(models.target, AiAppTarget::DirectorySummary);
        assert_eq!(cache.target, AiAppTarget::DirectoryContents);
    }
}
