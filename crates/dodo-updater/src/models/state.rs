//! The updater's states and the events that move between them.
//!
//! Both are plain data with no GPUI and no IO anywhere near them, which is what
//! lets [`state::machine`](crate::state::machine) be unit tested as a
//! pure function of `(state, event)`. The rule the whole module rests on:
//! **the state machine is the only thing that mutates state**, the pipeline only
//! emits events, and the dialog only reads state.
//!
//! # Why `Completed` and `ReadyToRestart` are both here
//!
//! They are the two different ways the flow ends and they need different words
//! on screen:
//!
//! - [`UpdaterState::ReadyToRestart`] — a new version is installed and is
//!   waiting for the restart. There is one thing left to do and the dialog says
//!   so.
//! - [`UpdaterState::Completed`] — nothing is left to do: the check found
//!   nothing newer, or the user skipped what it found. Collapsing the two would
//!   mean either offering a pointless Restart or hiding a needed one.
//!
//! # Why there is no `Cancelled` event
//!
//! Cancellation is not something the pipeline reports — it is the driver
//! *dropping the task*, which by definition emits nothing afterwards. It is
//! therefore a method on the machine ([`cancel`]), not an event, and the state
//! it returns to depends on what survives: cancelling a download leaves the
//! update still available, cancelling a check leaves nothing at all.
//!
//! [`cancel`]: crate::state::machine::UpdaterMachine::cancel

use std::path::PathBuf;

use crate::i18n::{Str, updater};
use crate::models::manifest::{Manifest, ManifestError, ManifestFile};
use crate::models::platform::PlatformKey;
use crate::models::version::{Channel, Version};

/// Everything the UI needs to describe an available update, lifted out of the
/// manifest so nothing downstream has to hold the whole document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateInfo {
    /// The offered version, as text — the exact string a skip records.
    pub version: String,
    pub parsed: Version,
    pub channel: Channel,
    pub notes: String,
    pub published_at: String,
    pub platform: PlatformKey,
    pub file: ManifestFile,
}

impl UpdateInfo {
    /// Builds the description of one platform's update from a manifest.
    /// `None` when the manifest names no archive for that platform — the caller
    /// turns that into a named error rather than "up to date".
    pub fn from_manifest(manifest: &Manifest, platform: PlatformKey) -> Option<UpdateInfo> {
        Some(UpdateInfo {
            version: manifest.version.clone(),
            parsed: manifest.parsed_version()?,
            channel: manifest.channel,
            notes: manifest.notes.clone(),
            published_at: manifest.published_at.clone(),
            platform,
            file: manifest.file_for(platform)?.clone(),
        })
    }

    /// The archive's filename, taken off the end of its URL. Used to name the
    /// downloaded file and to tell the user where it is when the install is
    /// refused.
    pub fn file_name(&self) -> String {
        self.file
            .url
            .rsplit('/')
            .next()
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| {
                format!("dodo-{}{}", self.version, self.platform.archive_extension())
            })
    }
}

/// How far a download has got.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DownloadProgress {
    pub downloaded: u64,
    /// The manifest's `size`. The manifest is validated to make this non-zero,
    /// so [`percent`](DownloadProgress::percent) never divides by zero.
    pub total: u64,
    /// 0–100, saturating. Carried alongside the byte counts rather than derived
    /// at every render, because it is what the progress bar and the label both
    /// read and they must not disagree.
    pub percent: u8,
}

impl DownloadProgress {
    pub fn new(downloaded: u64, total: u64) -> DownloadProgress {
        let percent = if total == 0 {
            0
        } else {
            // A server that sends more than it promised is clamped rather than
            // wrapped: the verification step is what decides such a file's fate.
            ((downloaded.min(total) as u128 * 100) / total as u128) as u8
        };
        DownloadProgress {
            downloaded,
            total,
            percent,
        }
    }
}

/// What an installer did, once verification passed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InstallOutcome {
    /// Swapped into place. The app needs a restart to run it.
    Installed,
    /// **A normal outcome, not a failure.** The archive is downloaded and
    /// verified, and this machine's copy of dodo cannot be replaced in place —
    /// a bare binary rather than a bundle, a read-only volume, a directory this
    /// user cannot write. The user is told where the verified archive is.
    Manual {
        reason: ManualReason,
        archive: PathBuf,
    },
}

/// Why an install could not be done for the user.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManualReason {
    /// macOS, running as a loose executable rather than from a `.app`. There is
    /// no bundle to swap.
    NotABundle,
    /// The directory holding the app cannot be written by this user —
    /// `/Applications` without permission, a managed install.
    NotWritable,
    /// The app is on a read-only or externally-mounted volume: a DMG, a
    /// read-only mount.
    ReadOnlyLocation,
}

impl ManualReason {
    pub fn message(self) -> Str {
        match self {
            ManualReason::NotABundle => updater::Text::ManualNotABundle.into(),
            ManualReason::NotWritable => updater::Text::ManualNotWritable.into(),
            ManualReason::ReadOnlyLocation => updater::Text::ManualReadOnly.into(),
        }
    }
}

/// Everything that can go wrong, in terms the dialog can show.
///
/// Hand-rolled rather than `thiserror`, with a [`Str`] accessor — the same
/// convention `TransportError::message` and `DockerError::message` follow, and
/// for the same reason: `thiserror`'s generated `Display` is an English
/// `String`, and English `String`s are what `i18n_lint` exists to keep out of
/// the UI. Errors therefore store a `Str`, never rendered text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateError {
    /// The manifest or the archive could not be fetched. The `reqwest` message
    /// is third-party English kept verbatim inside a translated frame.
    Network(String),
    /// The manifest was fetched and is not usable.
    Manifest(ManifestError),
    /// This build's target has no archive in the manifest — or no manifest key
    /// at all. Named, never shown as "up to date".
    PlatformMissing(String),
    /// The transfer failed part way through.
    Download(String),
    /// The archive's digest is not the one the manifest promised. The install
    /// stops here and the file is discarded.
    ChecksumMismatch { expected: String, actual: String },
    /// The archive is not the size the manifest promised — caught before
    /// hashing, so a truncated transfer says so plainly.
    SizeMismatch { expected: u64, actual: u64 },
    /// The install itself failed after verification passed.
    Install(String),
    /// A filesystem operation failed.
    Io(String),
}

impl UpdateError {
    pub fn message(&self) -> Str {
        match self {
            UpdateError::Network(detail) => updater::Text::ErrorNetwork(detail.clone()).into(),
            UpdateError::Manifest(error) => error.message(),
            UpdateError::PlatformMissing(key) => {
                updater::Text::ErrorPlatformMissing(key.clone()).into()
            }
            UpdateError::Download(detail) => updater::Text::ErrorDownload(detail.clone()).into(),
            UpdateError::ChecksumMismatch { expected, actual } => updater::Text::ErrorChecksum {
                expected: expected.clone(),
                actual: actual.clone(),
            }
            .into(),
            UpdateError::SizeMismatch { expected, actual } => updater::Text::ErrorSize {
                expected: *expected,
                actual: *actual,
            }
            .into(),
            UpdateError::Install(detail) => updater::Text::ErrorInstall(detail.clone()).into(),
            UpdateError::Io(detail) => updater::Text::ErrorIo(detail.clone()).into(),
        }
    }
}

/// Where the updater is.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum UpdaterState {
    /// Nothing has happened yet, or the flow was cancelled back to nothing.
    #[default]
    Idle,
    Checking,
    UpdateAvailable(UpdateInfo),
    Downloading {
        info: UpdateInfo,
        progress: DownloadProgress,
    },
    /// The bytes are on disk and not yet trusted.
    Downloaded {
        info: UpdateInfo,
        archive: PathBuf,
    },
    Verifying {
        info: UpdateInfo,
        archive: PathBuf,
    },
    Installing {
        info: UpdateInfo,
    },
    ReadyToRestart {
        info: UpdateInfo,
        outcome: InstallOutcome,
    },
    /// The flow ended with nothing left to do. See the module doc.
    Completed,
    Failed {
        /// Kept when the failure happened after an update was found, so
        /// **Try again** knows what it is retrying.
        info: Option<UpdateInfo>,
        error: UpdateError,
    },
}

impl UpdaterState {
    /// Whether work is in flight. Drives the dialog's spinner and disables the
    /// actions that would start a second one.
    pub fn is_busy(&self) -> bool {
        matches!(
            self,
            UpdaterState::Checking
                | UpdaterState::Downloading { .. }
                | UpdaterState::Verifying { .. }
                | UpdaterState::Installing { .. }
        )
    }

    /// The update this state is about, when there is one.
    pub fn info(&self) -> Option<&UpdateInfo> {
        match self {
            UpdaterState::UpdateAvailable(info)
            | UpdaterState::Downloading { info, .. }
            | UpdaterState::Downloaded { info, .. }
            | UpdaterState::Verifying { info, .. }
            | UpdaterState::Installing { info }
            | UpdaterState::ReadyToRestart { info, .. } => Some(info),
            UpdaterState::Failed { info, .. } => info.as_ref(),
            UpdaterState::Idle | UpdaterState::Checking | UpdaterState::Completed => None,
        }
    }
}

/// What the pipeline reports. Every one of these is produced off the UI thread
/// and applied to the machine on it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateEvent {
    CheckingStarted,
    /// The check's IO is over, whichever verdict came with it.
    ///
    /// **It deliberately changes no state.** The verdict events below carry the
    /// state; a marker that also mutated would make the order of two events
    /// load-bearing, and this one is emitted last precisely so that observers
    /// (the periodic loop's next tick) have something to hang off that does not
    /// have to know what was found.
    CheckingFinished,
    UpdateFound(UpdateInfo),
    NoUpdateAvailable,
    DownloadStarted,
    DownloadProgress(DownloadProgress),
    /// The archive is on disk at this path, unverified.
    DownloadCompleted(PathBuf),
    VerificationStarted,
    VerificationSucceeded,
    /// Split out from [`UpdateEvent::Error`] because it is the one failure that
    /// says something about the *release* rather than about this machine.
    VerificationFailed(UpdateError),
    Installing,
    ReadyToRestart(InstallOutcome),
    Error(UpdateError),
}

#[cfg(test)]
mod tests {
    use super::{DownloadProgress, ManualReason, UpdateError, UpdaterState};
    use crate::models::manifest::ManifestError;

    #[test]
    fn progress_is_a_percentage_of_the_promised_size() {
        assert_eq!(DownloadProgress::new(0, 200).percent, 0);
        assert_eq!(DownloadProgress::new(50, 200).percent, 25);
        assert_eq!(DownloadProgress::new(200, 200).percent, 100);
    }

    #[test]
    fn progress_never_divides_by_zero_or_exceeds_a_hundred() {
        assert_eq!(DownloadProgress::new(17, 0).percent, 0);
        assert_eq!(
            DownloadProgress::new(500, 200).percent,
            100,
            "a server sending more than promised is clamped, not wrapped"
        );
    }

    /// The byte counts are what the label shows, so they stay verbatim even
    /// when the percentage is clamped.
    #[test]
    fn progress_keeps_the_raw_byte_counts() {
        let progress = DownloadProgress::new(500, 200);
        assert_eq!((progress.downloaded, progress.total), (500, 200));
    }

    /// The multiplication is done in `u128` because `downloaded * 100`
    /// overflows a `u64` well below the sizes a disk image reaches.
    #[test]
    fn a_large_download_does_not_overflow_the_percentage() {
        assert_eq!(
            DownloadProgress::new(10_000_000_000, 20_000_000_000).percent,
            50
        );
        // And the extreme, which also shows the rounding: this is 49.999…%, and
        // integer division floors, which is the honest direction for a progress
        // bar — it never claims to be further along than it is.
        assert_eq!(DownloadProgress::new(u64::MAX / 2, u64::MAX).percent, 49);
    }

    #[test]
    fn busy_states_are_exactly_the_ones_with_work_in_flight() {
        assert!(UpdaterState::Checking.is_busy());
        assert!(!UpdaterState::Idle.is_busy());
        assert!(!UpdaterState::Completed.is_busy());
        assert!(
            !UpdaterState::Failed {
                info: None,
                error: UpdateError::Network("x".into())
            }
            .is_busy()
        );
    }

    /// Every error has to reach the user as a translated string, which means
    /// every variant has to have a row. The exhaustive `match` in `Str::text`
    /// makes a missing translation a compile error; this makes a missing
    /// *mapping* a test failure.
    #[test]
    fn every_error_renders_a_message() {
        use crate::i18n::Language;

        let errors = [
            UpdateError::Network("detail".into()),
            UpdateError::Manifest(ManifestError::MissingVersion),
            UpdateError::PlatformMissing("linux-x64".into()),
            UpdateError::Download("detail".into()),
            UpdateError::ChecksumMismatch {
                expected: "a".into(),
                actual: "b".into(),
            },
            UpdateError::SizeMismatch {
                expected: 1,
                actual: 2,
            },
            UpdateError::Install("detail".into()),
            UpdateError::Io("detail".into()),
        ];

        for error in errors {
            for language in Language::ALL {
                assert!(
                    !error.message().text(language).trim().is_empty(),
                    "{error:?} renders nothing in {}",
                    language.code()
                );
            }
        }
    }

    #[test]
    fn every_manual_reason_renders_a_message() {
        use crate::i18n::Language;

        for reason in [
            ManualReason::NotABundle,
            ManualReason::NotWritable,
            ManualReason::ReadOnlyLocation,
        ] {
            for language in Language::ALL {
                assert!(
                    !reason.message().text(language).trim().is_empty(),
                    "{reason:?}"
                );
            }
        }
    }
}
