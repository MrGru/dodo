//! Versioned storage for saved queries and persisted query history.
//!
//! `query-data.json` is one small document because both features are the same
//! user-owned query text and are saved together after an execution or snippet
//! edit. Writes use a sibling temporary file and rename, so a crash cannot
//! replace a valid document with a partial one. All methods are blocking and
//! callers run them on GPUI's background executor.

use std::path::PathBuf;
use std::sync::Mutex;

use serde_json::Value;

use crate::database::models::library::{QueryDataDocument, SCHEMA_VERSION};
use crate::i18n::Str;
use crate::paths::data_dir;

#[derive(Debug)]
pub enum QueryStoreError {
    Io { detail: String },
    MissingVersion,
    UnsupportedVersion { found: u64, supported: u32 },
}

impl QueryStoreError {
    fn io(detail: impl Into<String>) -> Self {
        Self::Io {
            detail: detail.into(),
        }
    }

    pub fn message(&self) -> Str {
        match self {
            Self::Io { detail } => Str::DbQueryStoreError(detail.clone()),
            Self::MissingVersion => Str::DbQueryStoreMissingVersion,
            Self::UnsupportedVersion { found, supported } => Str::DbQueryStoreUnsupportedVersion {
                found: *found,
                supported: *supported,
            },
        }
    }
}

pub trait QueryStore: Send + Sync + 'static {
    fn load(&self) -> Result<QueryDataDocument, QueryStoreError>;
    fn persist(&self, document: &QueryDataDocument) -> Result<(), QueryStoreError>;
}

pub fn parse_document(bytes: &[u8]) -> Result<QueryDataDocument, QueryStoreError> {
    let value: Value =
        serde_json::from_slice(bytes).map_err(|error| QueryStoreError::io(error.to_string()))?;
    let version = value
        .get("version")
        .and_then(Value::as_u64)
        .ok_or(QueryStoreError::MissingVersion)?;
    if version > u64::from(SCHEMA_VERSION) {
        return Err(QueryStoreError::UnsupportedVersion {
            found: version,
            supported: SCHEMA_VERSION,
        });
    }
    serde_json::from_value(value).map_err(|error| QueryStoreError::io(error.to_string()))
}

pub struct DiskQueryStore {
    path: PathBuf,
}

impl Default for DiskQueryStore {
    fn default() -> Self {
        Self {
            path: data_dir().join("query-data.json"),
        }
    }
}

impl DiskQueryStore {
    pub fn new() -> Self {
        Self::default()
    }

    #[cfg(test)]
    pub fn at(path: PathBuf) -> Self {
        Self { path }
    }
}

impl QueryStore for DiskQueryStore {
    fn load(&self) -> Result<QueryDataDocument, QueryStoreError> {
        match std::fs::read(&self.path) {
            Ok(bytes) => parse_document(&bytes).map_err(|error| match error {
                QueryStoreError::Io { detail } => {
                    QueryStoreError::io(format!("{}: {detail}", self.path.display()))
                }
                other => other,
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(QueryDataDocument::default())
            }
            Err(error) => Err(QueryStoreError::io(format!(
                "{}: {error}",
                self.path.display()
            ))),
        }
    }

    fn persist(&self, document: &QueryDataDocument) -> Result<(), QueryStoreError> {
        if let Some(directory) = self.path.parent() {
            std::fs::create_dir_all(directory).map_err(|error| {
                QueryStoreError::io(format!("{}: {error}", directory.display()))
            })?;
        }
        let json = serde_json::to_vec_pretty(document)
            .map_err(|error| QueryStoreError::io(error.to_string()))?;
        let temporary = self.path.with_extension("json.tmp");
        std::fs::write(&temporary, json)
            .map_err(|error| QueryStoreError::io(format!("{}: {error}", temporary.display())))?;
        std::fs::rename(&temporary, &self.path)
            .map_err(|error| QueryStoreError::io(format!("{}: {error}", self.path.display())))?;
        Ok(())
    }
}

#[derive(Default)]
#[allow(dead_code)]
pub struct InMemoryQueryStore {
    document: Mutex<QueryDataDocument>,
}

impl QueryStore for InMemoryQueryStore {
    fn load(&self) -> Result<QueryDataDocument, QueryStoreError> {
        Ok(self
            .document
            .lock()
            .map(|document| document.clone())
            .unwrap_or_default())
    }

    fn persist(&self, document: &QueryDataDocument) -> Result<(), QueryStoreError> {
        if let Ok(mut held) = self.document.lock() {
            *held = document.clone();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{DiskQueryStore, InMemoryQueryStore, QueryStore, QueryStoreError, parse_document};
    use crate::database::models::connection::ConnectionProfile;
    use crate::database::models::engine::Engine;
    use crate::database::models::library::{
        HistoryEntry, HistoryOutcome, QueryDataDocument, QueryScope, SCHEMA_VERSION, SavedQuery,
    };
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn temp_path() -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let number = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "dodo-query-store-test-{}-{number}/query-data.json",
            std::process::id()
        ))
    }

    fn document() -> QueryDataDocument {
        let mut profile = ConnectionProfile::new(4, Engine::PostgreSql);
        profile.name = "Local".into();
        let scope = QueryScope::from_profile(&profile);
        QueryDataDocument {
            saved_queries: vec![SavedQuery {
                id: 1,
                name: "Users".into(),
                statement: "SELECT * FROM users".into(),
                scope: scope.clone(),
            }],
            history: vec![HistoryEntry {
                statement: "SELECT 1".into(),
                scope,
                recorded_at: 42,
                outcome: HistoryOutcome::Succeeded,
                duration_ms: Some(3),
            }],
            ..QueryDataDocument::default()
        }
    }

    #[test]
    fn missing_and_older_documents_migrate_with_defaults() {
        let path = temp_path();
        assert_eq!(
            DiskQueryStore::at(path)
                .load()
                .expect("missing is first run"),
            QueryDataDocument::default()
        );

        let old = parse_document(br#"{"version":0,"saved_queries":[]}"#)
            .expect("older schema loads through serde defaults");
        assert!(old.history.is_empty());
    }

    #[test]
    fn corruption_and_newer_versions_are_refused_without_rewriting() {
        assert!(matches!(
            parse_document(b"not json"),
            Err(QueryStoreError::Io { .. })
        ));
        assert!(matches!(
            parse_document(br#"{"saved_queries":[]}"#),
            Err(QueryStoreError::MissingVersion)
        ));
        let newer = format!(r#"{{"version":{},"history":[]}}"#, SCHEMA_VERSION + 1);
        assert!(matches!(
            parse_document(newer.as_bytes()),
            Err(QueryStoreError::UnsupportedVersion { .. })
        ));
    }

    #[test]
    fn atomic_persistence_round_trips_and_replaces_the_old_document() {
        let path = temp_path();
        let store = DiskQueryStore::at(path.clone());
        store.persist(&document()).expect("first save");
        assert_eq!(store.load().expect("loads"), document());
        assert!(
            std::fs::read_to_string(&path)
                .expect("reads")
                .contains(&format!("\"version\": {SCHEMA_VERSION}"))
        );

        store
            .persist(&QueryDataDocument::default())
            .expect("replacement save");
        assert_eq!(
            DiskQueryStore::at(path.clone()).load().expect("reopens"),
            QueryDataDocument::default()
        );
        assert!(!path.with_extension("json.tmp").exists());
        let _ = std::fs::remove_dir_all(path.parent().expect("parent"));
    }

    #[test]
    fn the_in_memory_store_round_trips() {
        let store = InMemoryQueryStore::default();
        store.persist(&document()).expect("saves");
        assert_eq!(store.load().expect("loads"), document());
    }
}
