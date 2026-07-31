//! Where environments and their variables live between sessions.
//!
//! Deliberately the same shape as
//! [`CollectionStore`](crate::api_explorer::services::collection_store::CollectionStore),
//! down to the trait, the in-memory sibling used by tests, the temp-file-then-
//! rename write and the `data_dir()` it writes into — a second persistence
//! mechanism would be a second set of bugs. What is *not* shared is the file:
//! this writes `environments.json` beside `collections.json`, so importing or
//! deleting one cannot disturb the other.
//!
//! # Threading
//!
//! Both methods perform blocking file IO and are **blocking by contract**, like
//! `Transport::execute` and `CollectionStore`. Every caller runs them on GPUI's
//! background executor, never on the UI thread.
//!
//! # The version field is why this module has its own parser
//!
//! `serde_json::from_slice` straight into [`VariableDocument`] would accept a
//! file from a *newer* dodo and read it with whatever fields happen to still
//! line up — the silent-misread failure the schema version exists to prevent.
//! [`parse_document`] therefore reads `version` first and refuses anything
//! above [`SCHEMA_VERSION`] by name, so the user is told to update rather than
//! shown a half-loaded set of environments. It is pure, and unit tested without
//! touching a disk.

use std::path::PathBuf;
use std::sync::Mutex;

use serde_json::Value;

use crate::api_explorer::models::variables::{SCHEMA_VERSION, VariableDocument};
use crate::i18n::Str;
use crate::paths::data_dir;

/// Why environments could not be loaded or saved, in terms the UI can show.
#[derive(Debug)]
pub enum VariableStoreError {
    /// The file is unreadable, unwritable, or not the JSON it should be. The
    /// underlying `std::io` / `serde_json` message is third-party English and
    /// is kept verbatim inside a translated frame, the convention `i18n.rs`
    /// records.
    Io { detail: String },
    /// The file carries no `version` at all, so nothing can be said about how
    /// to read it.
    MissingVersion,
    /// The file was written by a newer dodo. Refused rather than misread.
    UnsupportedVersion { found: u64, supported: u32 },
}

impl VariableStoreError {
    fn io(detail: impl Into<String>) -> Self {
        Self::Io {
            detail: detail.into(),
        }
    }

    /// The message shown when saving or loading environments fails.
    pub fn message(&self) -> Str {
        match self {
            VariableStoreError::Io { detail } => Str::VariableStoreError(detail.clone()),
            VariableStoreError::MissingVersion => Str::VariableStoreMissingVersion,
            VariableStoreError::UnsupportedVersion { found, supported } => {
                Str::VariableStoreUnsupportedVersion {
                    found: *found,
                    supported: *supported,
                }
            }
        }
    }
}

/// A place environments are loaded from and saved to.
pub trait VariableStore: Send + Sync + 'static {
    /// The saved document, or an empty one when nothing has been saved yet.
    fn load(&self) -> Result<VariableDocument, VariableStoreError>;

    /// Replaces the saved document.
    fn persist(&self, document: &VariableDocument) -> Result<(), VariableStoreError>;
}

/// Reads a document, refusing a schema this build does not understand.
///
/// A version *below* [`SCHEMA_VERSION`] is accepted and read with serde's
/// defaults — that is the ordinary forward path, and the reason every field but
/// `version` is `#[serde(default)]`. A version above it is refused: the fields
/// this build knows might mean something else there, and half-loading someone's
/// environments is worse than telling them why nothing loaded.
pub fn parse_document(bytes: &[u8]) -> Result<VariableDocument, VariableStoreError> {
    let value: Value =
        serde_json::from_slice(bytes).map_err(|err| VariableStoreError::io(err.to_string()))?;

    let version = value
        .get("version")
        .and_then(Value::as_u64)
        .ok_or(VariableStoreError::MissingVersion)?;

    if version > u64::from(SCHEMA_VERSION) {
        return Err(VariableStoreError::UnsupportedVersion {
            found: version,
            supported: SCHEMA_VERSION,
        });
    }

    serde_json::from_value(value).map_err(|err| VariableStoreError::io(err.to_string()))
}

/// The environments document, stored as one JSON file under `data_dir()`.
pub struct DiskVariableStore {
    path: PathBuf,
}

impl Default for DiskVariableStore {
    fn default() -> Self {
        Self {
            path: data_dir().join("environments.json"),
        }
    }
}

impl DiskVariableStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// A store backed by a specific file, for tests.
    #[cfg(test)]
    pub fn at(path: PathBuf) -> Self {
        Self { path }
    }
}

impl VariableStore for DiskVariableStore {
    fn load(&self) -> Result<VariableDocument, VariableStoreError> {
        match std::fs::read(&self.path) {
            Ok(bytes) => parse_document(&bytes).map_err(|error| match error {
                // Name the file on the errors where the path is the useful half
                // of the answer; the version errors are about the file's
                // contents and read better without it.
                VariableStoreError::Io { detail } => {
                    VariableStoreError::io(format!("{}: {detail}", self.path.display()))
                }
                other => other,
            }),
            // A missing file is the ordinary first-run state, not an error.
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                Ok(VariableDocument::default())
            }
            Err(err) => Err(VariableStoreError::io(format!(
                "{}: {err}",
                self.path.display()
            ))),
        }
    }

    fn persist(&self, document: &VariableDocument) -> Result<(), VariableStoreError> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)
                .map_err(|err| VariableStoreError::io(format!("{}: {err}", dir.display())))?;
        }
        // Written from the document as held, so the `version` field goes out
        // with the very first save rather than being added later.
        let json = serde_json::to_vec_pretty(document)
            .map_err(|err| VariableStoreError::io(err.to_string()))?;

        // Write to a sibling temp file and rename over the target, so a crash
        // mid-write leaves the previous save intact rather than a half file.
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, &json)
            .map_err(|err| VariableStoreError::io(format!("{}: {err}", tmp.display())))?;
        std::fs::rename(&tmp, &self.path)
            .map_err(|err| VariableStoreError::io(format!("{}: {err}", self.path.display())))?;
        Ok(())
    }
}

/// A store that keeps the document in memory only. Used by tests, and available
/// as the session-only fallback behind the same trait — the app wires up the
/// disk-backed store, so this is not constructed in the shipping path.
#[derive(Default)]
#[allow(dead_code)]
pub struct InMemoryVariableStore {
    document: Mutex<VariableDocument>,
}

impl VariableStore for InMemoryVariableStore {
    fn load(&self) -> Result<VariableDocument, VariableStoreError> {
        Ok(self
            .document
            .lock()
            .map(|document| document.clone())
            .unwrap_or_default())
    }

    fn persist(&self, document: &VariableDocument) -> Result<(), VariableStoreError> {
        if let Ok(mut held) = self.document.lock() {
            *held = document.clone();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DiskVariableStore, InMemoryVariableStore, VariableStore, VariableStoreError, parse_document,
    };
    use crate::api_explorer::models::variables::{
        Environment, SCHEMA_VERSION, Variable, VariableDocument,
    };
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn temp_path() -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        std::env::temp_dir().join(format!("dodo-env-store-test-{pid}-{n}/environments.json"))
    }

    fn document() -> VariableDocument {
        VariableDocument {
            environments: vec![Environment {
                id: 1,
                name: "Staging".into(),
                variables: vec![
                    Variable::new("baseUrl", "https://staging.example.com"),
                    Variable::secret("token", "s3cr3t"),
                ],
            }],
            collection_variables: vec![Variable::new("version", "v1")],
            active_environment: Some(1),
            ..VariableDocument::default()
        }
    }

    #[test]
    fn loading_a_missing_file_is_an_empty_document_not_an_error() {
        let store = DiskVariableStore::at(temp_path());
        let loaded = store.load().expect("no error on first run");
        assert_eq!(loaded, VariableDocument::default());
        assert_eq!(loaded.version, SCHEMA_VERSION);
    }

    #[test]
    fn what_is_persisted_to_disk_is_loaded_back_with_its_version_and_secret_flag() {
        let path = temp_path();
        let store = DiskVariableStore::at(path.clone());
        store.persist(&document()).expect("persists");

        // The version is in the bytes, not merely in the struct's default: this
        // is the assertion that the first file ever written carries it.
        let written = std::fs::read_to_string(&path).expect("reads back");
        assert!(
            written.contains(&format!("\"version\": {SCHEMA_VERSION}")),
            "no version field in the written file:\n{written}"
        );

        // A brand new store at the same path — i.e. the next app launch.
        let reopened = DiskVariableStore::at(path.clone());
        let loaded = reopened.load().expect("loads");
        assert_eq!(loaded, document());

        let token = &loaded.environments[0].variables[1];
        assert!(
            token.secret,
            "the secret flag did not survive the round trip"
        );
        assert_eq!(token.value, "s3cr3t");

        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn a_file_from_a_newer_dodo_is_refused_by_version_rather_than_misread() {
        let json = format!(
            r#"{{"version":{},"environments":[],"somethingNew":42}}"#,
            SCHEMA_VERSION + 9
        );
        match parse_document(json.as_bytes()) {
            Err(VariableStoreError::UnsupportedVersion { found, supported }) => {
                assert_eq!(found, u64::from(SCHEMA_VERSION) + 9);
                assert_eq!(supported, SCHEMA_VERSION);
            }
            other => panic!("expected UnsupportedVersion, got {other:?}"),
        }
    }

    #[test]
    fn a_file_with_no_version_at_all_is_refused() {
        assert!(matches!(
            parse_document(br#"{"environments":[]}"#),
            Err(VariableStoreError::MissingVersion)
        ));
    }

    #[test]
    fn a_file_from_an_older_schema_loads_with_defaults() {
        // Version 0 is below this build's: read it, filling in what is absent.
        let document = parse_document(br#"{"version":0,"environments":[{"id":2,"name":"Old"}]}"#)
            .expect("an older schema is the ordinary forward path and must load");
        assert_eq!(document.environments[0].name, "Old");
        assert!(document.environments[0].variables.is_empty());
        assert!(document.collection_variables.is_empty());
        assert_eq!(document.active_environment, None);
    }

    #[test]
    fn a_file_that_is_not_json_is_an_io_error_with_the_detail_kept() {
        assert!(matches!(
            parse_document(b"not json at all"),
            Err(VariableStoreError::Io { .. })
        ));
    }

    #[test]
    fn the_in_memory_store_round_trips_too() {
        let store = InMemoryVariableStore::default();
        store.persist(&document()).expect("persists");
        assert_eq!(store.load().expect("loads"), document());
    }
}
