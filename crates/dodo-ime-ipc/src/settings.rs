//! `input-method.json`: what dodo tells the input method to type like.
//!
//! **dodo is the only writer.** The bundle reads this file when it starts and
//! again whenever [`SETTINGS_CHANGED`](crate::SETTINGS_CHANGED) arrives, and
//! never writes it — not even to repair it.
//!
//! # Why this is a mirror of `VietnameseConfig` rather than serde on it
//!
//! `dodo-ime-core` forbids serde, by test, so that the engine stays plain Rust
//! for the Windows and Linux hosts (`purity_lint::FORBIDDEN_NAMES` names
//! `serde::` explicitly). A `#[derive(Serialize)]` on `VietnameseConfig` would
//! be one line and would fail that test, correctly.
//!
//! The mirror buys two things beyond satisfying the lint, and they are the
//! reason not to fight it:
//!
//! - **A wire format that is not a Rust type's shape.** The engine's enums are
//!   free to gain variants, be reordered or be renamed; this file's strings are
//!   `"telex"`, `"vni"`, `"modern"`, `"traditional"` and must keep meaning the
//!   same thing to a bundle from a different release. [`InputScheme::code`] is
//!   the engine's own spelling of exactly this idea, written one round before
//!   there was a file to put it in.
//! - **A place to decide what is *not* a setting.** [`OutputMode`] is not in
//!   this document. macOS always has a marked-text channel, so the host always
//!   composes; making it configurable would expose a control whose only working
//!   value is the default. [`VietnameseSettings::to_config`] supplies it.

use dodo_ime_core::{InputScheme, LanguageId, OutputMode, TonePlacement, VietnameseConfig};
use serde::{Deserialize, Serialize};

use crate::document::{IpcError, parse_versioned, read_versioned, write_atomic};

/// The file's name under [`paths::support_dir`](crate::paths::support_dir).
pub const SETTINGS_FILE: &str = "input-method.json";

/// The schema this build writes and is willing to read.
///
/// Bumping this is a decision about *the other process*: a bundle built against
/// version 1 refuses a version 2 file outright and types with its compiled-in
/// defaults until it is updated. So a bump is for a change that would be
/// actively misread, and an added field with a `#[serde(default)]` is not one.
///
/// Version 2 adds the selected input language. A version-1 bundle would ignore
/// it and keep composing Vietnamese, so accepting that file would be a silent
/// misread rather than compatible evolution.
pub const SETTINGS_SCHEMA_VERSION: u32 = 2;

/// How a key sequence becomes Vietnamese, as this file spells it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scheme {
    #[default]
    Telex,
    Vni,
}

impl Scheme {
    pub const ALL: [Scheme; 2] = [Scheme::Telex, Scheme::Vni];

    pub fn of(scheme: InputScheme) -> Scheme {
        match scheme {
            InputScheme::Telex => Scheme::Telex,
            InputScheme::Vni => Scheme::Vni,
        }
    }

    pub fn to_engine(self) -> InputScheme {
        match self {
            Scheme::Telex => InputScheme::Telex,
            Scheme::Vni => InputScheme::Vni,
        }
    }

    /// The string in the file, which is also the value a settings dropdown
    /// carries. It must stay stable when anything else is renamed.
    pub fn code(self) -> &'static str {
        match self {
            Scheme::Telex => "telex",
            Scheme::Vni => "vni",
        }
    }

    pub fn from_code(code: &str) -> Option<Scheme> {
        Scheme::ALL.into_iter().find(|scheme| scheme.code() == code)
    }
}

/// Where the tone mark sits in `hoà`/`hòa`, as this file spells it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tone {
    #[default]
    Modern,
    Traditional,
}

impl Tone {
    pub const ALL: [Tone; 2] = [Tone::Modern, Tone::Traditional];

    pub fn of(placement: TonePlacement) -> Tone {
        match placement {
            TonePlacement::Modern => Tone::Modern,
            TonePlacement::Traditional => Tone::Traditional,
        }
    }

    pub fn to_engine(self) -> TonePlacement {
        match self {
            Tone::Modern => TonePlacement::Modern,
            Tone::Traditional => TonePlacement::Traditional,
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Tone::Modern => "modern",
            Tone::Traditional => "traditional",
        }
    }

    pub fn from_code(code: &str) -> Option<Tone> {
        Tone::ALL.into_iter().find(|tone| tone.code() == code)
    }
}

/// The Vietnamese engine's settings, as a file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct VietnameseSettings {
    pub scheme: Scheme,
    pub tone_placement: Tone,
    pub spell_check: bool,
    pub bracket_shortcuts: bool,
}

impl Default for VietnameseSettings {
    /// Unikey's defaults, which is what a Vietnamese typist's fingers already
    /// expect, and the same values `dodo_ime_macos::DEFAULT_CONFIG` compiles in.
    /// A test in that crate holds the two together.
    fn default() -> VietnameseSettings {
        VietnameseSettings::of(&VietnameseConfig::default())
    }
}

impl VietnameseSettings {
    pub fn of(config: &VietnameseConfig) -> VietnameseSettings {
        VietnameseSettings {
            scheme: Scheme::of(config.scheme),
            tone_placement: Tone::of(config.tone_placement),
            spell_check: config.spell_check,
            bracket_shortcuts: config.bracket_shortcuts,
        }
    }

    /// The engine configuration these settings mean.
    ///
    /// [`OutputMode::Composition`] is supplied here rather than read from the
    /// file: see the module docs for why it is not a setting.
    pub fn to_config(self) -> VietnameseConfig {
        VietnameseConfig {
            scheme: self.scheme.to_engine(),
            tone_placement: self.tone_placement.to_engine(),
            output: OutputMode::Composition,
            spell_check: self.spell_check,
            bracket_shortcuts: self.bracket_shortcuts,
        }
    }
}

/// The whole file.
///
/// `version` is first and mandatory; everything else is `#[serde(default)]` so a
/// file from an older build still loads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingsDocument {
    pub version: u32,
    /// The keyboard input language selected from dodo's menu bar.
    ///
    /// The custom serde adapter keeps `dodo-ime-core` free of serde while
    /// preserving `LanguageId` as the one identity on both sides of IPC.
    #[serde(default, with = "language")]
    pub language: LanguageId,
    /// Bumped by dodo on every write.
    ///
    /// The bundle echoes the revision it has applied into
    /// [`StatusDocument::settings_revision`](crate::status::StatusDocument::settings_revision),
    /// which is the only way either side can say "the change arrived" —
    /// comparing the settings themselves cannot distinguish "applied" from
    /// "happens to agree". It is a counter and not a timestamp because two
    /// writes inside one clock tick are ordinary and a clock that steps
    /// backwards is not this file's problem to solve.
    #[serde(default)]
    pub revision: u64,
    #[serde(default)]
    pub vietnamese: VietnameseSettings,
}

impl Default for SettingsDocument {
    fn default() -> SettingsDocument {
        SettingsDocument {
            version: SETTINGS_SCHEMA_VERSION,
            language: LanguageId::default(),
            revision: 0,
            vietnamese: VietnameseSettings::default(),
        }
    }
}

impl SettingsDocument {
    /// The document as it should be written next, after `previous`.
    ///
    /// dodo calls this rather than setting `revision` by hand, so that "every
    /// write bumps the revision" is one function with one test instead of a rule
    /// each caller has to remember.
    pub fn next(
        previous: &SettingsDocument,
        language: LanguageId,
        vietnamese: VietnameseSettings,
    ) -> SettingsDocument {
        SettingsDocument {
            version: SETTINGS_SCHEMA_VERSION,
            language,
            revision: previous.revision.saturating_add(1),
            vietnamese,
        }
    }

    /// Reads the file, answering `None` when it does not exist yet.
    pub fn read(path: &std::path::Path) -> Result<Option<SettingsDocument>, IpcError> {
        read_versioned(path, SETTINGS_SCHEMA_VERSION)
    }

    /// Reads the file, or the defaults when it is missing.
    ///
    /// This is what the *bundle* wants: there is no user to show an error to and
    /// no state worth holding, so a missing file and a refused file both mean
    /// "type with the defaults". The error is returned as well so the caller can
    /// decide — the bundle discards it, dodo shows it.
    pub fn read_or_default(path: &std::path::Path) -> (SettingsDocument, Option<IpcError>) {
        match SettingsDocument::read(path) {
            Ok(Some(document)) => (document, None),
            Ok(None) => (SettingsDocument::default(), None),
            Err(error) => (SettingsDocument::default(), Some(error)),
        }
    }

    /// Parses bytes. Separate from [`SettingsDocument::read`] so the version
    /// rule can be tested without a disk.
    pub fn parse(bytes: &[u8]) -> Result<SettingsDocument, IpcError> {
        parse_versioned(bytes, SETTINGS_SCHEMA_VERSION)
    }

    /// Writes the file. **dodo only** — the bundle has no business calling this,
    /// and the single-writer rule in the crate docs is what keeps the design
    /// free of locking.
    pub fn write(&self, path: &std::path::Path) -> Result<(), IpcError> {
        write_atomic(path, self)
    }
}

mod language {
    use dodo_ime_core::LanguageId;
    use serde::{Deserialize as _, Deserializer, Serializer, de::Error as _};

    pub fn serialize<S>(language: &LanguageId, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(language.code())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<LanguageId, D::Error>
    where
        D: Deserializer<'de>,
    {
        let code = String::deserialize(deserializer)?;
        LanguageId::from_code(&code)
            .ok_or_else(|| D::Error::custom(format!("unknown input language: {code}")))
    }
}

#[cfg(test)]
mod tests {
    use super::{SETTINGS_SCHEMA_VERSION, Scheme, SettingsDocument, Tone, VietnameseSettings};
    use crate::document::IpcError;
    use dodo_ime_core::{InputScheme, LanguageId, OutputMode, TonePlacement, VietnameseConfig};

    #[test]
    fn a_fresh_document_carries_the_current_version_and_unikeys_defaults() {
        let document = SettingsDocument::default();
        assert_eq!(document.version, SETTINGS_SCHEMA_VERSION);
        assert_eq!(document.revision, 0);
        assert_eq!(document.language, LanguageId::English);
        assert_eq!(document.vietnamese.scheme, Scheme::Telex);
        assert_eq!(document.vietnamese.tone_placement, Tone::Modern);
        assert!(document.vietnamese.spell_check);
        assert!(document.vietnamese.bracket_shortcuts);
    }

    /// The round trip that keeps the mirror honest: every field of
    /// `VietnameseConfig` that is a setting survives, and the one that is not is
    /// restored to the value macOS requires.
    #[test]
    fn every_engine_configuration_survives_the_mirror() {
        for scheme in [InputScheme::Telex, InputScheme::Vni] {
            for tone_placement in [TonePlacement::Modern, TonePlacement::Traditional] {
                for spell_check in [false, true] {
                    for bracket_shortcuts in [false, true] {
                        let config = VietnameseConfig {
                            scheme,
                            tone_placement,
                            output: OutputMode::Composition,
                            spell_check,
                            bracket_shortcuts,
                        };
                        assert_eq!(VietnameseSettings::of(&config).to_config(), config);
                    }
                }
            }
        }
    }

    /// The output mode is not a setting, so a file cannot ask for the direct
    /// mode however it is written.
    #[test]
    fn the_output_mode_is_always_composition() {
        let direct = VietnameseConfig {
            output: OutputMode::Direct,
            ..VietnameseConfig::default()
        };
        assert_eq!(
            VietnameseSettings::of(&direct).to_config().output,
            OutputMode::Composition
        );
    }

    /// The strings in the file are a wire format. A rename here silently stops a
    /// bundle from a different release understanding the setting, so they are
    /// asserted literally.
    #[test]
    fn the_codes_are_the_strings_the_file_carries() {
        assert_eq!(Scheme::Telex.code(), "telex");
        assert_eq!(Scheme::Vni.code(), "vni");
        assert_eq!(Tone::Modern.code(), "modern");
        assert_eq!(Tone::Traditional.code(), "traditional");

        let json = serde_json::to_string(&VietnameseSettings {
            scheme: Scheme::Vni,
            tone_placement: Tone::Traditional,
            spell_check: false,
            bracket_shortcuts: false,
        })
        .unwrap();
        assert_eq!(
            json,
            r#"{"scheme":"vni","tone-placement":"traditional","spell-check":false,"bracket-shortcuts":false}"#
        );
    }

    /// The scheme code and the engine's own `InputScheme::code` were written a
    /// round apart and have to agree, because a settings page shows one and the
    /// file carries the other.
    #[test]
    fn the_scheme_codes_agree_with_the_engines() {
        for scheme in InputScheme::ALL {
            assert_eq!(Scheme::of(scheme).code(), scheme.code());
        }
    }

    #[test]
    fn a_code_round_trips() {
        for scheme in Scheme::ALL {
            assert_eq!(Scheme::from_code(scheme.code()), Some(scheme));
        }
        for tone in Tone::ALL {
            assert_eq!(Tone::from_code(tone.code()), Some(tone));
        }
        assert_eq!(Scheme::from_code("viqr"), None);
        assert_eq!(Tone::from_code(""), None);
    }

    #[test]
    fn each_write_bumps_the_revision() {
        let first = SettingsDocument::default();
        let second = SettingsDocument::next(
            &first,
            LanguageId::Vietnamese,
            VietnameseSettings::default(),
        );
        let third =
            SettingsDocument::next(&second, LanguageId::Japanese, VietnameseSettings::default());
        assert_eq!((second.revision, third.revision), (1, 2));
        assert_eq!(third.version, SETTINGS_SCHEMA_VERSION);
    }

    /// A version-1 document had no language. It remains readable and adopts
    /// the established English menu default rather than guessing from its
    /// Vietnamese engine settings.
    #[test]
    fn a_legacy_file_fills_the_language_in_from_the_default() {
        let document =
            SettingsDocument::parse(br#"{"version":1,"vietnamese":{"scheme":"vni"}}"#).unwrap();
        assert_eq!(document.language, LanguageId::English);
        assert_eq!(document.vietnamese.scheme, Scheme::Vni);
        assert_eq!(document.vietnamese.tone_placement, Tone::Modern);
        assert!(document.vietnamese.spell_check);
        assert_eq!(document.revision, 0);
    }

    #[test]
    fn the_selected_language_survives_ipc_and_unknown_values_are_refused() {
        let document = SettingsDocument::next(
            &SettingsDocument::default(),
            LanguageId::Vietnamese,
            VietnameseSettings::default(),
        );
        let json = serde_json::to_string(&document).unwrap();
        assert!(json.contains(r#""language":"vi""#));
        assert_eq!(SettingsDocument::parse(json.as_bytes()).unwrap(), document);
        assert!(SettingsDocument::parse(br#"{"version":2,"language":"ko"}"#).is_err());
    }

    #[test]
    fn a_newer_schema_is_refused_rather_than_half_read() {
        let error = SettingsDocument::parse(br#"{"version":99,"vietnamese":{"scheme":"telex"}}"#)
            .unwrap_err();
        assert_eq!(
            error,
            IpcError::UnsupportedVersion {
                found: 99,
                supported: SETTINGS_SCHEMA_VERSION
            }
        );
    }

    /// An unknown scheme is a *refusal*, not a fallback to Telex. A future
    /// build's `"viqr"` inside a version-1 file is a file that lies about its
    /// version, and typing Telex under a setting that says otherwise is the
    /// silent misread the version rule exists to prevent.
    #[test]
    fn an_unknown_scheme_is_refused() {
        let error = SettingsDocument::parse(
            br#"{"version":2,"language":"en","vietnamese":{"scheme":"viqr"}}"#,
        )
        .unwrap_err();
        assert!(matches!(error, IpcError::Io { .. }), "{error:?}");
    }

    /// What the bundle does with every one of those failures: types with the
    /// defaults and says nothing.
    #[test]
    fn a_refused_file_reads_as_the_defaults_plus_an_error() {
        let dir = std::env::temp_dir().join(format!("dodo-ime-settings-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("input-method.json");

        std::fs::write(&path, br#"{"version":99}"#).unwrap();
        let (document, error) = SettingsDocument::read_or_default(&path);
        assert_eq!(document, SettingsDocument::default());
        assert!(error.is_some());

        std::fs::write(&path, b"{not json").unwrap();
        let (document, error) = SettingsDocument::read_or_default(&path);
        assert_eq!(document, SettingsDocument::default());
        assert!(matches!(error, Some(IpcError::Io { .. })));

        std::fs::remove_file(&path).unwrap();
        let (document, error) = SettingsDocument::read_or_default(&path);
        assert_eq!(document, SettingsDocument::default());
        assert_eq!(error, None, "a missing file is the first-run state");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_document_written_by_dodo_reads_back_whole() {
        let dir = std::env::temp_dir().join(format!("dodo-ime-settings-rt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("input-method.json");

        let document = SettingsDocument::next(
            &SettingsDocument::default(),
            LanguageId::Vietnamese,
            VietnameseSettings {
                scheme: Scheme::Vni,
                tone_placement: Tone::Traditional,
                spell_check: false,
                bracket_shortcuts: true,
            },
        );
        document.write(&path).unwrap();
        assert_eq!(SettingsDocument::read(&path).unwrap(), Some(document));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
