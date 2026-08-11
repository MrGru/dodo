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
//!   this document. Native Input Method always composes; Event Tap has no
//!   marked-text client and selects direct output in its own host code. Making
//!   that transport detail configurable would expose controls with no choice.
//!   [`VietnameseSettings::to_config`] supplies the native default.

use dodo_ime_core::{
    ActiveLanguages, InputScheme, Key, KeyEvent, LanguageId, OutputMode, TonePlacement,
    VietnameseConfig,
};
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
/// Version 2 adds the selected input language. Version 3 adds the backend.
/// Version 4 adds Windows' Keyboard Hook backend. A host that cannot understand
/// a selected fallback must refuse the file and pass keys through; accepting it
/// could put two transformers in one input path.
pub const SETTINGS_SCHEMA_VERSION: u32 = 4;

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

/// Which platform-specific host owns Vietnamese transformation.
///
/// The spelling is a compatibility boundary: a native host must refuse a
/// document selecting a fallback it does not own, not compose beside it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Backend {
    /// The InputMethodKit bundle macOS launches independently of dodo.
    #[default]
    Native,
    /// dodo's macOS Accessibility-gated CGEventTap, active only while dodo runs.
    EventTap,
    /// dodo's Windows `WH_KEYBOARD_LL` fallback, active only while dodo runs.
    KeyboardHook,
}

impl Backend {
    pub const ALL: [Backend; 3] = [Backend::Native, Backend::EventTap, Backend::KeyboardHook];

    /// Stable persisted identifier, never a localized label.
    pub fn code(self) -> &'static str {
        match self {
            Backend::Native => "native",
            Backend::EventTap => "event-tap",
            Backend::KeyboardHook => "keyboard-hook",
        }
    }

    pub fn from_code(code: &str) -> Option<Backend> {
        Self::ALL.into_iter().find(|backend| backend.code() == code)
    }
}

/// The non-printing key that changes the selected keyboard language.
///
/// These identities are stable on both macOS and Windows, unlike a physical
/// letter key under Control or Option.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LanguageSwitchKey {
    #[default]
    Space,
    Enter,
    Tab,
    Escape,
}

impl LanguageSwitchKey {
    pub const ALL: [LanguageSwitchKey; 4] = [
        LanguageSwitchKey::Space,
        LanguageSwitchKey::Enter,
        LanguageSwitchKey::Tab,
        LanguageSwitchKey::Escape,
    ];

    fn matches(self, event: &KeyEvent) -> bool {
        matches!(
            (self, event.key),
            (LanguageSwitchKey::Space, Key::Space)
                | (LanguageSwitchKey::Enter, Key::Enter)
                | (LanguageSwitchKey::Tab, Key::Tab)
                | (LanguageSwitchKey::Escape, Key::Escape)
        )
    }
}

/// The shared shortcut that cycles the enabled input languages.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct LanguageSwitch {
    pub key: LanguageSwitchKey,
    pub control: bool,
    pub alt: bool,
    pub shift: bool,
    pub meta: bool,
    pub beep: bool,
}

impl Default for LanguageSwitch {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl LanguageSwitch {
    /// Control-Shift-Space is unlikely to type into an application by accident.
    pub const DEFAULT: Self = Self {
        key: LanguageSwitchKey::Space,
        control: true,
        alt: false,
        shift: true,
        meta: false,
        beep: false,
    };

    /// Whether this is a safe shortcut rather than a plain typing key.
    pub fn has_modifier(self) -> bool {
        self.control || self.alt || self.shift || self.meta
    }

    /// Whether a normalized key press invokes the language switch.
    pub fn matches(self, event: &KeyEvent) -> bool {
        self.has_modifier()
            && self.key.matches(event)
            && self.control == event.modifiers.control
            && self.alt == event.modifiers.alt
            && self.shift == event.modifiers.shift
            && self.meta == event.modifiers.meta
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
    /// file: Native Input Method needs it, while Event Tap overrides it locally.
    /// See the module docs for why it is not a setting.
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
    /// The host that is allowed to transform input.
    #[serde(default)]
    pub backend: Backend,
    /// The keyboard input language selected from dodo's menu bar.
    ///
    /// The custom serde adapter keeps `dodo-ime-core` free of serde while
    /// preserving `LanguageId` as the one identity on both sides of IPC.
    #[serde(default, with = "language")]
    pub language: LanguageId,
    /// The language choices shown in the tray and cycled by the shortcut.
    #[serde(default, with = "active_languages")]
    pub active_languages: ActiveLanguages,
    /// The one cross-platform language-switch shortcut.
    #[serde(default)]
    pub language_switch: LanguageSwitch,
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
            backend: Backend::default(),
            language: LanguageId::default(),
            active_languages: ActiveLanguages::default(),
            language_switch: LanguageSwitch::default(),
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
        Self::next_with_backend(previous, previous.backend, language, vietnamese)
    }

    /// Carries arbitrary edited fields forward while owning the version and revision.
    pub fn next_from(previous: &SettingsDocument, mut next: SettingsDocument) -> SettingsDocument {
        next.version = SETTINGS_SCHEMA_VERSION;
        next.revision = previous.revision.saturating_add(1);
        next
    }

    /// Like [`next`](Self::next), with an explicitly selected host.
    pub fn next_with_backend(
        previous: &SettingsDocument,
        backend: Backend,
        language: LanguageId,
        vietnamese: VietnameseSettings,
    ) -> SettingsDocument {
        SettingsDocument {
            version: SETTINGS_SCHEMA_VERSION,
            backend,
            language,
            revision: previous.revision.saturating_add(1),
            vietnamese,
            ..*previous
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

mod active_languages {
    use dodo_ime_core::{ActiveLanguages, LanguageId};
    use serde::{Deserialize as _, Deserializer, Serializer, de::Error as _};

    pub fn serialize<S>(languages: &ActiveLanguages, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.collect_seq(languages.iter().map(LanguageId::code))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<ActiveLanguages, D::Error>
    where
        D: Deserializer<'de>,
    {
        let codes = Vec::<String>::deserialize(deserializer)?;
        let languages = codes
            .into_iter()
            .map(|code| {
                LanguageId::from_code(&code)
                    .ok_or_else(|| D::Error::custom(format!("unknown input language: {code}")))
            })
            .collect::<Result<Vec<_>, _>>()?;
        ActiveLanguages::from_languages(languages).ok_or_else(|| {
            D::Error::custom("active input languages must be non-empty and distinct")
        })
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
    use super::{
        Backend, LanguageSwitch, LanguageSwitchKey, SETTINGS_SCHEMA_VERSION, Scheme,
        SettingsDocument, Tone, VietnameseSettings,
    };
    use crate::document::IpcError;
    use dodo_ime_core::{
        ActiveLanguages, InputScheme, KeyEvent, LanguageId, Modifiers, OutputMode, TonePlacement,
        VietnameseConfig,
    };

    #[test]
    fn a_fresh_document_carries_the_current_version_and_unikeys_defaults() {
        let document = SettingsDocument::default();
        assert_eq!(document.version, SETTINGS_SCHEMA_VERSION);
        assert_eq!(document.revision, 0);
        assert_eq!(document.backend, Backend::Native);
        assert_eq!(document.language, LanguageId::English);
        assert_eq!(
            document.active_languages.iter().collect::<Vec<_>>(),
            vec![LanguageId::English, LanguageId::Vietnamese]
        );
        assert_eq!(document.language_switch, LanguageSwitch::default());
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
        assert_eq!(Backend::Native.code(), "native");
        assert_eq!(Backend::EventTap.code(), "event-tap");
        assert_eq!(Backend::KeyboardHook.code(), "keyboard-hook");

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
        for backend in Backend::ALL {
            assert_eq!(Backend::from_code(backend.code()), Some(backend));
        }
        assert_eq!(Backend::from_code("keyboard-kit"), None);
    }

    #[test]
    fn keyboard_hook_is_a_stable_persisted_backend() {
        let document = SettingsDocument::next_with_backend(
            &SettingsDocument::default(),
            Backend::KeyboardHook,
            LanguageId::Vietnamese,
            VietnameseSettings::default(),
        );
        let json = serde_json::to_vec(&document).unwrap();
        assert!(
            json.windows(b"keyboard-hook".len())
                .any(|part| part == b"keyboard-hook")
        );
        assert_eq!(SettingsDocument::parse(&json).unwrap(), document);
    }

    #[test]
    fn editing_a_document_keeps_new_language_settings_and_bumps_the_revision() {
        let previous = SettingsDocument::default();
        let mut edited = previous;
        edited.active_languages = ActiveLanguages::from_languages(LanguageId::ALL).unwrap();
        edited.language_switch.beep = true;
        let next = SettingsDocument::next_from(&previous, edited);
        assert_eq!(next.revision, 1);
        assert_eq!(next.active_languages, edited.active_languages);
        assert!(next.language_switch.beep);
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
        assert_eq!(third.backend, Backend::Native);
    }

    /// A version-1 document had no language. It remains readable and adopts
    /// the established English menu default rather than guessing from its
    /// Vietnamese engine settings.
    #[test]
    fn a_legacy_file_fills_the_language_in_from_the_default() {
        let document =
            SettingsDocument::parse(br#"{"version":1,"vietnamese":{"scheme":"vni"}}"#).unwrap();
        assert_eq!(document.language, LanguageId::English);
        assert_eq!(document.backend, Backend::Native);
        assert_eq!(document.vietnamese.scheme, Scheme::Vni);
        assert_eq!(document.vietnamese.tone_placement, Tone::Modern);
        assert!(document.vietnamese.spell_check);
        assert_eq!(document.revision, 0);
    }

    #[test]
    fn version_two_files_keep_native_as_the_safe_default() {
        let document = SettingsDocument::parse(br#"{"version":2,"language":"vi"}"#).unwrap();
        assert_eq!(document.backend, Backend::Native);
        assert_eq!(document.language, LanguageId::Vietnamese);
    }

    #[test]
    fn the_selected_language_survives_ipc_and_unknown_values_are_refused() {
        let document = SettingsDocument::next(
            &SettingsDocument::default(),
            LanguageId::Vietnamese,
            VietnameseSettings::default(),
        );
        let json = serde_json::to_string(&document).unwrap();
        assert!(json.contains(r#""backend":"native""#));
        assert!(json.contains(r#""language":"vi""#));
        assert_eq!(SettingsDocument::parse(json.as_bytes()).unwrap(), document);
        assert!(SettingsDocument::parse(br#"{"version":2,"language":"ko"}"#).is_err());
    }

    #[test]
    fn the_default_language_shortcut_requires_its_exact_modifiers_and_key() {
        let shortcut = LanguageSwitch::default();
        assert!(
            shortcut.matches(&KeyEvent::character(' ').with_modifiers(Modifiers {
                control: true,
                shift: true,
                ..Modifiers::NONE
            }))
        );
        assert!(
            !shortcut.matches(&KeyEvent::character(' ').with_modifiers(Modifiers {
                control: true,
                ..Modifiers::NONE
            }))
        );
        assert!(
            !LanguageSwitch {
                control: false,
                alt: false,
                shift: false,
                meta: false,
                ..shortcut
            }
            .matches(&KeyEvent::character(' '))
        );
    }

    #[test]
    fn active_languages_and_the_shortcut_round_trip_without_changing_the_schema() {
        let document = SettingsDocument {
            active_languages: ActiveLanguages::from_languages(LanguageId::ALL).unwrap(),
            language_switch: LanguageSwitch {
                key: LanguageSwitchKey::Enter,
                control: false,
                alt: true,
                shift: false,
                meta: false,
                beep: true,
            },
            ..SettingsDocument::default()
        };
        let json = serde_json::to_string(&document).unwrap();
        assert!(json.contains(r#""active_languages":["en","vi","ja"]"#));
        assert!(json.contains(r#""language_switch":{"key":"enter""#));
        assert_eq!(SettingsDocument::parse(json.as_bytes()).unwrap(), document);

        let old = SettingsDocument::parse(br#"{"version":4,"language":"vi"}"#).unwrap();
        assert_eq!(old.active_languages, ActiveLanguages::default());
        assert_eq!(old.language_switch, LanguageSwitch::default());
        assert!(
            SettingsDocument::parse(br#"{"version":4,"active_languages":["en","en"]}"#).is_err()
        );
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

        let document = SettingsDocument::next_with_backend(
            &SettingsDocument::default(),
            Backend::EventTap,
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
        assert_eq!(document.backend, Backend::EventTap);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
