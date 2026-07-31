//! Replacing `dodo.exe`, which is running.
//!
//! # The rename trick
//!
//! Windows will not let you delete or overwrite an executable that is running,
//! and it *will* let you rename one: the file keeps its identity, the running
//! process keeps its handle, and the path it used to occupy is free. So:
//!
//! 1. extract the new `dodo.exe` beside the running one;
//! 2. rename the running one to `dodo.exe.dodo-old`;
//! 3. rename the new one into `dodo.exe`;
//! 4. relaunch, and quit.
//!
//! Steps 2 and 3 are [`swap::swap`](super::swap::swap), which rolls back if
//! step 3 fails.
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
//! # Never built on Windows
//!
//! `docs/release.md` records that no part of dodo has ever run on a Windows
//! host. This module compiles on every platform (that is the point — see
//! [`MacosInstaller`](super::macos::MacosInstaller)'s note) and its swap,
//! sweep and refusal paths are exercised by the tests here on whatever host
//! runs them, because nothing in it is a Windows API. What is *unverified* is
//! the behaviour of the real `rename`-while-running on a real Windows, and that
//! is stated in `docs/release.md` rather than assumed away.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::updater::models::install_target::{classify, staging_dir};
use crate::updater::models::state::{InstallOutcome, ManualReason, UpdateError};
use crate::updater::services::PlatformInstaller;
use crate::updater::services::installers::{extract, swap};
use crate::updater::services::log;

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

        let result = swap::swap(&executable, &new_exe);
        let _ = std::fs::remove_dir_all(&staging);
        result?;

        log::note("installed; a restart will run the new version");
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
        }
    }
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
    use super::{WindowsInstaller, find_executable};
    use crate::updater::models::state::InstallOutcome;
    use crate::updater::services::PlatformInstaller;
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
    fn the_running_executable_is_renamed_aside_and_replaced() {
        let root = scratch();

        let source = root.join("source");
        std::fs::create_dir_all(&source).expect("creates");
        std::fs::write(source.join("dodo.exe"), b"new binary").expect("writes");
        let archive = root.join("dodo.tar.gz");
        pack_flat(&source, "dodo.exe", &archive);

        let installed = root.join("Program Files/dodo");
        std::fs::create_dir_all(&installed).expect("creates");
        let running = installed.join("dodo.exe");
        std::fs::write(&running, b"old binary").expect("writes");

        let outcome = WindowsInstaller::at(running.clone())
            .install(&archive)
            .expect("installs");

        assert_eq!(outcome, InstallOutcome::Installed);
        assert_eq!(std::fs::read(&running).expect("installed"), b"new binary");
        assert_eq!(
            std::fs::read(installed.join("dodo.exe.dodo-old")).expect("aside"),
            b"old binary",
            "the running file cannot be deleted, only renamed — so it has to still be there"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_next_launch_deletes_what_the_install_left_behind() {
        let root = scratch();
        let running = root.join("dodo.exe");
        std::fs::write(&running, b"current").expect("writes");
        std::fs::write(root.join("dodo.exe.dodo-old"), b"stale").expect("writes");

        WindowsInstaller::at(running.clone()).sweep_stale();

        assert!(!root.join("dodo.exe.dodo-old").exists());
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
