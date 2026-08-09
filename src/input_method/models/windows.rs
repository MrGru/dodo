//! Pure install paths and outcomes for the Windows TSF DLL.
#![cfg_attr(
    not(target_os = "windows"),
    allow(
        dead_code,
        reason = "Windows-only installer data is unit-tested on every host."
    )
)]
//!
//! The DLL is packaged beside `dodo.exe`, copied into dodo's per-user data
//! directory, then registered by its standard `DllRegisterServer` entry point.
//! Nothing requires administrator rights: COM registration is under HKCU.

use std::path::{Path, PathBuf};

use dodo_ime_ipc::tsf::{DLL_NAME, PACKAGE_DIRECTORY};

/// The current action and its last outcome.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum WindowsInstall {
    #[default]
    Idle,
    Installing,
    Uninstalling,
    Done(WindowsInstallOutcome),
}

/// What a Windows installation action achieved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WindowsInstallOutcome {
    Ready,
    Removed,
    Failed(WindowsInstallFailure),
}

/// Why a Windows installation action could not finish.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WindowsInstallFailure {
    NoSourceDll,
    Copy { detail: String },
    Register { detail: String },
    Unregister { detail: String },
}

/// Where the packaged server is expected and where it is installed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowsInstallPlan {
    pub source: PathBuf,
    pub destination: PathBuf,
}

/// Candidate artifacts, in shipping then development order.
///
/// The shipping ZIP places the DLL under `input-method` beside `dodo.exe`. A
/// bare `cargo run` / `cargo run --release` puts both artifacts in its target
/// profile directory, then the two conventional working-tree paths cover a
/// launcher whose executable is elsewhere.
pub fn source_candidates(executable: &Path, working_directory: &Path) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(directory) = executable.parent() {
        candidates.push(directory.join(PACKAGE_DIRECTORY).join(DLL_NAME));
        candidates.push(directory.join(DLL_NAME));
    }
    for profile in ["debug", "release"] {
        candidates.push(
            working_directory
                .join("target")
                .join(profile)
                .join(DLL_NAME),
        );
    }
    candidates
}

/// The per-user copy dodo registers with Windows.
pub fn installed_dll(data_dir: &Path) -> PathBuf {
    data_dir.join(PACKAGE_DIRECTORY).join(DLL_NAME)
}

/// The HKCU COM key that `DllRegisterServer` owns.
pub fn registration_key() -> String {
    format!(
        "Software\\Classes\\CLSID\\{}\\InprocServer32",
        dodo_ime_ipc::tsf::CLSID
    )
}

#[cfg(test)]
mod tests {
    use super::{WindowsInstallPlan, installed_dll, registration_key, source_candidates};
    use dodo_ime_ipc::tsf::{DLL_NAME, PACKAGE_DIRECTORY};
    use std::path::{Path, PathBuf};

    #[test]
    fn packaged_and_development_builds_have_unambiguous_dll_paths() {
        let candidates = source_candidates(
            Path::new("C:/Program Files/Dodo/dodo.exe"),
            Path::new("C:/repo"),
        );
        assert_eq!(
            candidates,
            vec![
                PathBuf::from(format!(
                    "C:/Program Files/Dodo/{PACKAGE_DIRECTORY}/{DLL_NAME}"
                )),
                PathBuf::from(format!("C:/Program Files/Dodo/{DLL_NAME}")),
                PathBuf::from(format!("C:/repo/target/debug/{DLL_NAME}")),
                PathBuf::from(format!("C:/repo/target/release/{DLL_NAME}")),
            ]
        );
        assert_eq!(
            installed_dll(Path::new("C:/Users/someone/AppData/Roaming/dodo")),
            PathBuf::from(format!(
                "C:/Users/someone/AppData/Roaming/dodo/{PACKAGE_DIRECTORY}/{DLL_NAME}"
            ))
        );
    }

    #[test]
    fn status_uses_the_same_per_user_com_key_as_registration() {
        assert!(registration_key().starts_with("Software\\Classes\\CLSID\\{"));
        assert!(registration_key().ends_with("\\InprocServer32"));
        let plan = WindowsInstallPlan {
            source: PathBuf::from(DLL_NAME),
            destination: PathBuf::from(DLL_NAME),
        };
        assert_eq!(plan.source, plan.destination);
    }
}
