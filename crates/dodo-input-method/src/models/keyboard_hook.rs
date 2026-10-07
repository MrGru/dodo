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

use crate::models::browser_rewrite::BrowserRewrite;
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

/// One event in the array handed to Windows' `SendInput`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WindowsInput {
    Key { virtual_key: u16, key_up: bool },
    Unicode { unit: u16, key_up: bool },
}

/// The Windows signals that prove composition is still aimed at one target.
///
/// It is the foreground process and its foreground window — the same question
/// the macOS tap asks (the target *process*, nothing finer), plus the top-level
/// window so switching between two windows of one application still resets.
///
/// # Why nothing below the top-level window is part of it
///
/// Twice now a finer Windows signal has flipped mid-word with the caret never
/// moving, and each time it broke only English words — `workflow`, `follow`,
/// `window`, `playwright`. The engine keeps one word's worth of state and
/// restores `work` from it once `k` proves the run is not Vietnamese; a reset in
/// between strands the provisional `ư` (`ưorkflow`, `playửight`). Vietnamese
/// needs no restore and composes from a clean state, which is why it looked fine.
///
/// - `GetGUIThreadInfo`'s `hwndCaret`: several toolkits call `CreateCaret`
///   lazily, on the first character, so it read `0` then the control's handle.
/// - `GetGUIThreadInfo`'s `hwndFocus`, and the call itself: Chromium and
///   Electron (Chrome, Edge, VS Code, Slack…) create their accessibility child
///   window lazily and move Win32 focus onto it, and the call can fail outright
///   for a thread that is busy or on another desktop. Either reads as a target
///   change, or as "no target", in the middle of a word.
///
/// A real caret move is already observed without them: a mouse-down (the mouse
/// hook resets), an arrow/Home/End/Tab (a boundary key), or a shortcut — every
/// key with Control, Alt or Windows held resets, including Alt+Tab, Alt+D and
/// Control+L. A program moving the caret inside one window with no observable
/// event is the residual risk macOS has always accepted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TargetIdentity {
    process: u32,
    window: usize,
}

impl TargetIdentity {
    pub(crate) fn new(process: u32, window: usize) -> Self {
        Self { process, window }
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

/// One complete, ordered Windows input batch for a rewritten plan.
///
/// `shift_held` is whether the user is physically holding Shift as the batch is
/// sent. macOS spells the Chromium `Shift`+`Left` as one arrow event carrying a
/// Shift *flag*, which leaves the real modifier alone; Windows has no flag, so
/// the batch presses and releases `VK_SHIFT` itself. Releasing it while the user
/// is holding Shift — typing a capitalised or all-caps word into Chrome — tells
/// every application, and `GetAsyncKeyState`, that Shift is up, so every letter
/// after the first rewritten one arrives lowercase until the key repeats. A held
/// Shift already extends the selection, so the batch then sends only `Left`.
pub(crate) fn output_inputs(
    plan: &OutputPlan,
    rewrite: &BrowserRewrite,
    shift_held: bool,
) -> Vec<WindowsInput> {
    let inserted = plan
        .insert
        .as_deref()
        .map_or(0, |text| text.encode_utf16().count());
    let committed = rewrite
        .commit_character
        .map_or(0, |text| text.encode_utf16().count());
    let mut inputs = Vec::with_capacity(
        rewrite
            .delete_before
            .saturating_add(inserted)
            .saturating_add(committed)
            .saturating_mul(2)
            .saturating_add(usize::from(rewrite.extend_selection) * 4),
    );
    if rewrite.extend_selection {
        let press_shift = !shift_held;
        if press_shift {
            inputs.push(WindowsInput::Key {
                virtual_key: vk::SHIFT as u16,
                key_up: false,
            });
        }
        inputs.extend([
            WindowsInput::Key {
                virtual_key: vk::LEFT as u16,
                key_up: false,
            },
            WindowsInput::Key {
                virtual_key: vk::LEFT as u16,
                key_up: true,
            },
        ]);
        if press_shift {
            inputs.push(WindowsInput::Key {
                virtual_key: vk::SHIFT as u16,
                key_up: true,
            });
        }
    }
    if let Some(text) = rewrite.commit_character {
        push_unicode(&mut inputs, text);
    }
    for _ in 0..rewrite.delete_before {
        inputs.extend([
            WindowsInput::Key {
                virtual_key: vk::BACK as u16,
                key_up: false,
            },
            WindowsInput::Key {
                virtual_key: vk::BACK as u16,
                key_up: true,
            },
        ]);
    }
    if let Some(text) = &plan.insert {
        push_unicode(&mut inputs, text);
    }
    inputs
}

fn push_unicode(inputs: &mut Vec<WindowsInput>, text: &str) {
    for unit in text.encode_utf16() {
        inputs.extend([
            WindowsInput::Unicode {
                unit,
                key_up: false,
            },
            WindowsInput::Unicode { unit, key_up: true },
        ]);
    }
}

/// Number of fully paired Windows `INPUT`s in one verbatim plan.
pub(crate) fn input_event_count(plan: &OutputPlan) -> usize {
    output_inputs(plan, &BrowserRewrite::verbatim(plan), false).len()
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
    pub const BACK: u32 = 0x08;
    pub const SHIFT: u32 = 0x10;
    pub const LEFT: u32 = 0x25;
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

/// Whether a key the composer cannot use leaves the word in flight alone.
///
/// Only a modifier does. It types nothing and moves no caret — `⇧` in the
/// middle of a word is how a capital letter is typed — and Windows reports it
/// as a key-down, then again on every autorepeat while it is held. The macOS
/// tap never sees one as a key-down at all (`FlagsChanged`), which is why the
/// same all-caps word composed there and not here. Any other untranslatable
/// key leaves the end cursor unknown and resets.
pub(crate) fn keeps_composition(key: Key) -> bool {
    key == Key::Modifier
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
        WindowsInput, adopt_after_send, handling, input_event_count, keeps_composition, key_event,
        layout_state, modifiers, output_inputs, physical_modifiers, target_changed, vk,
        with_key_down,
    };
    use crate::models::browser_rewrite::BrowserRewrite;
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
            ("cos", "có", 4, &[(1, Some("ó"))][..]),
            ("dar", "dả", 4, &[(1, Some("ả"))][..]),
            ("gif", "gì", 4, &[(1, Some("ì"))][..]),
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

    /// Applies a synthetic batch to the part of a Chromium address bar the
    /// composer knows. Inline autocomplete's selected suffix is represented by
    /// `suggestion_selected`: its first deletion dismisses the suggestion
    /// without touching that known prefix.
    fn chromium_address_bar_after(
        mut document: String,
        suggestion_selected: bool,
        inputs: &[WindowsInput],
    ) -> String {
        let mut suggestion_selected = suggestion_selected;
        let mut shift = false;
        let mut selected_tail = false;
        for input in inputs {
            match *input {
                WindowsInput::Key {
                    virtual_key,
                    key_up,
                } if virtual_key == vk::SHIFT as u16 => shift = !key_up,
                WindowsInput::Key {
                    virtual_key,
                    key_up: false,
                } if virtual_key == vk::LEFT as u16 && shift => {
                    suggestion_selected = false;
                    selected_tail = !document.is_empty();
                }
                WindowsInput::Key {
                    virtual_key,
                    key_up: false,
                } if virtual_key == vk::BACK as u16 => {
                    if suggestion_selected {
                        suggestion_selected = false;
                    } else {
                        document = dodo_ime_core::core::truncate_graphemes(&document, 1);
                        selected_tail = false;
                    }
                }
                WindowsInput::Unicode {
                    unit,
                    key_up: false,
                } => {
                    if selected_tail {
                        document = dodo_ime_core::core::truncate_graphemes(&document, 1);
                        selected_tail = false;
                    }
                    suggestion_selected = false;
                    document.push(char::from_u32(u32::from(unit)).expect("test uses BMP text"));
                }
                WindowsInput::Key { .. } | WindowsInput::Unicode { .. } => {}
            }
        }
        document
    }

    #[test]
    fn windows_chromium_stages_the_captains_english_w_restore_plans() {
        let plans = |keys: &str| {
            let mut composer = DirectComposer::new(VietnameseConfig::default());
            keys.chars()
                .filter_map(|key| {
                    let plan = composer.process(KeyEvent::character(key));
                    plan.transforms().then_some((key, plan))
                })
                .collect::<Vec<_>>()
        };

        let window = plans("window");
        assert_eq!(
            window
                .iter()
                .map(|(key, plan)| (*key, plan.delete_before, plan.insert.as_deref()))
                .collect::<Vec<_>>(),
            [('w', 0, Some("ư")), ('d', 3, Some("wind"))]
        );
        assert!(plans("gateway").is_empty());
        assert!(plans("follow").is_empty());

        let restore = &window[1].1;
        let mut before = chromium_address_bar_after(
            "ưin".into(),
            true,
            &output_inputs(restore, &BrowserRewrite::verbatim(restore), false),
        );
        before.push_str("ow");
        assert_eq!(before, "ưwindow");

        let rewrite = BrowserRewrite::plan(true, Some("chrome.exe"), restore);
        let mut after = chromium_address_bar_after(
            "ưin".into(),
            true,
            &output_inputs(restore, &rewrite, false),
        );
        after.push_str("ow");
        assert_eq!(after, "window");
    }

    #[test]
    fn chromium_output_stages_shift_left_before_the_replacement() {
        let plan = OutputPlan {
            delete_before: 1,
            insert: Some("ó".into()),
            pass_through: false,
        };
        let rewrite = BrowserRewrite::plan(true, Some("chrome.exe"), &plan);
        assert_eq!(
            output_inputs(&plan, &rewrite, false),
            vec![
                WindowsInput::Key {
                    virtual_key: vk::SHIFT as u16,
                    key_up: false,
                },
                WindowsInput::Key {
                    virtual_key: vk::LEFT as u16,
                    key_up: false,
                },
                WindowsInput::Key {
                    virtual_key: vk::LEFT as u16,
                    key_up: true,
                },
                WindowsInput::Key {
                    virtual_key: vk::SHIFT as u16,
                    key_up: true,
                },
                WindowsInput::Unicode {
                    unit: 'ó' as u16,
                    key_up: false,
                },
                WindowsInput::Unicode {
                    unit: 'ó' as u16,
                    key_up: true,
                },
            ]
        );
    }

    /// A physically held Shift must survive the Chromium batch: the batch may
    /// neither press nor release `VK_SHIFT`, or the rest of a capitalised word
    /// reaches the browser lowercase. macOS never had this, because its arrow
    /// carries Shift as a flag rather than as a separate key.
    #[test]
    fn a_held_shift_is_never_released_by_the_chromium_batch() {
        let plan = OutputPlan {
            delete_before: 1,
            insert: Some("Ó".into()),
            pass_through: false,
        };
        let rewrite = BrowserRewrite::plan(true, Some("chrome.exe"), &plan);
        let inputs = output_inputs(&plan, &rewrite, true);
        assert!(
            !inputs.iter().any(|input| matches!(
                input,
                WindowsInput::Key { virtual_key, .. } if *virtual_key == vk::SHIFT as u16
            )),
            "{inputs:?}"
        );
        assert_eq!(
            inputs[..2],
            [
                WindowsInput::Key {
                    virtual_key: vk::LEFT as u16,
                    key_up: false,
                },
                WindowsInput::Key {
                    virtual_key: vk::LEFT as u16,
                    key_up: true,
                },
            ]
        );
        // Otherwise the batch is exactly the one sent without a held Shift.
        let mut unheld = output_inputs(&plan, &rewrite, false);
        unheld.retain(|input| {
            !matches!(
                input,
                WindowsInput::Key { virtual_key, .. } if *virtual_key == vk::SHIFT as u16
            )
        });
        assert_eq!(inputs, unheld);
    }

    /// A modifier types nothing and moves no caret. macOS never even offers one
    /// to the composer (`FlagsChanged` is not a key-down); Windows reports it as
    /// a key-down — repeatedly, while it is held — so it must be told apart from
    /// an unknown key rather than resetting the word in flight.
    #[test]
    fn a_modifier_key_down_keeps_the_word_in_flight() {
        assert!(keeps_composition(Key::Modifier));
        for key in [Key::Other, Key::Character, Key::Backspace, Key::Space] {
            assert!(!keeps_composition(key), "{key:?}");
        }
        // Every key a held modifier autorepeats as is a modifier.
        for vkey in [
            vk::LSHIFT,
            vk::RSHIFT,
            vk::SHIFT,
            vk::LCONTROL,
            vk::LMENU,
            vk::LWIN,
        ] {
            assert!(keeps_composition(
                key_event(vkey, None, Modifiers::default()).key
            ));
        }
    }

    #[test]
    fn firefox_output_commits_the_suggestion_before_the_extra_backspace() {
        let plan = OutputPlan {
            delete_before: 1,
            insert: Some("ó".into()),
            pass_through: false,
        };
        let rewrite = BrowserRewrite::plan(true, Some("firefox.exe"), &plan);
        let inputs = output_inputs(&plan, &rewrite, false);
        assert_eq!(
            inputs[..2],
            [
                WindowsInput::Unicode {
                    unit: '\u{200b}' as u16,
                    key_up: false,
                },
                WindowsInput::Unicode {
                    unit: '\u{200b}' as u16,
                    key_up: true,
                },
            ]
        );
        assert_eq!(
            inputs
                .iter()
                .filter(|input| matches!(input, WindowsInput::Key { virtual_key, .. } if *virtual_key == vk::BACK as u16))
                .count(),
            4,
            "two Backspace pairs: the plan's one plus the commit character"
        );
        assert_eq!(
            inputs[inputs.len() - 2..],
            [
                WindowsInput::Unicode {
                    unit: 'ó' as u16,
                    key_up: false,
                },
                WindowsInput::Unicode {
                    unit: 'ó' as u16,
                    key_up: true,
                },
            ]
        );
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
        let target = TargetIdentity::new(1, 2);
        let mut observed = None;
        assert!(!target_changed(&mut observed, Some(target)));
        assert!(!target_changed(&mut observed, Some(target)));
        for changed in [TargetIdentity::new(9, 2), TargetIdentity::new(1, 9)] {
            let mut observed = Some(target);
            assert!(target_changed(&mut observed, Some(changed)));
        }

        let mut composer = DirectComposer::new(VietnameseConfig::default());
        assert!(composer.process(KeyEvent::character('d')).pass_through);
        assert!(target_changed(
            &mut observed,
            Some(TargetIdentity::new(1, 5))
        ));
        composer.reset();
        let after_window_change = composer.process(KeyEvent::character('d'));
        assert!(after_window_change.pass_through);
        assert!(!after_window_change.transforms());

        assert!(target_changed(&mut observed, None));
        assert_eq!(observed, None);
    }

    /// The Windows-only regression class: nothing finer than the foreground
    /// window is part of the target, so a control creating its caret, or a
    /// Chromium window moving focus onto its lazily created accessibility
    /// child, cannot read as a target change mid-word. The identity is built
    /// from the process and the top-level window alone, so two keystrokes in
    /// one window are one target whatever happened inside it.
    #[test]
    fn only_the_process_and_its_foreground_window_identify_the_target() {
        let mut observed = Some(TargetIdentity::new(42, 0x100));
        assert!(
            !target_changed(&mut observed, Some(TargetIdentity::new(42, 0x100))),
            "a change inside the window must not reset an in-flight English word"
        );

        // Another window, of this process or another, still resets.
        assert!(target_changed(
            &mut observed,
            Some(TargetIdentity::new(42, 0x999))
        ));
        assert!(target_changed(
            &mut observed,
            Some(TargetIdentity::new(7, 0x999))
        ));
    }

    /// What a spurious reset does to `playwright`, stated as the output the
    /// user saw. It takes two resets, one either side of the `w`, which is what
    /// a focus handle flipping between Chromium's top-level and accessibility
    /// windows produced.
    #[test]
    fn two_spurious_resets_turn_playwright_into_vietnamese() {
        let mut composer = DirectComposer::new(VietnameseConfig::default());
        let mut document = String::new();
        for (at, key) in "playwright".chars().enumerate() {
            if at == 4 || at == 5 {
                composer.reset();
            }
            let plan = composer.process(KeyEvent::character(key));
            document = plain_document_after(
                document,
                &output_inputs(&plan, &BrowserRewrite::verbatim(&plan), false),
            );
            if plan.pass_through {
                document.push(key);
            }
        }
        assert_eq!(document, "playưright");
        assert_eq!(windows_document("playwright"), "playwright");
    }

    /// Apply one verbatim Windows `SendInput` batch to a plain end-cursor
    /// document (no browser autocomplete selection): Backspace deletes one
    /// grapheme, a Unicode unit inserts itself.
    fn plain_document_after(mut document: String, inputs: &[WindowsInput]) -> String {
        for input in inputs {
            match *input {
                WindowsInput::Key {
                    virtual_key,
                    key_up: false,
                } if virtual_key == vk::BACK as u16 => {
                    document = dodo_ime_core::core::truncate_graphemes(&document, 1);
                }
                WindowsInput::Unicode {
                    unit,
                    key_up: false,
                } => document.push(char::from_u32(u32::from(unit)).expect("BMP test text")),
                WindowsInput::Key { .. } | WindowsInput::Unicode { .. } => {}
            }
        }
        document
    }

    /// Type `keys` the way the Windows hook does, into a plain (non-browser)
    /// end-cursor document: every transforming plan is staged through the real
    /// `output_inputs` batch and applied, and a passed-through key types itself
    /// after it. This is the whole Windows model path bar the OS callback, which
    /// is `#[cfg(windows)]` and can only run in CI.
    fn windows_document(keys: &str) -> String {
        let mut composer = DirectComposer::new(VietnameseConfig::default());
        let mut document = String::new();
        for key in keys.chars() {
            let event = KeyEvent::character(key);
            let plan = composer.process(event);
            if plan.transforms() {
                let rewrite = BrowserRewrite::plan(true, Some("notepad.exe"), &plan);
                document = plain_document_after(document, &output_inputs(&plan, &rewrite, false));
            }
            if plan.pass_through
                && let Some(typed) = event.typed()
            {
                document.push(typed);
            }
        }
        document
    }

    /// The captain's report, staged end to end through the Windows path: the
    /// English words that a Telex engine transforms mid-word must land literal.
    /// `wwindoww` collapses its leading doubled `w` to one (the Telex escape),
    /// exactly as the shared engine specifies.
    #[test]
    fn the_windows_path_keeps_the_captains_english_words_literal() {
        for (keys, document) in [
            ("workflow", "workflow"),
            ("follow", "follow"),
            ("playwright", "playwright"),
            ("window", "window"),
            ("gateway", "gateway"),
            ("widow", "widow"),
            ("wwindoww", "windoww"),
        ] {
            assert_eq!(windows_document(keys), document, "{keys}");
        }
    }

    /// Why the target reset must not fire mid-word, stated as the corruption it
    /// would cause. `workflow` reaches its literal spelling only because the
    /// engine keeps one word's worth of state and restores `work` from it once
    /// `k` proves the run is not Vietnamese. Resetting the composer after the
    /// opening `w` has become a provisional `ư` — which is what a spurious
    /// `TargetIdentity` change once did — strands that `ư` and the word comes out
    /// mangled, while Vietnamese, needing no restore, would be unharmed.
    #[test]
    fn a_reset_mid_english_word_strands_the_restore() {
        // Uninterrupted, the Windows path restores the literal spelling.
        assert_eq!(windows_document("workflow"), "workflow");

        // Interrupted right after the first key's `w` -> `ư` rewrite, the engine
        // loses the raw `w` it would have rebuilt `work` from.
        let mut composer = DirectComposer::new(VietnameseConfig::default());
        let mut document = String::new();
        for (index, key) in "workflow".chars().enumerate() {
            let event = KeyEvent::character(key);
            let plan = composer.process(event);
            if plan.transforms() {
                let rewrite = BrowserRewrite::plan(true, Some("notepad.exe"), &plan);
                document = plain_document_after(document, &output_inputs(&plan, &rewrite, false));
            }
            if plan.pass_through
                && let Some(typed) = event.typed()
            {
                document.push(typed);
            }
            if index == 0 {
                // The spurious reset: the document already shows the provisional
                // `ư`, and the engine's per-word state is thrown away.
                assert_eq!(document, "ư");
                composer.reset();
            }
        }
        assert_ne!(
            document, "workflow",
            "a mid-word reset must corrupt the word"
        );
        assert!(
            document.starts_with('ư'),
            "the stranded provisional letter stays in the document: {document:?}"
        );
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
