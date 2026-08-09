//! Windows virtual keys normalized into `dodo-ime-core` events.
//!
//! Windows supplies identity (`VK_*`) separately from the character resolved by
//! the user's keyboard layout. The host gives this module both. A dead key,
//! ligature, or failed layout conversion has no single-character reading and is
//! returned as `None` by the native adapter, so it passes through unchanged.

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
    use super::{VK_BACK, VK_LEFT, VK_SPACE, key_event, one_character};
    use dodo_ime_core::{Key, Modifiers};

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
