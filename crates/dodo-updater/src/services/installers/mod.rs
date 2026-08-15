//! The three installers, and the one place in the updater that asks what
//! platform it is on.
//!
//! - [`extract`] — unpacking an archive with the system's `tar`.
//! - [`swap`] — the rename-aside sequence, the writability probe and the
//!   stale-file sweep, shared by all three.
//! - [`macos`], [`windows`], [`linux`] — one [`PlatformInstaller`] each.
//!
//! # `#[cfg(target_os)]` appears exactly once, below
//!
//! Nowhere else in the updater, and nowhere inside the three modules. Two
//! things follow from that, and both are the point:
//!
//! 1. **All three compile on every platform.** A module that only compiles on
//!    its own platform is how this repo shipped a Windows build that did not
//!    build — `AGENTS.md` records the `#[cfg(unix)]`-only bollard connector
//!    that failed `build (windows-x64)` on its one real run. Here, a mistake in
//!    `windows.rs` is a compile error on this Mac.
//! 2. **All three are tested on every platform.** Nothing in them is a platform
//!    API: it is `std::fs`, `std::process::Command` and path arithmetic. So the
//!    Windows rename-aside sequence and the Linux refusal path are exercised by
//!    `cargo test` on a machine that cannot even *compile* those targets (see
//!    `docs/release.md`). What that does not prove is the platform's own
//!    behaviour — whether Windows really permits renaming a running `.exe` — and
//!    that limit is stated in `windows.rs` rather than glossed over.
//!
//! The `#[allow(dead_code)]` on each installer is the cost: on any one platform
//! the other two are constructed only by their tests. It is applied at the
//! definitions with the reason inline, which is the suppression style
//! `AGENTS.md` describes.

pub mod extract;
pub mod linux;
pub mod macos;
pub mod swap;
pub mod windows;

#[cfg(test)]
use std::path::{Path, PathBuf};
use std::sync::Arc;
#[cfg(test)]
use std::sync::Mutex;

#[cfg(test)]
use crate::models::state::{InstallOutcome, UpdateError};
use crate::services::PlatformInstaller;

/// The installer for the platform this binary was built for.
///
/// Mirrors `docker::services::default_engine`: one function, returning an
/// `Arc<dyn …>`, so nothing above this line knows which implementation it got.
#[cfg(target_os = "macos")]
pub fn platform_installer() -> Arc<dyn PlatformInstaller> {
    Arc::new(macos::MacosInstaller::new())
}

/// See the macOS arm above.
#[cfg(target_os = "windows")]
pub fn platform_installer() -> Arc<dyn PlatformInstaller> {
    Arc::new(windows::WindowsInstaller::new())
}

/// Everything else. Linux is what dodo publishes for; a BSD or an unusual
/// target gets the same in-place replacement, which either works or reports
/// that the location is not writable.
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn platform_installer() -> Arc<dyn PlatformInstaller> {
    Arc::new(linux::LinuxInstaller::new())
}

/// An installer that records what it was asked to do and does none of it.
///
/// The twin for the three above. It is what lets
/// [`pipeline`](super::pipeline)'s tests run a whole cycle through to
/// `ReadyToRestart` without touching an installed application — and a test
/// double only, so it is `#[cfg(test)]` and costs the shipped binary nothing.
#[cfg(test)]
pub struct RecordingInstaller {
    outcome: Result<InstallOutcome, UpdateError>,
    calls: Mutex<Vec<Call>>,
}

/// One thing a [`RecordingInstaller`] was asked to do.
#[cfg(test)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Call {
    Install(PathBuf),
    Relaunch,
    Sweep,
}

#[cfg(test)]
impl RecordingInstaller {
    /// An installer that succeeds, reporting `outcome`.
    pub fn returning(outcome: InstallOutcome) -> Self {
        Self {
            outcome: Ok(outcome),
            calls: Mutex::new(Vec::new()),
        }
    }

    /// An installer that fails — a broken archive, a rename that did not work.
    pub fn failing(error: UpdateError) -> Self {
        Self {
            outcome: Err(error),
            calls: Mutex::new(Vec::new()),
        }
    }

    /// Everything it was asked to do, in order.
    pub fn calls(&self) -> Vec<Call> {
        self.calls
            .lock()
            .map(|calls| calls.clone())
            .unwrap_or_default()
    }

    fn record(&self, call: Call) {
        if let Ok(mut calls) = self.calls.lock() {
            calls.push(call);
        }
    }
}

#[cfg(test)]
impl PlatformInstaller for RecordingInstaller {
    fn install(&self, archive: &Path) -> Result<InstallOutcome, UpdateError> {
        self.record(Call::Install(archive.to_path_buf()));
        self.outcome.clone()
    }

    fn relaunch(&self) -> Result<(), UpdateError> {
        self.record(Call::Relaunch);
        Ok(())
    }

    fn sweep_stale(&self) {
        self.record(Call::Sweep);
    }
}

#[cfg(test)]
mod tests {
    use super::{Call, RecordingInstaller, platform_installer};
    use crate::models::platform::PlatformKey;
    use crate::models::state::{InstallOutcome, UpdateError};
    use crate::services::PlatformInstaller;
    use std::path::Path;

    /// The factory has to produce *something* on whatever this is built for,
    /// and calling it must not panic.
    #[test]
    fn this_platform_has_an_installer() {
        let installer = platform_installer();
        // Sweeping is the one method safe to call for real: it deletes files
        // ending in `.dodo-old` beside the test binary, of which there are
        // none.
        installer.sweep_stale();
        assert!(
            PlatformKey::current().is_some(),
            "the installer exists for a platform the manifest also knows"
        );
    }

    #[test]
    fn the_recording_installer_reports_what_it_was_asked() {
        let installer = RecordingInstaller::returning(InstallOutcome::Installed);
        assert_eq!(
            installer.install(Path::new("/tmp/dodo.tar.gz")),
            Ok(InstallOutcome::Installed)
        );
        installer
            .relaunch()
            .expect("the twin never fails to relaunch");
        installer.sweep_stale();

        assert_eq!(
            installer.calls(),
            [
                Call::Install("/tmp/dodo.tar.gz".into()),
                Call::Relaunch,
                Call::Sweep
            ]
        );
    }

    #[test]
    fn the_recording_installer_can_fail_the_way_a_real_one_does() {
        let installer = RecordingInstaller::failing(UpdateError::Install("no space".into()));
        assert_eq!(
            installer.install(Path::new("/tmp/x")),
            Err(UpdateError::Install("no space".into()))
        );
    }
}
