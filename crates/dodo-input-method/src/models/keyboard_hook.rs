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
//! processes only Vietnamese input with a fully known key-down.
//! Repeats, injected input, shortcuts, and uncertain events pass through; only
//! a key-up paired with a consumed physical key-down is consumed.

use std::collections::HashSet;

use dodo_ime_core::{Key, KeyEvent, Modifiers};

use crate::models::direct_output::OutputPlan;
use crate::models::event_tap::DirectComposer;

/// What the pane can honestly report about the dodo-lifetime-only fallback.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum KeyboardHookStatus {
    #[default]
    Inactive,
    Running,
    Failed,
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

/// The Windows virtual keys this module names by identity.
///
/// Spelled out rather than imported, for the reason the whole module exists:
/// `windows-sys` is not available on the host these rules are tested on.
pub mod vk {
    pub const SHIFT: u32 = 0x10;
    pub const CONTROL: u32 = 0x11;
    pub const MENU: u32 = 0x12;
    pub const CAPITAL: u32 = 0x14;
    pub const LWIN: u32 = 0x5b;
    pub const RWIN: u32 = 0x5c;
    pub const LSHIFT: u32 = 0xa0;
    pub const RSHIFT: u32 = 0xa1;
    pub const LCONTROL: u32 = 0xa2;
    pub const RCONTROL: u32 = 0xa3;
    pub const LMENU: u32 = 0xa4;
    pub const RMENU: u32 = 0xa5;
}

/// The physical keyboard state a low-level hook has to supply for itself.
///
/// # Why `GetKeyboardState` is the wrong question here
///
/// A `WH_KEYBOARD_LL` callback runs on the thread that installed the hook —
/// dodo's own — while the keystroke is on its way to whichever application has
/// focus. `GetKeyboardState` answers **per thread**, and a thread's copy only
/// advances as it reads key messages from its own queue. dodo is in the
/// background whenever this matters, so its copy is frozen at whatever it was
/// when dodo last had focus: Shift reads as up.
///
/// Two things follow, and both are the defect this type exists to remove.
/// `ToUnicodeEx` handed that array obligingly returns the *unshifted*
/// character, so no capital letter could reach the Vietnamese engine and every
/// rewritten syllable came back lowercase. And [`Modifiers`] read from the same
/// array is always empty, so [`Shortcut::matches`](crate::models::settings::Shortcut::matches)
/// — which compares modifiers exactly and refuses a shortcut with no command
/// modifier — could never fire the language switch.
///
/// `GetAsyncKeyState` is not queue-bound and answers about the physical
/// keyboard, so the service reads these there and hands them here. Left and
/// right are kept apart because `ToUnicodeEx` distinguishes them: AltGr is
/// right-Alt plus left-Control, and folding either side away would break every
/// layout that has one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PhysicalKeys {
    pub left_shift: bool,
    pub right_shift: bool,
    pub left_control: bool,
    pub right_control: bool,
    pub left_alt: bool,
    pub right_alt: bool,
    pub left_windows: bool,
    pub right_windows: bool,
    /// The caps lock **toggle**, not the key. It decides which character the
    /// layout produces and is the second way a key arrives miscased.
    pub caps_lock: bool,
}

impl PhysicalKeys {
    pub const NONE: PhysicalKeys = PhysicalKeys {
        left_shift: false,
        right_shift: false,
        left_control: false,
        right_control: false,
        left_alt: false,
        right_alt: false,
        left_windows: false,
        right_windows: false,
        caps_lock: false,
    };

    pub fn shift(self) -> bool {
        self.left_shift || self.right_shift
    }

    pub fn control(self) -> bool {
        self.left_control || self.right_control
    }

    pub fn alt(self) -> bool {
        self.left_alt || self.right_alt
    }

    pub fn windows(self) -> bool {
        self.left_windows || self.right_windows
    }
}

/// The 256-byte array `ToUnicodeEx` reads, **built rather than fetched**.
///
/// Building it is not merely a workaround for the stale snapshot described on
/// [`PhysicalKeys`]: a stale array is worse than an empty one, because a
/// Control byte left set from an old focus would make `ToUnicodeEx` return a
/// control character for an ordinary letter. Starting from zero means exactly
/// the keys named here are down and nothing else is.
///
/// `0x80` is Windows' "key is down" bit and `0x01` its toggle bit.
pub fn layout_state(vkey: u32, physical: PhysicalKeys) -> [u8; 256] {
    let mut state = [0_u8; 256];
    let mut down = |key: u32, held: bool| {
        if held && let Some(byte) = state.get_mut(key as usize) {
            *byte |= 0x80;
        }
    };
    down(vk::LSHIFT, physical.left_shift);
    down(vk::RSHIFT, physical.right_shift);
    down(vk::SHIFT, physical.shift());
    down(vk::LCONTROL, physical.left_control);
    down(vk::RCONTROL, physical.right_control);
    down(vk::CONTROL, physical.control());
    down(vk::LMENU, physical.left_alt);
    down(vk::RMENU, physical.right_alt);
    down(vk::MENU, physical.alt());
    down(vk::LWIN, physical.left_windows);
    down(vk::RWIN, physical.right_windows);
    // The key being translated has not reached the queue yet, so the callback
    // is the only thing that knows it is down.
    down(vkey, true);
    if physical.caps_lock
        && let Some(byte) = state.get_mut(vk::CAPITAL as usize)
    {
        *byte |= 0x01;
    }
    state
}

/// Folds the key that is arriving into the physical state.
///
/// A low-level hook runs **before** Windows has recorded the press, so a
/// modifier's own key-down is the one press `GetAsyncKeyState` can still report
/// as up — and that is exactly the press a modifier-only shortcut fires on. The
/// hook is the only thing that knows about it, so it says so here.
///
/// A `WH_KEYBOARD_LL` callback reports the left/right virtual key rather than
/// the aggregate; the aggregate is handled anyway so a caller that normalizes
/// differently cannot silently lose the press.
pub fn with_key_down(mut physical: PhysicalKeys, vkey: u32) -> PhysicalKeys {
    match vkey {
        vk::LSHIFT | vk::SHIFT => physical.left_shift = true,
        vk::RSHIFT => physical.right_shift = true,
        vk::LCONTROL | vk::CONTROL => physical.left_control = true,
        vk::RCONTROL => physical.right_control = true,
        vk::LMENU | vk::MENU => physical.left_alt = true,
        vk::RMENU => physical.right_alt = true,
        vk::LWIN => physical.left_windows = true,
        vk::RWIN => physical.right_windows = true,
        _ => {}
    }
    physical
}

/// The engine modifiers those same physical keys mean.
///
/// Read from the identical source as [`layout_state`], deliberately: a flag
/// that disagreed with the character the layout produced is precisely the bug
/// class here — `shift` false beside a capital `D`, or the reverse.
pub fn physical_modifiers(physical: PhysicalKeys) -> Modifiers {
    modifiers(
        physical.control(),
        physical.alt(),
        physical.shift(),
        physical.windows(),
    )
}

/// Caps lock as a background hook can actually know it.
///
/// `GetKeyState(VK_CAPITAL)` answers per thread for the same reason
/// [`PhysicalKeys`] gives, so its toggle bit goes stale in the background too.
/// The hook sees every physical press and tracks the toggle from its startup
/// snapshot. Windows toggles the lock on key **down**.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CapsLock(bool);

impl CapsLock {
    pub fn new(initial: bool) -> CapsLock {
        CapsLock(initial)
    }

    pub fn on(self) -> bool {
        self.0
    }

    /// Records one **fresh** physical key-down.
    ///
    /// The caller owes the freshness: a held Caps Lock can autorepeat while
    /// Windows toggles the lock exactly once, so a repeat must not be offered
    /// here. The press that toggles the lock types nothing itself, so which
    /// side of the flip it is read on cannot matter.
    pub fn observe_key_down(&mut self, vkey: u32) {
        if vkey == vk::CAPITAL {
            self.0 = !self.0;
        }
    }
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
/// This lives in `models/` rather than beside the hook so the Windows key
/// vocabulary is unit-tested from every host, including macOS.
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
        CapsLock, Handling, HookEvent, PhysicalKeys, SuppressedKeyUps, TargetIdentity,
        adopt_after_send, handling, input_event_count, key_event, layout_state, modifiers,
        physical_modifiers, target_changed, vk, with_key_down,
    };
    use crate::models::direct_output::OutputPlan;
    use crate::models::event_tap::DirectComposer;
    use crate::models::live_switch::LiveSwitch;
    use crate::models::settings::{
        LanguageSwitch, SettingsDocument, Shortcut, ShortcutKey, ShortcutModifiers,
    };
    use dodo_ime_core::{ActiveLanguages, Key, KeyEvent, LanguageId, Modifiers, VietnameseConfig};

    /// Held Shift, and nothing else.
    fn shift_held() -> PhysicalKeys {
        PhysicalKeys {
            left_shift: true,
            ..PhysicalKeys::NONE
        }
    }

    fn down(state: &[u8; 256], key: u32) -> bool {
        state[key as usize] & 0x80 != 0
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

    /// The whole Windows casing defect, stated as an assertion.
    ///
    /// The array a background thread's `GetKeyboardState` hands back says every
    /// key is up. Built from the physical keyboard instead, Shift is down in
    /// both the array `ToUnicodeEx` reads and the modifiers the engine reads —
    /// which is the pairing that has to hold, because a `shift` flag that
    /// disagreed with the character would be the same bug wearing the other
    /// shoe.
    #[test]
    fn a_held_shift_reaches_both_the_layout_state_and_the_modifiers() {
        let queue_says_nothing_is_held = [0_u8; 256];
        assert!(!down(&queue_says_nothing_is_held, vk::SHIFT));

        let state = layout_state(0x44, shift_held());
        assert!(
            down(&state, vk::SHIFT),
            "ToUnicodeEx would type a lowercase d"
        );
        assert!(down(&state, vk::LSHIFT));
        assert!(!down(&state, vk::RSHIFT));
        assert!(down(&state, 0x44), "the key being translated is down");

        let modifiers = physical_modifiers(shift_held());
        assert!(modifiers.shift);
        assert!(
            modifiers.is_plain(),
            "Shift alone still types; only a command modifier suppresses text"
        );
        assert_eq!(
            key_event(0x44, Some('D'), modifiers).typed(),
            Some('D'),
            "the capital the layout produced must reach the engine"
        );
    }

    /// A modifier-only shortcut fires on the modifier's own key-down, which is
    /// the one press the physical read can still be a beat behind on.
    #[test]
    fn the_arriving_key_is_folded_into_the_physical_state() {
        let control_held = PhysicalKeys {
            left_control: true,
            ..PhysicalKeys::NONE
        };
        let completing = with_key_down(control_held, vk::RSHIFT);
        assert!(completing.right_shift && !completing.left_shift);

        let modifiers = physical_modifiers(completing);
        assert!(modifiers.control && modifiers.shift);
        assert!(
            Shortcut {
                modifiers: ShortcutModifiers {
                    control: true,
                    shift: true,
                    ..ShortcutModifiers::NONE
                },
                key: ShortcutKey::Modifiers,
            }
            .matches(&key_event(vk::RSHIFT, None, modifiers)),
            "the press that completes the combination has to match on itself"
        );

        // Each side lands on its own field, and an ordinary key changes nothing.
        for (vkey, expected) in [
            (vk::LSHIFT, shift_held()),
            (
                vk::LWIN,
                PhysicalKeys {
                    left_windows: true,
                    ..PhysicalKeys::NONE
                },
            ),
            (0x44, PhysicalKeys::NONE),
        ] {
            assert_eq!(
                with_key_down(PhysicalKeys::NONE, vkey),
                expected,
                "{vkey:#04x}"
            );
        }
    }

    /// Nothing is down that was not named. A stale array is worse than an empty
    /// one: a Control byte left over from an old focus makes `ToUnicodeEx`
    /// return a control character where the user typed a letter.
    #[test]
    fn the_layout_state_is_built_rather_than_inherited() {
        let state = layout_state(0x41, PhysicalKeys::NONE);
        for key in [
            vk::SHIFT,
            vk::LSHIFT,
            vk::RSHIFT,
            vk::CONTROL,
            vk::LCONTROL,
            vk::RCONTROL,
            vk::MENU,
            vk::LMENU,
            vk::RMENU,
            vk::LWIN,
            vk::RWIN,
        ] {
            assert!(!down(&state, key), "{key:#04x} was never held");
        }
        assert!(down(&state, 0x41));
        assert_eq!(state[vk::CAPITAL as usize] & 0x01, 0, "the toggle is off");
        assert_eq!(physical_modifiers(PhysicalKeys::NONE), Modifiers::NONE);
    }

    /// AltGr is right-Alt with left-Control, and a layout that has one needs
    /// both sides reported separately or its characters cannot be produced.
    #[test]
    fn each_side_of_a_modifier_survives_into_the_layout_state() {
        let alt_gr = PhysicalKeys {
            right_alt: true,
            left_control: true,
            ..PhysicalKeys::NONE
        };
        let state = layout_state(0x45, alt_gr);
        assert!(down(&state, vk::RMENU));
        assert!(!down(&state, vk::LMENU));
        assert!(down(&state, vk::MENU), "the aggregate is set too");
        assert!(down(&state, vk::LCONTROL));
        assert!(!down(&state, vk::RCONTROL));
        assert!(down(&state, vk::CONTROL));

        let modifiers = physical_modifiers(alt_gr);
        assert!(modifiers.alt && modifiers.control);
        assert!(
            !modifiers.is_plain(),
            "AltGr reads as a command press, exactly as Option does on macOS"
        );
    }

    /// The caps lock toggle is the second way a Windows key arrives miscased,
    /// and the hook has to track it rather than ask a background thread.
    #[test]
    fn caps_lock_is_a_toggle_the_hook_follows_itself() {
        let mut caps = CapsLock::new(false);
        assert!(!caps.on());
        caps.observe_key_down(vk::CAPITAL);
        assert!(caps.on());
        caps.observe_key_down(vk::CAPITAL);
        assert!(!caps.on());
        // Any other key leaves it alone.
        caps.observe_key_down(vk::CAPITAL);
        caps.observe_key_down(0x44);
        assert!(caps.on());

        let state = layout_state(
            0x44,
            PhysicalKeys {
                caps_lock: true,
                ..PhysicalKeys::NONE
            },
        );
        assert_eq!(state[vk::CAPITAL as usize] & 0x01, 0x01);
        assert!(
            !down(&state, vk::CAPITAL),
            "the toggle is not the key being held"
        );
        assert!(
            !physical_modifiers(PhysicalKeys {
                caps_lock: true,
                ..PhysicalKeys::NONE
            })
            .shift,
            "caps lock is applied by the layout, never reported as Shift"
        );
    }

    /// The Windows half of item 4, end to end at the layer that can be tested
    /// here: a physically held `⌃⇧` reaches the shortcut, the shortcut cycles
    /// the enabled languages, and the listener's own answer to "may I
    /// transform" follows in the same step.
    ///
    /// Everything after that is `InputMethod::set_language`, which is the one
    /// path the tray, the settings file and the pane all hang off.
    #[test]
    fn a_physically_held_shortcut_cycles_the_windows_language() {
        let document = SettingsDocument {
            language: LanguageId::English,
            active_languages: ActiveLanguages::from_languages([
                LanguageId::English,
                LanguageId::Vietnamese,
            ])
            .expect("two languages"),
            language_switch: LanguageSwitch {
                shortcut: Shortcut::DEFAULT,
                beep: false,
            },
            ..SettingsDocument::default()
        };
        let mut live = LiveSwitch::new(&document);
        assert!(!live.transforms(), "English types through");

        let physical = PhysicalKeys {
            left_control: true,
            right_shift: true,
            ..PhysicalKeys::NONE
        };
        // `ToUnicodeEx` gives Ctrl-Space a control character, which is not text;
        // the space bar's identity is what the shortcut is matched on.
        let press = key_event(0x20, None, physical_modifiers(physical));
        assert_eq!(press.key, Key::Space, "0x20 is the space bar");
        assert_eq!(
            live.cycle(&press).map(|cycled| cycled.language),
            Some(LanguageId::Vietnamese)
        );
        assert!(live.transforms(), "the engine follows in the same step");
        assert_eq!(
            live.cycle(&press).map(|cycled| cycled.language),
            Some(LanguageId::English),
            "and back, because the listener never stops observing keys"
        );

        // The stale-state reading this replaces: no modifiers at all, so a
        // shortcut that must hold a command modifier can never match.
        let stale = key_event(0x20, Some(' '), Modifiers::NONE);
        assert_eq!(live.cycle(&stale), None);
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
