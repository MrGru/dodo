//! The two `AiAppDefinition`s (`core::ai_app_provider`) this phase ships —
//! Ollama and LM Studio — plus Ollama's best-effort model-name extraction.
//!
//! # Why one file, not one-per-provider like `node_tooling`
//!
//! `macos::scanners::node_tooling` gives each of its six providers its own
//! file because each one implements a real trait method with genuinely
//! different environment-variable and fallback-path logic. An
//! [`AiAppDefinition`] has no behaviour at all — it is the static roots list
//! the ticket's own suggested struct describes — so two `const` definitions
//! and one small extraction helper fit comfortably in one file without the
//! module-per-provider ceremony buying anything. Adding a third provider
//! means adding one more `const ROOTS`/`fn definition()` pair here and one
//! more line in [`default_ai_apps`] — never touching
//! `macos::scanners::ai_apps`, which only ever asks for
//! `AiAppRole`/`AiAppDefinition` values, never for a provider-specific type.
//!
//! # Every macOS path below this phase claims with confidence, and the ones
//! it does not
//!
//! See `docs/cleaner/known-limitations.md` for the full accounting. In
//! short: `~/.ollama/models`, `~/Library/Logs/Ollama`,
//! `~/Library/Caches/Ollama` and `~/Library/Application Support/Ollama` are
//! asserted with confidence; both of LM Studio's candidate model directories
//! (`~/.cache/lm-studio/models` and `~/Library/Application Support/LM
//! Studio/models`) are checked defensively rather than asserted, because
//! which one — if either — is current for a given LM Studio version is not
//! confidently known here. Neither app registers a `TemporaryDownloads` or
//! `ChatHistory` root at all: there is no version-stable, confidently-known
//! on-disk convention for either sub-category for either app, the same
//! "report nothing rather than guess at a layout" posture Phase 11's Nub
//! provider took (`node_tooling::nub`). The `AiAppRole` variants and their
//! `NeverBulkSelect`/scan-only enforcement exist and are unit-tested
//! (`macos::scanners::ai_apps`'s tests use a synthetic definition with a
//! `ChatHistory` root, exactly as Node Tooling's tests use a `StubProvider`
//! rather than a real one) so a future session that *does* know a real path
//! only has to add it here.
//!
//! Both apps' exact macOS bundle identifiers are also not confidently known;
//! see [`OLLAMA_BUNDLE_IDS`] and [`LM_STUDIO_BUNDLE_IDS`].

use std::path::Path;

use crate::cleaner::core::ai_app_provider::{AiAppDefinition, AiAppRole, AiAppRoot};

/// Unverified guesses, both listed so a wrong first guess does not silently
/// disable the running-process warning — see `core::ai_app_provider`'s doc
/// comment on why the definition carries more than one candidate. If
/// neither matches Ollama's real bundle identifier, `AiAppRole::Models`
/// items simply never get the "app is running" warning; nothing in this
/// phase depends on the check succeeding for correctness, only for a nicer
/// warning.
const OLLAMA_BUNDLE_IDS: &[&str] = &["com.ollama.ollama", "ai.ollama.ollama"];

const OLLAMA_ROOTS: &[AiAppRoot] = &[
    AiAppRoot {
        role: AiAppRole::Models,
        path: "~/.ollama/models",
        group: "Ollama models",
    },
    AiAppRoot {
        role: AiAppRole::ApplicationSupport,
        path: "~/Library/Application Support/Ollama",
        group: "Ollama application support",
    },
    AiAppRoot {
        role: AiAppRole::Logs,
        path: "~/Library/Logs/Ollama",
        group: "Ollama logs",
    },
    AiAppRoot {
        role: AiAppRole::Cache,
        path: "~/Library/Caches/Ollama",
        group: "Ollama cache",
    },
];

pub(crate) fn ollama_definition() -> AiAppDefinition {
    AiAppDefinition {
        id: "ollama",
        display_name: "Ollama",
        bundle_ids: OLLAMA_BUNDLE_IDS,
        roots: OLLAMA_ROOTS,
    }
}

/// Same hedge as [`OLLAMA_BUNDLE_IDS`] — LM Studio is Electron-based, and
/// neither candidate here has been confirmed against a real installation.
const LM_STUDIO_BUNDLE_IDS: &[&str] = &["com.lmstudio.LMStudio", "com.electron.lmstudio"];

const LM_STUDIO_ROOTS: &[AiAppRoot] = &[
    // Both candidate model directories are registered; whichever does not
    // exist on a given machine is simply skipped as a missing root
    // (`ScanError::RootUnavailable`, folded into `ScanCompleteness::Partial`
    // the same way every other scanner here treats an absent optional root
    // — never an error). See this module's doc comment.
    AiAppRoot {
        role: AiAppRole::Models,
        path: "~/.cache/lm-studio/models",
        group: "LM Studio models",
    },
    AiAppRoot {
        role: AiAppRole::Models,
        path: "~/Library/Application Support/LM Studio/models",
        group: "LM Studio models",
    },
    AiAppRoot {
        role: AiAppRole::ApplicationSupport,
        path: "~/Library/Application Support/LM Studio",
        group: "LM Studio application support",
    },
    AiAppRoot {
        role: AiAppRole::Logs,
        path: "~/Library/Application Support/LM Studio/logs",
        group: "LM Studio logs",
    },
    AiAppRoot {
        role: AiAppRole::Cache,
        path: "~/Library/Caches/LM Studio",
        group: "LM Studio cache",
    },
];

pub(crate) fn lm_studio_definition() -> AiAppDefinition {
    AiAppDefinition {
        id: "lm-studio",
        display_name: "LM Studio",
        bundle_ids: LM_STUDIO_BUNDLE_IDS,
        roots: LM_STUDIO_ROOTS,
    }
}

/// One instance per app, in a fixed, deterministic order — the same "add one
/// definition here" seam `node_tooling::default_providers` uses for Node
/// Tooling Cache.
pub(crate) fn default_ai_apps() -> Vec<AiAppDefinition> {
    vec![ollama_definition(), lm_studio_definition()]
}

/// Best-effort model name extraction from Ollama's manifest directory tree:
/// `<manifests_dir>/<registry-host>/<namespace>/<model>/<tag>`, where the
/// `tag` component is a small JSON manifest *file*, never a directory. Only
/// directory and file *names* are ever read — the manifest file's own JSON
/// content (which lists the model's blob digests) is never opened, matching
/// the ticket's "do not inspect model content" for this display-only aid.
///
/// The `library` namespace (Ollama's default namespace for official,
/// first-party models) is dropped from the display name, matching how
/// `ollama list` itself shows official models as `<model>:<tag>` rather than
/// `library/<model>:<tag>`.
///
/// Tolerant of anything that does not match the expected four-level shape —
/// a missing or unreadable directory at any level, a non-directory where a
/// directory was expected, a non-file where the tag manifest was expected —
/// by simply omitting that branch rather than erroring: this is a display
/// nicety, never the thing that decides what gets scanned or cleaned, so a
/// partial or unexpected layout degrades to fewer names rather than a scan
/// failure.
pub(crate) fn collect_ollama_model_names(manifests_dir: &Path) -> Vec<String> {
    let mut names = Vec::new();
    let Ok(hosts) = std::fs::read_dir(manifests_dir) else {
        return names;
    };
    for host in hosts.flatten() {
        if !host.file_type().is_ok_and(|file_type| file_type.is_dir()) {
            continue;
        }
        let Ok(namespaces) = std::fs::read_dir(host.path()) else {
            continue;
        };
        for namespace in namespaces.flatten() {
            if !namespace
                .file_type()
                .is_ok_and(|file_type| file_type.is_dir())
            {
                continue;
            }
            let namespace_name = namespace.file_name().to_string_lossy().into_owned();
            let Ok(models) = std::fs::read_dir(namespace.path()) else {
                continue;
            };
            for model in models.flatten() {
                if !model.file_type().is_ok_and(|file_type| file_type.is_dir()) {
                    continue;
                }
                let model_name = model.file_name().to_string_lossy().into_owned();
                let Ok(tags) = std::fs::read_dir(model.path()) else {
                    continue;
                };
                for tag in tags.flatten() {
                    if !tag.file_type().is_ok_and(|file_type| file_type.is_file()) {
                        continue;
                    }
                    let tag_name = tag.file_name().to_string_lossy().into_owned();
                    names.push(if namespace_name == "library" {
                        format!("{model_name}:{tag_name}")
                    } else {
                        format!("{namespace_name}/{model_name}:{tag_name}")
                    });
                }
            }
        }
    }
    names.sort();
    names
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn default_ai_apps_registers_ollama_and_lm_studio() {
        let apps = default_ai_apps();
        assert_eq!(apps.len(), 2);
        assert!(apps.iter().any(|app| app.id == "ollama"));
        assert!(apps.iter().any(|app| app.id == "lm-studio"));
    }

    #[test]
    fn every_registered_root_path_is_home_relative_or_absolute() {
        for app in default_ai_apps() {
            for root in app.roots {
                assert!(
                    root.path.starts_with("~/") || root.path.starts_with('/'),
                    "{} root {:?} must be `~/`-relative or absolute",
                    app.id,
                    root.path
                );
            }
        }
    }

    #[test]
    fn neither_provider_registers_a_temporary_downloads_or_chat_history_root() {
        for app in default_ai_apps() {
            assert!(
                app.roots
                    .iter()
                    .all(|root| root.role != AiAppRole::ChatHistory),
                "{} must not register a ChatHistory root this phase — see this module's doc comment",
                app.id
            );
            assert!(
                app.roots
                    .iter()
                    .all(|root| root.role != AiAppRole::TemporaryDownloads),
                "{} must not register a TemporaryDownloads root this phase",
                app.id
            );
        }
    }

    #[test]
    fn ollama_registers_a_distinct_models_root_never_merged_with_cache() {
        let ollama = ollama_definition();
        let models_root = ollama
            .roots
            .iter()
            .find(|root| root.role == AiAppRole::Models)
            .expect("ollama registers a Models root");
        let cache_root = ollama
            .roots
            .iter()
            .find(|root| root.role == AiAppRole::Cache)
            .expect("ollama registers a Cache root");
        assert_ne!(models_root.group, cache_root.group);
        assert_ne!(models_root.path, cache_root.path);
    }

    #[test]
    fn collect_ollama_model_names_reads_the_manifest_tree_structure_only() {
        let temp = std::env::temp_dir().join(format!(
            "dodo-cleaner-ollama-manifests-{}",
            std::process::id()
        ));
        let manifests = temp.join("manifests");
        let official = manifests
            .join("registry.ollama.ai")
            .join("library")
            .join("llama3");
        fs::create_dir_all(&official).expect("creates official model dir");
        fs::write(official.join("8b"), b"{\"not\":\"read\"}").expect("writes tag manifest");

        let published = manifests
            .join("registry.ollama.ai")
            .join("someuser")
            .join("custom-model");
        fs::create_dir_all(&published).expect("creates published model dir");
        fs::write(published.join("latest"), b"{}").expect("writes tag manifest");

        let names = collect_ollama_model_names(&manifests);
        assert_eq!(
            names,
            vec![
                "llama3:8b".to_string(),
                "someuser/custom-model:latest".to_string(),
            ]
        );

        fs::remove_dir_all(&temp).expect("removes temp tree");
    }

    #[test]
    fn collect_ollama_model_names_is_empty_for_a_missing_manifests_dir() {
        let temp = std::env::temp_dir().join(format!(
            "dodo-cleaner-ollama-manifests-missing-{}",
            std::process::id()
        ));
        assert!(collect_ollama_model_names(&temp).is_empty());
    }
}
