//! `input-method.json`, behind the same disk/in-memory store seam as dodo's
//! other persisted settings.

use std::path::PathBuf;
use std::sync::Mutex;

use crate::i18n::{Str, input_method};
use crate::models::settings::{SETTINGS_FILE, SettingsDocument};
use crate::paths::data_dir;
use crate::services::document::SettingsError;

/// Turns an IPC failure into something the Input method tool can show.
///
/// The three cases are `environments.json`'s three, because it is the same
/// version rule — but the *message* is different in one way that matters: here a
/// refused file usually means dodo and the installed bundle are different
/// versions, and telling the user to reinstall the input method is actionable
/// where "update dodo" would not be.
pub fn message_for(error: &SettingsError) -> Str {
    match error {
        SettingsError::Io { detail } => input_method::Text::StoreError(detail.clone()).into(),
        SettingsError::MissingVersion => input_method::Text::StoreMissingVersion.into(),
        SettingsError::UnsupportedVersion { found, supported } => {
            input_method::Text::StoreUnsupportedVersion {
                found: *found,
                supported: *supported,
            }
            .into()
        }
    }
}

/// Where dodo's input-method settings are loaded from and saved to.
pub trait InputMethodStore: Send + Sync + 'static {
    /// The saved settings, or the defaults when nothing has been saved yet.
    fn load_settings(&self) -> Result<SettingsDocument, SettingsError>;

    /// Replaces the saved settings.
    fn persist_settings(&self, document: &SettingsDocument) -> Result<(), SettingsError>;
}

/// The file under `data_dir()`.
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
    fn load_settings(&self) -> Result<SettingsDocument, SettingsError> {
        // A missing file is the ordinary first-run state.
        Ok(SettingsDocument::read(&self.directory.join(SETTINGS_FILE))?.unwrap_or_default())
    }

    fn persist_settings(&self, document: &SettingsDocument) -> Result<(), SettingsError> {
        document.write(&self.directory.join(SETTINGS_FILE))
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
}

impl InputMethodStore for InMemoryInputMethodStore {
    fn load_settings(&self) -> Result<SettingsDocument, SettingsError> {
        Ok(self.settings.lock().map(|held| *held).unwrap_or_default())
    }

    fn persist_settings(&self, document: &SettingsDocument) -> Result<(), SettingsError> {
        if let Ok(mut held) = self.settings.lock() {
            *held = *document;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{DiskInputMethodStore, InMemoryInputMethodStore, InputMethodStore, message_for};
    use crate::i18n::{Str, input_method};
    use crate::models::settings::{
        LanguageSwitch, SETTINGS_FILE, SETTINGS_SCHEMA_VERSION, Scheme, SettingsDocument, Shortcut,
        ShortcutKey, ShortcutModifiers, Tone, VietnameseSettings,
    };
    use crate::services::document::SettingsError;
    use dodo_ime_core::LanguageId;

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
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn settings_round_trip_through_the_real_file_name() {
        let dir = scratch("round-trip");
        let store = DiskInputMethodStore::at(dir.clone());
        let document = SettingsDocument {
            language: LanguageId::Vietnamese,
            vietnamese: VietnameseSettings {
                scheme: Scheme::Vni,
                tone_placement: Tone::Traditional,
                spell_check: true,
                bracket_shortcuts: false,
            },
            ..SettingsDocument::default()
        };
        store.persist_settings(&document).unwrap();
        assert!(dir.join(SETTINGS_FILE).exists());
        assert_eq!(store.load_settings().unwrap(), document);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_recorded_shortcut_reloads_exactly() {
        let dir = scratch("shortcut-reload");
        let store = DiskInputMethodStore::at(dir.clone());
        let recorded = LanguageSwitch {
            shortcut: Shortcut {
                modifiers: ShortcutModifiers {
                    control: true,
                    shift: true,
                    ..ShortcutModifiers::NONE
                },
                key: ShortcutKey::Modifiers,
            },
            beep: true,
        };
        let document = SettingsDocument {
            language_switch: recorded,
            ..SettingsDocument::default()
        };
        store.persist_settings(&document).unwrap();
        assert_eq!(store.load_settings().unwrap().language_switch, recorded);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn invalid_versions_are_reported() {
        let dir = scratch("invalid");
        let store = DiskInputMethodStore::at(dir.clone());
        let newer = u64::from(SETTINGS_SCHEMA_VERSION) + 1;
        std::fs::write(dir.join(SETTINGS_FILE), format!(r#"{{"version":{newer}}}"#)).unwrap();
        let error = store.load_settings().unwrap_err();
        assert_eq!(
            error,
            SettingsError::UnsupportedVersion {
                found: newer,
                supported: SETTINGS_SCHEMA_VERSION,
            }
        );
        assert!(matches!(
            message_for(&error),
            Str::InputMethod(input_method::Text::StoreUnsupportedVersion { found, .. }) if found == newer
        ));

        std::fs::write(dir.join(SETTINGS_FILE), br#"{"vietnamese":{}}"#).unwrap();
        assert_eq!(
            store.load_settings().unwrap_err(),
            SettingsError::MissingVersion
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_in_memory_store_answers_the_same_questions() {
        let store = InMemoryInputMethodStore::default();
        let document = SettingsDocument {
            language: LanguageId::Vietnamese,
            ..SettingsDocument::default()
        };
        store.persist_settings(&document).unwrap();
        assert_eq!(store.load_settings().unwrap(), document);
    }
}
