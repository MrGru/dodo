//! The gate every install passes through: size, then SHA-256, both read from
//! the file on disk.
//!
//! # Size first, digest second
//!
//! A truncated transfer and a tampered archive are different things to be told,
//! and the cheap check separates them: a file that is not the promised length
//! *cannot* have the promised digest, so hashing it would spend a second to
//! reach a less informative error. Size is `metadata`; the digest is a full
//! read.
//!
//! # Streaming
//!
//! [`CHUNK`] bytes at a time into
//! [`Sha256`](crate::updater::models::sha256::Sha256). A 12 MB archive costs a
//! 64 KiB buffer, and an archive of any size would.
//!
//! # A failure discards the file
//!
//! Nothing that failed verification is left where an installer, or a curious
//! user, could find it. The error names both digests so a genuine release
//! problem can be reported without re-deriving them.

use std::fs::File;
use std::io::Read as _;
use std::path::Path;

use crate::updater::models::manifest::ManifestFile;
use crate::updater::models::sha256::{Sha256, digests_match};
use crate::updater::models::state::UpdateError;
use crate::updater::services::Verifier;

const CHUNK: usize = 64 * 1024;

/// The real verifier. Stateless, so one is shared by everything.
#[derive(Default)]
pub struct Sha256Verifier;

impl Sha256Verifier {
    pub fn new() -> Self {
        Self
    }
}

impl Verifier for Sha256Verifier {
    fn verify(&self, archive: &Path, expected: &ManifestFile) -> Result<(), UpdateError> {
        let actual_size = std::fs::metadata(archive)
            .map_err(|err| UpdateError::Io(format!("{}: {err}", archive.display())))?
            .len();

        if actual_size != expected.size {
            discard(archive);
            return Err(UpdateError::SizeMismatch {
                expected: expected.size,
                actual: actual_size,
            });
        }

        let digest = digest_of(archive)?;
        if !digests_match(&digest, &expected.sha256) {
            discard(archive);
            return Err(UpdateError::ChecksumMismatch {
                expected: expected.sha256.clone(),
                actual: digest,
            });
        }

        Ok(())
    }
}

/// Streams a file through SHA-256 and returns the lowercase hex digest.
///
/// Public because it is useful on its own — it is what a
/// `shasum -a 256` of the archive would print, and the same function verifies a
/// file dodo did not download.
pub fn digest_of(path: &Path) -> Result<String, UpdateError> {
    let mut file =
        File::open(path).map_err(|err| UpdateError::Io(format!("{}: {err}", path.display())))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; CHUNK];

    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|err| UpdateError::Io(format!("{}: {err}", path.display())))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }

    Ok(hasher.finalize_hex())
}

/// Best effort: a file that failed verification must not survive, and failing
/// to delete it is not a reason to report a different error than the real one.
fn discard(archive: &Path) {
    let _ = std::fs::remove_file(archive);
}

#[cfg(test)]
mod tests {
    use super::{Sha256Verifier, digest_of};
    use crate::updater::models::manifest::ManifestFile;
    use crate::updater::models::state::UpdateError;
    use crate::updater::services::Verifier;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn scratch(name: &str) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("dodo-verify-test-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("creates");
        dir.join(name)
    }

    fn manifest_file(sha256: &str, size: u64) -> ManifestFile {
        ManifestFile {
            url: "https://example.test/a.tar.gz".into(),
            sha256: sha256.to_owned(),
            size,
            signature: None,
        }
    }

    /// The digest a `shasum -a 256` of the same bytes would print.
    const ABC_DIGEST: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    #[test]
    fn a_matching_archive_passes() {
        let path = scratch("ok.tar.gz");
        std::fs::write(&path, b"abc").expect("writes");
        assert_eq!(
            Sha256Verifier::new().verify(&path, &manifest_file(ABC_DIGEST, 3)),
            Ok(())
        );
        assert!(path.exists(), "a passing archive stays where it is");
        let _ = std::fs::remove_dir_all(path.parent().expect("has a parent"));
    }

    #[test]
    fn an_uppercase_digest_in_the_manifest_still_matches() {
        let path = scratch("case.tar.gz");
        std::fs::write(&path, b"abc").expect("writes");
        assert_eq!(
            Sha256Verifier::new().verify(&path, &manifest_file(&ABC_DIGEST.to_uppercase(), 3)),
            Ok(())
        );
        let _ = std::fs::remove_dir_all(path.parent().expect("has a parent"));
    }

    /// The one that matters: a tampered or corrupt archive is refused, and is
    /// gone afterwards.
    #[test]
    fn a_checksum_mismatch_is_refused_and_the_file_is_discarded() {
        let path = scratch("bad.tar.gz");
        std::fs::write(&path, b"abd").expect("writes");

        let error = Sha256Verifier::new()
            .verify(&path, &manifest_file(ABC_DIGEST, 3))
            .expect_err("the bytes are not the promised ones");

        match error {
            UpdateError::ChecksumMismatch { expected, actual } => {
                assert_eq!(expected, ABC_DIGEST);
                assert_ne!(actual, ABC_DIGEST);
            }
            other => panic!("expected a checksum mismatch, got {other:?}"),
        }
        assert!(
            !path.exists(),
            "an archive that failed verification must not be left on disk"
        );
        let _ = std::fs::remove_dir_all(path.parent().expect("has a parent"));
    }

    #[test]
    fn a_short_download_is_reported_as_a_size_mismatch_not_a_checksum_one() {
        let path = scratch("short.tar.gz");
        std::fs::write(&path, b"ab").expect("writes");

        assert_eq!(
            Sha256Verifier::new().verify(&path, &manifest_file(ABC_DIGEST, 3)),
            Err(UpdateError::SizeMismatch {
                expected: 3,
                actual: 2
            })
        );
        assert!(!path.exists());
        let _ = std::fs::remove_dir_all(path.parent().expect("has a parent"));
    }

    #[test]
    fn a_missing_file_is_an_io_error_rather_than_a_panic() {
        let path = scratch("absent.tar.gz");
        assert!(matches!(
            Sha256Verifier::new().verify(&path, &manifest_file(ABC_DIGEST, 3)),
            Err(UpdateError::Io(_))
        ));
        let _ = std::fs::remove_dir_all(path.parent().expect("has a parent"));
    }

    /// Streaming over many chunks has to agree with a one-shot hash. The buffer
    /// is 64 KiB, so this crosses it several times.
    #[test]
    fn a_file_larger_than_the_read_buffer_hashes_correctly() {
        let path = scratch("big.bin");
        let body = vec![b'a'; 1_000_000];
        std::fs::write(&path, &body).expect("writes");

        assert_eq!(
            digest_of(&path).expect("reads"),
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0",
            "the million-'a' vector, read through the streaming buffer"
        );
        let _ = std::fs::remove_dir_all(path.parent().expect("has a parent"));
    }

    #[test]
    fn an_empty_file_hashes_to_the_empty_digest() {
        let path = scratch("empty.bin");
        std::fs::write(&path, b"").expect("writes");
        assert_eq!(
            digest_of(&path).expect("reads"),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        let _ = std::fs::remove_dir_all(path.parent().expect("has a parent"));
    }
}
