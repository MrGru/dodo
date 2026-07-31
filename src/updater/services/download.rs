//! Streaming an archive to disk.
//!
//! The second of the two modules that may name `reqwest` (see
//! [`manifest_source`](super::manifest_source)).
//!
//! # Why the blocking client, and not `bytes_stream()`
//!
//! `reqwest::Response::bytes_stream()` is async, and it is the obvious way to
//! stream a download — but it needs a tokio runtime, and this app's only tokio
//! runtime is walled inside `docker::services` by an explicit containment rule
//! in `Cargo.toml`. A running dodo already has four of them (one per Docker
//! page); a fifth, for one download, is the wrong trade.
//!
//! The blocking client streams perfectly well: `reqwest::blocking::Response`
//! implements `std::io::Read`, so the socket is read in [`CHUNK`]-sized pieces,
//! each one written straight to the file and fed to the hasher. **The archive is
//! never held in memory** — peak usage is one 64 KiB buffer, whatever the
//! download's size.
//!
//! # Its own, longer timeout
//!
//! `MAX_BODY_BYTES` and `REQUEST_TIMEOUT` from
//! `api_explorer::services::http` are deliberately not reused. That cap exists
//! because the API Explorer *displays* bodies — a reason that does not apply to
//! a file being written to disk — and that timeout is sized for a request a
//! person is watching. A 12 MB archive on a slow connection is a legitimate
//! several-minute transfer, so this client sets [`DOWNLOAD_TIMEOUT`] instead and
//! bounds the transfer by *size* rather than by patience.

use std::fs::File;
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use reqwest::blocking::Client;

use crate::updater::models::manifest::ManifestFile;
use crate::updater::models::sha256::Sha256;
use crate::updater::models::state::{DownloadProgress, UpdateError};
use crate::updater::services::{DownloadedArchive, Downloader, Flow};

/// How much is read from the socket at a time. Large enough that a 12 MB
/// archive is a couple of hundred iterations, small enough that the progress
/// callback fires often enough to animate.
const CHUNK: usize = 64 * 1024;

/// The whole-transfer budget. Generous on purpose: this is a background
/// download the user has agreed to, not a request anyone is watching, and a
/// 12 MB file over a bad connection legitimately takes minutes.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// How much larger than the promised size the transfer is allowed to get before
/// it is abandoned.
///
/// The manifest states an exact size, so anything past it is already wrong; the
/// slack exists only so that the error is "this is not what was promised"
/// rather than a truncation at exactly the boundary. Without a cap at all, a
/// redirect to something enormous would fill the user's disk before the digest
/// got a chance to disagree.
const OVERSHOOT_ALLOWANCE: u64 = 64 * 1024;

const USER_AGENT: &str = concat!("dodo/", env!("CARGO_PKG_VERSION"), " (updater)");

/// The detail an aborted transfer carries. Recognised by
/// [`pipeline`](super::pipeline), which turns a cancelled download into no
/// event at all rather than into an error the user has to dismiss.
pub const CANCELLED: &str = "cancelled";

/// Downloads over HTTPS, in chunks, hashing as it goes.
#[derive(Default)]
pub struct HttpDownloader {
    client: OnceLock<Result<Client, String>>,
}

impl HttpDownloader {
    pub fn new() -> Self {
        Self::default()
    }

    fn client(&self) -> Result<&Client, UpdateError> {
        self.client
            .get_or_init(|| {
                Client::builder()
                    .timeout(DOWNLOAD_TIMEOUT)
                    .connect_timeout(CONNECT_TIMEOUT)
                    .user_agent(USER_AGENT)
                    .build()
                    .map_err(|err| err.to_string())
            })
            .as_ref()
            .map_err(|detail| UpdateError::Network(detail.clone()))
    }
}

impl Downloader for HttpDownloader {
    fn download(
        &self,
        file: &ManifestFile,
        destination: &Path,
        progress: &dyn Fn(DownloadProgress) -> Flow,
    ) -> Result<DownloadedArchive, UpdateError> {
        // `models::manifest::parse` has already refused a non-https URL; this is
        // the belt to that braces, because this function is what turns a URL
        // into an executable on the user's disk.
        if !file.url.starts_with("https://") {
            return Err(UpdateError::Download(file.url.clone()));
        }

        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|err| UpdateError::Io(format!("{}: {err}", parent.display())))?;
        }

        let mut response = self
            .client()?
            .get(&file.url)
            .send()
            .map_err(|err| UpdateError::Download(err.to_string()))?;

        let status = response.status();
        if !status.is_success() {
            return Err(UpdateError::Download(format!(
                "{} {}",
                status.as_u16(),
                status.canonical_reason().unwrap_or_default()
            )));
        }

        let mut out = File::create(destination)
            .map_err(|err| UpdateError::Io(format!("{}: {err}", destination.display())))?;

        let mut hasher = Sha256::new();
        let mut buffer = vec![0u8; CHUNK];
        let mut written: u64 = 0;
        let limit = file.size.saturating_add(OVERSHOOT_ALLOWANCE);

        if progress(DownloadProgress::new(0, file.size)) == Flow::Abort {
            drop(out);
            let _ = std::fs::remove_file(destination);
            return Err(UpdateError::Download(CANCELLED.to_owned()));
        }

        loop {
            let read = response
                .read(&mut buffer)
                .map_err(|err| UpdateError::Download(err.to_string()))?;
            if read == 0 {
                break;
            }

            let chunk = &buffer[..read];
            out.write_all(chunk)
                .map_err(|err| UpdateError::Io(format!("{}: {err}", destination.display())))?;
            hasher.update(chunk);
            written += read as u64;

            if written > limit {
                // Abandon rather than fill the disk. The partial file goes with
                // it: nothing downstream should be able to find it.
                drop(out);
                let _ = std::fs::remove_file(destination);
                return Err(UpdateError::SizeMismatch {
                    expected: file.size,
                    actual: written,
                });
            }

            // The abort path is checked once per chunk, so cancelling a
            // download stops it within 64 KiB rather than at the end.
            if progress(DownloadProgress::new(written, file.size)) == Flow::Abort {
                drop(out);
                let _ = std::fs::remove_file(destination);
                return Err(UpdateError::Download(CANCELLED.to_owned()));
            }
        }

        out.flush()
            .map_err(|err| UpdateError::Io(format!("{}: {err}", destination.display())))?;
        // The bytes have to be on the disk, not in the page cache, before the
        // verifier re-reads them and the installer extracts them.
        out.sync_all()
            .map_err(|err| UpdateError::Io(format!("{}: {err}", destination.display())))?;

        Ok(DownloadedArchive {
            path: destination.to_path_buf(),
            bytes: written,
            sha256: hasher.finalize_hex(),
        })
    }
}

/// A downloader that writes bytes it was handed, for driving the pipeline with
/// no network. A test double only — see
/// [`InMemoryManifestSource`](super::manifest_source::InMemoryManifestSource)
/// for why that makes it `#[cfg(test)]`.
#[cfg(test)]
pub struct InMemoryDownloader {
    body: Result<Vec<u8>, UpdateError>,
    /// Every progress report it emitted, so a test can assert the callback
    /// actually fired and ended at 100%.
    reported: std::sync::Mutex<Vec<DownloadProgress>>,
}

#[cfg(test)]
impl InMemoryDownloader {
    pub fn serving(body: impl Into<Vec<u8>>) -> Self {
        Self {
            body: Ok(body.into()),
            reported: std::sync::Mutex::new(Vec::new()),
        }
    }

    pub fn failing(error: UpdateError) -> Self {
        Self {
            body: Err(error),
            reported: std::sync::Mutex::new(Vec::new()),
        }
    }

    pub fn reported(&self) -> Vec<DownloadProgress> {
        self.reported
            .lock()
            .map(|reports| reports.clone())
            .unwrap_or_default()
    }
}

#[cfg(test)]
impl Downloader for InMemoryDownloader {
    fn download(
        &self,
        file: &ManifestFile,
        destination: &Path,
        progress: &dyn Fn(DownloadProgress) -> Flow,
    ) -> Result<DownloadedArchive, UpdateError> {
        let body = self.body.clone()?;

        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|err| UpdateError::Io(format!("{}: {err}", parent.display())))?;
        }

        // Chunked like the real one, so a test sees more than one progress
        // report and the hashing path is the same.
        let mut hasher = Sha256::new();
        let mut written = 0u64;
        let mut out = File::create(destination)
            .map_err(|err| UpdateError::Io(format!("{}: {err}", destination.display())))?;

        let report = |written: u64, this: &Self| -> Flow {
            let update = DownloadProgress::new(written, file.size);
            if let Ok(mut reports) = this.reported.lock() {
                reports.push(update);
            }
            progress(update)
        };

        let abandon = |out: File, destination: &Path| {
            drop(out);
            let _ = std::fs::remove_file(destination);
            Err(UpdateError::Download(CANCELLED.to_owned()))
        };

        if report(0, self) == Flow::Abort {
            return abandon(out, destination);
        }
        for chunk in body.chunks(8.max(body.len().div_ceil(4))) {
            out.write_all(chunk)
                .map_err(|err| UpdateError::Io(format!("{}: {err}", destination.display())))?;
            hasher.update(chunk);
            written += chunk.len() as u64;
            if report(written, self) == Flow::Abort {
                return abandon(out, destination);
            }
        }

        Ok(DownloadedArchive {
            path: destination.to_path_buf(),
            bytes: written,
            sha256: hasher.finalize_hex(),
        })
    }
}

/// Where a download is staged before it is verified.
///
/// The system temp directory, in a per-process subdirectory, because a
/// half-downloaded archive is not something to leave in the user's data
/// directory — and because [`clean_temp_dir`] can then remove the whole thing.
pub fn temp_dir(pid: u32) -> PathBuf {
    std::env::temp_dir().join(format!("dodo-update-{pid}"))
}

/// Removes a download staging directory and everything in it. Best effort: a
/// leftover temp file is not worth failing an install over, and the system
/// clears the directory eventually anyway.
pub fn clean_temp_dir(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir);
}

#[cfg(test)]
mod tests {
    use super::{InMemoryDownloader, clean_temp_dir, temp_dir};
    use crate::updater::models::manifest::ManifestFile;
    use crate::updater::models::sha256::Sha256;
    use crate::updater::models::state::{DownloadProgress, UpdateError};
    use crate::updater::services::{Downloader, Flow};
    use std::path::PathBuf;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn scratch() -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("dodo-download-test-{}-{n}", std::process::id()))
    }

    fn file_for(body: &[u8]) -> ManifestFile {
        let mut hasher = Sha256::new();
        hasher.update(body);
        ManifestFile {
            url: "https://example.test/dodo.tar.gz".into(),
            sha256: hasher.finalize_hex(),
            size: body.len() as u64,
            signature: None,
        }
    }

    #[test]
    fn the_fake_writes_the_bytes_and_hashes_what_it_wrote() {
        let dir = scratch();
        let path = dir.join("dodo.tar.gz");
        let body = b"a fake archive, long enough to arrive in several chunks".to_vec();
        let file = file_for(&body);

        let downloader = InMemoryDownloader::serving(body.clone());
        let archive = downloader
            .download(&file, &path, &|_| Flow::Continue)
            .expect("the fake never fails");

        assert_eq!(std::fs::read(&path).expect("written"), body);
        assert_eq!(archive.bytes, body.len() as u64);
        assert_eq!(
            archive.sha256, file.sha256,
            "the streamed digest must equal the whole-body digest"
        );

        clean_temp_dir(&dir);
    }

    #[test]
    fn progress_is_reported_and_ends_at_a_hundred_percent() {
        let dir = scratch();
        let path = dir.join("dodo.tar.gz");
        let body = vec![b'x'; 400];
        let file = file_for(&body);

        let seen: Mutex<Vec<DownloadProgress>> = Mutex::new(Vec::new());
        let downloader = InMemoryDownloader::serving(body);
        downloader
            .download(&file, &path, &|update| {
                seen.lock().expect("uncontended").push(update);
                Flow::Continue
            })
            .expect("downloads");

        let seen = seen.into_inner().expect("uncontended");
        assert!(seen.len() > 2, "a chunked download reports more than once");
        assert_eq!(seen.first().map(|p| p.percent), Some(0));
        assert_eq!(seen.last().map(|p| p.percent), Some(100));
        assert_eq!(seen.last().map(|p| p.downloaded), Some(400));
        assert_eq!(
            seen,
            downloader.reported(),
            "the fake records exactly what it emitted"
        );

        clean_temp_dir(&dir);
    }

    #[test]
    fn a_failing_downloader_writes_nothing() {
        let dir = scratch();
        let path = dir.join("dodo.tar.gz");
        let downloader = InMemoryDownloader::failing(UpdateError::Download("reset".into()));

        assert_eq!(
            downloader.download(&file_for(b"x"), &path, &|_| Flow::Continue),
            Err(UpdateError::Download("reset".into()))
        );
        assert!(
            !path.exists(),
            "a failed download must leave no file behind"
        );

        clean_temp_dir(&dir);
    }

    /// Cancellation has to stop the transfer *and* take the partial file with
    /// it: half an archive that nothing verified must not be left where an
    /// installer could find it.
    #[test]
    fn aborting_mid_transfer_stops_it_and_removes_the_partial_file() {
        let dir = scratch();
        let path = dir.join("dodo.tar.gz");
        let body = vec![b'x'; 4000];
        let file = file_for(&body);

        let chunks = std::sync::atomic::AtomicU64::new(0);
        let error = InMemoryDownloader::serving(body)
            .download(&file, &path, &|_| {
                // Continue for the first two reports, then abort — so this
                // stops partway rather than before it starts.
                if chunks.fetch_add(1, Ordering::Relaxed) < 2 {
                    Flow::Continue
                } else {
                    Flow::Abort
                }
            })
            .expect_err("aborted");

        assert!(matches!(error, UpdateError::Download(_)), "{error:?}");
        assert!(
            !path.exists(),
            "an abandoned transfer must not leave a partial archive behind"
        );

        clean_temp_dir(&dir);
    }

    #[test]
    fn aborting_before_the_first_byte_writes_nothing() {
        let dir = scratch();
        let path = dir.join("dodo.tar.gz");
        let body = vec![b'x'; 100];

        assert!(
            InMemoryDownloader::serving(body.clone())
                .download(&file_for(&body), &path, &|_| Flow::Abort)
                .is_err()
        );
        assert!(!path.exists());

        clean_temp_dir(&dir);
    }

    #[test]
    fn the_staging_directory_is_per_process_and_removable() {
        assert_ne!(temp_dir(1), temp_dir(2));
        let dir = temp_dir(std::process::id());
        std::fs::create_dir_all(dir.join("nested")).expect("creates");
        std::fs::write(dir.join("nested/x"), b"x").expect("writes");
        clean_temp_dir(&dir);
        assert!(
            !dir.exists(),
            "cleaning removes the directory and its contents"
        );
    }
}
