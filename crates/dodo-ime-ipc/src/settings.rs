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
    ActiveLanguages, InputScheme, Key, KeyEvent, LanguageId, Modifiers, OutputMode, TonePlacement,
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
/// Version 4 adds Windows' Keyboard Hook backend. Version 5 briefly allowed a
/// modifier-only language shortcut. Version 6 forced a base key. Version 7
/// restores optional base keys and migrates that forced default back. Version 8
/// records the language switch as a generic [`Shortcut`] instead of four
/// modifier flags beside one of four base keys.
pub const SETTINGS_SCHEMA_VERSION: u32 = 8;

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

/// One modifier's presence in a recorded shortcut.
///
/// `meta` is Command on macOS and the Windows key on Windows, exactly as
/// [`Modifiers`] defines it. Every host normalizes into that vocabulary before
/// anything here is consulted, so one recorded shortcut means the same physical
/// hand shape on both platforms and no caller has to know which it is running
/// on. Caps lock is absent for the same reason it is absent from [`Modifiers`]:
/// the host has already applied it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct ShortcutModifiers {
    pub control: bool,
    pub alt: bool,
    pub shift: bool,
    pub meta: bool,
}

impl ShortcutModifiers {
    pub const NONE: ShortcutModifiers = ShortcutModifiers {
        control: false,
        alt: false,
        shift: false,
        meta: false,
    };

    /// The modifiers held during one normalized press.
    pub fn of(modifiers: Modifiers) -> ShortcutModifiers {
        ShortcutModifiers {
            control: modifiers.control,
            alt: modifiers.alt,
            shift: modifiers.shift,
            meta: modifiers.meta,
        }
    }

    /// How many modifier keys this holds.
    pub fn count(self) -> usize {
        usize::from(self.control)
            + usize::from(self.alt)
            + usize::from(self.shift)
            + usize::from(self.meta)
    }

    /// Whether one of the three modifiers that make a press a *command* rather
    /// than typing is held.
    ///
    /// Shift is deliberately not one of them, for the reason
    /// [`Modifiers::is_plain`] gives: it selects which character a key types
    /// and the host has already applied it, so `⇧Space` still types a space and
    /// a shortcut built from it would eat one.
    pub fn has_command(self) -> bool {
        self.control || self.alt || self.meta
    }

    /// Exact equality, never containment: `⌃⇧Space` must not fire on
    /// `⌃⌥⇧Space`, which is a different shortcut the user may have bound.
    fn matches(self, modifiers: Modifiers) -> bool {
        self == ShortcutModifiers::of(modifiers)
    }
}

/// The key half of a recorded shortcut.
///
/// This is the engine's non-printing [`Key`] set plus
/// [`ShortcutKey::Modifiers`], which says the recorded modifiers *are* the whole
/// shortcut. Adding a key is one variant here and one arm in
/// [`ShortcutKey::engine_key`]; nothing else in the flow names a key at all.
///
/// # Why a printing key is not in this list
///
/// A [`KeyEvent`] carries what a key *types*, deliberately, so that Telex works
/// on every keyboard layout — and it carries no physical key code, deliberately,
/// for the same reason. Under Option, macOS types `Ω` for the key labelled `Z`
/// and hands the host exactly that, so `⌥Z` recorded from a settings window
/// could not be recognised again by the input method. That is a property of the
/// host contract and not something this file can normalize away, so the
/// recorder refuses a printing key rather than storing one that silently never
/// fires.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ShortcutKey {
    /// The recorded modifiers are the complete shortcut, and it fires on the
    /// press that completes them.
    #[default]
    Modifiers,
    Space,
    Enter,
    Tab,
    Escape,
    Backspace,
    Delete,
    Home,
    End,
    PageUp,
    PageDown,
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    ArrowDown,
}

impl ShortcutKey {
    pub const ALL: [ShortcutKey; 15] = [
        ShortcutKey::Modifiers,
        ShortcutKey::Space,
        ShortcutKey::Enter,
        ShortcutKey::Tab,
        ShortcutKey::Escape,
        ShortcutKey::Backspace,
        ShortcutKey::Delete,
        ShortcutKey::Home,
        ShortcutKey::End,
        ShortcutKey::PageUp,
        ShortcutKey::PageDown,
        ShortcutKey::ArrowLeft,
        ShortcutKey::ArrowRight,
        ShortcutKey::ArrowUp,
        ShortcutKey::ArrowDown,
    ];

    /// The engine key this records. The mapping is total in both directions,
    /// which is what lets [`ShortcutKey::of`] answer "can this press be
    /// recorded" without a second table.
    pub fn engine_key(self) -> Key {
        match self {
            ShortcutKey::Modifiers => Key::Modifier,
            ShortcutKey::Space => Key::Space,
            ShortcutKey::Enter => Key::Enter,
            ShortcutKey::Tab => Key::Tab,
            ShortcutKey::Escape => Key::Escape,
            ShortcutKey::Backspace => Key::Backspace,
            ShortcutKey::Delete => Key::Delete,
            ShortcutKey::Home => Key::Home,
            ShortcutKey::End => Key::End,
            ShortcutKey::PageUp => Key::PageUp,
            ShortcutKey::PageDown => Key::PageDown,
            ShortcutKey::ArrowLeft => Key::ArrowLeft,
            ShortcutKey::ArrowRight => Key::ArrowRight,
            ShortcutKey::ArrowUp => Key::ArrowUp,
            ShortcutKey::ArrowDown => Key::ArrowDown,
        }
    }

    /// The shortcut key one engine key records as, or `None` for a key no
    /// shortcut may be built from — a printing key, or one no host names.
    pub fn of(key: Key) -> Option<ShortcutKey> {
        ShortcutKey::ALL
            .into_iter()
            .find(|candidate| candidate.engine_key() == key)
    }
}

/// A recorded key combination, independent of the host that observed it.
///
/// This is the whole shortcut vocabulary: a set of modifiers and one key. There
/// is no list of blessed combinations anywhere, and nothing counts keys for its
/// own sake — see [`Shortcut::is_valid`] for the one rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Shortcut {
    #[serde(default)]
    pub modifiers: ShortcutModifiers,
    #[serde(default)]
    pub key: ShortcutKey,
}

impl Shortcut {
    /// Control-Shift-Space is unlikely to type into an application by accident.
    pub const DEFAULT: Shortcut = Shortcut {
        modifiers: ShortcutModifiers {
            control: true,
            alt: false,
            shift: true,
            meta: false,
        },
        key: ShortcutKey::Space,
    };

    /// Whether this combination cannot fire while someone is simply typing.
    ///
    /// It is one rule and not a shape: **a command modifier must be held**, so
    /// no shortcut can consume a key an application would otherwise receive as
    /// input. A modifier-only shortcut needs a second modifier on top of that,
    /// because a single one fires the moment it goes down — before the letter
    /// of every ordinary `⌘C` the user meant.
    pub fn is_valid(self) -> bool {
        self.modifiers.has_command()
            && (self.key != ShortcutKey::Modifiers || self.modifiers.count() >= 2)
    }

    /// Whether a normalized key press invokes this shortcut.
    pub fn matches(self, event: &KeyEvent) -> bool {
        self.is_valid()
            && event.key == self.key.engine_key()
            && self.modifiers.matches(event.modifiers)
    }
}

/// The shared shortcut that cycles the enabled input languages, and whether it
/// says so out loud.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct LanguageSwitch {
    pub shortcut: Shortcut,
    pub beep: bool,
}

impl Default for LanguageSwitch {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl LanguageSwitch {
    pub const DEFAULT: LanguageSwitch = LanguageSwitch {
        shortcut: Shortcut::DEFAULT,
        beep: false,
    };

    pub fn is_valid(self) -> bool {
        self.shortcut.is_valid()
    }

    /// Whether a normalized key press invokes the language switch.
    pub fn matches(self, event: &KeyEvent) -> bool {
        self.shortcut.matches(event)
    }
}

/// The base key a document written by version 7 or earlier could name.
///
/// Kept only so those files keep working: `null` there meant "the modifiers are
/// the whole shortcut", which is now [`ShortcutKey::Modifiers`].
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum LegacyKey {
    Space,
    Enter,
    Tab,
    Escape,
}

impl From<LegacyKey> for ShortcutKey {
    fn from(key: LegacyKey) -> ShortcutKey {
        match key {
            LegacyKey::Space => ShortcutKey::Space,
            LegacyKey::Enter => ShortcutKey::Enter,
            LegacyKey::Tab => ShortcutKey::Tab,
            LegacyKey::Escape => ShortcutKey::Escape,
        }
    }
}

/// Both spellings of the shortcut at once: version 8's nested `shortcut`, and
/// the flat modifier flags every earlier version wrote.
#[derive(Deserialize, Default)]
#[serde(rename_all = "kebab-case", default)]
struct LanguageSwitchWire {
    shortcut: Option<Shortcut>,
    key: Option<LegacyKey>,
    control: bool,
    alt: bool,
    shift: bool,
    meta: bool,
    beep: bool,
}

impl<'de> Deserialize<'de> for LanguageSwitch {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = LanguageSwitchWire::deserialize(deserializer)?;
        let switch = LanguageSwitch {
            shortcut: wire.shortcut.unwrap_or(Shortcut {
                modifiers: ShortcutModifiers {
                    control: wire.control,
                    alt: wire.alt,
                    shift: wire.shift,
                    meta: wire.meta,
                },
                key: wire.key.map_or(ShortcutKey::Modifiers, ShortcutKey::from),
            }),
            beep: wire.beep,
        };
        // A shortcut that could fire while typing is not repaired field by
        // field: there is no way to guess which half the user meant, and the
        // default is the one answer that is safe in every application.
        Ok(if switch.is_valid() {
            switch
        } else {
            LanguageSwitch::DEFAULT
        })
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
        read_versioned::<SettingsDocument>(path, SETTINGS_SCHEMA_VERSION).map(|document| {
            document.map(|mut document| {
                document.migrate_language_switch();
                document
            })
        })
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
        let mut document = parse_versioned::<SettingsDocument>(bytes, SETTINGS_SCHEMA_VERSION)?;
        document.migrate_language_switch();
        Ok(document)
    }

    fn migrate_language_switch(&mut self) {
        // Version 6 was emitted only by the brief base-key migration. Its
        // forced default is the one shape that can return to the modifier-only
        // shortcut version 5 originally stored.
        if self.version == 6 && self.language_switch.shortcut == Shortcut::DEFAULT {
            self.language_switch.shortcut.key = ShortcutKey::Modifiers;
        }
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
        Backend, LanguageSwitch, SETTINGS_SCHEMA_VERSION, Scheme, SettingsDocument, Shortcut,
        ShortcutKey, ShortcutModifiers, Tone, VietnameseSettings,
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

    /// The wire spelling of every shortcut key, pinned. `rename_all` derives
    /// these from the variant names, so a rename that looked like tidying would
    /// otherwise silently stop a months-old bundle recognising the shortcut.
    #[test]
    fn every_shortcut_key_has_a_pinned_wire_spelling() {
        let expected = [
            "modifiers",
            "space",
            "enter",
            "tab",
            "escape",
            "backspace",
            "delete",
            "home",
            "end",
            "page-up",
            "page-down",
            "arrow-left",
            "arrow-right",
            "arrow-up",
            "arrow-down",
        ];
        for (key, code) in ShortcutKey::ALL.into_iter().zip(expected) {
            assert_eq!(serde_json::to_string(&key).unwrap(), format!("\"{code}\""));
            assert_eq!(
                serde_json::from_str::<ShortcutKey>(&format!("\"{code}\"")).unwrap(),
                key
            );
        }
        // Every recordable key round-trips through the engine's vocabulary, so
        // a host that can name the key can always match the shortcut.
        for key in ShortcutKey::ALL {
            assert_eq!(ShortcutKey::of(key.engine_key()), Some(key), "{key:?}");
        }
        assert_eq!(ShortcutKey::of(dodo_ime_core::Key::Character), None);
        assert_eq!(ShortcutKey::of(dodo_ime_core::Key::Other), None);
    }

    /// The one validity rule, stated as the thing it protects: a shortcut may
    /// never fire while someone is typing.
    #[test]
    fn a_shortcut_is_valid_only_when_it_cannot_fire_on_ordinary_typing() {
        let command_and_key = Shortcut::DEFAULT;
        assert!(command_and_key.is_valid());

        for key in ShortcutKey::ALL {
            let bare = Shortcut {
                modifiers: ShortcutModifiers::NONE,
                key,
            };
            assert!(!bare.is_valid(), "{key:?} with no modifier");
            let shift_only = Shortcut {
                modifiers: ShortcutModifiers {
                    shift: true,
                    ..ShortcutModifiers::NONE
                },
                key,
            };
            assert!(!shift_only.is_valid(), "{key:?} under Shift alone");
        }

        // One modifier is enough beside a key, and never enough on its own.
        assert!(
            Shortcut {
                modifiers: ShortcutModifiers {
                    meta: true,
                    ..ShortcutModifiers::NONE
                },
                key: ShortcutKey::Space,
            }
            .is_valid()
        );
        assert!(
            !Shortcut {
                modifiers: ShortcutModifiers {
                    control: true,
                    ..ShortcutModifiers::NONE
                },
                key: ShortcutKey::Modifiers,
            }
            .is_valid()
        );
        assert!(
            Shortcut {
                modifiers: ShortcutModifiers {
                    control: true,
                    shift: true,
                    ..ShortcutModifiers::NONE
                },
                key: ShortcutKey::Modifiers,
            }
            .is_valid()
        );
    }

    /// Recording a replacement must make the previous shortcut inert, with no
    /// second listener left believing in it.
    #[test]
    fn a_replacement_shortcut_leaves_the_previous_one_inert() {
        let old = LanguageSwitch::DEFAULT;
        let old_press = KeyEvent::character(' ').with_modifiers(Modifiers {
            control: true,
            shift: true,
            ..Modifiers::NONE
        });
        assert!(old.matches(&old_press));

        let replacement = LanguageSwitch {
            shortcut: Shortcut {
                modifiers: ShortcutModifiers {
                    meta: true,
                    ..ShortcutModifiers::NONE
                },
                key: ShortcutKey::Space,
            },
            beep: true,
        };
        let new_press = KeyEvent::character(' ').with_modifiers(Modifiers {
            meta: true,
            ..Modifiers::NONE
        });
        assert!(!replacement.matches(&old_press), "the old shortcut is gone");
        assert!(replacement.matches(&new_press));
        assert!(!replacement.matches(&KeyEvent::character(' ')));
    }

    /// Exact modifiers, never containment: a superset is a different shortcut
    /// somebody may have bound, and an unrelated key held with the same
    /// modifiers must not switch a second time.
    #[test]
    fn modifier_only_shortcuts_fire_on_the_press_that_completes_them() {
        let switch = LanguageSwitch {
            shortcut: Shortcut {
                modifiers: ShortcutModifiers {
                    control: true,
                    shift: true,
                    ..ShortcutModifiers::NONE
                },
                key: ShortcutKey::Modifiers,
            },
            beep: false,
        };
        let modifier =
            |modifiers| KeyEvent::special(dodo_ime_core::Key::Modifier).with_modifiers(modifiers);
        assert!(switch.matches(&modifier(Modifiers {
            control: true,
            shift: true,
            ..Modifiers::NONE
        })));
        assert!(
            !switch.matches(&modifier(Modifiers {
                control: true,
                ..Modifiers::NONE
            })),
            "Control alone must not switch"
        );
        assert!(
            !switch.matches(&modifier(Modifiers {
                control: true,
                shift: true,
                alt: true,
                ..Modifiers::NONE
            })),
            "a superset is a different shortcut"
        );
        assert!(
            !switch.matches(
                &KeyEvent::special(dodo_ime_core::Key::Other).with_modifiers(Modifiers {
                    control: true,
                    shift: true,
                    ..Modifiers::NONE
                })
            ),
            "an unrelated key held with the shortcut must not switch again"
        );
    }

    /// `meta` is one identity with two names, and this is where the two
    /// platforms are held to it: a shortcut recorded with Command on macOS is
    /// the same document a Windows host reads as the Windows key, and Option
    /// and Alt are likewise one field.
    #[test]
    fn command_and_windows_are_one_modifier_and_option_and_alt_are_another() {
        let meta = Shortcut {
            modifiers: ShortcutModifiers {
                meta: true,
                ..ShortcutModifiers::NONE
            },
            key: ShortcutKey::Space,
        };
        let alt = Shortcut {
            modifiers: ShortcutModifiers {
                alt: true,
                ..ShortcutModifiers::NONE
            },
            key: ShortcutKey::Space,
        };
        // What each host's keymap produces, spelled out as the normalized
        // event rather than as the platform's own flag.
        let with_meta = KeyEvent::special(dodo_ime_core::Key::Space).with_modifiers(Modifiers {
            meta: true,
            ..Modifiers::NONE
        });
        let with_alt = KeyEvent::special(dodo_ime_core::Key::Space).with_modifiers(Modifiers {
            alt: true,
            ..Modifiers::NONE
        });
        assert!(meta.matches(&with_meta));
        assert!(!meta.matches(&with_alt));
        assert!(alt.matches(&with_alt));
        assert!(!alt.matches(&with_meta));

        let json = serde_json::to_string(&meta).unwrap();
        assert!(json.contains(r#""meta":true"#), "{json}");
        assert_eq!(serde_json::from_str::<Shortcut>(&json).unwrap(), meta);
    }

    #[test]
    fn version_six_forced_control_shift_space_returns_to_modifier_only() {
        let document = SettingsDocument::parse(
            br#"{"version":6,"language_switch":{"key":"space","control":true,"shift":true,"beep":true}}"#,
        )
        .unwrap();
        assert_eq!(
            document.language_switch.shortcut.key,
            ShortcutKey::Modifiers
        );
        assert!(document.language_switch.shortcut.modifiers.control);
        assert!(document.language_switch.shortcut.modifiers.shift);
        assert!(document.language_switch.beep);
    }

    #[test]
    fn active_languages_and_shortcuts_round_trip_and_keep_legacy_files_readable() {
        let document = SettingsDocument {
            active_languages: ActiveLanguages::from_languages(LanguageId::ALL).unwrap(),
            language_switch: LanguageSwitch {
                shortcut: Shortcut {
                    modifiers: ShortcutModifiers {
                        alt: true,
                        ..ShortcutModifiers::NONE
                    },
                    key: ShortcutKey::ArrowRight,
                },
                beep: true,
            },
            ..SettingsDocument::default()
        };
        let json = serde_json::to_string(&document).unwrap();
        assert!(json.contains(r#""active_languages":["en","vi","ja"]"#));
        assert!(json.contains(r#""key":"arrow-right""#), "{json}");
        assert_eq!(SettingsDocument::parse(json.as_bytes()).unwrap(), document);

        // Version 4's flat spelling, and version 7's modifier-only `null`.
        let flat = SettingsDocument::parse(
            br#"{"version":4,"language":"vi","language_switch":{"key":"tab","control":true}}"#,
        )
        .unwrap();
        assert_eq!(flat.active_languages, ActiveLanguages::default());
        assert_eq!(flat.language_switch.shortcut.key, ShortcutKey::Tab);
        assert!(flat.language_switch.shortcut.modifiers.control);

        let modifier_only = SettingsDocument::parse(
            br#"{"version":7,"language_switch":{"key":null,"control":true,"alt":true,"beep":true}}"#,
        )
        .unwrap();
        assert_eq!(
            modifier_only.language_switch.shortcut,
            Shortcut {
                modifiers: ShortcutModifiers {
                    control: true,
                    alt: true,
                    ..ShortcutModifiers::NONE
                },
                key: ShortcutKey::Modifiers,
            }
        );
        assert!(modifier_only.language_switch.beep);

        // A stored shortcut that could fire while typing becomes the default
        // rather than being half-honoured.
        assert_eq!(
            SettingsDocument::parse(br#"{"version":7,"language_switch":{"key":"space"}}"#)
                .unwrap()
                .language_switch,
            LanguageSwitch::DEFAULT
        );
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
