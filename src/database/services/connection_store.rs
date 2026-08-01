//! Where saved connections live between sessions.
//!
//! `connections.json`, the fifth file under `data_dir()`, and deliberately the
//! same shape as `environments.json`: the trait, the in-memory sibling used by
//! tests, the temp-file-then-rename write, and an explicit `"version"` whose
//! parser refuses a file from a newer dodo rather than half-reading it. A
//! second persistence mechanism would be a second set of bugs.
//!
//! It follows `environments.json` / `script-consent.json` / `updater.json` and
//! **not** `collections.json`, whose `#[serde(default)]`-only versioning copes
//! with added fields and nothing else.
//!
//! # Threading
//!
//! Both methods perform blocking file IO and are **blocking by contract**.
//! Every caller runs them on GPUI's background executor, never on the UI
//! thread — which matters more here than usual, because the file holds
//! passwords and is written on every edit.

use std::path::PathBuf;
use std::sync::Mutex;

use serde_json::Value;

use crate::database::models::connection::{ConnectionDocument, SCHEMA_VERSION};
use crate::i18n::Str;
use crate::paths::data_dir;

/// Why connections could not be loaded or saved, in terms the UI can show.
#[derive(Debug)]
pub enum ConnectionStoreError {
    /// Unreadable, unwritable, or not the JSON it should be. The underlying
    /// `std::io` / `serde_json` message is third-party English kept verbatim
    /// inside a translated frame.
    Io { detail: String },
    /// The file carries no `version` at all, so nothing can be said about how
    /// to read it.
    MissingVersion,
    /// Written by a newer dodo. Refused rather than misread.
    UnsupportedVersion { found: u64, supported: u32 },
}

impl ConnectionStoreError {
    fn io(detail: impl Into<String>) -> Self {
        Self::Io {
            detail: detail.into(),
        }
    }

    pub fn message(&self) -> Str {
        match self {
            ConnectionStoreError::Io { detail } => Str::DbConnectionStoreError(detail.clone()),
            ConnectionStoreError::MissingVersion => Str::DbConnectionStoreMissingVersion,
            ConnectionStoreError::UnsupportedVersion { found, supported } => {
                Str::DbConnectionStoreUnsupportedVersion {
                    found: *found,
                    supported: *supported,
                }
            }
        }
    }
}

/// A place saved connections are loaded from and saved to.
pub trait ConnectionStore: Send + Sync + 'static {
    /// The saved document, or an empty one when nothing has been saved yet.
    fn load(&self) -> Result<ConnectionDocument, ConnectionStoreError>;

    /// Replaces the saved document.
    fn persist(&self, document: &ConnectionDocument) -> Result<(), ConnectionStoreError>;
}

/// Reads a document, refusing a schema this build does not understand.
///
/// A version *below* [`SCHEMA_VERSION`] is accepted and read with serde's
/// defaults — the ordinary forward path. A version above it is refused: the
/// fields this build knows might mean something else there, and half-loading
/// someone's connections is worse than telling them why nothing loaded.
pub fn parse_document(bytes: &[u8]) -> Result<ConnectionDocument, ConnectionStoreError> {
    let value: Value =
        serde_json::from_slice(bytes).map_err(|err| ConnectionStoreError::io(err.to_string()))?;

    let version = value
        .get("version")
        .and_then(Value::as_u64)
        .ok_or(ConnectionStoreError::MissingVersion)?;

    if version > u64::from(SCHEMA_VERSION) {
        return Err(ConnectionStoreError::UnsupportedVersion {
            found: version,
            supported: SCHEMA_VERSION,
        });
    }

    serde_json::from_value(value).map_err(|err| ConnectionStoreError::io(err.to_string()))
}

/// The connections document, stored as one JSON file under `data_dir()`.
pub struct DiskConnectionStore {
    path: PathBuf,
}

impl Default for DiskConnectionStore {
    fn default() -> Self {
        Self {
            path: data_dir().join("connections.json"),
        }
    }
}

impl DiskConnectionStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// A store backed by a specific file, for tests.
    #[cfg(test)]
    pub fn at(path: PathBuf) -> Self {
        Self { path }
    }
}

impl ConnectionStore for DiskConnectionStore {
    fn load(&self) -> Result<ConnectionDocument, ConnectionStoreError> {
        match std::fs::read(&self.path) {
            Ok(bytes) => parse_document(&bytes).map_err(|error| match error {
                // Name the file where the path is the useful half of the
                // answer; the version errors are about the contents.
                ConnectionStoreError::Io { detail } => {
                    ConnectionStoreError::io(format!("{}: {detail}", self.path.display()))
                }
                other => other,
            }),
            // A missing file is the ordinary first-run state, not an error.
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                Ok(ConnectionDocument::default())
            }
            Err(err) => Err(ConnectionStoreError::io(format!(
                "{}: {err}",
                self.path.display()
            ))),
        }
    }

    fn persist(&self, document: &ConnectionDocument) -> Result<(), ConnectionStoreError> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)
                .map_err(|err| ConnectionStoreError::io(format!("{}: {err}", dir.display())))?;
        }
        // Written from the document as held, so the `version` field goes out
        // with the very first save rather than being added later.
        let json = serde_json::to_vec_pretty(document)
            .map_err(|err| ConnectionStoreError::io(err.to_string()))?;

        // Write to a sibling temp file and rename over the target, so a crash
        // mid-write leaves the previous save intact rather than a half file.
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, &json)
            .map_err(|err| ConnectionStoreError::io(format!("{}: {err}", tmp.display())))?;
        std::fs::rename(&tmp, &self.path)
            .map_err(|err| ConnectionStoreError::io(format!("{}: {err}", self.path.display())))?;
        Ok(())
    }
}

/// A store that keeps the document in memory only. Used by tests, and available
/// behind the same trait — the app wires up the disk-backed store, so this is
/// not constructed in the shipping path.
#[derive(Default)]
#[allow(dead_code)]
pub struct InMemoryConnectionStore {
    document: Mutex<ConnectionDocument>,
}

impl ConnectionStore for InMemoryConnectionStore {
    fn load(&self) -> Result<ConnectionDocument, ConnectionStoreError> {
        Ok(self
            .document
            .lock()
            .map(|document| document.clone())
            .unwrap_or_default())
    }

    fn persist(&self, document: &ConnectionDocument) -> Result<(), ConnectionStoreError> {
        if let Ok(mut held) = self.document.lock() {
            *held = document.clone();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ConnectionStore, ConnectionStoreError, DiskConnectionStore, InMemoryConnectionStore,
        parse_document,
    };
    use crate::database::models::connection::{
        ConnectionDocument, ConnectionProfile, SCHEMA_VERSION,
    };
    use crate::database::models::engine::Engine;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn temp_path() -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        std::env::temp_dir().join(format!("dodo-conn-store-test-{pid}-{n}/connections.json"))
    }

    fn document() -> ConnectionDocument {
        ConnectionDocument {
            connections: vec![
                ConnectionProfile {
                    name: "Local".into(),
                    database: "shop".into(),
                    password: "s3cr3t".into(),
                    ..ConnectionProfile::new(1, Engine::PostgreSql)
                },
                ConnectionProfile {
                    name: "Notes".into(),
                    file: "/tmp/notes.db".into(),
                    ..ConnectionProfile::new(2, Engine::Sqlite)
                },
            ],
            selected: Some(2),
            ..ConnectionDocument::default()
        }
    }

    #[test]
    fn loading_a_missing_file_is_an_empty_document_not_an_error() {
        let store = DiskConnectionStore::at(temp_path());
        let loaded = store.load().expect("no error on first run");
        assert_eq!(loaded, ConnectionDocument::default());
        assert_eq!(loaded.version, SCHEMA_VERSION);
    }

    #[test]
    fn what_is_persisted_is_loaded_back_with_its_version_and_its_password() {
        let path = temp_path();
        let store = DiskConnectionStore::at(path.clone());
        store.persist(&document()).expect("persists");

        // The version is in the bytes, not merely in the struct's default:
        // this asserts the very first file ever written carries it.
        let written = std::fs::read_to_string(&path).expect("reads back");
        assert!(
            written.contains(&format!("\"version\": {SCHEMA_VERSION}")),
            "no version field in the written file:\n{written}"
        );
        // And the posture this module is honest about: the password really is
        // sitting there in plain text. The test states it so that anyone who
        // later thinks it is encrypted is corrected here.
        assert!(
            written.contains("s3cr3t"),
            "the password is stored unencrypted, and this test says so on purpose"
        );

        // A brand new store at the same path — i.e. the next app launch.
        let reopened = DiskConnectionStore::at(path.clone());
        assert_eq!(reopened.load().expect("loads"), document());

        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn a_file_from_a_newer_dodo_is_refused_by_version_rather_than_misread() {
        let json = format!(
            r#"{{"version":{},"connections":[],"somethingNew":42}}"#,
            SCHEMA_VERSION + 9
        );
        match parse_document(json.as_bytes()) {
            Err(ConnectionStoreError::UnsupportedVersion { found, supported }) => {
                assert_eq!(found, u64::from(SCHEMA_VERSION) + 9);
                assert_eq!(supported, SCHEMA_VERSION);
            }
            other => panic!("expected UnsupportedVersion, got {other:?}"),
        }
    }

    #[test]
    fn a_file_with_no_version_at_all_is_refused() {
        assert!(matches!(
            parse_document(br#"{"connections":[]}"#),
            Err(ConnectionStoreError::MissingVersion)
        ));
    }

    #[test]
    fn a_file_from_an_older_schema_loads_with_defaults() {
        let document =
            parse_document(br#"{"version":0,"connections":[{"id":2,"engine":"sqlite"}]}"#)
                .expect("an older schema is the ordinary forward path and must load");
        assert_eq!(document.connections[0].id, 2);
        assert_eq!(document.connections[0].engine, Engine::Sqlite);
        assert_eq!(document.selected, None);
    }

    #[test]
    fn a_file_that_is_not_json_is_an_io_error_with_the_detail_kept() {
        assert!(matches!(
            parse_document(b"not json at all"),
            Err(ConnectionStoreError::Io { .. })
        ));
    }

    /// The temp-file-then-rename write: a second save must replace the first
    /// completely rather than leaving the longer previous file's tail behind.
    #[test]
    fn a_second_save_replaces_the_first_rather_than_overlapping_it() {
        let path = temp_path();
        let store = DiskConnectionStore::at(path.clone());
        store.persist(&document()).expect("persists");

        let smaller = ConnectionDocument::default();
        store.persist(&smaller).expect("persists again");

        assert_eq!(store.load().expect("loads"), smaller);
        let written = std::fs::read_to_string(&path).expect("reads back");
        assert!(!written.contains("s3cr3t"), "the old contents survived");

        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn the_in_memory_store_round_trips_too() {
        let store = InMemoryConnectionStore::default();
        store.persist(&document()).expect("persists");
        assert_eq!(store.load().expect("loads"), document());
    }
}
