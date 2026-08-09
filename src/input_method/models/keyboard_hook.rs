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

use dodo_ime_core::LanguageId;
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
pub fn desired_status(
    backend: Backend,
    language: LanguageId,
    start_succeeded: bool,
) -> KeyboardHookStatus {
    if backend != Backend::KeyboardHook || language != LanguageId::Vietnamese {
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
    use super::{Handling, HookEvent, KeyboardHookStatus, desired_status, handling};
    use dodo_ime_core::LanguageId;
    use dodo_ime_ipc::settings::Backend;

    #[test]
    fn keyboard_hook_is_the_only_windows_fallback_owner() {
        assert_eq!(
            desired_status(Backend::Native, LanguageId::Vietnamese, true),
            KeyboardHookStatus::Inactive
        );
        assert_eq!(
            desired_status(Backend::KeyboardHook, LanguageId::English, true),
            KeyboardHookStatus::Inactive
        );
        assert_eq!(
            desired_status(Backend::KeyboardHook, LanguageId::Vietnamese, true),
            KeyboardHookStatus::Running
        );
        assert_eq!(
            desired_status(Backend::KeyboardHook, LanguageId::Vietnamese, false),
            KeyboardHookStatus::Failed
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
