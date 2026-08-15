//! Replacing `dodo.exe`, which is running.
//!
//! # The rename trick
//!
//! Windows will not let you delete or overwrite an executable that is running,
//! and it *will* let you rename one: the file keeps its identity, the running
//! process keeps its handle, and the path it used to occupy is free. So:
//!
//! 1. extract and validate `dodo.exe` and `input-method/dodo_ime_windows.dll`;
//! 2. replace the packaged sidecar beside the running executable;
//! 3. rename the running executable to `dodo.exe.dodo-old`;
//! 4. rename the new executable into `dodo.exe`;
//! 5. relaunch, and quit.
//!
//! Each replacement uses [`swap::swap`](super::swap::swap). If the executable
//! swap fails, the sidecar replacement is rolled back too. The separately
//! registered `%APPDATA%` DLL is deliberately untouched; Install/Reinstall owns
//! registration.
//!
//! # "Schedule the stale file for deletion"
//!
//! The scheduling is [`PlatformInstaller::sweep_stale`], run at every startup:
//! the *next* launch deletes `dodo.exe.dodo-old`, by which time nothing holds
//! it open. The alternative is `MoveFileEx` with `MOVEFILE_DELAY_UNTIL_REBOOT`,
//! which needs a Win32 call and, more to the point, defers the cleanup to a
//! reboot rather than to the next run of the program that made the mess. The
//! sweep is a plain `remove_file` and works identically on all three platforms,
//! which is why the other two use it too.
//!
//! # Windows evidence boundary
//!
//! Release compilation, packaging and `dodo.exe --build-info` have run on a
//! Windows runner. This module still has not performed an update on Windows.
//! It compiles on every platform (that is the point — see
//! [`MacosInstaller`](super::macos::MacosInstaller)'s note) and its swap,
//! sweep and refusal paths are exercised by the tests here because nothing in
//! it is a Windows API. The real `rename`-while-running and two-file rollback
//! remain captain runtime checks; `docs/release.md` records that boundary.

use std::path::{Path, PathBuf};
use std::process::Command;

use dodo_ime_ipc::tsf::{DLL_NAME, PACKAGE_DIRECTORY};

use crate::models::install_target::{classify, staging_dir};
use crate::models::state::{InstallOutcome, ManualReason, UpdateError};
use crate::services::PlatformInstaller;
use crate::services::installers::{extract, swap};
use crate::services::log;

/// The Windows installer. `#[allow(dead_code)]` for the reason given on
/// [`MacosInstaller`](super::macos::MacosInstaller).
#[allow(dead_code)]
#[derive(Default)]
pub struct WindowsInstaller {
    executable: Option<PathBuf>,
}

#[allow(dead_code)]
impl WindowsInstaller {
    pub fn new() -> Self {
        Self::default()
    }

    #[cfg(test)]
    pub fn at(executable: PathBuf) -> Self {
        Self {
            executable: Some(executable),
        }
    }

    fn executable(&self) -> Result<PathBuf, UpdateError> {
        match &self.executable {
            Some(path) => Ok(path.clone()),
            None => std::env::current_exe()
                .map_err(|err| UpdateError::Install(format!("current_exe: {err}"))),
        }
    }
}

impl PlatformInstaller for WindowsInstaller {
    fn install(&self, archive: &Path) -> Result<InstallOutcome, UpdateError> {
        let executable = self.executable()?;
        let Some(parent) = executable.parent() else {
            return Ok(InstallOutcome::Manual {
                reason: ManualReason::ReadOnlyLocation,
                archive: archive.to_path_buf(),
            });
        };

        // A default install under `C:\Program Files` is not writable by a
        // normal user, and that is the common case rather than an exotic one.
        if let Err(reason) = swap::probe_writable(parent) {
            log::note("the install directory is not writable; leaving the archive in place");
            return Ok(InstallOutcome::Manual {
                reason,
                archive: archive.to_path_buf(),
            });
        }

        let staging = staging_dir(parent, std::process::id());
        extract::extract(archive, &staging)?;

        let new_exe = find_executable(&staging, &executable).ok_or_else(|| {
            let _ = std::fs::remove_dir_all(&staging);
            UpdateError::Install("the archive does not contain dodo.exe".to_owned())
        })?;
        let new_dll = new_exe
            .parent()
            .map(|root| root.join(PACKAGE_DIRECTORY).join(DLL_NAME))
            .filter(|path| path.is_file())
            .ok_or_else(|| {
                let _ = std::fs::remove_dir_all(&staging);
                UpdateError::Install(format!(
                    "the archive does not contain {PACKAGE_DIRECTORY}/{DLL_NAME}"
                ))
            })?;

        // Both payloads are known-good before the installed files are touched.
        // This only updates the packaged sidecar; registration of the separate
        // %APPDATA% copy remains the explicit Install/Reinstall action.
        let dll_dir = parent.join(PACKAGE_DIRECTORY);
        let created_dll_dir = !dll_dir.exists();
        if let Err(err) = std::fs::create_dir_all(&dll_dir) {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(UpdateError::Io(format!("{}: {err}", dll_dir.display())));
        }
        let installed_dll = dll_dir.join(DLL_NAME);
        let old_dll = match swap::swap(&installed_dll, &new_dll) {
            Ok(old) => old,
            Err(error) => {
                if created_dll_dir {
                    let _ = std::fs::remove_dir(&dll_dir);
                }
                let _ = std::fs::remove_dir_all(&staging);
                return Err(error);
            }
        };

        if let Err(error) = swap::swap(&executable, &new_exe) {
            let rollback = restore_sidecar(&installed_dll, &old_dll);
            if created_dll_dir {
                let _ = std::fs::remove_dir(&dll_dir);
            }
            let _ = std::fs::remove_dir_all(&staging);
            return match rollback {
                Ok(()) => Err(error),
                Err(rollback) => Err(UpdateError::Install(format!(
                    "{error:?}; the TSF sidecar could not be restored: {rollback:?}"
                ))),
            };
        }

        let _ = std::fs::remove_dir_all(&staging);
        log::note("installed executable and TSF sidecar; a restart will run the new version");
        Ok(InstallOutcome::Installed)
    }

    fn relaunch(&self) -> Result<(), UpdateError> {
        let executable = self.executable()?;
        Command::new(&executable)
            .spawn()
            .map(|_| ())
            .map_err(|err| UpdateError::Install(format!("could not relaunch: {err}")))
    }

    fn sweep_stale(&self) {
        let Ok(executable) = self.executable() else {
            return;
        };
        if let Some(parent) = classify(&executable).writable_parent() {
            swap::sweep(parent);
            swap::sweep(&parent.join(PACKAGE_DIRECTORY));
        }
    }
}

/// Restores the packaged sidecar after a later executable swap fails.
#[allow(dead_code)] // Reached only through this platform's installer; see `installers/mod.rs`.
fn restore_sidecar(installed: &Path, old: &Path) -> Result<(), UpdateError> {
    if let Err(err) = std::fs::remove_file(installed)
        && err.kind() != std::io::ErrorKind::NotFound
    {
        return Err(UpdateError::Install(format!(
            "could not remove the new TSF sidecar {}: {err}",
            installed.display()
        )));
    }
    if old.exists() {
        std::fs::rename(old, installed).map_err(|err| {
            UpdateError::Install(format!(
                "the previous TSF sidecar remains at {}: {err}",
                old.display()
            ))
        })?;
    }
    Ok(())
}

/// Finds the new executable in an extracted archive.
///
/// The Windows archive is flat — `scripts/package.ps1` zips `dodo.exe` at the
/// top level — but a future archive might nest it under a directory, so one
/// level down is searched as well. Matching is by **the running executable's own
/// filename**, which is what has to be replaced whatever it is called.
#[allow(dead_code)] // Reached only through this platform's installer; see `installers/mod.rs`.
fn find_executable(staging: &Path, running: &Path) -> Option<PathBuf> {
    let wanted = running.file_name()?;

    let direct = staging.join(wanted);
    if direct.is_file() {
        return Some(direct);
    }

    for entry in std::fs::read_dir(staging).ok()?.flatten() {
        let nested = entry.path().join(wanted);
        if nested.is_file() {
            return Some(nested);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{WindowsInstaller, find_executable, restore_sidecar};
    use crate::models::state::InstallOutcome;
    use crate::services::PlatformInstaller;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    fn scratch() -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("dodo-windows-test-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("creates");
        dir
    }

    fn pack_flat(dir: &Path, name: &str, into: &Path) {
        assert!(
            std::process::Command::new("tar")
                .arg("-czf")
                .arg(into)
                .arg("-C")
                .arg(dir)
                .arg(name)
                .status()
                .expect("tar")
                .success()
        );
    }

    /// The rename-aside install, run for real on this host. What it proves is
    /// the *sequence* — the running file is moved aside, the new one takes its
    /// path, the old one survives for the sweep — which is all pure `std::fs`.
    /// Whether Windows permits the rename of a genuinely running `.exe` is not
    /// something this host can answer; `docs/release.md` says so.
    #[test]
    fn the_executable_and_sidecar_are_replaced_and_staging_is_removed() {
        let root = scratch();

        let source = root.join("source/dodo-v9.9.9-windows-x64");
        std::fs::create_dir_all(source.join("input-method")).expect("creates");
        std::fs::write(source.join("dodo.exe"), b"new binary").expect("writes");
        std::fs::write(source.join("input-method/dodo_ime_windows.dll"), b"new dll")
            .expect("writes");
        let archive = root.join("dodo.tar.gz");
        pack_flat(
            source.parent().expect("archive root"),
            "dodo-v9.9.9-windows-x64",
            &archive,
        );

        let installed = root.join("Program Files/dodo");
        std::fs::create_dir_all(installed.join("input-method")).expect("creates");
        let running = installed.join("dodo.exe");
        std::fs::write(&running, b"old binary").expect("writes");
        std::fs::write(
            installed.join("input-method/dodo_ime_windows.dll"),
            b"old dll",
        )
        .expect("writes");

        let outcome = WindowsInstaller::at(running.clone())
            .install(&archive)
            .expect("installs");

        assert_eq!(outcome, InstallOutcome::Installed);
        assert_eq!(std::fs::read(&running).expect("installed"), b"new binary");
        assert_eq!(
            std::fs::read(installed.join("input-method/dodo_ime_windows.dll")).expect("installed"),
            b"new dll"
        );
        assert_eq!(
            std::fs::read(installed.join("dodo.exe.dodo-old")).expect("aside"),
            b"old binary",
            "the running file cannot be deleted, only renamed — so it has to still be there"
        );
        assert_eq!(
            std::fs::read(installed.join("input-method/dodo_ime_windows.dll.dodo-old"))
                .expect("aside"),
            b"old dll",
            "both previous payloads remain available for rollback until the next launch"
        );
        assert!(
            !installed
                .join(format!(".dodo-update-{}", std::process::id()))
                .exists(),
            "staging must be removed only after both payloads survive"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn restoring_the_sidecar_reinstates_the_previous_bytes() {
        let root = scratch();
        let installed = root.join("dodo_ime_windows.dll");
        let old = root.join("dodo_ime_windows.dll.dodo-old");
        std::fs::write(&installed, b"new").expect("writes");
        std::fs::write(&old, b"old").expect("writes");

        restore_sidecar(&installed, &old).expect("restores");

        assert_eq!(std::fs::read(&installed).expect("restored"), b"old");
        assert!(!old.exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_next_launch_deletes_what_the_install_left_behind() {
        let root = scratch();
        let running = root.join("dodo.exe");
        std::fs::write(&running, b"current").expect("writes");
        std::fs::write(root.join("dodo.exe.dodo-old"), b"stale").expect("writes");
        std::fs::create_dir_all(root.join("input-method")).expect("creates");
        std::fs::write(
            root.join("input-method/dodo_ime_windows.dll.dodo-old"),
            b"stale",
        )
        .expect("writes");

        WindowsInstaller::at(running.clone()).sweep_stale();

        assert!(!root.join("dodo.exe.dodo-old").exists());
        assert!(
            !root
                .join("input-method/dodo_ime_windows.dll.dodo-old")
                .exists()
        );
        assert!(running.exists());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_archive_without_the_executable_is_a_real_failure() {
        let root = scratch();
        let source = root.join("source");
        std::fs::create_dir_all(&source).expect("creates");
        std::fs::write(source.join("README.txt"), b"nothing useful").expect("writes");
        let archive = root.join("dodo.tar.gz");
        pack_flat(&source, "README.txt", &archive);

        let running = root.join("dodo.exe");
        std::fs::write(&running, b"old binary").expect("writes");

        let error = WindowsInstaller::at(running.clone())
            .install(&archive)
            .expect_err("nothing to install");
        assert!(format!("{error:?}").contains("dodo.exe"), "{error:?}");
        assert_eq!(
            std::fs::read(&running).expect("untouched"),
            b"old binary",
            "a failed install must leave the installation alone"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_missing_sidecar_fails_before_the_executable_is_touched() {
        let root = scratch();
        let source = root.join("source");
        std::fs::create_dir_all(&source).expect("creates");
        std::fs::write(source.join("dodo.exe"), b"new binary").expect("writes");
        let archive = root.join("dodo.tar.gz");
        pack_flat(&source, "dodo.exe", &archive);

        let running = root.join("dodo.exe");
        std::fs::write(&running, b"old binary").expect("writes");
        let error = WindowsInstaller::at(running.clone())
            .install(&archive)
            .expect_err("the sidecar is required");

        assert!(
            format!("{error:?}").contains("input-method/dodo_ime_windows.dll"),
            "{error:?}"
        );
        assert_eq!(std::fs::read(&running).expect("untouched"), b"old binary");
        assert!(
            !root
                .join(format!(".dodo-update-{}", std::process::id()))
                .exists()
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_executable_is_found_flat_or_one_level_down() {
        let root = scratch();
        let running = root.join("dodo.exe");

        let flat = root.join("flat");
        std::fs::create_dir_all(&flat).expect("creates");
        std::fs::write(flat.join("dodo.exe"), b"x").expect("writes");
        assert_eq!(
            find_executable(&flat, &running),
            Some(flat.join("dodo.exe"))
        );

        let nested = root.join("nested");
        std::fs::create_dir_all(nested.join("dodo-v9.9.9")).expect("creates");
        std::fs::write(nested.join("dodo-v9.9.9/dodo.exe"), b"x").expect("writes");
        assert_eq!(
            find_executable(&nested, &running),
            Some(nested.join("dodo-v9.9.9/dodo.exe"))
        );

        let empty = root.join("empty");
        std::fs::create_dir_all(&empty).expect("creates");
        assert_eq!(find_executable(&empty, &running), None);

        let _ = std::fs::remove_dir_all(&root);
    }

    /// The archive holds `dodo.exe`; a user who renamed their copy still gets
    /// *their* file replaced, not a second one created beside it.
    #[test]
    fn a_renamed_installation_is_matched_by_its_own_filename() {
        let root = scratch();
        std::fs::write(root.join("dodo.exe"), b"x").expect("writes");
        assert_eq!(
            find_executable(&root, &PathBuf::from("/wherever/dodo-beta.exe")),
            None,
            "matching by the archive's name instead would replace the wrong file"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
