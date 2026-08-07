//! One keystroke, with every platform's spelling of it already erased.
//!
//! This is the type each future OS host normalizes *into*: an `NSEvent` on
//! macOS, a `WM_KEYDOWN` plus its `ToUnicode` result on Windows, an IBus keysym
//! on Linux. None of those names appears here or ever will — a [`KeyEvent`] is
//! three fields of plain data, so an engine can be driven from a unit test as
//! easily as from a window server.
//!
//! # Why identity *and* character, rather than one or the other
//!
//! An input method needs both readings of the same press, and they answer
//! different questions:
//!
//! - [`KeyEvent::key`] is what the key **is** — `Backspace`, `Escape`, the
//!   right arrow. Composition editing, candidate navigation and word boundaries
//!   are all decided from this, and none of them cares what the key would type.
//! - [`KeyEvent::text`] is what the key would **type** under the user's own
//!   keyboard layout, after the host has applied it. Telex reads `w`; VNI reads
//!   `7`; a Dvorak user's physical key positions are irrelevant to both. Asking
//!   for a scan code here would silently break every non-QWERTY layout.
//!
//! `Space` has both: it is a distinct [`Key`] because CJK conversion is
//! triggered by it, and it carries `Some(' ')` because it also types a space.

/// What a key is, independent of what it types.
///
/// Deliberately short. It covers the keys an input method must recognise by
/// identity — composition editing, candidate navigation, conversion control —
/// and folds everything else into [`Key::Other`], which every engine treats as
/// a word boundary. A host that cannot classify a key should send
/// [`Key::Other`] rather than guessing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Key {
    /// An ordinary printing key. [`KeyEvent::text`] says which character.
    Character,
    /// The space bar. Types `' '`, and triggers conversion in the CJK engines.
    Space,
    Backspace,
    Delete,
    Enter,
    Tab,
    Escape,
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    ArrowDown,
    Home,
    End,
    PageUp,
    PageDown,
    /// A function key, a bare modifier, a media key — anything an engine has no
    /// reading for.
    Other,
}

/// The modifier keys held down with a press.
///
/// `caps_lock` is absent on purpose: the host has already applied it, so it
/// shows up in [`KeyEvent::text`] as an uppercase character, which is exactly
/// what an engine needs to know.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Modifiers {
    pub shift: bool,
    pub control: bool,
    pub alt: bool,
    /// Command on macOS, Windows key on Windows, Super on Linux.
    pub meta: bool,
}

impl Modifiers {
    pub const NONE: Modifiers = Modifiers {
        shift: false,
        control: false,
        alt: false,
        meta: false,
    };

    pub const SHIFT: Modifiers = Modifiers {
        shift: true,
        control: false,
        alt: false,
        meta: false,
    };

    /// True when nothing is held that turns the press into a *command* rather
    /// than typing.
    ///
    /// Shift does not count: it changes which character is typed, which the
    /// host has already resolved into [`KeyEvent::text`]. Control, Alt and Meta
    /// do — `Cmd+S` is not a letter, and an engine that consumed it would eat
    /// the user's save.
    pub fn is_plain(self) -> bool {
        !(self.control || self.alt || self.meta)
    }
}

/// One key press, normalized.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct KeyEvent {
    pub key: Key,
    /// The character this press would type, or `None` for a key that types
    /// nothing.
    pub text: Option<char>,
    pub modifiers: Modifiers,
}

impl KeyEvent {
    /// A plain press of a printing key.
    ///
    /// Classifies the three characters that are also identities — space, tab
    /// and newline — so a caller driving the engine from a string does not have
    /// to. Shift is inferred from the character's own case, which is all any
    /// engine here reads it for.
    pub fn character(text: char) -> KeyEvent {
        let key = match text {
            ' ' => Key::Space,
            '\t' => Key::Tab,
            '\n' | '\r' => Key::Enter,
            _ => Key::Character,
        };
        KeyEvent {
            key,
            text: Some(text),
            modifiers: Modifiers {
                shift: text.is_uppercase(),
                ..Modifiers::NONE
            },
        }
    }

    /// A plain press of a key that types nothing.
    pub fn special(key: Key) -> KeyEvent {
        KeyEvent {
            key,
            text: None,
            modifiers: Modifiers::NONE,
        }
    }

    pub fn with_modifiers(mut self, modifiers: Modifiers) -> KeyEvent {
        self.modifiers = modifiers;
        self
    }

    /// The character this press types, but only when no command modifier is
    /// held.
    ///
    /// `Cmd+A` types nothing an engine may consume, even though the host will
    /// happily report `'a'` for it.
    pub fn typed(&self) -> Option<char> {
        self.modifiers.is_plain().then_some(self.text).flatten()
    }

    /// `0..=9` for a plain digit press.
    ///
    /// The CJK engines select a candidate by number; VNI reads digits as tone
    /// and diacritic keys. Both go through here so neither has to re-derive
    /// "is this a digit, and is it *really* being typed".
    pub fn digit(&self) -> Option<u32> {
        self.typed()?.to_digit(10)
    }
}

#[cfg(test)]
mod tests {
    use super::{Key, KeyEvent, Modifiers};

    #[test]
    fn character_classifies_the_three_keys_that_are_also_identities() {
        assert_eq!(KeyEvent::character('a').key, Key::Character);
        assert_eq!(KeyEvent::character(' ').key, Key::Space);
        assert_eq!(KeyEvent::character('\t').key, Key::Tab);
        assert_eq!(KeyEvent::character('\n').key, Key::Enter);
        assert_eq!(KeyEvent::character('\r').key, Key::Enter);
        // Space still types a space; it is both things at once.
        assert_eq!(KeyEvent::character(' ').text, Some(' '));
    }

    #[test]
    fn shift_is_inferred_from_the_character_not_asked_for() {
        assert!(KeyEvent::character('A').modifiers.shift);
        assert!(!KeyEvent::character('a').modifiers.shift);
    }

    /// The whole reason `typed` exists rather than reading `text` directly: a
    /// command shortcut must never look like typing.
    #[test]
    fn a_command_modifier_types_nothing() {
        let plain = KeyEvent::character('s');
        assert_eq!(plain.typed(), Some('s'));

        for modifiers in [
            Modifiers {
                control: true,
                ..Modifiers::NONE
            },
            Modifiers {
                alt: true,
                ..Modifiers::NONE
            },
            Modifiers {
                meta: true,
                ..Modifiers::NONE
            },
        ] {
            assert_eq!(plain.with_modifiers(modifiers).typed(), None);
        }

        // Shift is not a command modifier.
        assert_eq!(
            KeyEvent::character('S')
                .with_modifiers(Modifiers::SHIFT)
                .typed(),
            Some('S')
        );
    }

    #[test]
    fn digits_are_read_only_when_actually_typed() {
        assert_eq!(KeyEvent::character('7').digit(), Some(7));
        assert_eq!(KeyEvent::character('0').digit(), Some(0));
        assert_eq!(KeyEvent::character('a').digit(), None);
        assert_eq!(
            KeyEvent::character('7')
                .with_modifiers(Modifiers {
                    meta: true,
                    ..Modifiers::NONE
                })
                .digit(),
            None
        );
    }

    #[test]
    fn a_key_that_types_nothing_carries_no_text() {
        let event = KeyEvent::special(Key::Backspace);
        assert_eq!(event.text, None);
        assert_eq!(event.typed(), None);
        assert!(event.modifiers.is_plain());
    }
}
