//! Where script approvals live between sessions.
//!
//! Third file under `data_dir()`, beside `collections.json` and
//! `environments.json`, and deliberately the same shape as
//! [`variable_store`](crate::services::variable_store): a trait,
//! a disk implementation, a temp-file-then-rename write, and a `version` field
//! written from the **first** save with a parser that refuses anything newer.
//! `docs/architecture/persistence.md` names that as the pattern to copy; this is
//! a place where getting
//! it wrong would mean half-reading a record of what the user agreed to run.
//!
//! Why persist at all: an approval that expired every launch would train the
//! user to click through the prompt without reading it, which is the only thing
//! the prompt is for.
//!
//! # Threading
//!
//! Blocking by contract, like every other store here. Always called from the
//! background executor.

use std::path::PathBuf;
use std::sync::Mutex;

use serde_json::Value;

use crate::i18n::{Str, api_scripts};
use crate::models::script_consent::{ConsentDocument, SCHEMA_VERSION};
use crate::paths::data_dir;

/// Why approvals could not be loaded or saved.
#[derive(Debug)]
pub enum ConsentStoreError {
    Io { detail: String },
    MissingVersion,
    UnsupportedVersion { found: u64, supported: u32 },
}

impl ConsentStoreError {
    fn io(detail: impl Into<String>) -> Self {
        Self::Io {
            detail: detail.into(),
        }
    }

    pub fn message(&self) -> Str {
        match self {
            ConsentStoreError::Io { detail } => {
                api_scripts::Text::ConsentStoreError(detail.clone()).into()
            }
            ConsentStoreError::MissingVersion => {
                api_scripts::Text::ConsentStoreMissingVersion.into()
            }
            ConsentStoreError::UnsupportedVersion { found, supported } => {
                api_scripts::Text::ConsentStoreUnsupportedVersion {
                    found: *found,
                    supported: *supported,
                }
                .into()
            }
        }
    }
}

/// A place script approvals are loaded from and saved to.
pub trait ConsentStore: Send + Sync + 'static {
    fn load(&self) -> Result<ConsentDocument, ConsentStoreError>;
    fn persist(&self, document: &ConsentDocument) -> Result<(), ConsentStoreError>;
}

/// Reads a document, refusing a schema this build does not understand.
///
/// A **failed read fails closed**: the caller keeps an empty ledger, so every
/// imported script asks again. That is the safe direction — the alternative is
/// treating an unreadable file as "everything is approved".
pub fn parse_document(bytes: &[u8]) -> Result<ConsentDocument, ConsentStoreError> {
    let value: Value =
        serde_json::from_slice(bytes).map_err(|err| ConsentStoreError::io(err.to_string()))?;

    let version = value
        .get("version")
        .and_then(Value::as_u64)
        .ok_or(ConsentStoreError::MissingVersion)?;

    if version > u64::from(SCHEMA_VERSION) {
        return Err(ConsentStoreError::UnsupportedVersion {
            found: version,
            supported: SCHEMA_VERSION,
        });
    }

    serde_json::from_value(value).map_err(|err| ConsentStoreError::io(err.to_string()))
}

/// The approvals document, stored as one JSON file under `data_dir()`.
pub struct DiskConsentStore {
    path: PathBuf,
}

impl Default for DiskConsentStore {
    fn default() -> Self {
        Self {
            path: data_dir().join("script-consent.json"),
        }
    }
}

impl DiskConsentStore {
    pub fn new() -> Self {
        Self::default()
    }

    #[cfg(test)]
    pub fn at(path: PathBuf) -> Self {
        Self { path }
    }
}

impl ConsentStore for DiskConsentStore {
    fn load(&self) -> Result<ConsentDocument, ConsentStoreError> {
        match std::fs::read(&self.path) {
            Ok(bytes) => parse_document(&bytes).map_err(|error| match error {
                ConsentStoreError::Io { detail } => {
                    ConsentStoreError::io(format!("{}: {detail}", self.path.display()))
                }
                other => other,
            }),
            // Nothing approved yet is the ordinary first-run state.
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                Ok(ConsentDocument::default())
            }
            Err(err) => Err(ConsentStoreError::io(format!(
                "{}: {err}",
                self.path.display()
            ))),
        }
    }

    fn persist(&self, document: &ConsentDocument) -> Result<(), ConsentStoreError> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)
                .map_err(|err| ConsentStoreError::io(format!("{}: {err}", dir.display())))?;
        }
        let json = serde_json::to_vec_pretty(document)
            .map_err(|err| ConsentStoreError::io(err.to_string()))?;

        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, &json)
            .map_err(|err| ConsentStoreError::io(format!("{}: {err}", tmp.display())))?;
        std::fs::rename(&tmp, &self.path)
            .map_err(|err| ConsentStoreError::io(format!("{}: {err}", self.path.display())))?;
        Ok(())
    }
}

/// An in-memory store, for tests and as the session-only fallback behind the
/// same trait.
#[derive(Default)]
#[allow(dead_code)]
pub struct InMemoryConsentStore {
    document: Mutex<ConsentDocument>,
}

impl ConsentStore for InMemoryConsentStore {
    fn load(&self) -> Result<ConsentDocument, ConsentStoreError> {
        Ok(self
            .document
            .lock()
            .map(|document| document.clone())
            .unwrap_or_default())
    }

    fn persist(&self, document: &ConsentDocument) -> Result<(), ConsentStoreError> {
        if let Ok(mut held) = self.document.lock() {
            *held = document.clone();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{ConsentStore, ConsentStoreError, DiskConsentStore, parse_document};
    use crate::models::script_consent::{
        ConsentDocument, ConsentKey, ConsentLedger, SCHEMA_VERSION,
    };
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn temp_path() -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        std::env::temp_dir().join(format!(
            "dodo-consent-store-test-{pid}-{n}/script-consent.json"
        ))
    }

    fn document() -> ConsentDocument {
        let mut ledger = ConsentLedger::default();
        ledger.approve(&ConsentKey::new(Some(4), "console.log(1);", ""));
        ledger.document().clone()
    }

    #[test]
    fn nothing_approved_yet_is_an_empty_document_not_an_error() {
        let store = DiskConsentStore::at(temp_path());
        assert_eq!(store.load().expect("first run"), ConsentDocument::default());
    }

    #[test]
    fn an_approval_survives_a_restart_with_its_version_in_the_file() {
        let path = temp_path();
        DiskConsentStore::at(path.clone())
            .persist(&document())
            .expect("persists");

        let written = std::fs::read_to_string(&path).expect("reads back");
        assert!(
            written.contains(&format!("\"version\": {SCHEMA_VERSION}")),
            "no version field in the first file written:\n{written}"
        );

        let loaded = DiskConsentStore::at(path.clone()).load().expect("loads");
        let mut ledger = ConsentLedger::default();
        ledger.set_document(loaded);
        assert!(ledger.is_approved(&ConsentKey::new(Some(4), "console.log(1);", "")));
        assert!(!ledger.is_approved(&ConsentKey::new(Some(4), "console.log(2);", "")));

        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn a_file_from_a_newer_dodo_is_refused_rather_than_misread() {
        let json = format!(r#"{{"version":{},"approvals":[]}}"#, SCHEMA_VERSION + 3);
        assert!(matches!(
            parse_document(json.as_bytes()),
            Err(ConsentStoreError::UnsupportedVersion { .. })
        ));
    }

    #[test]
    fn a_file_with_no_version_is_refused() {
        assert!(matches!(
            parse_document(br#"{"approvals":[]}"#),
            Err(ConsentStoreError::MissingVersion)
        ));
    }

    #[test]
    fn an_unreadable_file_fails_closed_rather_than_approving_everything() {
        // The error path leaves the caller with an empty ledger; this asserts
        // it *is* an error rather than a document that happens to parse.
        assert!(matches!(
            parse_document(b"{ not json"),
            Err(ConsentStoreError::Io { .. })
        ));
    }
}
