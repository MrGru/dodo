//! The updater's outside world, behind four traits.
//!
//! - [`manifest_source`] — fetching `update.json`.
//!   [`HttpManifestSource`](manifest_source::HttpManifestSource) is, with
//!   [`download`], **the only new place in dodo that may name `reqwest`**;
//!   `Cargo.toml`'s comment on that dependency names both owners.
//! - [`download`] — streaming an archive to a temp file.
//! - [`verify`] — re-reading that file in chunks and checking it against the
//!   manifest's size and SHA-256.
//! - [`installers`] — [`PlatformInstaller`] and its three implementations, with
//!   the factory that is the module's only `#[cfg(target_os)]`.
//! - [`config_store`] — `updater.json`.
//! - [`pipeline`] — the blocking function that sequences all of the above and
//!   emits [`UpdateEvent`](crate::updater::models::state::UpdateEvent)s.
//! - [`log`] — the two `eprintln!`s that are the updater's whole diagnostic
//!   surface.
//!
//! # Blocking by contract
//!
//! Every method here performs blocking IO and is always called from GPUI's
//! background executor, never the UI thread — the same discipline
//! `Transport::execute` and `DockerEngine` follow. That is also why
//! [`PlatformInstaller`] is a plain trait rather than `async fn`: an `async fn`
//! in a trait is not `dyn`-compatible, and every service seam in this codebase
//! is an `Arc<dyn …>`.
//!
//! # Every trait has an in-memory twin
//!
//! [`manifest_source::InMemoryManifestSource`],
//! [`download::InMemoryDownloader`], [`verify::Sha256Verifier`] (which needs no
//! twin — it is pure over a path) and
//! [`installers::RecordingInstaller`], shaped after
//! `consent_store::InMemoryConsentStore`. They are what let
//! [`pipeline`]'s tests drive a whole check → download → verify → install cycle
//! with **no network at all**.

pub mod config_store;
pub mod download;
pub mod installers;
pub mod log;
pub mod manifest_source;
pub mod pipeline;
pub mod verify;

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::updater::models::manifest::ManifestFile;
use crate::updater::models::state::{DownloadProgress, InstallOutcome, UpdateError};

/// A flag the UI sets and the background job reads.
///
/// Cancellation has to reach *inside* a running transfer, not merely stop the
/// task that is waiting for it: dropping the GPUI task stops the UI listening,
/// but a 12 MB download on a slow link would carry on to completion and then
/// install itself, which is precisely what the user asked not to happen. So the
/// flag is checked between every pipeline stage **and** on every progress
/// callback, which is the granularity of one 64 KiB chunk.
#[derive(Clone, Default)]
pub struct Cancellation(Arc<AtomicBool>);

impl Cancellation {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// What a progress callback tells the downloader to do next.
///
/// Returning a value rather than reading a shared flag inside the downloader
/// keeps the [`Downloader`] trait free of any opinion about *why* a transfer
/// might stop — a test aborts one with a counter, the app aborts one with a
/// [`Cancellation`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flow {
    Continue,
    Abort,
}

/// Where `update.json` comes from.
pub trait ManifestSource: Send + Sync + 'static {
    /// The raw document. Parsing is
    /// [`models::manifest::parse`](crate::updater::models::manifest::parse)'s
    /// job, so a source never has to know the schema — which is what makes a
    /// fake source one line of setup.
    fn fetch(&self, url: &str) -> Result<Vec<u8>, UpdateError>;
}

/// An archive that has been written to disk but not yet trusted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DownloadedArchive {
    pub path: std::path::PathBuf,
    pub bytes: u64,
    /// The digest computed *while streaming*. A cheap early failure — see
    /// [`Downloader`] — and not what the install gates on.
    pub sha256: String,
}

/// Streams one archive to a path.
///
/// **The archive is never held in memory.** It is read from the socket in
/// chunks, each written straight out and fed to a
/// [`Sha256`](crate::updater::models::sha256::Sha256), so a 12 MB download
/// costs one 64 KiB buffer. `api_explorer::services::http::client`'s
/// `MAX_BODY_BYTES` cap is deliberately *not* inherited: that cap exists
/// because the API Explorer displays bodies, and nothing here displays
/// anything.
pub trait Downloader: Send + Sync + 'static {
    /// `progress` is called as bytes arrive, and its answer decides whether the
    /// transfer continues. It is `&dyn Fn` rather than a generic so the trait
    /// stays `dyn`-compatible; the pipeline hands it a closure that forwards a
    /// [`DownloadProgress`] event and consults a [`Cancellation`].
    ///
    /// A [`Flow::Abort`] deletes the partial file and returns an error: half an
    /// archive is worse than none, and nothing downstream should be able to
    /// find one.
    fn download(
        &self,
        file: &ManifestFile,
        destination: &Path,
        progress: &dyn Fn(DownloadProgress) -> Flow,
    ) -> Result<DownloadedArchive, UpdateError>;
}

/// Checks a downloaded archive against what the manifest promised.
///
/// Separate from [`Downloader`], and it **re-reads the file from disk** rather
/// than trusting the digest the download computed. That is not belt and braces:
/// the bytes that matter are the ones that will be extracted, and those are the
/// ones on disk at this moment — not the ones that went past on the socket
/// earlier. It also means a file downloaded by any other means can be checked
/// by the same code.
pub trait Verifier: Send + Sync + 'static {
    fn verify(&self, archive: &Path, expected: &ManifestFile) -> Result<(), UpdateError>;
}

/// Replaces this installation with a verified archive.
///
/// One implementation per platform ([`installers`]), chosen by
/// [`installers::platform_installer`]. Blocking, like everything else here.
///
/// **Nothing in the archive is ever executed.** Extraction runs the *system's*
/// `tar`; the installed binary runs only when the user presses Restart, and
/// only after [`Verifier`] has passed.
pub trait PlatformInstaller: Send + Sync + 'static {
    /// Installs `archive` over this installation.
    ///
    /// Returning [`InstallOutcome::Manual`] is a **success**: the archive is
    /// verified and this machine simply cannot be updated in place. Only a
    /// genuine failure — a broken archive, a rename that failed halfway — is an
    /// `Err`.
    fn install(&self, archive: &Path) -> Result<InstallOutcome, UpdateError>;

    /// Starts the installed binary and leaves it running. The caller quits
    /// immediately afterwards; this does not quit for it.
    fn relaunch(&self) -> Result<(), UpdateError>;

    /// Deletes the files a previous install renamed aside. Called once at
    /// startup, because on Windows the running executable cannot be deleted
    /// while it is running — only renamed — so the deletion has to happen in a
    /// later process.
    fn sweep_stale(&self);
}
