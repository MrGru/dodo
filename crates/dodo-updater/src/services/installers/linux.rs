//! Replacing the Linux binary.
//!
//! The simplest of the three: the release's `linux-x64` archive is a
//! `.tar.gz` holding one `dodo`, and the install is extract, swap, relaunch —
//! the same rename-aside sequence Windows needs, used here too so there is one
//! swap and not three.
//!
//! Where the binary lives decides whether that is possible at all. A copy in
//! `~/.local/bin` is replaceable; one in `/usr/local/bin` or installed by a
//! package manager is not, without privileges dodo does not ask for and should
//! not have. Both end in [`InstallOutcome::Manual`] with the archive's path,
//! which is a complete answer: the user extracts it wherever they like, or
//! `sudo`s the copy themselves.
//!
//! # AppImage is out of scope
//!
//! Deliberately, and stated here rather than discovered. An AppImage updates
//! through its own mechanism (`AppImageUpdate`, a delta protocol keyed off an
//! embedded update-information string), and dodo publishes no AppImage. A
//! running AppImage's `current_exe` points inside a read-only FUSE mount, so
//! this installer reports [`ManualReason::ReadOnlyLocation`] and leaves the
//! archive — the correct outcome, reached without pretending to understand the
//! format.
//!
//! # Never built on Linux
//!
//! Like the Windows installer, this compiles and is tested on whatever host
//! runs the suite, and nothing in it is Linux-specific. That no dodo has ever
//! been built on Linux is recorded in `docs/release.md`.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::models::install_target::{classify, staging_dir};
use crate::models::state::{InstallOutcome, ManualReason, UpdateError};
use crate::services::PlatformInstaller;
use crate::services::installers::{extract, swap};
use crate::services::log;

/// The Linux installer. `#[allow(dead_code)]` for the reason given on
/// [`MacosInstaller`](super::macos::MacosInstaller).
#[allow(dead_code)]
#[derive(Default)]
pub struct LinuxInstaller {
    executable: Option<PathBuf>,
}

#[allow(dead_code)]
impl LinuxInstaller {
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

impl PlatformInstaller for LinuxInstaller {
    fn install(&self, archive: &Path) -> Result<InstallOutcome, UpdateError> {
        let executable = self.executable()?;
        let Some(parent) = executable.parent() else {
            return Ok(InstallOutcome::Manual {
                reason: ManualReason::ReadOnlyLocation,
                archive: archive.to_path_buf(),
            });
        };

        if let Err(reason) = swap::probe_writable(parent) {
            log::note("the install directory is not writable; leaving the archive in place");
            return Ok(InstallOutcome::Manual {
                reason,
                archive: archive.to_path_buf(),
            });
        }

        let staging = staging_dir(parent, std::process::id());
        extract::extract(archive, &staging)?;

        let new_binary = find_binary(&staging, &executable).ok_or_else(|| {
            let _ = std::fs::remove_dir_all(&staging);
            UpdateError::Install("the archive does not contain the dodo binary".to_owned())
        })?;

        let result = swap::swap(&executable, &new_binary);
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

/// Finds the replacement binary, flat or one level down, by the running
/// binary's own filename. Same rule as the Windows installer's, for the same
/// reason: it is *this* file that has to be replaced.
#[allow(dead_code)] // Reached only through this platform's installer; see `installers/mod.rs`.
fn find_binary(staging: &Path, running: &Path) -> Option<PathBuf> {
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
    use super::LinuxInstaller;
    use crate::models::state::InstallOutcome;
    use crate::services::PlatformInstaller;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn scratch() -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("dodo-linux-test-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("creates");
        dir
    }

    #[test]
    fn a_writable_location_is_replaced_in_place() {
        let root = scratch();

        let source = root.join("source");
        std::fs::create_dir_all(&source).expect("creates");
        std::fs::write(source.join("dodo"), b"new binary").expect("writes");
        let archive = root.join("dodo.tar.gz");
        assert!(
            std::process::Command::new("tar")
                .arg("-czf")
                .arg(&archive)
                .arg("-C")
                .arg(&source)
                .arg("dodo")
                .status()
                .expect("tar")
                .success()
        );

        let bin = root.join(".local/bin");
        std::fs::create_dir_all(&bin).expect("creates");
        let running = bin.join("dodo");
        std::fs::write(&running, b"old binary").expect("writes");

        assert_eq!(
            LinuxInstaller::at(running.clone())
                .install(&archive)
                .expect("installs"),
            InstallOutcome::Installed
        );
        assert_eq!(std::fs::read(&running).expect("installed"), b"new binary");

        let _ = std::fs::remove_dir_all(&root);
    }

    /// The AppImage and `/usr/local/bin` case: a location this process cannot
    /// write is a refusal that hands the user the verified archive, not a
    /// failure.
    #[test]
    fn an_unwritable_location_reports_where_the_archive_is() {
        let root = scratch();
        let archive = root.join("dodo.tar.gz");
        std::fs::write(&archive, b"never extracted").expect("writes");

        // A directory that does not exist stands in for one this process cannot
        // write to: both fail the probe, and the classification of *why* is
        // tested directly in `swap`.
        let running = root.join("nonexistent/dodo");

        let outcome = LinuxInstaller::at(running)
            .install(&archive)
            .expect("refuses cleanly rather than failing");
        assert!(
            matches!(outcome, InstallOutcome::Manual { archive: ref path, .. } if path == &archive),
            "{outcome:?}"
        );
        assert!(
            archive.exists(),
            "the verified archive is kept for the user"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_next_launch_sweeps_what_the_install_left() {
        let root = scratch();
        let running = root.join("dodo");
        std::fs::write(&running, b"current").expect("writes");
        std::fs::write(root.join("dodo.dodo-old"), b"stale").expect("writes");

        LinuxInstaller::at(running).sweep_stale();
        assert!(!root.join("dodo.dodo-old").exists());

        let _ = std::fs::remove_dir_all(&root);
    }
}
