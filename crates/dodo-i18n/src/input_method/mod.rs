//! The Input method tool.
//!
//! `en` and `vi` each render every variant below; the compiler names any
//! string a language has not been given.

pub(crate) mod en;
pub(crate) mod vi;

#[cfg(test)]
pub(crate) mod samples;

/// The strings this area owns.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Text {
    Description,
    #[cfg_attr(
        not(target_os = "windows"),
        allow(dead_code, reason = "Windows-only input-method copy.")
    )]
    WindowsDescription,
    StorageProblem,
    StoreError(String),
    StoreMissingVersion,
    StoreUnsupportedVersion {
        found: u64,
        supported: u32,
    },
    EventTapStatus,
    EventTapInactive,
    EventTapNeedsAccessibility,
    EventTapRunning,
    EventTapFailed,
    #[cfg_attr(
        not(target_os = "windows"),
        allow(dead_code, reason = "Windows-only input-method copy.")
    )]
    KeyboardHookStatus,
    #[cfg_attr(
        not(target_os = "windows"),
        allow(dead_code, reason = "Windows-only input-method copy.")
    )]
    KeyboardHookInactive,
    #[cfg_attr(
        not(target_os = "windows"),
        allow(dead_code, reason = "Windows-only input-method copy.")
    )]
    KeyboardHookRunning,
    #[cfg_attr(
        not(target_os = "windows"),
        allow(dead_code, reason = "Windows-only input-method copy.")
    )]
    KeyboardHookFailed,
    Scheme,
    SchemeDescription,
    Telex,
    Vni,
    TonePlacement,
    TonePlacementDescription,
    ToneModern,
    ToneTraditional,
    SpellCheck,
    SpellCheckDescription,
    BracketShortcuts,
    BracketShortcutsDescription,
    ActiveLanguages,
    ActiveLanguagesDescription,
    LanguageDescription,
    LanguageSwitch,
    LanguageSwitchDescription,
    ShortcutBeep,
    ShortcutSpace,
    ShortcutEnter,
    ShortcutTab,
    ShortcutEscape,
    ShortcutRecording,
    ShortcutUnsupportedKey,
    ShortcutBackspace,
    ShortcutDelete,
    ShortcutHome,
    ShortcutEnd,
    ShortcutPageUp,
    ShortcutPageDown,
    ShortcutArrowLeft,
    ShortcutArrowRight,
    ShortcutArrowUp,
    ShortcutArrowDown,
    BrowserFix,
    BrowserFixDescription,
}
