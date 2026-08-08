//! What macOS says a key was, in the engine's vocabulary.
//!
//! `-[IMKInputController inputText:key:modifiers:client:]` hands over three
//! things: the characters the key would type under the user's own layout, the
//! hardware virtual key code, and the `NSEvent` modifier mask. [`key_event`]
//! turns those into a [`KeyEvent`], and it is the only place in this crate that
//! knows a number like `0x33` means Backspace.
//!
//! # The string is the character, the key code is the identity
//!
//! Both readings are needed and they answer different questions — the engine's
//! [`KeyEvent`] docs make the general argument. What is specific to macOS is
//! *which* of the two to trust:
//!
//! - **The string, for what was typed.** macOS has already applied the keyboard
//!   layout, the shift key and caps lock, so a Dvorak user's `w` arrives as
//!   `"w"` and a French user's `,` arrives as `","`. Deriving the character from
//!   the key code instead would re-implement the layout, badly, and break Telex
//!   for everyone not on US QWERTY.
//! - **The key code, for what the key is.** Backspace, Escape and the arrows
//!   type nothing, so the string is empty or a private-use control character and
//!   cannot tell them apart.
//!
//! # The modifier trap
//!
//! An arrow key arrives with `NSEventModifierFlagFunction` **and**
//! `NSEventModifierFlagNumericPad` set. Neither is a command modifier, and
//! folding either into [`Modifiers::alt`] or [`Modifiers::meta`] — which looks
//! reasonable, `Fn` being a key on the keyboard — would make every arrow press
//! read as a command shortcut. It happens to reach the same place today (the
//! engine ends the composition either way) and would be wrong the moment
//! anything distinguished them. Only the four documented command modifiers are
//! read; caps lock is deliberately dropped, because macOS has already applied it
//! to the string, which is the only form the engine wants it in.

use dodo_ime_core::{Key, KeyEvent, Modifiers};

/// `NSEventModifierFlagShift`.
const FLAG_SHIFT: u64 = 1 << 17;
/// `NSEventModifierFlagControl`.
const FLAG_CONTROL: u64 = 1 << 18;
/// `NSEventModifierFlagOption`.
const FLAG_OPTION: u64 = 1 << 19;
/// `NSEventModifierFlagCommand`.
const FLAG_COMMAND: u64 = 1 << 20;

// The virtual key codes from `Carbon/HIToolbox/Events.h`. Only the keys an
// input method must recognise *by identity* are here; everything else is a
// character and is read from the string.
const VK_RETURN: u16 = 0x24;
const VK_TAB: u16 = 0x30;
const VK_SPACE: u16 = 0x31;
const VK_DELETE: u16 = 0x33; // Backspace. Apple's name for it is misleading.
const VK_ESCAPE: u16 = 0x35;
const VK_KEYPAD_ENTER: u16 = 0x4C;
const VK_FORWARD_DELETE: u16 = 0x75; // The key labelled ⌦, and the real Delete.
const VK_HOME: u16 = 0x73;
const VK_PAGE_UP: u16 = 0x74;
const VK_END: u16 = 0x77;
const VK_PAGE_DOWN: u16 = 0x79;
const VK_LEFT_ARROW: u16 = 0x7B;
const VK_RIGHT_ARROW: u16 = 0x7C;
const VK_DOWN_ARROW: u16 = 0x7D;
const VK_UP_ARROW: u16 = 0x7E;

/// The four command modifiers, read out of an `NSEvent` modifier mask.
pub fn modifiers(flags: u64) -> Modifiers {
    Modifiers {
        shift: flags & FLAG_SHIFT != 0,
        control: flags & FLAG_CONTROL != 0,
        alt: flags & FLAG_OPTION != 0,
        meta: flags & FLAG_COMMAND != 0,
    }
}

/// The [`Key`] a virtual key code names, or `None` for a key whose identity is
/// simply "it types something".
fn identity(key_code: u16) -> Option<Key> {
    Some(match key_code {
        VK_RETURN | VK_KEYPAD_ENTER => Key::Enter,
        VK_TAB => Key::Tab,
        VK_SPACE => Key::Space,
        VK_DELETE => Key::Backspace,
        VK_ESCAPE => Key::Escape,
        VK_FORWARD_DELETE => Key::Delete,
        VK_HOME => Key::Home,
        VK_END => Key::End,
        VK_PAGE_UP => Key::PageUp,
        VK_PAGE_DOWN => Key::PageDown,
        VK_LEFT_ARROW => Key::ArrowLeft,
        VK_RIGHT_ARROW => Key::ArrowRight,
        VK_UP_ARROW => Key::ArrowUp,
        VK_DOWN_ARROW => Key::ArrowDown,
        _ => return None,
    })
}

/// The character `text` types, if it types exactly one.
///
/// An empty string is a key that types nothing. A string of two or more
/// characters is something this engine has no reading for — a dead-key sequence
/// resolving, an input source producing a ligature — and comes back `None`, so
/// the caller ends the composition and hands the event back to the application
/// intact rather than typing the first character and dropping the rest.
fn single_char(text: &str) -> Option<char> {
    let mut chars = text.chars();
    let first = chars.next()?;
    chars.next().is_none().then_some(first)
}

/// One `inputText:key:modifiers:client:` call, normalized.
///
/// `text` is the string macOS passed, `key_code` the hardware key, `flags` the
/// `NSEvent` modifier mask.
pub fn key_event(text: &str, key_code: u16, flags: u64) -> KeyEvent {
    let modifiers = modifiers(flags);
    let typed = single_char(text).filter(|character| !character.is_control());

    // The identity wins where there is one, because Backspace is not "whatever
    // U+0008 means to this application". Space is the exception in the other
    // direction: it is an identity *and* a character, and the engine reads both.
    let key = match identity(key_code) {
        Some(Key::Space) => Key::Space,
        Some(key) => {
            return KeyEvent {
                key,
                text: None,
                modifiers,
            };
        }
        None if typed.is_some() => Key::Character,
        // A key that types nothing and that this table does not name: a function
        // key, a media key, a bare modifier. `Key::Other` is a word boundary,
        // which is what the engine should do with F5.
        None => Key::Other,
    };

    KeyEvent {
        key,
        text: typed,
        modifiers,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        FLAG_COMMAND, FLAG_CONTROL, FLAG_OPTION, FLAG_SHIFT, key_event, modifiers, single_char,
    };
    use dodo_ime_core::{Key, Modifiers};

    /// `NSEventModifierFlagCapsLock` (1 << 16) and
    /// `NSEventModifierFlagFunction` (1 << 23), which arrive on real events and
    /// must not be read as anything.
    const FLAG_CAPS_LOCK: u64 = 1 << 16;
    const FLAG_NUMERIC_PAD: u64 = 1 << 21;
    const FLAG_FUNCTION: u64 = 1 << 23;

    #[test]
    fn the_four_command_modifiers_are_read_and_nothing_else_is() {
        assert_eq!(modifiers(0), Modifiers::NONE);
        assert_eq!(modifiers(FLAG_SHIFT), Modifiers::SHIFT);
        assert!(modifiers(FLAG_CONTROL).control);
        assert!(modifiers(FLAG_OPTION).alt);
        assert!(modifiers(FLAG_COMMAND).meta);

        // Caps lock is already in the string. Reading it here would make every
        // capital letter look like a held modifier.
        assert_eq!(modifiers(FLAG_CAPS_LOCK), Modifiers::NONE);
    }

    /// The trap this module exists to avoid: an arrow key is not a shortcut.
    #[test]
    fn an_arrow_keys_function_flags_are_not_command_modifiers() {
        let flags = FLAG_FUNCTION | FLAG_NUMERIC_PAD;
        let event = key_event("", 0x7B, flags);
        assert_eq!(event.key, Key::ArrowLeft);
        assert!(
            event.modifiers.is_plain(),
            "Fn and NumericPad must not read as a command modifier"
        );
    }

    #[test]
    fn a_letter_is_whatever_the_layout_typed() {
        let event = key_event("w", 0x0D, 0);
        assert_eq!(event.key, Key::Character);
        assert_eq!(event.text, Some('w'));

        // The same physical key on a layout that types something else. Nothing
        // here consults the key code to decide the character.
        let event = key_event("é", 0x0D, 0);
        assert_eq!(event.text, Some('é'));
    }

    #[test]
    fn shift_reaches_the_engine_as_both_the_flag_and_the_capital() {
        let event = key_event("S", 0x01, FLAG_SHIFT);
        assert_eq!(event.text, Some('S'));
        assert!(event.modifiers.shift);
        assert_eq!(event.typed(), Some('S'));
    }

    /// Every key the engine reads by identity, with the string macOS actually
    /// sends alongside it — which for most of them is a control character that
    /// must not become text.
    #[test]
    fn the_keys_with_an_identity_are_classified_by_key_code() {
        let cases: [(&str, u16, Key); 15] = [
            ("\r", 0x24, Key::Enter),
            ("\r", 0x4C, Key::Enter),
            ("\t", 0x30, Key::Tab),
            ("\u{8}", 0x33, Key::Backspace),
            ("\u{1b}", 0x35, Key::Escape),
            ("\u{7f}", 0x75, Key::Delete),
            ("", 0x73, Key::Home),
            ("", 0x77, Key::End),
            ("", 0x74, Key::PageUp),
            ("", 0x79, Key::PageDown),
            ("", 0x7B, Key::ArrowLeft),
            ("", 0x7C, Key::ArrowRight),
            ("", 0x7E, Key::ArrowUp),
            ("", 0x7D, Key::ArrowDown),
            ("", 0x3F, Key::Other),
        ];
        for (text, key_code, expected) in cases {
            let event = key_event(text, key_code, 0);
            assert_eq!(event.key, expected, "key code {key_code:#x}");
            assert_eq!(
                event.text, None,
                "key code {key_code:#x} must type nothing at all"
            );
        }
    }

    /// Space is the one key that is an identity and a character at once — the
    /// engine's word boundary reads the first, and a host performing a
    /// pass-through types the second.
    #[test]
    fn space_keeps_both_readings() {
        let event = key_event(" ", 0x31, 0);
        assert_eq!(event.key, Key::Space);
        assert_eq!(event.text, Some(' '));
        assert_eq!(event.typed(), Some(' '));
    }

    /// A command shortcut must reach the application. The engine decides that
    /// from `is_plain`, so all this has to do is not lose the flag.
    #[test]
    fn a_command_shortcut_types_nothing() {
        let event = key_event("s", 0x01, FLAG_COMMAND);
        assert_eq!(event.key, Key::Character);
        assert_eq!(event.typed(), None);
    }

    #[test]
    fn a_string_that_is_not_one_character_types_nothing() {
        assert_eq!(single_char(""), None);
        assert_eq!(single_char("a"), Some('a'));
        assert_eq!(single_char("ế"), Some('ế'));
        // A dead-key sequence resolving, or a ligature. Typing the first
        // character and dropping the rest would be worse than passing it back.
        assert_eq!(single_char("ffi"), None);

        let event = key_event("ffi", 0x03, 0);
        assert_eq!(event.key, Key::Other);
        assert_eq!(event.text, None);
    }

    /// A control character in the string is not text, whatever key produced it.
    /// Without this a key code the table does not name would turn `\u{8}` into
    /// a letter the engine tried to compose with.
    #[test]
    fn a_control_character_is_never_typed_text() {
        let event = key_event("\u{3}", 0x00, 0);
        assert_eq!(event.text, None);
        assert_eq!(event.key, Key::Other);
    }
}
