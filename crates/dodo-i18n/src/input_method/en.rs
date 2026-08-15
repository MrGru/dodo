//! The English column of the Input method tool.

use std::borrow::Cow;

use super::Text;

pub(crate) fn text(text: Text) -> Cow<'static, str> {
    match text {
        Text::Description => {
                "Choose Dodo's Vietnamese input method. Native Input Method works after Dodo \
                 closes; Event Tap asks macOS for Accessibility permission and works while Dodo is open."
                    .into()
            }
        Text::Install => "Install".into(),
        Text::Reinstall => "Reinstall".into(),
        Text::Installing => "Installing…".into(),
        Text::Installed => {
                "Installed, and macOS switched to it. Type Vietnamese anywhere.".into()
            }
        Text::InstalledNotActive(status) => format!(
                "Installed, but macOS would not switch to it (error {status}). Turn it on in \
                 System Settings → Keyboard → Input Sources."
            )
            .into(),
        Text::NoBundle => {
                "This build of Dodo carries no input method to install.".into()
            }
        Text::CopyFailed(detail) => {
                format!("The input method could not be copied: {detail}").into()
            }
        Text::InvalidSignature(detail) => {
                format!("The input method has an invalid code signature: {detail}").into()
            }
        Text::NeverAppeared(attempts) => format!(
                "macOS accepted the input method but never listed it, after {attempts} attempts."
            )
            .into(),
        Text::Status => "Status".into(),
        Text::NotInstalled => "Not installed.".into(),
        Text::Running(version) => {
                format!("Running, version {version}.").into()
            }
        Text::InstalledIdle => {
                "Installed. macOS starts it when you switch to it.".into()
            }
        Text::SettingsPending => {
                "The input method has not picked these settings up yet.".into()
            }
        Text::StorageProblem => "Settings file".into(),
        Text::StoreError(detail) => {
                format!("The input method's settings could not be read or saved: {detail}").into()
            }
        Text::StoreMissingVersion => {
                "The input method's settings file carries no schema version, so it cannot be \
                 read safely."
                    .into()
            }
        Text::StoreUnsupportedVersion { found, supported } => format!(
                "The input method's settings file uses schema {found}; this build of dodo reads \
                 {supported}. Update dodo rather than risk misreading it."
            )
            .into(),
        Text::Scheme => "Input scheme".into(),
        Text::SchemeDescription => {
                "Telex marks tones with letters (aa, ow, s, f); VNI marks them with digits \
                 (a6, o7, 1, 2)."
                    .into()
            }
        Text::Telex => "Telex".into(),
        Text::Vni => "VNI".into(),
        Text::TonePlacement => "Tone mark placement".into(),
        Text::TonePlacementDescription => {
                "Modern puts the mark on the main vowel (hoà); traditional puts it on the first \
                 (hòa)."
                    .into()
            }
        Text::ToneModern => "Modern".into(),
        Text::ToneTraditional => "Traditional".into(),
        Text::SpellCheck => "Spell check".into(),
        Text::SpellCheckDescription => {
                "Hand back the keys as typed when the result is not a Vietnamese syllable, so \
                 English words survive."
                    .into()
            }
        Text::BracketShortcuts => "Bracket shortcuts".into(),
        Text::BracketShortcutsDescription => {
                "In Telex, [ and ] type ơ and ư — the only way to type uơ (thuở, huơ).".into()
            }
        Text::Backend => "Backend".into(),
        Text::BackendDescription => {
                "Only one backend transforms keys at a time.".into()
            }
        Text::Native => "Native Input Method".into(),
        Text::EventTap => "Event Tap".into(),
        Text::EventTapStatus => "Event Tap status".into(),
        Text::EventTapInactive => {
                "Select Vietnamese from Dodo's Keyboard Input menu to start Event Tap.".into()
            }
        Text::EventTapWaitingForNative => {
                "Waiting for Native Input Method to apply this selection.".into()
            }
        Text::EventTapNeedsAccessibility => {
                "macOS needs you to enable Dodo in System Settings → Privacy & Security → Accessibility. Keys are passing through unchanged."
                    .into()
            }
        Text::EventTapRunning => {
                "Event Tap is active while Dodo is open. It never stores or sends what you type."
                    .into()
            }
        Text::EventTapFailed => {
                "Event Tap could not start. Keys are passing through unchanged.".into()
            }
        Text::WindowsDescription => {
                "Choose Dodo's Vietnamese input method. Native TSF works after Dodo closes and requires installation; Keyboard Hook needs Dodo to remain open."
                    .into()
            }
        Text::WindowsLanguageDescription => {
                "Select Vietnamese before either Windows backend transforms input.".into()
            }
        Text::NativeTsf => "Native TSF".into(),
        Text::WindowsTsfStatus => "Native TSF status".into(),
        Text::WindowsTsfNotInstalled => {
                "Not installed. Install Native TSF to type when Dodo is closed.".into()
            }
        Text::WindowsTsfInstalled => {
                "Installed for this Windows account. Select Dodo Vietnamese from Windows input methods.".into()
            }
        Text::WindowsTsfRemoved => {
                "Native TSF was removed for this Windows account.".into()
            }
        Text::WindowsTsfNoDll => {
                "This build carries no Windows TSF DLL to install.".into()
            }
        Text::WindowsTsfRegisterFailed(detail) => {
                format!("Windows could not register Native TSF: {detail}").into()
            }
        Text::WindowsTsfUnregisterFailed(detail) => {
                format!("Windows could not remove Native TSF: {detail}").into()
            }
        Text::KeyboardHook => "Keyboard Hook".into(),
        Text::KeyboardHookStatus => "Keyboard Hook status".into(),
        Text::KeyboardHookInactive => {
                "Select Vietnamese from Dodo's Keyboard Input menu to start Keyboard Hook.".into()
            }
        Text::KeyboardHookRunning => {
                "Keyboard Hook is active only while Dodo is open. It never stores or sends what you type.".into()
            }
        Text::KeyboardHookFailed => {
                "Keyboard Hook could not start. Keys are passing through unchanged.".into()
            }
        Text::Uninstall => "Uninstall".into(),
        Text::Uninstalling => "Uninstalling…".into(),
        Text::ActiveLanguages => "Active languages".into(),
        Text::ActiveLanguagesDescription => {
                "Choose the languages shown in the menu and used by the switch shortcut.".into()
            }
        Text::LanguageDescription => {
                "Select the current input language.".into()
            }
        Text::LanguageSwitch => "Language switch".into(),
        Text::LanguageSwitchDescription => {
                "Cycles the enabled languages. Click the shortcut, then press the combination you want.".into()
            }
        Text::ShortcutBeep => "Beep".into(),
        Text::ShortcutSpace => "Space".into(),
        Text::ShortcutEnter => "Enter".into(),
        Text::ShortcutTab => "Tab".into(),
        Text::ShortcutEscape => "Escape".into(),
        Text::ShortcutRecording => {
                "Press a combination…".into()
            }
        Text::ShortcutUnsupportedKey => {
                "That key cannot be recorded. Hold a modifier and press a key that types nothing, or hold two modifiers on their own.".into()
            }
        Text::ShortcutNeedsEventTap => {
                "Native Input Method never sees a modifier-only shortcut. Select Event Tap, or record a combination that ends in another key.".into()
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
                "Work around browsers that keep an autocomplete suggestion selected while you \
                 type, which would otherwise put the tone mark on the wrong letter."
                    .into()
            }
    }
}
