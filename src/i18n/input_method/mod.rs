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
    Install,
    Reinstall,
    Installing,
    Installed,
    /// "Installed, but macOS would not switch to it (error {0})." The number is
    /// an `OSStatus`, shown because `-50` is the one a reader can look up.
    InstalledNotActive(i32),
    NoBundle,
    /// `ditto`'s own message, kept verbatim inside a translated frame.
    CopyFailed(String),
    /// `codesign`'s own message, kept verbatim inside a translated frame.
    InvalidSignature(String),
    /// How many `TISRegisterInputSource` calls were made before giving up.
    NeverAppeared(u32),
    Status,
    NotInstalled,
    /// The installed bundle's version, as the bundle itself reported it.
    Running(String),
    InstalledIdle,
    SettingsPending,
    StorageProblem,
    StoreError(String),
    StoreMissingVersion,
    StoreUnsupportedVersion {
        found: u64,
        supported: u32,
    },
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
    Backend,
    BackendDescription,
    Native,
    EventTap,
    EventTapStatus,
    EventTapInactive,
    EventTapWaitingForNative,
    EventTapNeedsAccessibility,
    EventTapRunning,
    EventTapFailed,

    // Windows input method. Kept separate from the macOS backend names: TSF
    // installation and the no-install Keyboard Hook have different promises.
    #[cfg_attr(
        not(target_os = "windows"),
        allow(dead_code, reason = "Windows-only input-method copy.")
    )]
    WindowsDescription,
    #[cfg_attr(
        not(target_os = "windows"),
        allow(dead_code, reason = "Windows-only input-method copy.")
    )]
    WindowsLanguageDescription,
    #[cfg_attr(
        not(target_os = "windows"),
        allow(dead_code, reason = "Windows-only input-method copy.")
    )]
    NativeTsf,
    #[cfg_attr(
        not(target_os = "windows"),
        allow(dead_code, reason = "Windows-only input-method copy.")
    )]
    WindowsTsfStatus,
    #[cfg_attr(
        not(target_os = "windows"),
        allow(dead_code, reason = "Windows-only input-method copy.")
    )]
    WindowsTsfNotInstalled,
    #[cfg_attr(
        not(target_os = "windows"),
        allow(dead_code, reason = "Windows-only input-method copy.")
    )]
    WindowsTsfInstalled,
    #[cfg_attr(
        not(target_os = "windows"),
        allow(dead_code, reason = "Windows-only input-method copy.")
    )]
    WindowsTsfRemoved,
    #[cfg_attr(
        not(target_os = "windows"),
        allow(dead_code, reason = "Windows-only input-method copy.")
    )]
    WindowsTsfNoDll,
    #[cfg_attr(
        not(target_os = "windows"),
        allow(dead_code, reason = "Windows-only input-method copy.")
    )]
    WindowsTsfRegisterFailed(String),
    #[cfg_attr(
        not(target_os = "windows"),
        allow(dead_code, reason = "Windows-only input-method copy.")
    )]
    WindowsTsfUnregisterFailed(String),
    #[cfg_attr(
        not(target_os = "windows"),
        allow(dead_code, reason = "Windows-only input-method copy.")
    )]
    KeyboardHook,
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
    #[cfg_attr(
        not(target_os = "windows"),
        allow(dead_code, reason = "Windows-only input-method copy.")
    )]
    Uninstall,
    #[cfg_attr(
        not(target_os = "windows"),
        allow(dead_code, reason = "Windows-only input-method copy.")
    )]
    Uninstalling,

    // Input-language selection.
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
    ShortcutNeedsEventTap,
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

    // The Event Tap's browser workaround.
    BrowserFix,
    BrowserFixDescription,
}
