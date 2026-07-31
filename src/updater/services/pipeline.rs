//! The whole flow, as two blocking functions over trait objects.
//!
//! Shaped after `api_explorer::services::send`, and for exactly the same
//! reason: with the sequence written as one blocking function over
//! `&dyn ManifestSource` / `&dyn Downloader` / `&dyn Verifier` /
//! `&dyn PlatformInstaller`, the *ordering* is unit-testable with four fakes,
//! no network and no `Window`.
//!
//! ```text
//! check:   fetch manifest -> parse -> find this platform -> compare -> verdict
//! install: download (streaming, hashing) -> verify -> install -> outcome
//! ```
//!
//! # The two halves are separate because the user is between them
//!
//! **Check silently, ask before downloading** was decided with the captain, and
//! it is enforced structurally rather than by remembering to ask:
//! [`check`] cannot download, because it is handed no [`Downloader`]. The only
//! way to reach the network a second time is [`download_and_install`], and
//! nothing calls that except a button.
//!
//! # Events, not return values
//!
//! Both functions take an `emit` callback and report through it, so a caller
//! sees `DownloadStarted`, then progress, then `DownloadCompleted` as they
//! happen rather than at the end. They still *return* the outcome, because a
//! caller that only wants the verdict should not have to reconstruct it from
//! the events it saw.
//!
//! # Threading
//!
//! Blocking by contract. Both run on GPUI's background executor; `emit` is
//! called from that thread, and it is the caller's job to hop back to the UI.

use std::path::{Path, PathBuf};

use crate::updater::models::config::UpdaterConfig;
use crate::updater::models::manifest::{self, ManifestError};
use crate::updater::models::platform::PlatformKey;
use crate::updater::models::state::{InstallOutcome, UpdateError, UpdateEvent, UpdateInfo};
use crate::updater::models::version::{UpdateDecision, Version, decide};
use crate::updater::services::{Downloader, ManifestSource, PlatformInstaller, Verifier, log};

/// What a check concluded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CheckOutcome {
    /// Newer, on this channel, not skipped.
    Found(UpdateInfo),
    /// Nothing to do — up to date, skipped, or a release this channel does not
    /// follow. All three are reported to the user as "up to date": the last two
    /// are consequences of the user's own settings, not news.
    UpToDate,
}

/// Fetches the manifest and decides whether to offer what it names.
///
/// `current` is the running version — always
/// [`VERSION_INFO.version`](crate::build_info::VERSION_INFO) in the app, and a
/// parameter so a test can pretend to be older.
pub fn check(
    source: &dyn ManifestSource,
    config: &UpdaterConfig,
    current: &Version,
    emit: &dyn Fn(UpdateEvent),
) -> Result<CheckOutcome, UpdateError> {
    emit(UpdateEvent::CheckingStarted);
    let result = run_check(source, config, current);

    match &result {
        Ok(CheckOutcome::Found(info)) => {
            log::note(&format!("update available: {}", info.version));
            emit(UpdateEvent::UpdateFound(info.clone()));
        }
        Ok(CheckOutcome::UpToDate) => emit(UpdateEvent::NoUpdateAvailable),
        Err(error) => {
            log::problem(&format!("check failed: {error:?}"));
            emit(UpdateEvent::Error(error.clone()));
        }
    }

    // Last, and deliberately after the verdict — see `UpdateEvent`'s doc for
    // why this one changes no state.
    emit(UpdateEvent::CheckingFinished);
    result
}

fn run_check(
    source: &dyn ManifestSource,
    config: &UpdaterConfig,
    current: &Version,
) -> Result<CheckOutcome, UpdateError> {
    let bytes = source.fetch(&config.manifest_url)?;
    let manifest = manifest::parse(&bytes).map_err(UpdateError::Manifest)?;

    // A target with no manifest key at all — someone's own `linux-arm64` build.
    let platform = PlatformKey::current().ok_or_else(|| {
        UpdateError::PlatformMissing(crate::build_info::VERSION_INFO.target.to_owned())
    })?;

    // The manifest has an entry for every platform the release publishes, and
    // `docs/release.md` records that a missing one *fails the release* rather
    // than being omitted. If one is missing anyway, saying "up to date" would
    // be the exact silent failure that rule exists to prevent.
    let info = UpdateInfo::from_manifest(&manifest, platform)
        .ok_or_else(|| UpdateError::PlatformMissing(platform.key().to_owned()))?;

    let candidate = info.parsed.clone();
    match decide(
        current,
        &candidate,
        config.channel,
        config.skipped_version.as_deref(),
    ) {
        UpdateDecision::Offer => Ok(CheckOutcome::Found(info)),
        UpdateDecision::UpToDate | UpdateDecision::Skipped | UpdateDecision::WrongChannel => {
            Ok(CheckOutcome::UpToDate)
        }
    }
}

/// Downloads the archive, verifies it, and installs it.
///
/// `into` is the directory the archive is written to — the caller's temp
/// staging directory, so a failure leaves nothing in the user's data directory.
///
/// Returns the outcome. An [`InstallOutcome::Manual`] is an `Ok`: the archive is
/// downloaded and verified and this machine cannot be updated in place, which is
/// a normal answer and not a failure.
pub fn download_and_install(
    downloader: &dyn Downloader,
    verifier: &dyn Verifier,
    installer: &dyn PlatformInstaller,
    info: &UpdateInfo,
    into: &Path,
    emit: &dyn Fn(UpdateEvent),
) -> Result<InstallOutcome, UpdateError> {
    let destination = into.join(info.file_name());

    emit(UpdateEvent::DownloadStarted);
    let archive = match downloader.download(&info.file, &destination, &|progress| {
        emit(UpdateEvent::DownloadProgress(progress))
    }) {
        Ok(archive) => archive,
        Err(error) => {
            log::problem(&format!("download failed: {error:?}"));
            emit(UpdateEvent::Error(error.clone()));
            return Err(error);
        }
    };
    emit(UpdateEvent::DownloadCompleted(archive.path.clone()));

    emit(UpdateEvent::VerificationStarted);
    if let Err(error) = verifier.verify(&archive.path, &info.file) {
        // A verification failure is the one that says something about the
        // *release* rather than about this machine, which is why it has its own
        // event. The verifier has already deleted the file.
        log::problem(&format!("verification failed: {error:?}"));
        emit(UpdateEvent::VerificationFailed(error.clone()));
        return Err(error);
    }
    emit(UpdateEvent::VerificationSucceeded);

    emit(UpdateEvent::Installing);
    match installer.install(&archive.path) {
        Ok(outcome) => {
            emit(UpdateEvent::ReadyToRestart(outcome.clone()));
            Ok(outcome)
        }
        Err(error) => {
            log::problem(&format!("install failed: {error:?}"));
            emit(UpdateEvent::Error(error.clone()));
            Err(error)
        }
    }
}

/// The running version, parsed. `None` would mean `build.rs` embedded something
/// that is not a version, which the `build_info` tests already rule out — but
/// the updater refuses to guess rather than unwrapping.
pub fn current_version() -> Result<Version, UpdateError> {
    let text = crate::build_info::VERSION_INFO.version;
    Version::parse(text)
        .ok_or_else(|| UpdateError::Manifest(ManifestError::UnreadableVersion(text.to_owned())))
}

/// Where a download is staged: the process's own temp directory.
pub fn staging_directory() -> PathBuf {
    crate::updater::services::download::temp_dir(std::process::id())
}

#[cfg(test)]
mod tests {
    use super::{CheckOutcome, check, current_version, download_and_install};
    use crate::updater::models::config::UpdaterConfig;
    use crate::updater::models::platform::PlatformKey;
    use crate::updater::models::sha256::Sha256;
    use crate::updater::models::state::{
        DownloadProgress, InstallOutcome, ManualReason, UpdateError, UpdateEvent,
    };
    use crate::updater::models::version::{Channel, Version};
    use crate::updater::services::download::InMemoryDownloader;
    use crate::updater::services::installers::{Call, RecordingInstaller};
    use crate::updater::services::manifest_source::InMemoryManifestSource;
    use crate::updater::services::verify::Sha256Verifier;
    use std::path::PathBuf;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn scratch() -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("dodo-pipeline-test-{}-{n}", std::process::id()))
    }

    /// A manifest naming `version`, with a real archive digest for the platform
    /// this test is running on — so `PlatformKey::current()` finds its entry
    /// whichever machine the suite runs on.
    fn manifest_for(version: &str, body: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(body);
        let digest = hasher.finalize_hex();
        let key = PlatformKey::current().expect("a supported build target");
        format!(
            r#"{{
              "manifest_version": 1,
              "channel": "stable",
              "version": "{version}",
              "notes": "what changed",
              "published_at": "2026-07-30T15:03:24Z",
              "files": {{
                "{}": {{
                  "url": "https://example.test/dodo-v{version}{}",
                  "sha256": "{digest}",
                  "size": {},
                  "signature": null
                }}
              }}
            }}"#,
            key.key(),
            key.archive_extension(),
            body.len()
        )
    }

    fn v(text: &str) -> Version {
        Version::parse(text).expect("a version")
    }

    /// Collects every event, so a test can assert on the *order* — which is the
    /// thing this module exists to make testable.
    #[derive(Default)]
    struct Recorder(Mutex<Vec<UpdateEvent>>);

    impl Recorder {
        fn emit(&self) -> impl Fn(UpdateEvent) + '_ {
            move |event| {
                if let Ok(mut events) = self.0.lock() {
                    events.push(event);
                }
            }
        }

        fn events(&self) -> Vec<UpdateEvent> {
            self.0.lock().map(|e| e.clone()).unwrap_or_default()
        }

        /// The event kinds, with progress collapsed — there are dozens of those
        /// and their count is not the point.
        fn shape(&self) -> Vec<&'static str> {
            let mut shape: Vec<&'static str> = Vec::new();
            for event in self.events() {
                let name = match event {
                    UpdateEvent::CheckingStarted => "CheckingStarted",
                    UpdateEvent::CheckingFinished => "CheckingFinished",
                    UpdateEvent::UpdateFound(_) => "UpdateFound",
                    UpdateEvent::NoUpdateAvailable => "NoUpdateAvailable",
                    UpdateEvent::DownloadStarted => "DownloadStarted",
                    UpdateEvent::DownloadProgress(_) => "DownloadProgress",
                    UpdateEvent::DownloadCompleted(_) => "DownloadCompleted",
                    UpdateEvent::VerificationStarted => "VerificationStarted",
                    UpdateEvent::VerificationSucceeded => "VerificationSucceeded",
                    UpdateEvent::VerificationFailed(_) => "VerificationFailed",
                    UpdateEvent::Installing => "Installing",
                    UpdateEvent::ReadyToRestart(_) => "ReadyToRestart",
                    UpdateEvent::Error(_) => "Error",
                };
                if name == "DownloadProgress" && shape.last() == Some(&"DownloadProgress") {
                    continue;
                }
                shape.push(name);
            }
            shape
        }
    }

    // ---- check --------------------------------------------------------------

    #[test]
    fn a_newer_release_is_found_and_reported_in_order() {
        let source =
            InMemoryManifestSource::serving(manifest_for("0.2.0", b"archive").into_bytes());
        let recorder = Recorder::default();

        let outcome = check(
            &source,
            &UpdaterConfig::default(),
            &v("0.1.6"),
            &recorder.emit(),
        )
        .expect("checks");

        assert!(matches!(outcome, CheckOutcome::Found(ref info) if info.version == "0.2.0"));
        assert_eq!(
            recorder.shape(),
            ["CheckingStarted", "UpdateFound", "CheckingFinished"],
            "the verdict comes before the marker that the check is over"
        );
    }

    #[test]
    fn the_configured_url_is_the_one_fetched() {
        let source = InMemoryManifestSource::serving(manifest_for("0.2.0", b"a").into_bytes());
        let mut config = UpdaterConfig::default();
        config.manifest_url = "https://example.test/custom.json".into();

        check(&source, &config, &v("0.1.6"), &|_| {}).expect("checks");
        assert_eq!(source.requested(), ["https://example.test/custom.json"]);
    }

    #[test]
    fn the_same_version_is_up_to_date() {
        let source = InMemoryManifestSource::serving(manifest_for("0.1.6", b"a").into_bytes());
        let recorder = Recorder::default();

        assert_eq!(
            check(
                &source,
                &UpdaterConfig::default(),
                &v("0.1.6"),
                &recorder.emit()
            ),
            Ok(CheckOutcome::UpToDate)
        );
        assert_eq!(
            recorder.shape(),
            ["CheckingStarted", "NoUpdateAvailable", "CheckingFinished"]
        );
    }

    #[test]
    fn a_skipped_version_is_not_offered_again() {
        let source = InMemoryManifestSource::serving(manifest_for("0.2.0", b"a").into_bytes());
        let mut config = UpdaterConfig::default();
        config.skip("0.2.0");

        assert_eq!(
            check(&source, &config, &v("0.1.6"), &|_| {}),
            Ok(CheckOutcome::UpToDate)
        );
    }

    #[test]
    fn a_pre_release_is_not_offered_on_the_stable_channel() {
        let source =
            InMemoryManifestSource::serving(manifest_for("0.2.0-beta.1", b"a").into_bytes());

        assert_eq!(
            check(&source, &UpdaterConfig::default(), &v("0.1.6"), &|_| {}),
            Ok(CheckOutcome::UpToDate)
        );

        let mut beta = UpdaterConfig::default();
        beta.channel = Channel::Beta;
        assert!(matches!(
            check(&source, &beta, &v("0.1.6"), &|_| {}),
            Ok(CheckOutcome::Found(_))
        ));
    }

    #[test]
    fn a_network_failure_becomes_an_error_event_and_still_finishes() {
        let source = InMemoryManifestSource::failing(UpdateError::Network("offline".into()));
        let recorder = Recorder::default();

        assert_eq!(
            check(
                &source,
                &UpdaterConfig::default(),
                &v("0.1.6"),
                &recorder.emit()
            ),
            Err(UpdateError::Network("offline".into()))
        );
        assert_eq!(
            recorder.shape(),
            ["CheckingStarted", "Error", "CheckingFinished"],
            "a failed check still has to report that it is over"
        );
    }

    /// The rule `docs/release.md` argues for on the publishing side, enforced on
    /// the reading side: a platform with no entry is *named*, never reported as
    /// "you are up to date".
    #[test]
    fn a_manifest_missing_this_platform_is_an_error_not_up_to_date() {
        let json = r#"{
          "manifest_version": 1, "channel": "stable", "version": "9.9.9",
          "notes": "", "published_at": "x", "files": {}
        }"#;
        let source = InMemoryManifestSource::serving(json.as_bytes().to_vec());

        let error = check(&source, &UpdaterConfig::default(), &v("0.1.6"), &|_| {})
            .expect_err("no entry for this platform");
        assert!(
            matches!(error, UpdateError::PlatformMissing(_)),
            "{error:?}"
        );
    }

    #[test]
    fn a_manifest_from_the_future_stops_the_check() {
        let json = manifest_for("9.9.9", b"a")
            .replace("\"manifest_version\": 1", "\"manifest_version\": 99");
        let source = InMemoryManifestSource::serving(json.into_bytes());

        assert!(matches!(
            check(&source, &UpdaterConfig::default(), &v("0.1.6"), &|_| {}),
            Err(UpdateError::Manifest(_))
        ));
    }

    /// The structural half of "ask before downloading": `check` is handed no
    /// downloader, so it cannot fetch an archive however it is called.
    #[test]
    fn checking_never_downloads_anything() {
        let dir = scratch();
        let source = InMemoryManifestSource::serving(manifest_for("0.2.0", b"a").into_bytes());
        check(&source, &UpdaterConfig::default(), &v("0.1.6"), &|_| {}).expect("checks");
        assert!(
            !dir.exists(),
            "a check must not create a staging directory, let alone fill one"
        );
    }

    // ---- download, verify, install -------------------------------------------

    /// The whole second half, over four fakes and no network.
    #[test]
    fn a_full_cycle_downloads_verifies_installs_and_reports_in_order() {
        let dir = scratch();
        let body = b"a plausible archive, long enough to arrive in several chunks".to_vec();
        let source = InMemoryManifestSource::serving(manifest_for("0.2.0", &body).into_bytes());
        let info = match check(&source, &UpdaterConfig::default(), &v("0.1.6"), &|_| {})
            .expect("checks")
        {
            CheckOutcome::Found(info) => info,
            other => panic!("expected an update, got {other:?}"),
        };

        let installer = RecordingInstaller::returning(InstallOutcome::Installed);
        let recorder = Recorder::default();

        let outcome = download_and_install(
            &InMemoryDownloader::serving(body.clone()),
            &Sha256Verifier::new(),
            &installer,
            &info,
            &dir,
            &recorder.emit(),
        )
        .expect("installs");

        assert_eq!(outcome, InstallOutcome::Installed);
        assert_eq!(
            recorder.shape(),
            [
                "DownloadStarted",
                "DownloadProgress",
                "DownloadCompleted",
                "VerificationStarted",
                "VerificationSucceeded",
                "Installing",
                "ReadyToRestart",
            ],
            "verification has to sit between the download and the install"
        );

        let archive = dir.join(info.file_name());
        assert_eq!(
            installer.calls(),
            [Call::Install(archive.clone())],
            "the installer is handed the file that was verified, not the URL"
        );
        assert_eq!(std::fs::read(&archive).expect("kept"), body);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The one that matters most: a tampered archive never reaches the
    /// installer.
    #[test]
    fn a_checksum_mismatch_stops_before_the_installer_is_called() {
        let dir = scratch();
        let promised = b"the archive the manifest describes".to_vec();
        let source = InMemoryManifestSource::serving(manifest_for("0.2.0", &promised).into_bytes());
        let info = match check(&source, &UpdaterConfig::default(), &v("0.1.6"), &|_| {})
            .expect("checks")
        {
            CheckOutcome::Found(info) => info,
            other => panic!("expected an update, got {other:?}"),
        };

        let installer = RecordingInstaller::returning(InstallOutcome::Installed);
        let recorder = Recorder::default();

        // Same length, different bytes: this gets past the size check and is
        // caught by the digest, which is the case that matters.
        let tampered = b"the archive an attacker substituted".to_vec();
        assert_eq!(tampered.len(), promised.len() + 1);
        let tampered = tampered[..promised.len()].to_vec();

        let error = download_and_install(
            &InMemoryDownloader::serving(tampered),
            &Sha256Verifier::new(),
            &installer,
            &info,
            &dir,
            &recorder.emit(),
        )
        .expect_err("the bytes are not the promised ones");

        assert!(
            matches!(error, UpdateError::ChecksumMismatch { .. }),
            "{error:?}"
        );
        assert!(
            installer.calls().is_empty(),
            "nothing may reach the installer that did not verify"
        );
        assert_eq!(
            recorder.shape().last(),
            Some(&"VerificationFailed"),
            "and the failure is reported as a verification failure, not a generic error"
        );
        assert!(
            !dir.join(info.file_name()).exists(),
            "the rejected archive is discarded"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_failed_download_never_reaches_verification() {
        let dir = scratch();
        let source = InMemoryManifestSource::serving(manifest_for("0.2.0", b"body").into_bytes());
        let info = match check(&source, &UpdaterConfig::default(), &v("0.1.6"), &|_| {})
            .expect("checks")
        {
            CheckOutcome::Found(info) => info,
            other => panic!("expected an update, got {other:?}"),
        };

        let installer = RecordingInstaller::returning(InstallOutcome::Installed);
        let recorder = Recorder::default();

        assert!(
            download_and_install(
                &InMemoryDownloader::failing(UpdateError::Download("reset".into())),
                &Sha256Verifier::new(),
                &installer,
                &info,
                &dir,
                &recorder.emit(),
            )
            .is_err()
        );
        assert_eq!(recorder.shape(), ["DownloadStarted", "Error"]);
        assert!(installer.calls().is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A refusal is an `Ok` carrying where the archive is, and the pipeline
    /// reports it through the same `ReadyToRestart` event — the dialog is what
    /// tells the two apart.
    #[test]
    fn an_install_this_machine_cannot_do_is_a_normal_outcome() {
        let dir = scratch();
        let body = b"archive".to_vec();
        let source = InMemoryManifestSource::serving(manifest_for("0.2.0", &body).into_bytes());
        let info = match check(&source, &UpdaterConfig::default(), &v("0.1.6"), &|_| {})
            .expect("checks")
        {
            CheckOutcome::Found(info) => info,
            other => panic!("expected an update, got {other:?}"),
        };

        let archive_path = dir.join(info.file_name());
        let installer = RecordingInstaller::returning(InstallOutcome::Manual {
            reason: ManualReason::NotABundle,
            archive: archive_path.clone(),
        });
        let recorder = Recorder::default();

        let outcome = download_and_install(
            &InMemoryDownloader::serving(body),
            &Sha256Verifier::new(),
            &installer,
            &info,
            &dir,
            &recorder.emit(),
        )
        .expect("a refusal is not a failure");

        assert!(matches!(outcome, InstallOutcome::Manual { .. }));
        assert_eq!(recorder.shape().last(), Some(&"ReadyToRestart"));
        assert!(
            archive_path.exists(),
            "the verified archive stays where the user was told it is"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn progress_runs_from_zero_to_a_hundred() {
        let dir = scratch();
        let body = vec![b'x'; 1000];
        let source = InMemoryManifestSource::serving(manifest_for("0.2.0", &body).into_bytes());
        let info = match check(&source, &UpdaterConfig::default(), &v("0.1.6"), &|_| {})
            .expect("checks")
        {
            CheckOutcome::Found(info) => info,
            other => panic!("expected an update, got {other:?}"),
        };

        let recorder = Recorder::default();
        download_and_install(
            &InMemoryDownloader::serving(body),
            &Sha256Verifier::new(),
            &RecordingInstaller::returning(InstallOutcome::Installed),
            &info,
            &dir,
            &recorder.emit(),
        )
        .expect("installs");

        let progress: Vec<DownloadProgress> = recorder
            .events()
            .into_iter()
            .filter_map(|event| match event {
                UpdateEvent::DownloadProgress(progress) => Some(progress),
                _ => None,
            })
            .collect();
        assert_eq!(progress.first().map(|p| p.percent), Some(0));
        assert_eq!(progress.last().map(|p| p.percent), Some(100));
        assert!(
            progress
                .windows(2)
                .all(|pair| pair[0].downloaded <= pair[1].downloaded),
            "progress must not go backwards: {progress:?}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn this_build_reports_a_version_the_updater_can_compare() {
        let version = current_version().expect("build.rs embeds a semantic version");
        assert_eq!(
            version.to_display(),
            crate::build_info::VERSION_INFO.version
        );
    }
}
