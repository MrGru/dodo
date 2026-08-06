//! Where `quick-nav.json` lives between sessions.
//!
//! The seventh file under [`data_dir`](crate::paths::data_dir), and
//! deliberately the same shape as
//! [`config_store`](crate::updater::services::config_store) and
//! [`variable_store`](crate::api_explorer::services::variable_store): a trait,
//! a disk implementation, a temp-file-then-rename write, and a `version` field
//! written from the **first** save with a parser that refuses anything newer.
//! `AGENTS.md` names that as the pattern to copy, and `collections.json`'s
//! `#[serde(default)]`-only versioning as the one not to.
//!
//! # Why a *higher* version is refused rather than read
//!
//! A future dodo might give a pattern an anchoring flag, or turn
//! [`QuickNavDocument::patterns`] into a per-detector object. Reading such a
//! file with today's `serde` would take the parts that still line up and drop
//! the rest — and what it would drop is *the user's own text*. Refusing leaves
//! the defaults in place, which is the safe end: quick navigation on, every
//! detector at its built-in behaviour, and a message saying why.
//!
//! # A missing file is first run, not an error
//!
//! Every key has a default, so dodo works with no file at all and writes one
//! the first time something is changed.
//!
//! # Threading
//!
//! Blocking by contract, like every other store here. Always called from the
//! background executor, never the UI thread.

use std::path::PathBuf;
#[cfg(test)]
use std::sync::Mutex;

use serde_json::Value;

use crate::i18n::Str;
use crate::paths::data_dir;
use crate::quick_nav::models::config::{QuickNavDocument, SCHEMA_VERSION};

/// Why the settings could not be read or written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QuickNavStoreError {
    /// The filesystem, or serde. Carries the underlying English wording.
    Io(String),
    /// A document with no `version` at all. Refused rather than assumed to be
    /// version 1: dodo has written one since the very first save, so a file
    /// without one was written by something else.
    MissingVersion,
    /// A document from a newer dodo.
    UnsupportedVersion { found: u64, understood: u32 },
}

impl QuickNavStoreError {
    pub fn message(&self) -> Str {
        match self {
            QuickNavStoreError::Io(detail) => Str::QuickNavStoreError(detail.clone()),
            QuickNavStoreError::MissingVersion => Str::QuickNavStoreMissingVersion,
            QuickNavStoreError::UnsupportedVersion { found, understood } => {
                Str::QuickNavStoreUnsupportedVersion {
                    found: *found,
                    understood: *understood,
                }
            }
        }
    }
}

/// A place the quick-navigation settings are loaded from and saved to.
pub trait QuickNavConfigStore: Send + Sync + 'static {
    fn load(&self) -> Result<QuickNavDocument, QuickNavStoreError>;
    fn persist(&self, document: &QuickNavDocument) -> Result<(), QuickNavStoreError>;
}

/// Reads a settings document, refusing a schema this build does not understand.
pub fn parse_document(bytes: &[u8]) -> Result<QuickNavDocument, QuickNavStoreError> {
    let value: Value =
        serde_json::from_slice(bytes).map_err(|err| QuickNavStoreError::Io(err.to_string()))?;

    let version = value
        .get("version")
        .and_then(Value::as_u64)
        .ok_or(QuickNavStoreError::MissingVersion)?;

    if version > u64::from(SCHEMA_VERSION) {
        return Err(QuickNavStoreError::UnsupportedVersion {
            found: version,
            understood: SCHEMA_VERSION,
        });
    }

    serde_json::from_value(value).map_err(|err| QuickNavStoreError::Io(err.to_string()))
}

/// The settings, as one JSON file under [`data_dir`].
pub struct DiskQuickNavConfigStore {
    path: PathBuf,
}

impl Default for DiskQuickNavConfigStore {
    fn default() -> Self {
        Self {
            path: data_dir().join("quick-nav.json"),
        }
    }
}

impl DiskQuickNavConfigStore {
    pub fn new() -> Self {
        Self::default()
    }

    #[cfg(test)]
    pub fn at(path: PathBuf) -> Self {
        Self { path }
    }
}

impl QuickNavConfigStore for DiskQuickNavConfigStore {
    fn load(&self) -> Result<QuickNavDocument, QuickNavStoreError> {
        match std::fs::read(&self.path) {
            Ok(bytes) => parse_document(&bytes),
            // No file yet is the ordinary first-run state.
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                Ok(QuickNavDocument::default())
            }
            Err(err) => Err(QuickNavStoreError::Io(format!(
                "{}: {err}",
                self.path.display()
            ))),
        }
    }

    fn persist(&self, document: &QuickNavDocument) -> Result<(), QuickNavStoreError> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)
                .map_err(|err| QuickNavStoreError::Io(format!("{}: {err}", dir.display())))?;
        }
        let json = serde_json::to_vec_pretty(document)
            .map_err(|err| QuickNavStoreError::Io(err.to_string()))?;

        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, &json)
            .map_err(|err| QuickNavStoreError::Io(format!("{}: {err}", tmp.display())))?;
        std::fs::rename(&tmp, &self.path)
            .map_err(|err| QuickNavStoreError::Io(format!("{}: {err}", self.path.display())))?;
        Ok(())
    }
}

/// Settings held in memory. A test double only.
#[cfg(test)]
#[derive(Default)]
pub struct InMemoryQuickNavConfigStore {
    document: Mutex<QuickNavDocument>,
}

#[cfg(test)]
impl QuickNavConfigStore for InMemoryQuickNavConfigStore {
    fn load(&self) -> Result<QuickNavDocument, QuickNavStoreError> {
        Ok(self
            .document
            .lock()
            .map(|document| document.clone())
            .unwrap_or_default())
    }

    fn persist(&self, document: &QuickNavDocument) -> Result<(), QuickNavStoreError> {
        if let Ok(mut held) = self.document.lock() {
            *held = document.clone();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DiskQuickNavConfigStore, InMemoryQuickNavConfigStore, QuickNavConfigStore,
        QuickNavStoreError, parse_document,
    };
    use crate::i18n::Language;
    use crate::quick_nav::models::config::{QuickNavDocument, SCHEMA_VERSION};
    use crate::quick_nav::models::detect::Detector;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn temp_path() -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "dodo-quick-nav-test-{}-{n}/quick-nav.json",
            std::process::id()
        ))
    }

    #[test]
    fn no_file_yet_is_the_defaults_not_an_error() {
        let store = DiskQuickNavConfigStore::at(temp_path());
        assert_eq!(
            store.load().expect("first run"),
            QuickNavDocument::default()
        );
    }

    /// The claim this file exists to make: a pattern the user typed is still
    /// there after a restart, with a version in the very first document written.
    #[test]
    fn a_pattern_survives_a_restart_with_its_version_in_the_file() {
        let path = temp_path();
        let mut document = QuickNavDocument::default();
        document.enabled = false;
        document.set_pattern(Detector::Base64, r"^[A-Za-z0-9+/]{16,}={0,2}$");

        DiskQuickNavConfigStore::at(path.clone())
            .persist(&document)
            .expect("persists");

        let written = std::fs::read_to_string(&path).expect("reads back");
        assert!(
            written.contains(&format!("\"version\": {SCHEMA_VERSION}")),
            "no version field in the first file written:\n{written}"
        );

        let loaded = DiskQuickNavConfigStore::at(path.clone())
            .load()
            .expect("loads");
        assert_eq!(loaded, document);
        assert_eq!(
            loaded.pattern(Detector::Base64),
            r"^[A-Za-z0-9+/]{16,}={0,2}$"
        );
        assert!(!loaded.enabled);

        if let Some(dir) = path.parent() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    #[test]
    fn a_file_from_a_newer_dodo_is_refused_rather_than_misread() {
        let json = format!(r#"{{"version":{},"enabled":false}}"#, SCHEMA_VERSION + 3);
        assert_eq!(
            parse_document(json.as_bytes()),
            Err(QuickNavStoreError::UnsupportedVersion {
                found: u64::from(SCHEMA_VERSION) + 3,
                understood: SCHEMA_VERSION,
            })
        );
    }

    #[test]
    fn a_file_with_no_version_is_refused() {
        assert_eq!(
            parse_document(br#"{"enabled":false}"#),
            Err(QuickNavStoreError::MissingVersion)
        );
    }

    #[test]
    fn a_corrupt_file_is_an_error_rather_than_silent_defaults() {
        assert!(matches!(
            parse_document(b"{ not json"),
            Err(QuickNavStoreError::Io(_))
        ));
    }

    #[test]
    fn the_write_is_atomic_and_leaves_no_temp_file() {
        let path = temp_path();
        DiskQuickNavConfigStore::at(path.clone())
            .persist(&QuickNavDocument::default())
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
        let store = InMemoryQuickNavConfigStore::default();
        let mut document = QuickNavDocument::default();
        document.set_pattern(Detector::Curl, "^curl ");
        store.persist(&document).expect("persists");
        assert_eq!(store.load().expect("loads"), document);
    }

    #[test]
    fn every_failure_says_something_in_every_language() {
        for error in [
            QuickNavStoreError::Io("disk on fire".to_owned()),
            QuickNavStoreError::MissingVersion,
            QuickNavStoreError::UnsupportedVersion {
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
