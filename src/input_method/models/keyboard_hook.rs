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
//! processes only a selected Vietnamese backend with a fully known key-down.
//! Repeats, injected input, shortcuts, and uncertain events pass through; only
//! a key-up paired with a consumed physical key-down is consumed.

use std::collections::HashSet;

use dodo_ime_core::{Key, KeyEvent, Modifiers};
use dodo_ime_ipc::settings::Backend;

use crate::input_method::models::direct_output::OutputPlan;
use crate::input_method::models::event_tap::DirectComposer;

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
    KeyUp {
        suppress: bool,
    },
    Other,
}

/// What the native callback must do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handling {
    Process,
    PassThrough,
    Suppress,
}

/// The Windows signals that prove composition is still aimed at one control.
///
/// A null caret owner is allowed because custom controls often draw their own;
/// physical mouse-down is observed separately and invalidates even that case.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TargetIdentity {
    foreground: usize,
    thread: u32,
    focus: usize,
    caret: usize,
}

impl TargetIdentity {
    pub(crate) fn new(foreground: usize, thread: u32, focus: usize, caret: usize) -> Self {
        Self {
            foreground,
            thread,
            focus,
            caret,
        }
    }
}

/// Updates the observed target and reports whether retained text is unsafe.
pub(crate) fn target_changed(
    current: &mut Option<TargetIdentity>,
    next: Option<TargetIdentity>,
) -> bool {
    let Some(next) = next else {
        *current = None;
        return true;
    };
    current
        .replace(next)
        .is_some_and(|previous| previous != next)
}

/// Physical key-ups whose corresponding downs did not reach the application.
#[derive(Default)]
pub(crate) struct SuppressedKeyUps(HashSet<u32>);

impl SuppressedKeyUps {
    pub(crate) fn suppress(&mut self, key: u32) {
        self.0.insert(key);
    }

    pub(crate) fn allow(&mut self, key: u32) {
        self.0.remove(&key);
    }

    pub(crate) fn take(&mut self, key: u32) -> bool {
        self.0.remove(&key)
    }
}

/// Number of fully paired Windows `INPUT`s in one direct-output plan.
pub(crate) fn input_event_count(plan: &OutputPlan) -> usize {
    let inserted = plan
        .insert
        .as_deref()
        .map_or(0, |text| text.encode_utf16().count());
    plan.delete_before
        .saturating_add(inserted)
        .saturating_mul(2)
}

/// Commits a staged plan only when `SendInput` accepted every event.
pub(crate) fn adopt_after_send(
    current: &mut DirectComposer,
    next: DirectComposer,
    sent: usize,
    requested: usize,
) -> bool {
    let complete = requested != 0 && sent == requested;
    if complete {
        *current = next;
    } else {
        current.reset();
    }
    complete
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
        HookEvent::KeyUp { suppress: true } => Handling::Suppress,
        HookEvent::KeyDown { .. } | HookEvent::KeyUp { suppress: false } | HookEvent::Other => {
            Handling::PassThrough
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Handling, HookEvent, KeyboardHookStatus, SuppressedKeyUps, TargetIdentity,
        adopt_after_send, desired_status, handling, input_event_count, key_event, modifiers,
        target_changed,
    };
    use crate::input_method::models::direct_output::OutputPlan;
    use crate::input_method::models::event_tap::DirectComposer;
    use dodo_ime_core::{Key, KeyEvent, Modifiers, VietnameseConfig};
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
            HookEvent::KeyUp { suppress: false },
            HookEvent::Other,
        ] {
            assert_eq!(handling(event), Handling::PassThrough, "{event:?}");
        }
    }

    struct WindowsHarness {
        composer: DirectComposer,
        document: String,
        events: usize,
        rewrites: Vec<OutputPlan>,
    }

    impl WindowsHarness {
        fn new() -> Self {
            Self {
                composer: DirectComposer::new(VietnameseConfig::default()),
                document: String::new(),
                events: 0,
                rewrites: Vec::new(),
            }
        }

        fn type_keys(&mut self, keys: &str) {
            for key in keys.chars() {
                let event = KeyEvent::character(key);
                let plan = self.composer.process(event);
                self.document =
                    dodo_ime_core::core::truncate_graphemes(&self.document, plan.delete_before);
                if let Some(insert) = &plan.insert {
                    self.document.push_str(insert);
                }
                self.events += input_event_count(&plan);
                if plan.transforms() {
                    self.rewrites.push(plan.clone());
                }
                if plan.pass_through {
                    self.document.push(key);
                }
            }
        }
    }

    #[test]
    fn minimal_windows_plans_match_the_investigation_traces() {
        for (keys, expected_text, expected_events, expected_rewrites) in [
            ("dd", "đ", 4, &[(1, Some("đ"))][..]),
            ("uw", "ư", 4, &[(1, Some("ư"))][..]),
            ("hoiw", "hơi", 8, &[(2, Some("ơi"))][..]),
            (
                "tieengs",
                "tiếng",
                16,
                &[(1, Some("ê")), (3, Some("ếng"))][..],
            ),
        ] {
            let mut harness = WindowsHarness::new();
            harness.type_keys(keys);
            let rewrites: Vec<_> = harness
                .rewrites
                .iter()
                .map(|plan| (plan.delete_before, plan.insert.as_deref()))
                .collect();

            assert_eq!(harness.document, expected_text, "{keys}");
            assert_eq!(harness.events, expected_events, "{keys}");
            assert_eq!(rewrites, expected_rewrites, "{keys}");
        }
    }

    #[test]
    fn swallowed_down_suppresses_one_up_but_a_passed_repeat_clears_it() {
        let mut suppressed = SuppressedKeyUps::default();
        suppressed.suppress(0x44);
        assert_eq!(
            handling(HookEvent::KeyUp {
                suppress: suppressed.take(0x44),
            }),
            Handling::Suppress
        );
        assert!(!suppressed.take(0x44), "only the paired up is consumed");

        suppressed.suppress(0x44);
        suppressed.allow(0x44);
        assert_eq!(
            handling(HookEvent::KeyUp {
                suppress: suppressed.take(0x44),
            }),
            Handling::PassThrough,
            "a repeated down reached the app, so its final up must too"
        );
    }

    #[test]
    fn target_changes_and_uncertainty_reset_retained_text() {
        let target = TargetIdentity::new(1, 2, 3, 4);
        let mut observed = None;
        assert!(!target_changed(&mut observed, Some(target)));
        assert!(!target_changed(&mut observed, Some(target)));
        for changed in [
            TargetIdentity::new(9, 2, 3, 4),
            TargetIdentity::new(1, 9, 3, 4),
            TargetIdentity::new(1, 2, 9, 4),
            TargetIdentity::new(1, 2, 3, 9),
        ] {
            let mut observed = Some(target);
            assert!(target_changed(&mut observed, Some(changed)));
        }

        let mut composer = DirectComposer::new(VietnameseConfig::default());
        assert!(composer.process(KeyEvent::character('d')).pass_through);
        assert!(target_changed(
            &mut observed,
            Some(TargetIdentity::new(1, 2, 5, 4))
        ));
        composer.reset();
        let after_focus_change = composer.process(KeyEvent::character('d'));
        assert!(after_focus_change.pass_through);
        assert!(!after_focus_change.transforms());

        assert!(target_changed(&mut observed, None));
        assert_eq!(observed, None);
    }

    #[test]
    fn partial_send_resets_instead_of_adopting_the_planned_state() {
        let mut composer = DirectComposer::new(VietnameseConfig::default());
        assert!(composer.process(KeyEvent::character('d')).pass_through);
        let mut next = composer.clone();
        let plan = next.process(KeyEvent::character('d'));
        assert_eq!(input_event_count(&plan), 4);

        assert!(!adopt_after_send(&mut composer, next, 2, 4));
        let retry = composer.process(KeyEvent::character('d'));
        assert!(retry.pass_through);
        assert!(!retry.transforms(), "the failed replacement was forgotten");
    }
}
