//! Where one syllable stops and the next thing begins.
//!
//! A Vietnamese syllable is built from letters and nothing else. Every other
//! key — a space, a comma, an arrow, Enter, a click somewhere else — ends it:
//! whatever has been composed is accepted as final text, and the key itself
//! goes on to the application untouched.
//!
//! # Why the rule is stated here rather than inlined
//!
//! It is asked in three different shapes and the three have to agree. The
//! engine asks *does this key end the syllable*; the input schemes ask *is this
//! one of my keys*; and the commit path asks *may this syllable stand as
//! typed*. A disagreement between the first two loses a keystroke — the engine
//! ends the syllable and the scheme also consumes the key — which is the one
//! failure this module refuses to have.
//!
//! # What is not here
//!
//! There is no notion of a *word*. Vietnamese writes each syllable separately,
//! so `tiếng Việt` is two compositions with a space between them and nothing
//! joins them. Multi-syllable handling would only be needed for abbreviation
//! expansion, which is a later round and will read committed text rather than
//! widening the composition.

use crate::input_method::core::{Key, KeyEvent};

/// Whether a key is one a syllable can be built from at all.
///
/// ASCII letters only. A diacritic that arrived from the *keyboard* rather than
/// from this engine — someone with a Vietnamese hardware layout typing `ê`
/// directly — is deliberately not one: it needs no engine, so it passes through
/// and the engine stays out of its way.
pub fn is_syllable_letter(key: char) -> bool {
    key.is_ascii_alphabetic()
}

/// Whether a key ends the composition purely by its identity, before anything
/// looks at what it types.
///
/// Backspace is the exception that makes this worth a function: it is not a
/// character, and it does not end the composition — it edits it. Everything
/// else that is not a printing key is a boundary, including the arrow keys,
/// because moving the caret away from the composition and then typing into it
/// would insert letters where the user is no longer looking.
pub fn breaks_composition(key: Key) -> bool {
    !matches!(key, Key::Character | Key::Space | Key::Backspace)
}

/// Whether this press should be handed to the application untouched, whatever
/// the engine is composing.
///
/// A command shortcut is never typing: `Cmd+S` must reach the application as
/// `Cmd+S`, with the composition committed first so the user's half-typed
/// syllable is not lost to the save dialog.
pub fn is_command(event: &KeyEvent) -> bool {
    !event.modifiers.is_plain()
}

#[cfg(test)]
mod tests {
    use super::{breaks_composition, is_command, is_syllable_letter};
    use crate::input_method::core::{Key, KeyEvent, Modifiers};

    #[test]
    fn only_ascii_letters_build_a_syllable() {
        for key in ['a', 'z', 'A', 'Z', 'w', 'd'] {
            assert!(is_syllable_letter(key), "{key}");
        }
        for key in [' ', '1', '.', '-', 'ê', 'đ', '\n'] {
            assert!(!is_syllable_letter(key), "{key:?}");
        }
    }

    /// Backspace edits the composition; everything else non-printing ends it.
    #[test]
    fn backspace_is_the_only_non_printing_key_that_does_not_break() {
        assert!(!breaks_composition(Key::Backspace));
        assert!(!breaks_composition(Key::Character));
        assert!(!breaks_composition(Key::Space));

        for key in [
            Key::Enter,
            Key::Tab,
            Key::Escape,
            Key::Delete,
            Key::ArrowLeft,
            Key::ArrowRight,
            Key::ArrowUp,
            Key::ArrowDown,
            Key::Home,
            Key::End,
            Key::PageUp,
            Key::PageDown,
            Key::Other,
        ] {
            assert!(breaks_composition(key), "{key:?}");
        }
    }

    #[test]
    fn a_command_shortcut_is_never_typing() {
        assert!(!is_command(&KeyEvent::character('s')));
        assert!(!is_command(&KeyEvent::character('S')));
        for modifiers in [
            Modifiers {
                meta: true,
                ..Modifiers::NONE
            },
            Modifiers {
                control: true,
                ..Modifiers::NONE
            },
            Modifiers {
                alt: true,
                ..Modifiers::NONE
            },
        ] {
            assert!(is_command(
                &KeyEvent::character('s').with_modifiers(modifiers)
            ));
        }
    }
}
