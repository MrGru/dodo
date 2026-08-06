//! Where `cleaner-ignored-items.json` lives between sessions.
//!
//! The **eighth** file dodo persists under [`data_dir`](crate::paths::data_dir),
//! and deliberately the same shape as
//! [`consent_store`](crate::api_explorer::services::consent_store) and
//! [`config_store`](crate::quick_nav::services::config_store): a trait, a disk
//! implementation, a temp-file-then-rename write, and a `version` field
//! written from the **first** save with a parser that refuses anything newer.
//! `CLAUDE.md` names that as the pattern to copy, and `collections.json`'s
//! `#[serde(default)]`-only versioning as the one not to.
//!
//! # Why a *higher* version is refused rather than read
//!
//! A kept item is a decision the user made about one specific path. Reading a
//! file from a newer dodo with today's `serde` would take whatever fields
//! still line up and silently drop the rest — and what it could drop is a
//! path the user does not want to see again. Refusing leaves the keep list
//! empty for that path, which is the safe end: the item is not deleted, it
//! just needs "Keep" clicked again.
//!
//! # A missing file is first run, not an error
//!
//! Nothing has been kept yet, so dodo works with no file at all and writes
//! one the first time an orphan candidate is marked "Keep".
//!
//! # Threading
//!
//! Blocking by contract, like every other store here. Always called from the
//! background executor, never the UI thread — Cleaner's scan already runs
//! there, and so does `views::cleaner_view::CleanerView`'s save on "Keep".

use std::path::PathBuf;
#[cfg(test)]
use std::sync::Mutex;

use serde_json::Value;

use crate::cleaner::core::ignore::IgnoredItemsDocument;
use crate::i18n::Str;
use crate::paths::data_dir;

/// Why the keep list could not be read or written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OrphanIgnoreStoreError {
    /// The filesystem, or serde. Carries the underlying English wording.
    Io(String),
    /// A document with no `version` at all. Refused rather than assumed to
    /// be version 1: dodo has written one since the very first save, so a
    /// file without one was written by something else.
    MissingVersion,
    /// A document from a newer dodo.
    UnsupportedVersion { found: u64, understood: u32 },
}

impl OrphanIgnoreStoreError {
    pub fn message(&self) -> Str {
        match self {
            OrphanIgnoreStoreError::Io(detail) => Str::CleanerIgnoreStoreError(detail.clone()),
            OrphanIgnoreStoreError::MissingVersion => Str::CleanerIgnoreStoreMissingVersion,
            OrphanIgnoreStoreError::UnsupportedVersion { found, understood } => {
                Str::CleanerIgnoreStoreUnsupportedVersion {
                    found: *found,
                    understood: *understood,
                }
            }
        }
    }
}

/// A place the orphan-detection "keep" list is loaded from and saved to.
pub trait OrphanIgnoreStore: Send + Sync + 'static {
    fn load(&self) -> Result<IgnoredItemsDocument, OrphanIgnoreStoreError>;
    fn persist(&self, document: &IgnoredItemsDocument) -> Result<(), OrphanIgnoreStoreError>;
}

/// Reads a document, refusing a schema this build does not understand.
///
/// A **failed read fails closed**: the caller keeps an empty keep list, so an
/// item that was marked "Keep" before may need marking again — never the
/// other direction (this never invents an ignored path that was not there).
pub fn parse_document(bytes: &[u8]) -> Result<IgnoredItemsDocument, OrphanIgnoreStoreError> {
    let value: Value =
        serde_json::from_slice(bytes).map_err(|err| OrphanIgnoreStoreError::Io(err.to_string()))?;

    let version = value
        .get("version")
        .and_then(Value::as_u64)
        .ok_or(OrphanIgnoreStoreError::MissingVersion)?;

    if version > u64::from(crate::cleaner::core::ignore::SCHEMA_VERSION) {
        return Err(OrphanIgnoreStoreError::UnsupportedVersion {
            found: version,
            understood: crate::cleaner::core::ignore::SCHEMA_VERSION,
        });
    }

    serde_json::from_value(value).map_err(|err| OrphanIgnoreStoreError::Io(err.to_string()))
}

/// The keep list, as one JSON file under [`data_dir`].
pub struct DiskOrphanIgnoreStore {
    path: PathBuf,
}

impl Default for DiskOrphanIgnoreStore {
    fn default() -> Self {
        Self {
            path: data_dir().join("cleaner-ignored-items.json"),
        }
    }
}

impl DiskOrphanIgnoreStore {
    pub fn new() -> Self {
        Self::default()
    }

    #[cfg(test)]
    pub fn at(path: PathBuf) -> Self {
        Self { path }
    }
}

impl OrphanIgnoreStore for DiskOrphanIgnoreStore {
    fn load(&self) -> Result<IgnoredItemsDocument, OrphanIgnoreStoreError> {
        match std::fs::read(&self.path) {
            Ok(bytes) => parse_document(&bytes).map_err(|error| match error {
                OrphanIgnoreStoreError::Io(detail) => {
                    OrphanIgnoreStoreError::Io(format!("{}: {detail}", self.path.display()))
                }
                other => other,
            }),
            // Nothing kept yet is the ordinary first-run state.
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                Ok(IgnoredItemsDocument::default())
            }
            Err(err) => Err(OrphanIgnoreStoreError::Io(format!(
                "{}: {err}",
                self.path.display()
            ))),
        }
    }

    fn persist(&self, document: &IgnoredItemsDocument) -> Result<(), OrphanIgnoreStoreError> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)
                .map_err(|err| OrphanIgnoreStoreError::Io(format!("{}: {err}", dir.display())))?;
        }
        let json = serde_json::to_vec_pretty(document)
            .map_err(|err| OrphanIgnoreStoreError::Io(err.to_string()))?;

        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, &json)
            .map_err(|err| OrphanIgnoreStoreError::Io(format!("{}: {err}", tmp.display())))?;
        std::fs::rename(&tmp, &self.path)
            .map_err(|err| OrphanIgnoreStoreError::Io(format!("{}: {err}", self.path.display())))?;
        Ok(())
    }
}

/// An in-memory store, for tests.
#[cfg(test)]
#[derive(Default)]
pub struct InMemoryOrphanIgnoreStore {
    document: Mutex<IgnoredItemsDocument>,
}

#[cfg(test)]
impl OrphanIgnoreStore for InMemoryOrphanIgnoreStore {
    fn load(&self) -> Result<IgnoredItemsDocument, OrphanIgnoreStoreError> {
        Ok(self
            .document
            .lock()
            .map(|document| document.clone())
            .unwrap_or_default())
    }

    fn persist(&self, document: &IgnoredItemsDocument) -> Result<(), OrphanIgnoreStoreError> {
        if let Ok(mut held) = self.document.lock() {
            *held = document.clone();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::{
        DiskOrphanIgnoreStore, InMemoryOrphanIgnoreStore, OrphanIgnoreStore,
        OrphanIgnoreStoreError, parse_document,
    };
    use crate::cleaner::core::ignore::{IgnoredItemsDocument, SCHEMA_VERSION};
    use crate::i18n::Language;

    fn temp_path() -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "dodo-cleaner-ignore-store-test-{}-{n}/cleaner-ignored-items.json",
            std::process::id()
        ))
    }

    #[test]
    fn no_file_yet_is_the_defaults_not_an_error() {
        let store = DiskOrphanIgnoreStore::at(temp_path());
        assert_eq!(
            store.load().expect("first run"),
            IgnoredItemsDocument::default()
        );
    }

    /// The claim this file exists to make: a "Keep" decision survives a
    /// restart, with a version in the very first document written.
    #[test]
    fn a_kept_path_survives_a_restart_with_its_version_in_the_file() {
        let path = temp_path();
        let mut document = IgnoredItemsDocument::default();
        document.keep(Path::new("/Users/someone/Library/Caches/Orphan"));

        DiskOrphanIgnoreStore::at(path.clone())
            .persist(&document)
            .expect("persists");

        let written = std::fs::read_to_string(&path).expect("reads back");
        assert!(
            written.contains(&format!("\"version\": {SCHEMA_VERSION}")),
            "no version field in the first file written:\n{written}"
        );

        let loaded = DiskOrphanIgnoreStore::at(path.clone())
            .load()
            .expect("loads");
        assert_eq!(loaded, document);
        assert!(loaded.is_ignored(Path::new("/Users/someone/Library/Caches/Orphan")));

        if let Some(dir) = path.parent() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    #[test]
    fn a_file_from_a_newer_dodo_is_refused_rather_than_misread() {
        let json = format!(
            r#"{{"version":{},"ignored_paths":["/tmp/x"]}}"#,
            SCHEMA_VERSION + 3
        );
        assert_eq!(
            parse_document(json.as_bytes()),
            Err(OrphanIgnoreStoreError::UnsupportedVersion {
                found: u64::from(SCHEMA_VERSION) + 3,
                understood: SCHEMA_VERSION,
            })
        );
    }

    #[test]
    fn a_file_with_no_version_is_refused() {
        assert_eq!(
            parse_document(br#"{"ignored_paths":[]}"#),
            Err(OrphanIgnoreStoreError::MissingVersion)
        );
    }

    #[test]
    fn a_corrupt_file_is_an_error_rather_than_silent_defaults() {
        assert!(matches!(
            parse_document(b"{ not json"),
            Err(OrphanIgnoreStoreError::Io(_))
        ));
    }

    #[test]
    fn the_write_is_atomic_and_leaves_no_temp_file() {
        let path = temp_path();
        DiskOrphanIgnoreStore::at(path.clone())
            .persist(&IgnoredItemsDocument::default())
            .expect("persists");
        assert!(path.exists());
        assert!(
            !path.with_extension("json.tmp").exists(),
            "the temp file has to be renamed, not left beside the real one"
        );

        if let Some(dir) = path.parent() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    #[test]
    fn the_in_memory_store_round_trips() {
        let store = InMemoryOrphanIgnoreStore::default();
        let mut document = IgnoredItemsDocument::default();
        document.keep(Path::new("/tmp/example"));
        store.persist(&document).expect("persists");
        assert_eq!(store.load().expect("loads"), document);
    }

    #[test]
    fn every_failure_says_something_in_every_language() {
        for error in [
            OrphanIgnoreStoreError::Io("disk on fire".to_owned()),
            OrphanIgnoreStoreError::MissingVersion,
            OrphanIgnoreStoreError::UnsupportedVersion {
                found: 9,
                understood: SCHEMA_VERSION,
            },
        ] {
            for language in Language::ALL {
                assert!(!error.message().text(language).trim().is_empty());
            }
        }
    }
}
