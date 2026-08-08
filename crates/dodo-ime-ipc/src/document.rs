//! The version rule and the atomic write, shared by both files.
//!
//! Everything here is pure but for [`write_atomic`], and every rule the two
//! processes depend on is decided in this file so that it can be decided once.

use std::path::Path;

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

/// Why a file could not be read or written.
///
/// No `Str` and no localization: this crate is linked by the input-method
/// bundle, which must not link `dodo` and has no user interface to show a
/// message in. dodo maps these onto its own `Str` variants at the boundary; the
/// bundle logs nothing at all (see the privacy note on `dodo_ime_macos`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IpcError {
    /// Unreadable, unwritable, or not the JSON it should be. The underlying
    /// `std::io` / `serde_json` message is third-party English, kept verbatim.
    Io { detail: String },
    /// The file carries no `version`, so nothing can be said about how to read
    /// it. Distinct from [`IpcError::Io`] because it is the one malformed shape
    /// that is worth telling the user about specifically: it means the file was
    /// hand-edited or written by something that is not dodo.
    MissingVersion,
    /// Written by a newer build than this one. Refused rather than misread —
    /// see the version rule in the crate docs.
    UnsupportedVersion { found: u64, supported: u32 },
}

impl IpcError {
    pub fn io(detail: impl Into<String>) -> IpcError {
        IpcError::Io {
            detail: detail.into(),
        }
    }
}

impl std::fmt::Display for IpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IpcError::Io { detail } => write!(f, "{detail}"),
            IpcError::MissingVersion => write!(f, "no schema version"),
            IpcError::UnsupportedVersion { found, supported } => {
                write!(f, "schema version {found} is newer than {supported}")
            }
        }
    }
}

/// Reads a versioned document, refusing a schema this build does not
/// understand.
///
/// The order is the whole function, and it is the order `environments.json`'s
/// parser established:
///
/// 1. Parse as generic JSON. A truncated or malformed file stops here, so a
///    half-written file can never be mistaken for a version-less one.
/// 2. Read `version` as a number. Absent, or present as a string or object,
///    is [`IpcError::MissingVersion`].
/// 3. **Refuse anything above `supported`.** A version *below* it is the
///    ordinary forward path and is read with serde's defaults, which is why
///    every field but `version` carries `#[serde(default)]`.
/// 4. Only then deserialize into the typed document.
///
/// Step 3 is the one that matters between two independently versioned processes:
/// the fields this build knows might mean something else in a newer schema, and
/// typing under settings nobody chose is worse than typing under the defaults.
pub fn parse_versioned<T: DeserializeOwned>(bytes: &[u8], supported: u32) -> Result<T, IpcError> {
    let value: Value =
        serde_json::from_slice(bytes).map_err(|err| IpcError::io(err.to_string()))?;

    let version = value
        .get("version")
        .and_then(Value::as_u64)
        .ok_or(IpcError::MissingVersion)?;

    if version > u64::from(supported) {
        return Err(IpcError::UnsupportedVersion {
            found: version,
            supported,
        });
    }

    serde_json::from_value(value).map_err(|err| IpcError::io(err.to_string()))
}

/// Reads a file that may not exist yet, refusing a newer schema.
///
/// A missing file is the ordinary first-run state on both sides, so it answers
/// `Ok(None)` rather than an error. Every other IO failure is an error: a file
/// that exists and cannot be read is a problem worth showing.
pub fn read_versioned<T: DeserializeOwned>(
    path: &Path,
    supported: u32,
) -> Result<Option<T>, IpcError> {
    match std::fs::read(path) {
        Ok(bytes) => parse_versioned(&bytes, supported)
            .map(Some)
            .map_err(|error| match error {
                // Name the file where the path is the useful half of the answer.
                // The version errors are about the contents and read better without
                // it.
                IpcError::Io { detail } => IpcError::io(format!("{}: {detail}", path.display())),
                other => other,
            }),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(IpcError::io(format!("{}: {err}", path.display()))),
    }
}

/// Writes a document so that no reader can ever see half of it.
///
/// Temp file beside the target, then `rename` over it: within one directory
/// `rename(2)` is atomic, so the other process either reads the previous
/// complete file or the new complete file. This is the same shape every store
/// under `src/api_explorer/services/` uses, and here it is load-bearing rather
/// than merely careful — the reader is a *different process* and cannot be
/// asked to wait.
///
/// The temp name carries the process id so that dodo writing settings and the
/// bundle writing status can never collide on a scratch path, even though they
/// never write the same target.
pub fn write_atomic<T: Serialize>(path: &Path, document: &T) -> Result<(), IpcError> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|err| IpcError::io(format!("{}: {err}", dir.display())))?;
    }

    let json = serde_json::to_vec_pretty(document).map_err(|err| IpcError::io(err.to_string()))?;

    let tmp = path.with_extension(format!("json.{}.tmp", std::process::id()));
    std::fs::write(&tmp, &json).map_err(|err| IpcError::io(format!("{}: {err}", tmp.display())))?;
    std::fs::rename(&tmp, path).map_err(|err| {
        // Leave nothing behind if the rename is the step that failed.
        let _ = std::fs::remove_file(&tmp);
        IpcError::io(format!("{}: {err}", path.display()))
    })
}

#[cfg(test)]
mod tests {
    use super::{IpcError, parse_versioned, read_versioned, write_atomic};
    use serde::{Deserialize, Serialize};

    /// A stand-in for either real document: a mandatory version and one
    /// defaulted field, which is the shape both of them have.
    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    struct Probe {
        version: u32,
        #[serde(default)]
        value: String,
    }

    const SUPPORTED: u32 = 2;

    #[test]
    fn the_supported_version_is_read() {
        let probe: Probe = parse_versioned(br#"{"version":2,"value":"x"}"#, SUPPORTED).unwrap();
        assert_eq!(probe.value, "x");
    }

    /// The ordinary forward path: a file written by an older build, read with
    /// serde's defaults for whatever it does not carry.
    #[test]
    fn an_older_version_is_read_with_defaults() {
        let probe: Probe = parse_versioned(br#"{"version":1}"#, SUPPORTED).unwrap();
        assert_eq!(probe.version, 1);
        assert_eq!(probe.value, "");
    }

    /// The rule the whole scheme exists for. Two independently updated
    /// processes means this case is *normal*, not exotic.
    #[test]
    fn a_newer_version_is_refused_by_name() {
        let error = parse_versioned::<Probe>(br#"{"version":3,"value":"x"}"#, SUPPORTED)
            .expect_err("a newer schema must be refused");
        assert_eq!(
            error,
            IpcError::UnsupportedVersion {
                found: 3,
                supported: SUPPORTED
            }
        );
    }

    #[test]
    fn a_much_newer_version_is_still_reported_as_the_number_found() {
        let error = parse_versioned::<Probe>(br#"{"version":9999}"#, SUPPORTED).unwrap_err();
        assert_eq!(
            error,
            IpcError::UnsupportedVersion {
                found: 9999,
                supported: SUPPORTED
            }
        );
    }

    #[test]
    fn a_file_with_no_version_is_refused_as_such() {
        let error = parse_versioned::<Probe>(br#"{"value":"x"}"#, SUPPORTED).unwrap_err();
        assert_eq!(error, IpcError::MissingVersion);
    }

    /// `"version": "1"` is not a version. Reading a string as one would accept
    /// a hand-edited file whose author guessed at the format.
    #[test]
    fn a_version_that_is_not_a_number_is_not_a_version() {
        for body in [
            br#"{"version":"2"}"#.as_slice(),
            br#"{"version":null}"#.as_slice(),
            br#"{"version":{"major":2}}"#.as_slice(),
            br#"{"version":-1}"#.as_slice(),
            br#"{"version":1.5}"#.as_slice(),
        ] {
            assert_eq!(
                parse_versioned::<Probe>(body, SUPPORTED).unwrap_err(),
                IpcError::MissingVersion,
                "{}",
                String::from_utf8_lossy(body)
            );
        }
    }

    /// The half-written-file case. It must fail *before* the version check, so
    /// that a truncated file is never reported as "no version" — which would
    /// send whoever reads the message looking at the wrong thing.
    #[test]
    fn a_truncated_file_fails_as_malformed_json() {
        let whole = br#"{"version":2,"value":"something"}"#;
        for cut in 1..whole.len() {
            let error = parse_versioned::<Probe>(&whole[..cut], SUPPORTED).unwrap_err();
            assert!(
                matches!(error, IpcError::Io { .. }),
                "byte {cut} of a truncated file produced {error:?}"
            );
        }
    }

    #[test]
    fn junk_is_not_json() {
        for body in [
            b"".as_slice(),
            b"not json".as_slice(),
            b"[]".as_slice(),
            b"null".as_slice(),
            b"7".as_slice(),
        ] {
            assert!(
                matches!(
                    parse_versioned::<Probe>(body, SUPPORTED),
                    Err(IpcError::Io { .. }) | Err(IpcError::MissingVersion)
                ),
                "{:?} was accepted",
                String::from_utf8_lossy(body)
            );
        }
    }

    /// A version this build understands, carrying a field whose *type* is wrong.
    /// Refused rather than defaulted: a `value` that is a number means the file
    /// was written by something that does not share this schema.
    #[test]
    fn a_known_version_with_a_wrong_field_type_is_refused() {
        let error = parse_versioned::<Probe>(br#"{"version":2,"value":7}"#, SUPPORTED).unwrap_err();
        assert!(matches!(error, IpcError::Io { .. }), "{error:?}");
    }

    #[test]
    fn a_missing_file_is_not_an_error() {
        let dir = std::env::temp_dir().join(format!("dodo-ime-ipc-absent-{}", std::process::id()));
        let read: Option<Probe> = read_versioned(&dir.join("nothing.json"), SUPPORTED).unwrap();
        assert_eq!(read, None);
    }

    #[test]
    fn a_written_document_reads_back_and_leaves_no_scratch_file() {
        let dir = std::env::temp_dir().join(format!("dodo-ime-ipc-write-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("probe.json");

        let document = Probe {
            version: SUPPORTED,
            value: "written".to_owned(),
        };
        write_atomic(&path, &document).unwrap();

        let read: Option<Probe> = read_versioned(&path, SUPPORTED).unwrap();
        assert_eq!(read, Some(document));

        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name != "probe.json")
            .collect();
        assert!(leftovers.is_empty(), "left behind {leftovers:?}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The directory is created on demand, because the input method can be the
    /// first of the two processes to run on a fresh account.
    #[test]
    fn writing_creates_the_directory() {
        let dir = std::env::temp_dir()
            .join(format!("dodo-ime-ipc-mkdir-{}", std::process::id()))
            .join("nested");
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("probe.json");

        write_atomic(
            &path,
            &Probe {
                version: SUPPORTED,
                value: String::new(),
            },
        )
        .unwrap();
        assert!(path.exists());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
