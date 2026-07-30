//! SHA-256 over a file, and the sidecar cross-check.
//!
//! Hashing is **streamed**. A release archive is tens of megabytes and the
//! runner is not generous with memory; more to the point there is no reason to
//! hold a file in RAM to digest it, so `read_to_end` is not used anywhere here.

use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

/// 64 KiB: comfortably larger than a page, small enough to stay in L2.
const CHUNK: usize = 64 * 1024;

/// Streams `path` through SHA-256 and returns the lowercase hex digest.
pub fn sha256_file(path: &Path) -> Result<String, String> {
    let file = File::open(path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    let mut reader = BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; CHUNK];

    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }

    Ok(hex(&hasher.finalize()))
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut acc, byte| {
        // Writing into a String cannot fail; the result is discarded rather
        // than unwrapped so this stays free of `.unwrap()`.
        let _ = write!(acc, "{byte:02x}");
        acc
    })
}

/// What a `.sha256` sidecar says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sidecar {
    pub digest: String,
    /// The filename the sidecar names, which must be the archive's own.
    pub file_name: String,
}

/// Reads and parses `<archive>.sha256`.
///
/// The format is the one `sha256sum -c` and `shasum -a 256 -c` read back —
/// `<hex>  <filename>` — because `scripts/package.sh` writes it with those very
/// tools and the release notes tell users to verify with them.
pub fn read_sidecar(sidecar_path: &Path) -> Result<Sidecar, String> {
    let text = std::fs::read_to_string(sidecar_path).map_err(|e| {
        format!(
            "cannot read checksum sidecar {}: {e}",
            sidecar_path.display()
        )
    })?;
    let line = text.lines().next().unwrap_or("").trim();
    if line.is_empty() {
        return Err(format!(
            "checksum sidecar {} is empty",
            sidecar_path.display()
        ));
    }

    let mut fields = line.split_whitespace();
    let digest = fields
        .next()
        .ok_or_else(|| format!("checksum sidecar {} has no digest", sidecar_path.display()))?;
    let name = fields.next().unwrap_or("");

    if digest.len() != 64 || !digest.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!(
            "checksum sidecar {} does not start with a 64-character hex digest (found `{digest}`)",
            sidecar_path.display()
        ));
    }

    Ok(Sidecar {
        digest: digest.to_ascii_lowercase(),
        // A `*name` prefix is `sha256sum`'s binary-mode marker; strip it so the
        // comparison below is against the plain filename.
        file_name: name.trim_start_matches('*').to_string(),
    })
}

/// Hashes `archive` and requires its sidecar to agree.
///
/// Free to run and it catches a truncated or corrupted upload, which is the one
/// failure that would otherwise produce a perfectly well-formed manifest
/// pointing at a file nobody can install.
pub fn verify_against_sidecar(archive: &Path) -> Result<String, String> {
    let Some(file_name) = archive.file_name().and_then(|n| n.to_str()) else {
        return Err(format!(
            "archive path {} has no filename",
            archive.display()
        ));
    };

    let mut sidecar_path = archive.as_os_str().to_os_string();
    sidecar_path.push(".sha256");
    let sidecar_path = Path::new(&sidecar_path);

    let computed = sha256_file(archive)?;
    let sidecar = read_sidecar(sidecar_path)?;

    if sidecar.digest != computed {
        return Err(format!(
            "checksum mismatch for {file_name}: computed {computed}, but {} records {}. \
             The archive was corrupted or replaced after it was packaged; do not publish it.",
            sidecar_path.display(),
            sidecar.digest
        ));
    }

    if !sidecar.file_name.is_empty() && sidecar.file_name != file_name {
        return Err(format!(
            "checksum sidecar {} names `{}` but sits beside `{file_name}`; \
             the sidecars have been shuffled",
            sidecar_path.display(),
            sidecar.file_name
        ));
    }

    Ok(computed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TempDir;

    /// The canonical NIST/FIPS-180 vector, and the empty-input vector. If the
    /// streaming loop ever mis-handles a chunk boundary these are the first
    /// things to break.
    #[test]
    fn hashes_known_vectors() {
        let dir = TempDir::new("hash-vectors");

        let abc = dir.write("abc.txt", b"abc");
        assert_eq!(
            sha256_file(&abc),
            Ok("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".to_string())
        );

        let empty = dir.write("empty.txt", b"");
        assert_eq!(
            sha256_file(&empty),
            Ok("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".to_string())
        );

        let hello = dir.write("hello.txt", b"hello world");
        assert_eq!(
            sha256_file(&hello),
            Ok("b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9".to_string())
        );
    }

    /// Larger than one 64 KiB chunk, so the loop runs more than once. The
    /// expected digest is SHA-256 of 200_000 `a` bytes.
    #[test]
    fn streams_across_chunk_boundaries() {
        let dir = TempDir::new("hash-chunks");
        let big = dir.write("big.bin", &vec![b'a'; 200_000]);
        assert_eq!(
            sha256_file(&big),
            Ok("2287d207f24a941ff3b56c04c8a25ad56b63e3023207b3bb5b4ac0c9869d74be".to_string())
        );
    }

    #[test]
    fn sidecar_parses_the_shasum_layout() {
        let dir = TempDir::new("sidecar-parse");
        let path = dir.write(
            "a.tar.gz.sha256",
            b"b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9  a.tar.gz\n",
        );
        assert_eq!(
            read_sidecar(&path),
            Ok(Sidecar {
                digest: "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
                    .to_string(),
                file_name: "a.tar.gz".to_string(),
            })
        );
    }

    #[test]
    fn sidecar_rejects_a_non_digest() {
        let dir = TempDir::new("sidecar-garbage");
        let path = dir.write("a.tar.gz.sha256", b"wronghash  a.tar.gz\n");
        let err = read_sidecar(&path).expect_err("should reject");
        assert!(err.contains("64-character hex digest"), "{err}");
        assert!(err.contains("wronghash"), "{err}");
    }

    #[test]
    fn verify_accepts_a_matching_sidecar() {
        let dir = TempDir::new("sidecar-ok");
        let archive = dir.write("a.tar.gz", b"hello world");
        dir.write(
            "a.tar.gz.sha256",
            b"b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9  a.tar.gz\n",
        );
        assert_eq!(
            verify_against_sidecar(&archive),
            Ok("b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9".to_string())
        );
    }

    #[test]
    fn verify_rejects_a_disagreeing_sidecar_and_names_both_digests() {
        let dir = TempDir::new("sidecar-mismatch");
        let archive = dir.write("a.tar.gz", b"hello world");
        // A valid-looking digest that is simply not this file's.
        dir.write(
            "a.tar.gz.sha256",
            b"0000000000000000000000000000000000000000000000000000000000000000  a.tar.gz\n",
        );
        let err = verify_against_sidecar(&archive).expect_err("should reject");
        assert!(err.contains("checksum mismatch for a.tar.gz"), "{err}");
        assert!(
            err.contains("b94d27b9934d3e08"),
            "computed digest missing: {err}"
        );
        assert!(
            err.contains("0000000000000000"),
            "sidecar digest missing: {err}"
        );
        assert!(err.contains("do not publish"), "{err}");
    }

    #[test]
    fn verify_rejects_a_shuffled_sidecar() {
        let dir = TempDir::new("sidecar-shuffled");
        let archive = dir.write("a.tar.gz", b"hello world");
        dir.write(
            "a.tar.gz.sha256",
            b"b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9  b.tar.gz\n",
        );
        let err = verify_against_sidecar(&archive).expect_err("should reject");
        assert!(err.contains("shuffled"), "{err}");
    }

    #[test]
    fn verify_reports_a_missing_sidecar_by_path() {
        let dir = TempDir::new("sidecar-missing");
        let archive = dir.write("a.tar.gz", b"hello world");
        let err = verify_against_sidecar(&archive).expect_err("should reject");
        assert!(err.contains("cannot read checksum sidecar"), "{err}");
        assert!(err.contains("a.tar.gz.sha256"), "{err}");
    }
}
