//! Windows virtual keys normalized into `dodo-ime-core` events.
//!
//! Windows supplies identity (`VK_*`) separately from the character resolved by
//! the user's keyboard layout. The host gives this module both. A dead key,
//! ligature, or failed layout conversion has no single-character reading and is
//! returned as `None` by the native adapter, so it passes through unchanged.
//!
//! # One array decides both the character and the modifier flags
//!
//! `ToUnicodeEx` reads a 256-byte keyboard state, and so does
//! [`state_modifiers`]. That is deliberate: a `shift` flag that disagreed with
//! the case of the character beside it is the defect this file exists to make
//! impossible, and it is exactly what the Keyboard Hook fallback used to
//! produce. [`merge_physical`] is the other half — see its docs for what
//! `GetKeyboardState` does and does not promise.

use dodo_ime_core::{Key, KeyEvent, Modifiers};

pub const VK_BACK: u32 = 0x08;
pub const VK_TAB: u32 = 0x09;
pub const VK_RETURN: u32 = 0x0d;
pub const VK_ESCAPE: u32 = 0x1b;
pub const VK_SPACE: u32 = 0x20;
pub const VK_PRIOR: u32 = 0x21;
pub const VK_NEXT: u32 = 0x22;
pub const VK_END: u32 = 0x23;
pub const VK_HOME: u32 = 0x24;
pub const VK_LEFT: u32 = 0x25;
pub const VK_UP: u32 = 0x26;
pub const VK_RIGHT: u32 = 0x27;
pub const VK_DOWN: u32 = 0x28;
pub const VK_DELETE: u32 = 0x2e;
pub const VK_SHIFT: u32 = 0x10;
pub const VK_CONTROL: u32 = 0x11;
pub const VK_MENU: u32 = 0x12;
pub const VK_LWIN: u32 = 0x5b;
pub const VK_RWIN: u32 = 0x5c;
pub const VK_LSHIFT: u32 = 0xa0;
pub const VK_RSHIFT: u32 = 0xa1;
pub const VK_LCONTROL: u32 = 0xa2;
pub const VK_RCONTROL: u32 = 0xa3;
pub const VK_LMENU: u32 = 0xa4;
pub const VK_RMENU: u32 = 0xa5;

/// Windows' "this key is down" bit in a keyboard-state array.
const DOWN: u8 = 0x80;

/// Forces the physical keyboard into a queue-synchronised snapshot.
///
/// `GetKeyboardState` answers about the **calling thread**, and only advances as
/// that thread reads key messages from its own queue. Inside this DLL that is
/// the application's own thread while it is handling the very key press being
/// translated, so the snapshot should already be right — but "should" is doing
/// a lot of work in a text service loaded into somebody else's process, and the
/// failure mode is silent and total: a Shift byte that is not set makes
/// `ToUnicodeEx` return the unshifted character and makes every recorded
/// shortcut with a modifier in it unmatchable.
///
/// So the modifiers `held` reports are merged in rather than trusted to be
/// there already. Merging can only ever *add* a modifier the user is physically
/// holding, so a correct snapshot is unchanged; it cannot repair the opposite
/// race, where a modifier was released between the press and this call, and
/// nothing can.
///
/// `vkey` is forced down for the same reason: a key event sink can be called
/// before the press reaches the queue.
///
/// `held` is `GetAsyncKeyState`, which is not queue-bound. It is a parameter so
/// that every rule above is a unit test on a host that has no `user32.dll`.
pub fn merge_physical(state: &mut [u8; 256], vkey: u32, held: impl Fn(u32) -> bool) {
    // Left and right are merged separately because `ToUnicodeEx` reads them:
    // AltGr is right-Alt with left-Control, and a layout that has one cannot
    // produce its characters from the aggregates alone.
    for key in [
        VK_LSHIFT,
        VK_RSHIFT,
        VK_LCONTROL,
        VK_RCONTROL,
        VK_LMENU,
        VK_RMENU,
        VK_LWIN,
        VK_RWIN,
    ] {
        if held(key) {
            state[key as usize] |= DOWN;
        }
    }
    for (aggregate, sides) in [
        (VK_SHIFT, [VK_LSHIFT, VK_RSHIFT]),
        (VK_CONTROL, [VK_LCONTROL, VK_RCONTROL]),
        (VK_MENU, [VK_LMENU, VK_RMENU]),
    ] {
        if sides
            .into_iter()
            .any(|side| state[side as usize] & DOWN != 0)
        {
            state[aggregate as usize] |= DOWN;
        }
    }
    // A virtual key is not bounded by the array, so this is the one index that
    // has to be checked rather than known.
    if let Some(byte) = state.get_mut(vkey as usize) {
        *byte |= DOWN;
    }
}

/// The engine modifiers one keyboard-state array means.
///
/// Read from the same array `ToUnicodeEx` was handed, never from a second
/// source — that pairing is this module's whole contract. Caps lock is
/// deliberately absent, exactly as it is on macOS: the layout has already
/// applied it to the character, which is the only form the engine wants it in.
pub fn state_modifiers(state: &[u8; 256]) -> Modifiers {
    let down = |key: u32| state[key as usize] & DOWN != 0;
    Modifiers {
        shift: down(VK_SHIFT) || down(VK_LSHIFT) || down(VK_RSHIFT),
        control: down(VK_CONTROL) || down(VK_LCONTROL) || down(VK_RCONTROL),
        alt: down(VK_MENU) || down(VK_LMENU) || down(VK_RMENU),
        meta: down(VK_LWIN) || down(VK_RWIN),
    }
}

/// Converts exactly one non-control Unicode scalar from `ToUnicodeEx`.
pub fn one_character(units: &[u16]) -> Option<char> {
    let text = std::char::decode_utf16(units.iter().copied())
        .collect::<Result<String, _>>()
        .ok()?;
    let mut characters = text.chars();
    let character = characters.next()?;
    (characters.next().is_none() && !character.is_control()).then_some(character)
}

/// The normalized event for one non-repeated Windows key-down.
pub fn key_event(vkey: u32, text: Option<char>, modifiers: Modifiers) -> KeyEvent {
    let identity = match vkey {
        VK_BACK => Some(Key::Backspace),
        VK_TAB => Some(Key::Tab),
        VK_RETURN => Some(Key::Enter),
        VK_ESCAPE => Some(Key::Escape),
        VK_SPACE => Some(Key::Space),
        VK_PRIOR => Some(Key::PageUp),
        VK_NEXT => Some(Key::PageDown),
        VK_END => Some(Key::End),
        VK_HOME => Some(Key::Home),
        VK_LEFT => Some(Key::ArrowLeft),
        VK_UP => Some(Key::ArrowUp),
        VK_RIGHT => Some(Key::ArrowRight),
        VK_DOWN => Some(Key::ArrowDown),
        VK_DELETE => Some(Key::Delete),
        VK_SHIFT | VK_CONTROL | VK_MENU | VK_LWIN | VK_RWIN | VK_LSHIFT | VK_RSHIFT
        | VK_LCONTROL | VK_RCONTROL | VK_LMENU | VK_RMENU => Some(Key::Modifier),
        _ => None,
    };
    let key = identity.unwrap_or_else(|| text.map_or(Key::Other, |_| Key::Character));

    KeyEvent {
        key,
        // Space is both a word boundary and text. Other identities must never
        // turn an accompanying control code into composition input.
        text: if key == Key::Space {
            Some(' ')
        } else if key == Key::Character {
            text
        } else {
            None
        },
        modifiers,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        VK_BACK, VK_CONTROL, VK_LCONTROL, VK_LEFT, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_MENU, VK_RMENU,
        VK_RSHIFT, VK_SHIFT, VK_SPACE, key_event, merge_physical, one_character, state_modifiers,
    };
    use dodo_ime_core::{Key, Modifiers};
    use dodo_ime_ipc::settings::{Shortcut, ShortcutKey, ShortcutModifiers};

    /// The Windows key is `meta` — the same field macOS fills from Command —
    /// and Alt is `alt`, the same field it fills from Option. One recorded
    /// shortcut therefore means one hand shape on both platforms.
    #[test]
    fn the_windows_key_is_meta_and_alt_is_alt() {
        let space = |modifiers| Shortcut {
            modifiers,
            key: ShortcutKey::Space,
        };
        let meta = space(ShortcutModifiers {
            meta: true,
            ..ShortcutModifiers::NONE
        });
        let alt = space(ShortcutModifiers {
            alt: true,
            ..ShortcutModifiers::NONE
        });
        let with_windows = key_event(
            VK_SPACE,
            Some(' '),
            Modifiers {
                meta: true,
                ..Modifiers::NONE
            },
        );
        let with_alt = key_event(
            VK_SPACE,
            Some(' '),
            Modifiers {
                alt: true,
                ..Modifiers::NONE
            },
        );
        assert!(meta.matches(&with_windows));
        assert!(!meta.matches(&with_alt));
        assert!(alt.matches(&with_alt));
        assert!(!alt.matches(&with_windows));

        // Every key that can hold a shortcut has the identity this table gives
        // it, including the bare modifiers a modifier-only shortcut needs.
        for vkey in [VK_CONTROL, VK_MENU, VK_LWIN] {
            assert_eq!(key_event(vkey, None, Modifiers::NONE).key, Key::Modifier);
        }
        assert!(
            Shortcut {
                modifiers: ShortcutModifiers {
                    control: true,
                    shift: true,
                    ..ShortcutModifiers::NONE
                },
                key: ShortcutKey::Modifiers,
            }
            .matches(&key_event(
                VK_CONTROL,
                None,
                Modifiers {
                    control: true,
                    shift: true,
                    ..Modifiers::NONE
                }
            ))
        );
    }

    #[test]
    fn identity_and_layout_text_keep_their_separate_jobs() {
        let letter = key_event(0x57, Some('w'), Modifiers::NONE);
        assert_eq!(letter.key, Key::Character);
        assert_eq!(letter.typed(), Some('w'));

        let backspace = key_event(VK_BACK, Some('\u{8}'), Modifiers::NONE);
        assert_eq!(backspace.key, Key::Backspace);
        assert_eq!(backspace.typed(), None);

        let space = key_event(VK_SPACE, Some(' '), Modifiers::NONE);
        assert_eq!(space.key, Key::Space);
        assert_eq!(space.typed(), Some(' '));

        assert_eq!(
            key_event(VK_LEFT, None, Modifiers::NONE).key,
            Key::ArrowLeft
        );
        assert_eq!(
            key_event(VK_CONTROL, None, Modifiers::NONE).key,
            Key::Modifier
        );
    }

    /// The failure this pairing exists to prevent: a snapshot that says nothing
    /// is held while the user is holding Shift, which makes `ToUnicodeEx` type
    /// a lowercase letter and leaves every shortcut unmatchable.
    #[test]
    fn a_held_shift_reaches_both_the_layout_state_and_the_modifiers() {
        let mut empty = [0_u8; 256];
        assert_eq!(state_modifiers(&empty), Modifiers::NONE);

        merge_physical(&mut empty, 0x44, |key| key == VK_LSHIFT);
        assert_ne!(empty[VK_LSHIFT as usize] & 0x80, 0);
        assert_ne!(
            empty[VK_SHIFT as usize] & 0x80,
            0,
            "ToUnicodeEx reads the aggregate"
        );
        assert_eq!(empty[VK_RSHIFT as usize] & 0x80, 0);
        assert_ne!(empty[0x44] & 0x80, 0, "the arriving key is down");

        let modifiers = state_modifiers(&empty);
        assert_eq!(modifiers, Modifiers::SHIFT);
        assert_eq!(key_event(0x44, Some('D'), modifiers).typed(), Some('D'));
    }

    /// A snapshot that was already right is left alone, and each side of a
    /// modifier keeps its own byte — AltGr is right-Alt with left-Control and
    /// cannot be produced from the aggregates.
    #[test]
    fn merging_only_adds_and_keeps_the_two_sides_apart() {
        let mut correct = [0_u8; 256];
        correct[VK_RSHIFT as usize] = 0x80;
        correct[VK_SHIFT as usize] = 0x80;
        let before = correct;
        merge_physical(&mut correct, 0x41, |_| false);
        assert_eq!(correct[VK_RSHIFT as usize], before[VK_RSHIFT as usize]);
        assert_eq!(correct[VK_LSHIFT as usize], 0);
        assert_eq!(state_modifiers(&correct), Modifiers::SHIFT);

        let mut alt_gr = [0_u8; 256];
        merge_physical(&mut alt_gr, 0x45, |key| {
            key == VK_RMENU || key == VK_LCONTROL
        });
        assert_ne!(alt_gr[VK_RMENU as usize] & 0x80, 0);
        assert_eq!(alt_gr[VK_LMENU as usize] & 0x80, 0);
        assert_ne!(alt_gr[VK_MENU as usize] & 0x80, 0);
        assert_ne!(alt_gr[VK_CONTROL as usize] & 0x80, 0);

        let modifiers = state_modifiers(&alt_gr);
        assert!(modifiers.alt && modifiers.control);
        assert!(!modifiers.is_plain());
    }

    /// A modifier-only shortcut fires on the modifier's own key-down, which is
    /// the one press the queue may not carry yet.
    #[test]
    fn the_arriving_modifier_completes_its_own_shortcut() {
        let mut state = [0_u8; 256];
        merge_physical(&mut state, VK_RSHIFT, |key| key == VK_LCONTROL);
        let modifiers = state_modifiers(&state);
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
            .matches(&key_event(VK_RSHIFT, None, modifiers))
        );
    }

    #[test]
    fn shortcuts_keep_their_modifier_and_uncertain_text_is_rejected() {
        let shortcut = key_event(
            0x53,
            Some('s'),
            Modifiers {
                control: true,
                ..Modifiers::NONE
            },
        );
        assert!(shortcut.modifiers.control);
        assert_eq!(shortcut.typed(), None);
        assert_eq!(one_character(&[b'a' as u16]), Some('a'));
        assert_eq!(one_character(&[b'a' as u16, b'b' as u16]), None);
        assert_eq!(one_character(&[0xd800]), None);
    }
}
