//! dodo's end of the two files, behind a trait so the state layer never learns
//! where they live.
//!
//! The same shape as every other store in dodo — a trait, a disk implementation
//! over `data_dir()`, an in-memory sibling for tests, blocking by contract and
//! always called on the background executor. What is different is that **the
//! parsing and the atomic write are not here**: they are in `dodo-ime-ipc`,
//! because the input-method bundle has to do exactly the same reading with
//! exactly the same version rule, and a second implementation would be a second
//! set of bugs in a place no test could compare.
//!
//! # One store, two files, one writer each
//!
//! [`InputMethodStore`] can write `input-method.json` and can only *read*
//! `input-method-status.json`. That asymmetry is the concurrency design and is
//! deliberately visible in the trait: there is no `persist_status`, because dodo
//! writing the bundle's file would mean two writers and therefore a lock.

use std::path::PathBuf;
use std::sync::Mutex;

use dodo_ime_ipc::document::IpcError;
use dodo_ime_ipc::settings::{SETTINGS_FILE, SettingsDocument};
use dodo_ime_ipc::status::{STATUS_FILE, StatusDocument};

use crate::i18n::Str;
use crate::paths::data_dir;

/// Turns an IPC failure into something the Input method tool can show.
///
/// The three cases are `environments.json`'s three, because it is the same
/// version rule — but the *message* is different in one way that matters: here a
/// refused file usually means dodo and the installed bundle are different
/// versions, and telling the user to reinstall the input method is actionable
/// where "update dodo" would not be.
pub fn message_for(error: &IpcError) -> Str {
    match error {
        IpcError::Io { detail } => Str::InputMethodStoreError(detail.clone()),
        IpcError::MissingVersion => Str::InputMethodStoreMissingVersion,
        IpcError::UnsupportedVersion { found, supported } => {
            Str::InputMethodStoreUnsupportedVersion {
                found: *found,
                supported: *supported,
            }
        }
    }
}

/// Where dodo's input-method settings are loaded from and saved to, and where the
/// bundle's status is read from.
pub trait InputMethodStore: Send + Sync + 'static {
    /// The saved settings, or the defaults when nothing has been saved yet.
    fn load_settings(&self) -> Result<SettingsDocument, IpcError>;

    /// Replaces the saved settings. **dodo is the only writer of this file.**
    fn persist_settings(&self, document: &SettingsDocument) -> Result<(), IpcError>;

    /// What the input-method process last said about itself, or `None` when it
    /// has never run. Read-only: the bundle owns this file.
    fn read_status(&self) -> Result<Option<StatusDocument>, IpcError>;
}

/// The two files under `data_dir()`.
pub struct DiskInputMethodStore {
    directory: PathBuf,
}

impl Default for DiskInputMethodStore {
    fn default() -> Self {
        Self {
            directory: data_dir(),
        }
    }
}

impl DiskInputMethodStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// A store over a specific directory, for tests.
    #[cfg(test)]
    pub fn at(directory: PathBuf) -> Self {
        Self { directory }
    }
}

impl InputMethodStore for DiskInputMethodStore {
    fn load_settings(&self) -> Result<SettingsDocument, IpcError> {
        // A missing file is the ordinary first-run state, not an error, and the
        // defaults are what the bundle would be typing with anyway.
        Ok(SettingsDocument::read(&self.directory.join(SETTINGS_FILE))?.unwrap_or_default())
    }

    fn persist_settings(&self, document: &SettingsDocument) -> Result<(), IpcError> {
        document.write(&self.directory.join(SETTINGS_FILE))
    }

    fn read_status(&self) -> Result<Option<StatusDocument>, IpcError> {
        StatusDocument::read(&self.directory.join(STATUS_FILE))
    }
}

/// A store that keeps the settings in memory. Used by tests; available behind the
/// same trait as the session-only fallback, so nothing in the shipping path
/// constructs it — which is what the `allow` is for, and it comes off the day
/// something wires a settings-only-this-run mode. The same shape and the same
/// reason as `InMemoryVariableStore`.
#[derive(Default)]
#[allow(dead_code)]
pub struct InMemoryInputMethodStore {
    settings: Mutex<SettingsDocument>,
    /// What a bundle would have written. Set by tests; never written by dodo.
    status: Mutex<Option<StatusDocument>>,
}

impl InMemoryInputMethodStore {
    #[cfg(test)]
    pub fn with_status(status: StatusDocument) -> Self {
        Self {
            settings: Mutex::new(SettingsDocument::default()),
            status: Mutex::new(Some(status)),
        }
    }
}

impl InputMethodStore for InMemoryInputMethodStore {
    fn load_settings(&self) -> Result<SettingsDocument, IpcError> {
        Ok(self.settings.lock().map(|held| *held).unwrap_or_default())
    }

    fn persist_settings(&self, document: &SettingsDocument) -> Result<(), IpcError> {
        if let Ok(mut held) = self.settings.lock() {
            *held = *document;
        }
        Ok(())
    }

    fn read_status(&self) -> Result<Option<StatusDocument>, IpcError> {
        Ok(self.status.lock().map(|held| held.clone()).unwrap_or(None))
    }
}

#[cfg(test)]
mod tests {
    use super::{DiskInputMethodStore, InMemoryInputMethodStore, InputMethodStore, message_for};
    use crate::i18n::Str;
    use dodo_ime_ipc::document::IpcError;
    use dodo_ime_ipc::settings::{
        SETTINGS_FILE, Scheme, SettingsDocument, Tone, VietnameseSettings,
    };
    use dodo_ime_ipc::status::{STATUS_FILE, StatusDocument};

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("dodo-im-store-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn nothing_saved_yet_loads_the_defaults() {
        let dir = scratch("empty");
        let store = DiskInputMethodStore::at(dir.clone());

        assert_eq!(store.load_settings().unwrap(), SettingsDocument::default());
        assert_eq!(store.read_status().unwrap(), None);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn settings_round_trip_through_the_real_file_name() {
        let dir = scratch("round-trip");
        let store = DiskInputMethodStore::at(dir.clone());

        let document = SettingsDocument::next(
            &SettingsDocument::default(),
            dodo_ime_core::LanguageId::Vietnamese,
            VietnameseSettings {
                scheme: Scheme::Vni,
                tone_placement: Tone::Traditional,
                spell_check: true,
                bracket_shortcuts: false,
            },
        );
        store.persist_settings(&document).unwrap();

        assert!(dir.join(SETTINGS_FILE).exists(), "the agreed file name");
        assert_eq!(store.load_settings().unwrap(), document);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The version rule, at dodo's end: a file from a newer dodo is refused
    /// rather than half-read, and the user is told which version it was.
    #[test]
    fn a_newer_settings_file_is_refused_and_reported() {
        let dir = scratch("newer");
        let newer = u64::from(dodo_ime_ipc::settings::SETTINGS_SCHEMA_VERSION) + 1;
        std::fs::write(dir.join(SETTINGS_FILE), format!(r#"{{"version":{newer}}}"#)).unwrap();
        let store = DiskInputMethodStore::at(dir.clone());

        let error = store.load_settings().unwrap_err();
        assert_eq!(
            error,
            IpcError::UnsupportedVersion {
                found: newer,
                supported: dodo_ime_ipc::settings::SETTINGS_SCHEMA_VERSION
            }
        );
        assert!(matches!(
            message_for(&error),
            Str::InputMethodStoreUnsupportedVersion { found, .. } if found == newer
        ));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_mangled_settings_file_is_reported_with_the_parsers_own_words() {
        let dir = scratch("mangled");
        std::fs::write(
            dir.join(SETTINGS_FILE),
            format!(
                "{{\"version\":{},",
                dodo_ime_ipc::settings::SETTINGS_SCHEMA_VERSION
            ),
        )
        .unwrap();
        let store = DiskInputMethodStore::at(dir.clone());

        let error = store.load_settings().unwrap_err();
        assert!(matches!(error, IpcError::Io { .. }), "{error:?}");
        assert!(matches!(message_for(&error), Str::InputMethodStoreError(_)));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_with_no_version_is_named_as_such() {
        let dir = scratch("versionless");
        std::fs::write(dir.join(SETTINGS_FILE), br#"{"vietnamese":{}}"#).unwrap();
        let store = DiskInputMethodStore::at(dir.clone());

        assert_eq!(store.load_settings().unwrap_err(), IpcError::MissingVersion);
        assert_eq!(
            message_for(&IpcError::MissingVersion),
            Str::InputMethodStoreMissingVersion
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The bundle's file, read by dodo. Written here by hand because in the real
    /// world the *other process* writes it — which is exactly why dodo has no
    /// method that could.
    #[test]
    fn the_bundles_status_is_read_and_never_written() {
        let dir = scratch("status");
        let store = DiskInputMethodStore::at(dir.clone());

        let status = StatusDocument::now("0.1.0", 4);
        status.write(&dir.join(STATUS_FILE)).unwrap();
        assert_eq!(store.read_status().unwrap(), Some(status));

        // And a status file from a newer bundle is refused, not half-read.
        std::fs::write(dir.join(STATUS_FILE), br#"{"version":99}"#).unwrap();
        assert!(matches!(
            store.read_status(),
            Err(IpcError::UnsupportedVersion { found: 99, .. })
        ));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_in_memory_store_answers_the_same_questions() {
        let store = InMemoryInputMethodStore::default();
        assert_eq!(store.load_settings().unwrap(), SettingsDocument::default());
        assert_eq!(store.read_status().unwrap(), None);

        let document = SettingsDocument::next(
            &SettingsDocument::default(),
            dodo_ime_core::LanguageId::English,
            VietnameseSettings::default(),
        );
        store.persist_settings(&document).unwrap();
        assert_eq!(store.load_settings().unwrap(), document);

        let with_status = InMemoryInputMethodStore::with_status(StatusDocument::now("9.9.9", 2));
        assert_eq!(
            with_status.read_status().unwrap().unwrap().bundle_version,
            "9.9.9"
        );
    }
}
