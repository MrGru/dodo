//! Replacing `dodo.app`.
//!
//! The manifest's macOS entry points at the release's `-app.tar.gz`, which
//! contains one `dodo.app` — `docs/release.md` records why (an installer swaps
//! the bundle, not the bare binary). So the sequence is: extract beside the
//! installed bundle, strip the quarantine attribute, swap, relaunch.
//!
//! # The quarantine attribute
//!
//! dodo's binaries are **not code-signed or notarised** (`docs/release.md`), so
//! anything downloaded by a browser carries `com.apple.quarantine` and
//! Gatekeeper refuses to open it. dodo's own download does not set the
//! attribute — it is applied by the downloading application, and this one does
//! not apply it — but the archive may carry it in its extended attributes, so
//! `xattr -dr` is run over the extracted bundle before the swap. Its failure is
//! logged and does not stop the install: on a Mac where it is not needed the
//! command is a no-op, and refusing to update because a cleanup step was
//! unnecessary would be absurd.
//!
//! # Refusing is a normal outcome
//!
//! Three situations end in [`InstallOutcome::Manual`] rather than an error, all
//! of them ordinary: dodo running as a bare binary (a `cargo run` build, an
//! extracted bare archive) has no bundle to swap; `/Applications` may not be
//! writable by this user; the bundle may be on a read-only volume, which is what
//! running dodo straight out of a mounted DMG looks like. In each the archive is
//! downloaded and verified and the user is told where it is.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::updater::models::install_target::{InstallTarget, classify, staging_dir};
use crate::updater::models::state::{InstallOutcome, ManualReason, UpdateError};
use crate::updater::services::PlatformInstaller;
use crate::updater::services::installers::{extract, swap};
use crate::updater::services::log;

/// The macOS installer.
///
/// `#[allow(dead_code)]`: all three installers are compiled on every platform
/// on purpose — a platform-specific module that only compiles on its own
/// platform is how this repo shipped a Windows build that did not build (see
/// `AGENTS.md` on `docker/services/engine.rs`) — but only one is *constructed*,
/// by the single `#[cfg(target_os)]` in
/// [`platform_installer`](super::platform_installer).
#[allow(dead_code)]
#[derive(Default)]
pub struct MacosInstaller {
    /// The running executable, or `None` to ask the OS.
    ///
    /// An override exists solely so the install path can be tested against a
    /// fabricated bundle instead of against the test binary itself. Production
    /// always leaves it `None`.
    executable: Option<PathBuf>,
}

#[allow(dead_code)]
impl MacosInstaller {
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

impl PlatformInstaller for MacosInstaller {
    fn install(&self, archive: &Path) -> Result<InstallOutcome, UpdateError> {
        let executable = self.executable()?;
        let InstallTarget::MacosBundle { bundle, .. } = classify(&executable) else {
            log::note("not running from an app bundle; leaving the archive for a manual install");
            return Ok(InstallOutcome::Manual {
                reason: ManualReason::NotABundle,
                archive: archive.to_path_buf(),
            });
        };

        let Some(parent) = bundle.parent() else {
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

        let new_bundle = extract::sole_entry_ending_with(&staging, ".app").ok_or_else(|| {
            let _ = std::fs::remove_dir_all(&staging);
            UpdateError::Install("the archive does not contain exactly one .app bundle".to_owned())
        })?;

        strip_quarantine(&new_bundle);

        let result = swap::swap(&bundle, &new_bundle);
        let _ = std::fs::remove_dir_all(&staging);
        result?;

        log::note("installed; a restart will run the new version");
        Ok(InstallOutcome::Installed)
    }

    fn relaunch(&self) -> Result<(), UpdateError> {
        let executable = self.executable()?;
        let target = classify(&executable);

        // `open -n` launches a *new* instance of the bundle. Launching the
        // inner executable directly would work but would produce a process the
        // Dock and the window server treat as unbundled.
        let started = match &target {
            InstallTarget::MacosBundle { bundle, .. } => Command::new("open")
                .arg("-n")
                .arg(bundle)
                .spawn()
                .map(|_| ()),
            // A bare binary has no bundle to hand `open`; run it directly.
            bare => Command::new(bare.executable()).spawn().map(|_| ()),
        };

        started.map_err(|err| UpdateError::Install(format!("could not relaunch: {err}")))
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

/// Clears `com.apple.quarantine` off the extracted bundle.
///
/// Best effort by design — see the module doc. Logged rather than returned so
/// the reason is recoverable if Gatekeeper does refuse the result.
fn strip_quarantine(bundle: &Path) {
    match Command::new("xattr")
        .arg("-dr")
        .arg("com.apple.quarantine")
        .arg(bundle)
        .output()
    {
        Ok(output) if output.status.success() => {}
        Ok(_) => log::note("xattr reported nothing to clear"),
        Err(err) => log::problem(&format!("could not run xattr: {err}")),
    }
}

#[cfg(test)]
mod tests {
    use super::MacosInstaller;
    use crate::updater::models::state::{InstallOutcome, ManualReason};
    use crate::updater::services::PlatformInstaller;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    fn scratch() -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("dodo-macos-test-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("creates");
        dir
    }

    /// Builds `<dir>/dodo.app` with a marker inside, and returns the path the
    /// running executable would have.
    fn fake_bundle(dir: &Path, marker: &[u8]) -> PathBuf {
        let macos = dir.join("dodo.app/Contents/MacOS");
        std::fs::create_dir_all(&macos).expect("creates");
        std::fs::write(macos.join("dodo"), marker).expect("writes");
        macos.join("dodo")
    }

    /// Packs `<dir>/dodo.app` into a `-app.tar.gz`, the shape the release
    /// publishes.
    fn pack(dir: &Path, into: &Path) {
        assert!(
            std::process::Command::new("tar")
                .arg("-czf")
                .arg(into)
                .arg("-C")
                .arg(dir)
                .arg("dodo.app")
                .status()
                .expect("tar")
                .success()
        );
    }

    /// The whole macOS install, end to end, against a fabricated bundle: pack a
    /// new `dodo.app`, install it over an old one, and see the new bytes in
    /// place with the old copy kept aside for the next launch to sweep.
    #[test]
    fn installs_a_new_bundle_over_the_running_one() {
        let root = scratch();

        let source = root.join("source");
        std::fs::create_dir_all(&source).expect("creates");
        fake_bundle(&source, b"new binary");
        let archive = root.join("dodo-app.tar.gz");
        pack(&source, &archive);

        let installed = root.join("installed");
        std::fs::create_dir_all(&installed).expect("creates");
        let running = fake_bundle(&installed, b"old binary");

        let outcome = MacosInstaller::at(running)
            .install(&archive)
            .expect("installs");
        assert_eq!(outcome, InstallOutcome::Installed);
        assert_eq!(
            std::fs::read(installed.join("dodo.app/Contents/MacOS/dodo")).expect("installed"),
            b"new binary"
        );
        assert!(
            installed.join("dodo.app.dodo-old").exists(),
            "the previous bundle stays until a later launch sweeps it"
        );
        assert!(
            !installed
                .join(format!(".dodo-update-{}", std::process::id()))
                .exists(),
            "the staging directory is cleaned up"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_later_launch_sweeps_the_bundle_left_behind() {
        let root = scratch();
        let installed = root.join("installed");
        std::fs::create_dir_all(&installed).expect("creates");
        let running = fake_bundle(&installed, b"current");
        std::fs::create_dir_all(installed.join("dodo.app.dodo-old")).expect("creates");

        MacosInstaller::at(running).sweep_stale();

        assert!(!installed.join("dodo.app.dodo-old").exists());
        assert!(installed.join("dodo.app").exists(), "the live bundle stays");

        let _ = std::fs::remove_dir_all(&root);
    }

    /// A refusal is a success carrying the archive's location, not an `Err`.
    #[test]
    fn a_bare_binary_is_refused_cleanly_rather_than_failing() {
        let root = scratch();
        let archive = root.join("dodo-app.tar.gz");
        std::fs::write(&archive, b"unused: nothing gets extracted").expect("writes");
        let running = root.join("dodo");
        std::fs::write(&running, b"a loose binary").expect("writes");

        assert_eq!(
            MacosInstaller::at(running)
                .install(&archive)
                .expect("refuses cleanly"),
            InstallOutcome::Manual {
                reason: ManualReason::NotABundle,
                archive: archive.clone(),
            }
        );
        assert!(
            archive.exists(),
            "the verified archive is kept for the user"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_archive_with_no_bundle_in_it_is_a_real_failure() {
        let root = scratch();

        let source = root.join("source");
        std::fs::create_dir_all(source.join("not-an-app")).expect("creates");
        std::fs::write(source.join("not-an-app/x"), b"x").expect("writes");
        let archive = root.join("wrong.tar.gz");
        assert!(
            std::process::Command::new("tar")
                .arg("-czf")
                .arg(&archive)
                .arg("-C")
                .arg(&source)
                .arg("not-an-app")
                .status()
                .expect("tar")
                .success()
        );

        let installed = root.join("installed");
        std::fs::create_dir_all(&installed).expect("creates");
        let running = fake_bundle(&installed, b"old binary");

        let error = MacosInstaller::at(running)
            .install(&archive)
            .expect_err("an archive with no .app cannot be installed");
        assert!(format!("{error:?}").contains(".app"), "{error:?}");
        assert_eq!(
            std::fs::read(installed.join("dodo.app/Contents/MacOS/dodo")).expect("untouched"),
            b"old binary",
            "a failed install must leave the installation alone"
        );

        let _ = std::fs::remove_dir_all(&root);
    }
}
