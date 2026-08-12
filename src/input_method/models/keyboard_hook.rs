//! Pure lifecycle and callback policy for Windows' Keyboard Hook fallback.
#![cfg_attr(
    not(target_os = "windows"),
    allow(
        dead_code,
        reason = "Windows-only hook policy is unit-tested on every host."
    )
)]
//!
//! The OS callback contains no reliable password-field signal, so the service
//! processes only a selected Vietnamese backend with a fully known key-down. It
//! never consumes key-up, repeat, injected, shortcut, or uncertain events.

use dodo_ime_core::{Key, KeyEvent, Modifiers};
use dodo_ime_ipc::settings::Backend;

/// What the pane can honestly report about the dodo-lifetime-only fallback.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum KeyboardHookStatus {
    #[default]
    Inactive,
    Running,
    Failed,
}

/// Whether the hook may own transformation at all.
///
/// Not a function of the selected language, for the reason
/// `models::event_tap::desired_status` gives: the hook owns the language-switch
/// shortcut while it runs, so stopping it in English would make the shortcut
/// one-way.
pub fn desired_status(backend: Backend, start_succeeded: bool) -> KeyboardHookStatus {
    if backend != Backend::KeyboardHook {
        KeyboardHookStatus::Inactive
    } else if start_succeeded {
        KeyboardHookStatus::Running
    } else {
        KeyboardHookStatus::Failed
    }
}

/// The callback facts relevant to safety, without Windows handles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HookEvent {
    KeyDown {
        injected: bool,
        repeat: bool,
        shortcut: bool,
        text_is_known: bool,
    },
    KeyUp,
    Other,
}

/// What the native callback must do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handling {
    Process,
    PassThrough,
}

/// Windows' Alt and Windows-key state in the shared vocabulary.
///
/// The argument names are Windows' own and the fields are the engine's, which
/// is the whole job: `VK_MENU` is `alt`, and either Windows key is `meta` — the
/// same field macOS fills from Command. A shortcut recorded on one platform is
/// the same document on the other because this is the only place either name
/// appears.
pub fn modifiers(control: bool, alt: bool, shift: bool, windows: bool) -> Modifiers {
    Modifiers {
        control,
        alt,
        shift,
        meta: windows,
    }
}

/// One Windows virtual key in the shared vocabulary.
///
/// It mirrors `dodo_ime_windows::keymap::key_event`, deliberately: dodo does not
/// link the TSF DLL, so the two tables are separate code, and a shortcut that
/// worked under Native TSF and not under the hook would be the exact class of
/// bug this round is fixing. This copy lives in `models/` rather than beside the
/// hook so it is unit-tested from every host, including the Mac this is written
/// on.
pub fn key_event(vkey: u32, text: Option<char>, modifiers: Modifiers) -> KeyEvent {
    let identity = match vkey {
        0x08 => Some(Key::Backspace),
        0x09 => Some(Key::Tab),
        0x0d => Some(Key::Enter),
        0x1b => Some(Key::Escape),
        0x20 => Some(Key::Space),
        0x21 => Some(Key::PageUp),
        0x22 => Some(Key::PageDown),
        0x23 => Some(Key::End),
        0x24 => Some(Key::Home),
        0x25 => Some(Key::ArrowLeft),
        0x26 => Some(Key::ArrowUp),
        0x27 => Some(Key::ArrowRight),
        0x28 => Some(Key::ArrowDown),
        0x2e => Some(Key::Delete),
        // `VK_SHIFT`/`VK_CONTROL`/`VK_MENU`, the two Windows keys, and the
        // left/right pairs a low-level hook reports instead of the first three.
        0x10..=0x12 | 0x5b | 0x5c | 0xa0..=0xa5 => Some(Key::Modifier),
        _ => None,
    };
    let key = identity.unwrap_or_else(|| text.map_or(Key::Other, |_| Key::Character));
    KeyEvent {
        key,
        // Space is both a word boundary and text. No other identity may turn an
        // accompanying control code into composition input.
        text: match key {
            Key::Space => Some(' '),
            Key::Character => text,
            _ => None,
        },
        modifiers,
    }
}

/// Process exactly one known, plain physical key-down.
pub fn handling(event: HookEvent) -> Handling {
    match event {
        HookEvent::KeyDown {
            injected: false,
            repeat: false,
            shortcut: false,
            text_is_known: true,
        } => Handling::Process,
        HookEvent::KeyDown { .. } | HookEvent::KeyUp | HookEvent::Other => Handling::PassThrough,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Handling, HookEvent, KeyboardHookStatus, desired_status, handling, key_event, modifiers,
    };
    use dodo_ime_core::{Key, Modifiers};
    use dodo_ime_ipc::settings::{Backend, Shortcut, ShortcutKey, ShortcutModifiers};

    #[test]
    fn keyboard_hook_is_the_only_windows_fallback_owner() {
        assert_eq!(
            desired_status(Backend::Native, true),
            KeyboardHookStatus::Inactive
        );
        assert_eq!(
            desired_status(Backend::EventTap, true),
            KeyboardHookStatus::Inactive
        );
        assert_eq!(
            desired_status(Backend::KeyboardHook, true),
            KeyboardHookStatus::Running
        );
        assert_eq!(
            desired_status(Backend::KeyboardHook, false),
            KeyboardHookStatus::Failed
        );
    }

    /// Windows' Alt is the engine's `alt` and either Windows key is its `meta`,
    /// which is the same field macOS fills from Command. Getting this wrong
    /// would make one recorded shortcut mean two different hand shapes.
    #[test]
    fn alt_and_the_windows_key_normalize_the_way_macos_does() {
        let alt = modifiers(false, true, false, false);
        let windows = modifiers(false, false, false, true);
        assert_eq!(
            alt,
            Modifiers {
                alt: true,
                ..Modifiers::NONE
            }
        );
        assert_eq!(
            windows,
            Modifiers {
                meta: true,
                ..Modifiers::NONE
            }
        );

        let alt_space = Shortcut {
            modifiers: ShortcutModifiers {
                alt: true,
                ..ShortcutModifiers::NONE
            },
            key: ShortcutKey::Space,
        };
        let meta_space = Shortcut {
            modifiers: ShortcutModifiers {
                meta: true,
                ..ShortcutModifiers::NONE
            },
            key: ShortcutKey::Space,
        };
        assert!(alt_space.matches(&key_event(0x20, Some(' '), alt)));
        assert!(!alt_space.matches(&key_event(0x20, Some(' '), windows)));
        assert!(meta_space.matches(&key_event(0x20, Some(' '), windows)));
        assert!(!meta_space.matches(&key_event(0x20, Some(' '), alt)));
    }

    /// Every key the hook can build a shortcut from, including the modifier
    /// identities a low-level hook reports as the left/right pair.
    #[test]
    fn the_hook_names_the_same_keys_a_shortcut_can_hold() {
        for (vkey, key) in [
            (0x08_u32, Key::Backspace),
            (0x09, Key::Tab),
            (0x0d, Key::Enter),
            (0x1b, Key::Escape),
            (0x20, Key::Space),
            (0x21, Key::PageUp),
            (0x22, Key::PageDown),
            (0x23, Key::End),
            (0x24, Key::Home),
            (0x25, Key::ArrowLeft),
            (0x26, Key::ArrowUp),
            (0x27, Key::ArrowRight),
            (0x28, Key::ArrowDown),
            (0x2e, Key::Delete),
            (0x10, Key::Modifier),
            (0x11, Key::Modifier),
            (0x12, Key::Modifier),
            (0x5b, Key::Modifier),
            (0x5c, Key::Modifier),
            (0xa2, Key::Modifier),
            (0xa5, Key::Modifier),
        ] {
            let event = key_event(vkey, None, Modifiers::NONE);
            assert_eq!(event.key, key, "{vkey:#04x}");
            assert!(ShortcutKey::of(key).is_some(), "{key:?} must be recordable");
        }
        // A letter is a character and never a shortcut key.
        let letter = key_event(0x57, Some('w'), Modifiers::NONE);
        assert_eq!(letter.key, Key::Character);
        assert_eq!(letter.typed(), Some('w'));
        assert_eq!(ShortcutKey::of(Key::Character), None);
        // An identity never smuggles a control code into composition.
        assert_eq!(key_event(0x08, Some('\u{8}'), Modifiers::NONE).text, None);
    }

    /// A modifier-only shortcut is delivered to the hook as an ordinary
    /// key-down for the modifier itself, so it must match before `handling`
    /// declines it as a shortcut press.
    #[test]
    fn a_modifier_only_shortcut_is_a_key_down_the_hook_can_match() {
        let control_shift = Shortcut {
            modifiers: ShortcutModifiers {
                control: true,
                shift: true,
                ..ShortcutModifiers::NONE
            },
            key: ShortcutKey::Modifiers,
        };
        let completing_press = key_event(0xa0, None, modifiers(true, false, true, false));
        assert!(control_shift.matches(&completing_press));
        assert_eq!(
            handling(HookEvent::KeyDown {
                injected: false,
                repeat: false,
                shortcut: !completing_press.modifiers.is_plain(),
                text_is_known: true,
            }),
            Handling::PassThrough,
            "the engine must never see it; only the switch may"
        );
    }

    #[test]
    fn injected_repeated_shortcut_and_key_up_events_are_never_claimed() {
        for event in [
            HookEvent::KeyDown {
                injected: true,
                repeat: false,
                shortcut: false,
                text_is_known: true,
            },
            HookEvent::KeyDown {
                injected: false,
                repeat: true,
                shortcut: false,
                text_is_known: true,
            },
            HookEvent::KeyDown {
                injected: false,
                repeat: false,
                shortcut: true,
                text_is_known: true,
            },
            HookEvent::KeyDown {
                injected: false,
                repeat: false,
                shortcut: false,
                text_is_known: false,
            },
            HookEvent::KeyUp,
            HookEvent::Other,
        ] {
            assert_eq!(handling(event), Handling::PassThrough, "{event:?}");
        }
    }
}
