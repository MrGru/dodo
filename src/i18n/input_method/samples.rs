//! One sample per [`Text`] variant, for the language tests.
//!
//! `samples!` also emits an exhaustive `match` over [`Text`], so a variant
//! with no entry here is a compile error.

use crate::i18n::tests::{DETAIL, NUMBER, NUMBER_TEXT, Sample, plain, term, with};

use super::Text;

samples! {
    plain Description;
    plain Install;
    plain Reinstall;
    plain Installing;
    plain Installed;
    with InstalledNotActive(NUMBER as i32) [NUMBER_TEXT];
    plain NoBundle;
    with CopyFailed(DETAIL.into()) [DETAIL];
    with InvalidSignature(DETAIL.into()) [DETAIL];
    with NeverAppeared(NUMBER as u32) [NUMBER_TEXT];
    plain Status;
    plain NotInstalled;
    with Running(DETAIL.into()) [DETAIL];
    plain InstalledIdle;
    plain SettingsPending;
    plain StorageProblem;
    with StoreError(DETAIL.into()) [DETAIL];
    plain StoreMissingVersion;
    with StoreUnsupportedVersion { found: NUMBER as u64, supported: 7 } [NUMBER_TEXT, "7"];
    plain Scheme;
    plain SchemeDescription;
    term Telex;
    term Vni;
    plain TonePlacement;
    plain TonePlacementDescription;
    plain ToneModern;
    plain ToneTraditional;
    plain SpellCheck;
    plain SpellCheckDescription;
    plain BracketShortcuts;
    plain BracketShortcutsDescription;
    plain Backend;
    plain BackendDescription;
    plain Native;
    plain EventTap;
    plain EventTapStatus;
    plain EventTapInactive;
    plain EventTapWaitingForNative;
    plain EventTapNeedsAccessibility;
    plain EventTapRunning;
    plain EventTapFailed;
    plain WindowsDescription;
    plain WindowsLanguageDescription;
    plain NativeTsf;
    plain WindowsTsfStatus;
    plain WindowsTsfNotInstalled;
    plain WindowsTsfInstalled;
    plain WindowsTsfRemoved;
    plain WindowsTsfNoDll;
    with WindowsTsfRegisterFailed(DETAIL.into()) [DETAIL];
    with WindowsTsfUnregisterFailed(DETAIL.into()) [DETAIL];
    term KeyboardHook;
    plain KeyboardHookStatus;
    plain KeyboardHookInactive;
    plain KeyboardHookRunning;
    plain KeyboardHookFailed;
    plain Uninstall;
    plain Uninstalling;
    plain ActiveLanguages;
    plain ActiveLanguagesDescription;
    plain LanguageDescription;
    plain LanguageSwitch;
    plain LanguageSwitchDescription;
    plain ShortcutBeep;
    plain ShortcutSpace;
    plain ShortcutEnter;
    plain ShortcutTab;
    plain ShortcutEscape;
    plain ShortcutRecording;
    plain ShortcutUnsupportedKey;
    plain ShortcutNeedsEventTap;
    plain ShortcutBackspace;
    plain ShortcutDelete;
    plain ShortcutHome;
    plain ShortcutEnd;
    plain ShortcutPageUp;
    plain ShortcutPageDown;
    plain ShortcutArrowLeft;
    plain ShortcutArrowRight;
    plain ShortcutArrowUp;
    plain ShortcutArrowDown;
    plain BrowserFix;
    plain BrowserFixDescription;
}
