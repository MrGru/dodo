//! The English column of the Input method tool.

use std::borrow::Cow;

use super::Text;

pub(crate) fn text(text: Text) -> Cow<'static, str> {
    match text {
        Text::Description => {
            "Configure keyboard input, languages and typing behavior.".into()
        }
        Text::AccessibilityRequired => "Accessibility permission required (macOS)".into(),
        Text::AccessibilityRequiredDescription => {
            "Dodo needs accessibility permission to monitor keyboard input.".into()
        }
        Text::OpenAccessibilitySettings => "Open Accessibility Settings".into(),
        Text::CurrentLanguage => "Current input language".into(),
        Text::SwitchShortcut => "Switch shortcut".into(),
        Text::BeepDescription => "Play a sound when the input language changes.".into(),
        Text::VietnameseInput => "Vietnamese input".into(),
        Text::VietnameseInputDescription => "Configure how Vietnamese is typed.".into(),
        Text::StorageProblem => "Settings file".into(),
        Text::StoreError(detail) => {
            format!("The input method's settings could not be read or saved: {detail}").into()
        }
        Text::StoreMissingVersion => {
            "The input method's settings file carries no schema version, so it cannot be read safely.".into()
        }
        Text::StoreUnsupportedVersion { found, supported } => format!(
            "The input method's settings file uses schema {found}; this build of dodo reads {supported}. Update dodo rather than risk misreading it."
        ).into(),
        Text::EventTapStatus => "Event Tap status".into(),
        Text::EventTapInactive => "Event Tap is not active.".into(),
        Text::EventTapNeedsAccessibility => {
            "macOS needs you to enable Dodo in System Settings → Privacy & Security → Accessibility. Keys are passing through unchanged.".into()
        }
        Text::EventTapRunning => {
            "Event Tap is active while Dodo is open. It never stores or sends what you type.".into()
        }
        Text::EventTapFailed => {
            "Event Tap could not start. Keys are passing through unchanged.".into()
        }
        Text::KeyboardHookStatus => "Keyboard Hook status".into(),
        Text::KeyboardHookInactive => "Keyboard Hook is not active.".into(),
        Text::KeyboardHookRunning => {
            "Keyboard Hook is active only while Dodo is open. It never stores or sends what you type.".into()
        }
        Text::KeyboardHookFailed => {
            "Keyboard Hook could not start. Keys are passing through unchanged.".into()
        }
        Text::Scheme => "Input method".into(),
        Text::SchemeDescription => {
            "Telex marks tones with letters (aa, ow, s, f); VNI marks them with digits (a6, o7, 1, 2).".into()
        }
        Text::Telex => "Telex".into(),
        Text::Vni => "VNI".into(),
        Text::TonePlacement => "Tone mark placement".into(),
        Text::TonePlacementDescription => {
            "Modern puts the mark on the main vowel (hoà); traditional puts it on the first (hòa).".into()
        }
        Text::ToneModern => "Modern (hoà)".into(),
        Text::ToneTraditional => "Traditional (hòa)".into(),
        Text::SpellCheck => "Spell check".into(),
        Text::SpellCheckDescription => {
            "Keep English words unchanged when the input is not valid Vietnamese.".into()
        }
        Text::BracketShortcuts => "Bracket shortcuts".into(),
        Text::BracketShortcutsDescription => {
            "In Telex, [ and ] type ơ and ư — the only way to type ươ (thuở, huơ).".into()
        }
        Text::ActiveLanguages => "Languages".into(),
        Text::ActiveLanguagesDescription => {
            "Choose the languages shown in the menu and used by the switch shortcut.".into()
        }
        Text::LanguageDescription => "Select the current input language.".into(),
        Text::LanguageSwitch => "Language switch".into(),
        Text::LanguageSwitchDescription => {
            "Cycles the enabled languages. Click the shortcut, then press the combination you want.".into()
        }
        Text::ShortcutBeep => "Beep when switching".into(),
        Text::ShortcutSpace => "Space".into(),
        Text::ShortcutEnter => "Enter".into(),
        Text::ShortcutTab => "Tab".into(),
        Text::ShortcutEscape => "Escape".into(),
        Text::ShortcutRecording => "Press a combination…".into(),
        Text::ShortcutUnsupportedKey => {
            "That key cannot be recorded. Hold a modifier and press a key that types nothing, or hold two modifiers on their own.".into()
        }
        Text::ShortcutBackspace => "Backspace".into(),
        Text::ShortcutDelete => "Delete".into(),
        Text::ShortcutHome => "Home".into(),
        Text::ShortcutEnd => "End".into(),
        Text::ShortcutPageUp => "Page Up".into(),
        Text::ShortcutPageDown => "Page Down".into(),
        Text::ShortcutArrowLeft => "Left".into(),
        Text::ShortcutArrowRight => "Right".into(),
        Text::ShortcutArrowUp => "Up".into(),
        Text::ShortcutArrowDown => "Down".into(),
        Text::BrowserFix => "Browser address bars".into(),
        Text::BrowserFixDescription => {
            "Work around browsers that keep an autocomplete suggestion selected while you type.".into()
        }
    }
}
