//! Where `session.json` lives between sessions.
//!
//! The eighth file under [`data_dir`](crate::paths::data_dir), and deliberately
//! the same shape as
//! [`config_store`](crate::quick_nav::services::config_store): a trait, a disk
//! implementation, a temp-file-then-rename write, and a `version` field written
//! from the **first** save with a parser that refuses anything newer.
//! `AGENTS.md` names that as the pattern to copy, and `collections.json`'s
//! `#[serde(default)]`-only versioning as the one not to.
//!
//! # Why a *higher* version is refused rather than read
//!
//! A future dodo might restore a tool's own inner tab, or store per-display
//! geometry, or split the window record by monitor UUID. Reading such a file
//! with today's `serde` would take the parts that still line up and drop the
//! rest — and then **write it back pruned** on the next resize, so a newer
//! dodo's session would be destroyed by one launch of an older one. Refusing
//! leaves the file alone and opens on the defaults, which is recoverable.
//!
//! That last part is load-bearing and is why
//! [`SessionStoreError`] reaches the Settings dialog: while a session file is
//! unreadable dodo **stops writing it**, or the refusal would be undone by the
//! first window move. [`crate::session::Session`] holds that rule.
//!
//! # A missing file is first run, not an error
//!
//! Every key is optional, so dodo works with no file at all and writes one the
//! first time anything is changed.
//!
//! # Threading
//!
//! Blocking by contract, like every other store here. Always called from the
//! background executor, never the UI thread — including the flush at quit,
//! which `App::on_app_quit` awaits.

use std::path::PathBuf;
#[cfg(test)]
use std::sync::Mutex;

use serde_json::Value;

use crate::i18n::Str;
use crate::paths::data_dir;
use crate::session::models::document::{SCHEMA_VERSION, SessionDocument};

/// Why the session could not be read or written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionStoreError {
    /// The filesystem, or serde. Carries the underlying English wording.
    Io(String),
    /// A document with no `version` at all. Refused rather than assumed to be
    /// version 1: dodo has written one since the very first save, so a file
    /// without one was written by something else.
    MissingVersion,
    /// A document from a newer dodo.
    UnsupportedVersion { found: u64, understood: u32 },
}

impl SessionStoreError {
    pub fn message(&self) -> Str {
        match self {
            SessionStoreError::Io(detail) => Str::SessionStoreError(detail.clone()),
            SessionStoreError::MissingVersion => Str::SessionStoreMissingVersion,
            SessionStoreError::UnsupportedVersion { found, understood } => {
                Str::SessionStoreUnsupportedVersion {
                    found: *found,
                    understood: *understood,
                }
            }
        }
    }
}

/// A place the session is loaded from and saved to.
pub trait SessionStore: Send + Sync + 'static {
    fn load(&self) -> Result<SessionDocument, SessionStoreError>;
    fn persist(&self, document: &SessionDocument) -> Result<(), SessionStoreError>;
}

/// Reads a session document, refusing a schema this build does not understand.
pub fn parse_document(bytes: &[u8]) -> Result<SessionDocument, SessionStoreError> {
    let value: Value =
        serde_json::from_slice(bytes).map_err(|err| SessionStoreError::Io(err.to_string()))?;

    let version = value
        .get("version")
        .and_then(Value::as_u64)
        .ok_or(SessionStoreError::MissingVersion)?;

    if version > u64::from(SCHEMA_VERSION) {
        return Err(SessionStoreError::UnsupportedVersion {
            found: version,
            understood: SCHEMA_VERSION,
        });
    }

    let mut document: SessionDocument =
        serde_json::from_value(value).map_err(|err| SessionStoreError::Io(err.to_string()))?;

    // **Upgrade in memory, so the next write carries this build's version.**
    // Without this the field is whatever the file said, and dodo writes it
    // straight back — which quietly defeats the refusal above: a key added in
    // version 3 would be written into a file still labelled version 1, where an
    // older build is happy to read it, drop the key it does not know, and save
    // it pruned. That is the exact loss `SCHEMA_VERSION`'s doc says the version
    // exists to prevent, so the stamp is what makes the claim true.
    //
    // The cost is stated there too and is the accepted one: after this write an
    // older dodo refuses the file whole and opens on its defaults, instead of
    // silently eating one setting.
    document.version = SCHEMA_VERSION;
    Ok(document)
}

/// The session, as one JSON file under [`data_dir`].
pub struct DiskSessionStore {
    path: PathBuf,
}

impl Default for DiskSessionStore {
    fn default() -> Self {
        Self {
            path: data_dir().join("session.json"),
        }
    }
}

impl DiskSessionStore {
    pub fn new() -> Self {
        Self::default()
    }

    #[cfg(test)]
    pub fn at(path: PathBuf) -> Self {
        Self { path }
    }
}

impl SessionStore for DiskSessionStore {
    fn load(&self) -> Result<SessionDocument, SessionStoreError> {
        match std::fs::read(&self.path) {
            Ok(bytes) => parse_document(&bytes),
            // No file yet is the ordinary first-run state.
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(SessionDocument::new()),
            Err(err) => Err(SessionStoreError::Io(format!(
                "{}: {err}",
                self.path.display()
            ))),
        }
    }

    fn persist(&self, document: &SessionDocument) -> Result<(), SessionStoreError> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)
                .map_err(|err| SessionStoreError::Io(format!("{}: {err}", dir.display())))?;
        }
        let json = serde_json::to_vec_pretty(document)
            .map_err(|err| SessionStoreError::Io(err.to_string()))?;

        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, &json)
            .map_err(|err| SessionStoreError::Io(format!("{}: {err}", tmp.display())))?;
        std::fs::rename(&tmp, &self.path)
            .map_err(|err| SessionStoreError::Io(format!("{}: {err}", self.path.display())))?;
        Ok(())
    }
}

/// A session held in memory. A test double only.
///
/// It counts its writes, because "a resize drag is one write, not hundreds" is
/// a claim about how often `persist` is *called* — a store that only remembered
/// the last document would let an uncoalesced burst pass.
#[cfg(test)]
#[derive(Default)]
pub struct InMemorySessionStore {
    document: Mutex<SessionDocument>,
    writes: std::sync::atomic::AtomicUsize,
}

#[cfg(test)]
impl InMemorySessionStore {
    pub fn writes(&self) -> usize {
        self.writes.load(std::sync::atomic::Ordering::SeqCst)
    }
}

#[cfg(test)]
impl SessionStore for InMemorySessionStore {
    fn load(&self) -> Result<SessionDocument, SessionStoreError> {
        Ok(self
            .document
            .lock()
            .map(|document| document.clone())
            .unwrap_or_default())
    }

    fn persist(&self, document: &SessionDocument) -> Result<(), SessionStoreError> {
        self.writes
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if let Ok(mut held) = self.document.lock() {
            *held = document.clone();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::{
        DiskSessionStore, InMemorySessionStore, SessionStore, SessionStoreError, parse_document,
    };
    use crate::i18n::Language;
    use crate::session::models::document::{
        SCHEMA_VERSION, SessionDocument, ToolRecord, WindowMode, WindowRecord,
    };

    fn temp_path() -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "dodo-session-test-{}-{n}/session.json",
            std::process::id()
        ))
    }

    fn saved() -> SessionDocument {
        let mut document = SessionDocument::new();
        document.appearance.language = Some("vi".to_owned());
        document.appearance.theme = Some("Ayu Dark".to_owned());
        document.appearance.font_size = Some(18.);
        document.appearance.border_radius = Some(0.);
        document.window = Some(WindowRecord {
            mode: WindowMode::Fullscreen,
            x: 120.,
            y: 80.,
            width: 1000.,
            height: 700.,
            display: Some("6E1E9C3F-0000-0000-0000-000000000001".to_owned()),
        });
        document.workspace.active_tool = Some("database".to_owned());
        document.workspace.sidebar_collapsed = Some(false);
        document.workspace.tools = Some(vec![
            ToolRecord {
                code: "database".to_owned(),
                enabled: true,
            },
            ToolRecord {
                code: "docker".to_owned(),
                enabled: false,
            },
        ]);
        document
    }

    #[test]
    fn no_file_yet_is_the_defaults_not_an_error() {
        let store = DiskSessionStore::at(temp_path());
        assert_eq!(store.load().expect("first run"), SessionDocument::new());
    }

    /// The claim this file exists to make: everything the captain asked for is
    /// still there after a restart, with a version in the very first document
    /// written.
    #[test]
    fn a_session_survives_a_restart_with_its_version_in_the_file() {
        let path = temp_path();
        let document = saved();

        DiskSessionStore::at(path.clone())
            .persist(&document)
            .expect("persists");

        let written = std::fs::read_to_string(&path).expect("reads back");
        assert!(
            written.contains(&format!("\"version\": {SCHEMA_VERSION}")),
            "no version field in the first file written:\n{written}"
        );

        let loaded = DiskSessionStore::at(path.clone()).load().expect("loads");
        assert_eq!(loaded, document);

        if let Some(dir) = path.parent() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    #[test]
    fn a_file_from_a_newer_dodo_is_refused_rather_than_misread() {
        let json = format!(r#"{{"version":{},"workspace":{{}}}}"#, SCHEMA_VERSION + 3);
        assert_eq!(
            parse_document(json.as_bytes()),
            Err(SessionStoreError::UnsupportedVersion {
                found: u64::from(SCHEMA_VERSION) + 3,
                understood: SCHEMA_VERSION,
            })
        );
    }

    /// The other half of the version rule, and the half a bump could break: a
    /// file written by an **older** dodo still loads, with the keys it never
    /// had reading as "never chosen".
    #[test]
    fn a_file_from_an_older_dodo_still_loads() {
        let document = parse_document(
            br#"{"version":1,"appearance":{"theme":"Ayu Dark"},
                 "workspace":{"active_tool":"docker","sidebar_collapsed":true}}"#,
        )
        .expect("a version-1 file is readable");

        assert_eq!(document.appearance.theme.as_deref(), Some("Ayu Dark"));
        assert_eq!(document.workspace.active_tool.as_deref(), Some("docker"));
        assert_eq!(
            document.workspace.tools, None,
            "a file that predates the Features page has not chosen a tool list",
        );
        assert_eq!(
            document.tray.input_language, None,
            "a file that predates the menu bar item has not chosen an input language",
        );
    }

    /// The other side of reading an older file: it comes back **stamped with
    /// this build's version**, so the first save republishes it as a document
    /// this schema owns. Without this a newly added key would be written into a
    /// file an older dodo still believes it understands.
    #[test]
    fn an_older_file_is_upgraded_in_memory_so_the_next_write_declares_this_schema() {
        let document = parse_document(br#"{"version":1,"appearance":{"theme":"Ayu Dark"}}"#)
            .expect("a version-1 file is readable");

        assert_eq!(
            document.version, SCHEMA_VERSION,
            "a document read from an older file must be saved back as this schema"
        );
        assert_eq!(
            document.appearance.theme.as_deref(),
            Some("Ayu Dark"),
            "upgrading the version must not disturb anything else"
        );
    }

    #[test]
    fn a_file_with_no_version_is_refused() {
        assert_eq!(
            parse_document(br#"{"workspace":{"active_tool":"docker"}}"#),
            Err(SessionStoreError::MissingVersion)
        );
    }

    #[test]
    fn a_corrupt_file_is_an_error_rather_than_silent_defaults() {
        assert!(matches!(
            parse_document(b"{ not json"),
            Err(SessionStoreError::Io(_))
        ));
    }

    /// The version is checked before the body, so a file that is *both* too new
    /// and shaped differently reports the version — the actionable half.
    #[test]
    fn a_newer_file_reports_its_version_rather_than_its_shape() {
        let json = format!(
            r#"{{"version":{},"window":"who knows"}}"#,
            SCHEMA_VERSION + 1
        );
        assert!(matches!(
            parse_document(json.as_bytes()),
            Err(SessionStoreError::UnsupportedVersion { .. })
        ));
    }

    #[test]
    fn the_write_is_atomic_and_leaves_no_temp_file() {
        let path = temp_path();
        DiskSessionStore::at(path.clone())
            .persist(&SessionDocument::new())
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
        let store = InMemorySessionStore::default();
        store.persist(&saved()).expect("persists");
        assert_eq!(store.load().expect("loads"), saved());
    }

    #[test]
    fn every_failure_says_something_in_every_language() {
        for error in [
            SessionStoreError::Io("disk on fire".to_owned()),
            SessionStoreError::MissingVersion,
            SessionStoreError::UnsupportedVersion {
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
