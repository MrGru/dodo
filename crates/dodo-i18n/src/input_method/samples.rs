//! One sample per [`Text`] variant, for the language tests.

use crate::tests::{DETAIL, NUMBER, NUMBER_TEXT, Sample, plain, term, with};

use super::Text;

samples! {
    plain Description;
    plain WindowsDescription;
    plain StorageProblem;
    with StoreError(DETAIL.into()) [DETAIL];
    plain StoreMissingVersion;
    with StoreUnsupportedVersion { found: NUMBER as u64, supported: 8 } [NUMBER_TEXT, "8"];
    plain EventTapStatus;
    plain EventTapInactive;
    plain EventTapNeedsAccessibility;
    plain EventTapRunning;
    plain EventTapFailed;
    plain KeyboardHookStatus;
    plain KeyboardHookInactive;
    plain KeyboardHookRunning;
    plain KeyboardHookFailed;
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
