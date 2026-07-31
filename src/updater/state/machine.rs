//! The state machine, and the only thing in the updater that mutates state.
//!
//! Everything else has one job: [`services::pipeline`](crate::updater::services::pipeline)
//! produces [`UpdateEvent`]s, the dialog applies them here and renders whatever
//! [`UpdaterMachine::state`] then says. The dialog performs no IO and the
//! pipeline holds no state, so this file is the whole of the updater's logic
//! about *what happens next* — and it has no GPUI in it, which is what lets
//! every transition be a unit test.
//!
//! # Two events deliberately change nothing
//!
//! [`UpdateEvent::CheckingFinished`] and [`UpdateEvent::VerificationSucceeded`]
//! are markers, not transitions:
//!
//! - `CheckingFinished` is emitted *after* the verdict, so that a caller with no
//!   interest in the verdict (the periodic loop, deciding when to sleep again)
//!   has something to wait for. If it also set a state it would clobber the
//!   `UpdateAvailable` that arrived a moment earlier, and the order of two
//!   events would become load-bearing.
//! - `VerificationSucceeded` has no state to move to: the required state list
//!   has `Verifying` and `Installing` and nothing between them. Inventing a
//!   `Verified` state to give the marker somewhere to land would be adding a
//!   state nobody asked for; the honest answer is that verification passing is
//!   news, not a place.
//!
//! # Refusals
//!
//! Several events are *ignored* rather than applied, and each is a rule:
//!
//! - A check cannot start while a download or an install is in flight. The
//!   user pressing "Check for updates" mid-download must not throw away the
//!   download.
//! - An install cannot be cancelled. Between the two renames of a swap there is
//!   a moment when the application is not where it was, and abandoning that on
//!   request would be the one way to leave a machine without a dodo.
//! - Anything that arrives out of sequence — progress with no download, a
//!   verdict with no check — is dropped. The pipeline never emits those; the
//!   machine refusing them is what makes that testable rather than assumed.

use crate::updater::models::state::{
    DownloadProgress, UpdateError, UpdateEvent, UpdateInfo, UpdaterState,
};

/// Where a retry should resume.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RetryFrom {
    /// The failure was in the check itself; start over from the manifest.
    Check,
    /// The check succeeded and something after it failed. The update is still
    /// known, so the retry is a download rather than another round trip.
    Download(Box<UpdateInfo>),
}

/// The updater's state, and the only thing that changes it.
#[derive(Debug, Default)]
pub struct UpdaterMachine {
    state: UpdaterState,
}

impl UpdaterMachine {
    pub fn new() -> Self {
        Self::default()
    }

    /// A machine that already knows about an update — how the dialog opens when
    /// a *background* check found something, so it does not check twice.
    pub fn holding(info: UpdateInfo) -> Self {
        Self {
            state: UpdaterState::UpdateAvailable(info),
        }
    }

    pub fn state(&self) -> &UpdaterState {
        &self.state
    }

    /// Applies an event. Returns whether the state actually changed, so the
    /// caller only repaints when there is something new to draw — the same
    /// contract `docker::state::diff`'s merges use.
    pub fn apply(&mut self, event: UpdateEvent) -> bool {
        let next = self.next_state(event);
        match next {
            Some(state) if state != self.state => {
                self.state = state;
                true
            }
            _ => false,
        }
    }

    /// `None` means "no transition": either a marker event, or one that arrived
    /// in a state that refuses it.
    fn next_state(&self, event: UpdateEvent) -> Option<UpdaterState> {
        match event {
            UpdateEvent::CheckingStarted => {
                // A check must not interrupt work already in flight.
                match self.state {
                    UpdaterState::Downloading { .. }
                    | UpdaterState::Downloaded { .. }
                    | UpdaterState::Verifying { .. }
                    | UpdaterState::Installing { .. }
                    | UpdaterState::ReadyToRestart { .. } => None,
                    _ => Some(UpdaterState::Checking),
                }
            }

            // Markers. See the module doc.
            UpdateEvent::CheckingFinished | UpdateEvent::VerificationSucceeded => None,

            UpdateEvent::UpdateFound(info) => match self.state {
                UpdaterState::Checking => Some(UpdaterState::UpdateAvailable(info)),
                _ => None,
            },
            UpdateEvent::NoUpdateAvailable => match self.state {
                UpdaterState::Checking => Some(UpdaterState::Completed),
                _ => None,
            },

            UpdateEvent::DownloadStarted => match &self.state {
                UpdaterState::UpdateAvailable(info) => Some(UpdaterState::Downloading {
                    info: info.clone(),
                    progress: DownloadProgress::new(0, info.file.size),
                }),
                _ => None,
            },
            UpdateEvent::DownloadProgress(progress) => match &self.state {
                UpdaterState::Downloading { info, .. } => Some(UpdaterState::Downloading {
                    info: info.clone(),
                    progress,
                }),
                _ => None,
            },
            UpdateEvent::DownloadCompleted(archive) => match &self.state {
                UpdaterState::Downloading { info, .. } => Some(UpdaterState::Downloaded {
                    info: info.clone(),
                    archive,
                }),
                _ => None,
            },

            UpdateEvent::VerificationStarted => match &self.state {
                UpdaterState::Downloaded { info, archive } => Some(UpdaterState::Verifying {
                    info: info.clone(),
                    archive: archive.clone(),
                }),
                _ => None,
            },
            UpdateEvent::VerificationFailed(error) => Some(UpdaterState::Failed {
                info: self.state.info().cloned(),
                error,
            }),

            UpdateEvent::Installing => match &self.state {
                UpdaterState::Verifying { info, .. } => {
                    Some(UpdaterState::Installing { info: info.clone() })
                }
                _ => None,
            },
            UpdateEvent::ReadyToRestart(outcome) => match &self.state {
                UpdaterState::Installing { info } => Some(UpdaterState::ReadyToRestart {
                    info: info.clone(),
                    outcome,
                }),
                _ => None,
            },

            UpdateEvent::Error(error) => Some(UpdaterState::Failed {
                info: self.state.info().cloned(),
                error,
            }),
        }
    }

    /// Abandons whatever is in flight.
    ///
    /// The driver drops its task; this decides what is left. A cancelled
    /// *check* leaves nothing, so the machine goes back to [`UpdaterState::Idle`];
    /// a cancelled *download* leaves the update itself still known and offered,
    /// so it goes back to [`UpdaterState::UpdateAvailable`] rather than making
    /// the user check again.
    ///
    /// **An install is not cancellable** — see the module doc.
    pub fn cancel(&mut self) -> bool {
        let next = match &self.state {
            UpdaterState::Checking => Some(UpdaterState::Idle),
            UpdaterState::Downloading { info, .. }
            | UpdaterState::Downloaded { info, .. }
            | UpdaterState::Verifying { info, .. } => {
                Some(UpdaterState::UpdateAvailable(info.clone()))
            }
            _ => None,
        };
        match next {
            Some(state) => {
                self.state = state;
                true
            }
            None => false,
        }
    }

    /// Puts a failed machine back where the retry should start, and says where
    /// that is. `None` when there is nothing to retry.
    pub fn retry(&mut self) -> Option<RetryFrom> {
        let UpdaterState::Failed { info, .. } = &self.state else {
            return None;
        };

        match info.clone() {
            Some(info) => {
                self.state = UpdaterState::UpdateAvailable(info.clone());
                Some(RetryFrom::Download(Box::new(info)))
            }
            None => {
                self.state = UpdaterState::Idle;
                Some(RetryFrom::Check)
            }
        }
    }

    /// Records that the user does not want this version. Returns the version
    /// string to write into the config — the manifest's own text, so a version
    /// this build could not parse would still round-trip.
    pub fn skip(&mut self) -> Option<String> {
        let version = self.state.info()?.version.clone();
        self.state = UpdaterState::Completed;
        Some(version)
    }

    /// The error a failed machine holds, for the dialog to render.
    pub fn error(&self) -> Option<&UpdateError> {
        match &self.state {
            UpdaterState::Failed { error, .. } => Some(error),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{RetryFrom, UpdaterMachine};
    use crate::updater::models::manifest::ManifestFile;
    use crate::updater::models::platform::PlatformKey;
    use crate::updater::models::state::{
        DownloadProgress, InstallOutcome, ManualReason, UpdateError, UpdateEvent, UpdateInfo,
        UpdaterState,
    };
    use crate::updater::models::version::{Channel, Version};
    use std::path::PathBuf;

    fn info() -> UpdateInfo {
        UpdateInfo {
            version: "0.2.0".into(),
            parsed: Version::parse("0.2.0").expect("a version"),
            channel: Channel::Stable,
            notes: "what changed".into(),
            published_at: "2026-07-30T15:03:24Z".into(),
            platform: PlatformKey::MacosArm64,
            file: ManifestFile {
                url: "https://example.test/dodo-v0.2.0-macos-arm64-app.tar.gz".into(),
                sha256: "0".repeat(64),
                size: 1000,
                signature: None,
            },
        }
    }

    fn archive() -> PathBuf {
        PathBuf::from("/tmp/dodo-v0.2.0-macos-arm64-app.tar.gz")
    }

    /// Drives a machine from `Idle` all the way to `ReadyToRestart`, applying
    /// exactly the events the pipeline emits, in the order it emits them.
    fn installed() -> UpdaterMachine {
        let mut machine = UpdaterMachine::new();
        for event in [
            UpdateEvent::CheckingStarted,
            UpdateEvent::UpdateFound(info()),
            UpdateEvent::CheckingFinished,
            UpdateEvent::DownloadStarted,
            UpdateEvent::DownloadProgress(DownloadProgress::new(1000, 1000)),
            UpdateEvent::DownloadCompleted(archive()),
            UpdateEvent::VerificationStarted,
            UpdateEvent::VerificationSucceeded,
            UpdateEvent::Installing,
            UpdateEvent::ReadyToRestart(InstallOutcome::Installed),
        ] {
            machine.apply(event);
        }
        machine
    }

    #[test]
    fn a_fresh_machine_is_idle() {
        assert_eq!(*UpdaterMachine::new().state(), UpdaterState::Idle);
    }

    #[test]
    fn the_whole_happy_path_ends_ready_to_restart() {
        assert_eq!(
            *installed().state(),
            UpdaterState::ReadyToRestart {
                info: info(),
                outcome: InstallOutcome::Installed
            }
        );
    }

    #[test]
    fn every_step_of_the_happy_path_lands_where_it_should() {
        let mut machine = UpdaterMachine::new();

        assert!(machine.apply(UpdateEvent::CheckingStarted));
        assert_eq!(*machine.state(), UpdaterState::Checking);

        assert!(machine.apply(UpdateEvent::UpdateFound(info())));
        assert_eq!(*machine.state(), UpdaterState::UpdateAvailable(info()));

        assert!(machine.apply(UpdateEvent::DownloadStarted));
        assert!(matches!(machine.state(), UpdaterState::Downloading { .. }));

        assert!(machine.apply(UpdateEvent::DownloadCompleted(archive())));
        assert!(matches!(machine.state(), UpdaterState::Downloaded { .. }));

        assert!(machine.apply(UpdateEvent::VerificationStarted));
        assert!(matches!(machine.state(), UpdaterState::Verifying { .. }));

        assert!(machine.apply(UpdateEvent::Installing));
        assert!(matches!(machine.state(), UpdaterState::Installing { .. }));
    }

    #[test]
    fn nothing_newer_ends_completed_rather_than_idle() {
        let mut machine = UpdaterMachine::new();
        machine.apply(UpdateEvent::CheckingStarted);
        assert!(machine.apply(UpdateEvent::NoUpdateAvailable));
        assert_eq!(*machine.state(), UpdaterState::Completed);
    }

    /// The two markers. Both are emitted in the real sequence, and neither may
    /// disturb the state the verdict just set.
    #[test]
    fn the_marker_events_change_nothing() {
        let mut machine = UpdaterMachine::new();
        machine.apply(UpdateEvent::CheckingStarted);
        machine.apply(UpdateEvent::UpdateFound(info()));

        assert!(
            !machine.apply(UpdateEvent::CheckingFinished),
            "CheckingFinished arrives after the verdict and must not clobber it"
        );
        assert_eq!(*machine.state(), UpdaterState::UpdateAvailable(info()));

        machine.apply(UpdateEvent::DownloadStarted);
        machine.apply(UpdateEvent::DownloadCompleted(archive()));
        machine.apply(UpdateEvent::VerificationStarted);
        assert!(!machine.apply(UpdateEvent::VerificationSucceeded));
        assert!(matches!(machine.state(), UpdaterState::Verifying { .. }));
    }

    #[test]
    fn progress_updates_the_downloading_state_in_place() {
        let mut machine = UpdaterMachine::new();
        machine.apply(UpdateEvent::CheckingStarted);
        machine.apply(UpdateEvent::UpdateFound(info()));
        machine.apply(UpdateEvent::DownloadStarted);

        assert!(
            machine.apply(UpdateEvent::DownloadProgress(DownloadProgress::new(
                500, 1000
            )))
        );
        match machine.state() {
            UpdaterState::Downloading { progress, .. } => {
                assert_eq!(progress.percent, 50);
                assert_eq!(progress.downloaded, 500);
            }
            other => panic!("expected Downloading, got {other:?}"),
        }

        assert!(
            !machine.apply(UpdateEvent::DownloadProgress(DownloadProgress::new(
                500, 1000
            ))),
            "the same progress twice is not a repaint"
        );
    }

    // ---- Refusals ------------------------------------------------------------

    #[test]
    fn events_out_of_sequence_are_dropped() {
        let mut machine = UpdaterMachine::new();
        for event in [
            UpdateEvent::UpdateFound(info()),
            UpdateEvent::NoUpdateAvailable,
            UpdateEvent::DownloadStarted,
            UpdateEvent::DownloadProgress(DownloadProgress::new(1, 2)),
            UpdateEvent::DownloadCompleted(archive()),
            UpdateEvent::VerificationStarted,
            UpdateEvent::Installing,
            UpdateEvent::ReadyToRestart(InstallOutcome::Installed),
        ] {
            assert!(
                !machine.apply(event.clone()),
                "{event:?} must not move an idle machine"
            );
        }
        assert_eq!(*machine.state(), UpdaterState::Idle);
    }

    /// Pressing "Check for updates" mid-download must not throw the download
    /// away.
    #[test]
    fn a_check_cannot_interrupt_work_in_flight() {
        let mut machine = UpdaterMachine::new();
        machine.apply(UpdateEvent::CheckingStarted);
        machine.apply(UpdateEvent::UpdateFound(info()));
        machine.apply(UpdateEvent::DownloadStarted);

        assert!(!machine.apply(UpdateEvent::CheckingStarted));
        assert!(matches!(machine.state(), UpdaterState::Downloading { .. }));

        assert!(
            !installed().apply(UpdateEvent::CheckingStarted),
            "nor may it discard a finished install waiting for a restart"
        );
    }

    #[test]
    fn a_check_may_start_from_idle_completed_or_failed() {
        for start in [
            UpdaterState::Idle,
            UpdaterState::Completed,
            UpdaterState::Failed {
                info: None,
                error: UpdateError::Network("offline".into()),
            },
            UpdaterState::UpdateAvailable(info()),
        ] {
            let mut machine = UpdaterMachine::new();
            machine.state = start.clone();
            assert!(
                machine.apply(UpdateEvent::CheckingStarted),
                "a check should be able to start from {start:?}"
            );
        }
    }

    // ---- Failure, cancellation, retry ---------------------------------------

    #[test]
    fn a_failure_before_an_update_is_known_holds_no_info() {
        let mut machine = UpdaterMachine::new();
        machine.apply(UpdateEvent::CheckingStarted);
        assert!(machine.apply(UpdateEvent::Error(UpdateError::Network("offline".into()))));
        assert_eq!(
            *machine.state(),
            UpdaterState::Failed {
                info: None,
                error: UpdateError::Network("offline".into())
            }
        );
    }

    /// A failure *after* the check keeps what was found, so Try again is a
    /// download rather than another round trip to the manifest.
    #[test]
    fn a_failure_after_the_check_keeps_what_was_found() {
        let mut machine = UpdaterMachine::new();
        machine.apply(UpdateEvent::CheckingStarted);
        machine.apply(UpdateEvent::UpdateFound(info()));
        machine.apply(UpdateEvent::DownloadStarted);
        machine.apply(UpdateEvent::Error(UpdateError::Download("reset".into())));

        assert_eq!(machine.state().info(), Some(&info()));
        assert_eq!(machine.retry(), Some(RetryFrom::Download(Box::new(info()))),);
        assert_eq!(*machine.state(), UpdaterState::UpdateAvailable(info()));
    }

    #[test]
    fn retrying_a_failed_check_starts_over_from_the_manifest() {
        let mut machine = UpdaterMachine::new();
        machine.apply(UpdateEvent::CheckingStarted);
        machine.apply(UpdateEvent::Error(UpdateError::Network("offline".into())));

        assert_eq!(machine.retry(), Some(RetryFrom::Check));
        assert_eq!(*machine.state(), UpdaterState::Idle);
    }

    #[test]
    fn there_is_nothing_to_retry_when_nothing_failed() {
        assert_eq!(UpdaterMachine::new().retry(), None);
        assert_eq!(installed().retry(), None);
    }

    /// A verification failure gets its own event and still lands in `Failed` —
    /// what differs is that the dialog can say the release is wrong rather than
    /// the machine.
    #[test]
    fn a_verification_failure_fails_the_machine() {
        let mut machine = UpdaterMachine::new();
        machine.apply(UpdateEvent::CheckingStarted);
        machine.apply(UpdateEvent::UpdateFound(info()));
        machine.apply(UpdateEvent::DownloadStarted);
        machine.apply(UpdateEvent::DownloadCompleted(archive()));
        machine.apply(UpdateEvent::VerificationStarted);

        let error = UpdateError::ChecksumMismatch {
            expected: "a".into(),
            actual: "b".into(),
        };
        assert!(machine.apply(UpdateEvent::VerificationFailed(error.clone())));
        assert_eq!(machine.error(), Some(&error));
        assert_eq!(machine.state().info(), Some(&info()));
    }

    #[test]
    fn cancelling_a_check_leaves_nothing_behind() {
        let mut machine = UpdaterMachine::new();
        machine.apply(UpdateEvent::CheckingStarted);
        assert!(machine.cancel());
        assert_eq!(*machine.state(), UpdaterState::Idle);
    }

    /// Cancelling a download keeps the update on offer: the user said "not
    /// now", not "never tell me".
    #[test]
    fn cancelling_a_download_keeps_the_update_on_offer() {
        for event in [
            UpdateEvent::DownloadStarted,
            UpdateEvent::DownloadCompleted(archive()),
            UpdateEvent::VerificationStarted,
        ] {
            let mut machine = UpdaterMachine::new();
            machine.apply(UpdateEvent::CheckingStarted);
            machine.apply(UpdateEvent::UpdateFound(info()));
            machine.apply(UpdateEvent::DownloadStarted);
            if !matches!(event, UpdateEvent::DownloadStarted) {
                machine.apply(UpdateEvent::DownloadCompleted(archive()));
            }
            if matches!(event, UpdateEvent::VerificationStarted) {
                machine.apply(UpdateEvent::VerificationStarted);
            }

            assert!(machine.cancel(), "cancelling at {event:?}");
            assert_eq!(*machine.state(), UpdaterState::UpdateAvailable(info()));
        }
    }

    /// The one refusal that protects the user's machine rather than their
    /// patience.
    #[test]
    fn an_install_cannot_be_cancelled() {
        let mut machine = UpdaterMachine::new();
        machine.apply(UpdateEvent::CheckingStarted);
        machine.apply(UpdateEvent::UpdateFound(info()));
        machine.apply(UpdateEvent::DownloadStarted);
        machine.apply(UpdateEvent::DownloadCompleted(archive()));
        machine.apply(UpdateEvent::VerificationStarted);
        machine.apply(UpdateEvent::Installing);

        assert!(
            !machine.cancel(),
            "between the two renames of a swap there is no application on disk; \
             abandoning that on request is the one way to leave a machine without a dodo"
        );
        assert!(matches!(machine.state(), UpdaterState::Installing { .. }));
    }

    #[test]
    fn cancelling_when_nothing_is_happening_does_nothing() {
        assert!(!UpdaterMachine::new().cancel());
        assert!(!installed().cancel());
    }

    // ---- Skip ----------------------------------------------------------------

    #[test]
    fn skipping_reports_the_version_verbatim_and_finishes() {
        let mut machine = UpdaterMachine::holding(info());
        assert_eq!(machine.skip(), Some("0.2.0".to_owned()));
        assert_eq!(*machine.state(), UpdaterState::Completed);
    }

    #[test]
    fn there_is_nothing_to_skip_without_an_update() {
        let mut machine = UpdaterMachine::new();
        assert_eq!(machine.skip(), None);
        assert_eq!(*machine.state(), UpdaterState::Idle);
    }

    #[test]
    fn a_machine_seeded_with_a_find_does_not_check_again() {
        let machine = UpdaterMachine::holding(info());
        assert_eq!(*machine.state(), UpdaterState::UpdateAvailable(info()));
    }

    // ---- The refused install -------------------------------------------------

    #[test]
    fn an_install_this_machine_cannot_do_still_ends_ready_to_restart() {
        let mut machine = UpdaterMachine::new();
        machine.apply(UpdateEvent::CheckingStarted);
        machine.apply(UpdateEvent::UpdateFound(info()));
        machine.apply(UpdateEvent::DownloadStarted);
        machine.apply(UpdateEvent::DownloadCompleted(archive()));
        machine.apply(UpdateEvent::VerificationStarted);
        machine.apply(UpdateEvent::Installing);

        let outcome = InstallOutcome::Manual {
            reason: ManualReason::NotABundle,
            archive: archive(),
        };
        assert!(machine.apply(UpdateEvent::ReadyToRestart(outcome.clone())));
        assert_eq!(
            *machine.state(),
            UpdaterState::ReadyToRestart {
                info: info(),
                outcome
            },
            "a refusal is carried in the outcome, not in a failure state — the \
             dialog is what tells the two apart"
        );
    }

    #[test]
    fn busy_is_true_exactly_while_something_is_running() {
        let mut machine = UpdaterMachine::new();
        assert!(!machine.state().is_busy());

        machine.apply(UpdateEvent::CheckingStarted);
        assert!(machine.state().is_busy());

        machine.apply(UpdateEvent::UpdateFound(info()));
        assert!(
            !machine.state().is_busy(),
            "waiting for the user to agree is not work in flight"
        );

        machine.apply(UpdateEvent::DownloadStarted);
        assert!(machine.state().is_busy());

        assert!(!installed().state().is_busy());
    }
}
