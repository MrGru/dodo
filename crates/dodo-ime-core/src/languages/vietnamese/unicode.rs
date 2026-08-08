//! The output boundary: semantic state in, NFC text out.
//!
//! **This is the only file in the Vietnamese engine that knows what a
//! diacritic looks like.** Everywhere else a letter is
//! `(base, mark, tone, case)` — `ế` is *e, circumflex, acute, lower*, never a
//! character that got there by rewriting `e` into `ê` into `ế`. Keeping the
//! rewrite chain out of the state machine is what makes the tone move on its
//! own when a final consonant arrives, and what makes `ss` mean "undo the
//! acute" rather than "search the string for something that looks accented".
//!
//! # How composition actually works here
//!
//! [`render_letter`] builds the base-plus-mark character from a table
//! ([`base_with_mark`]) and then *appends the tone as a combining mark*, and
//! lets NFC do the rest. That is not laziness: canonical ordering is the part
//! that is easy to get wrong by hand. `â` + U+0323 (dot below) has to become
//! `ậ` U+1EAD, and it only does because NFD reorders the dot below (combining
//! class 220) ahead of the circumflex (230) before recomposing. A hand-written
//! 12 × 5 table of precomposed characters would have to encode that reordering
//! correctly sixty times; `unicode-normalization` already does, and it is
//! already a dependency.
//!
//! Every Vietnamese letter does have a precomposed NFC form, so each call here
//! returns exactly one `char` in practice — but nothing downstream assumes it.
//! Lengths are counted in graphemes
//! ([`grapheme_count`](crate::core::grapheme_count)), so a
//! hypothetical combination with no precomposed form would still measure as one
//! visible character.

use unicode_normalization::UnicodeNormalization;

use super::syllable::{Mark, Tone};

/// The base letter carrying `mark`, or `None` when Vietnamese has no such
/// letter.
///
/// The whole table. Twelve vowels and one consonant is the entirety of the
/// Vietnamese alphabet's diacritic inventory, tones excluded — `ư` and `ơ` take
/// a horn, `ă` a breve, `â`/`ê`/`ô` a circumflex, `đ` a stroke, and `i` and `y`
/// take nothing at all.
///
/// `None` is a real answer and callers must honour it: a horn on `e` is not a
/// letter, so a key that would produce one has to fall through as typed rather
/// than invent something.
pub fn base_with_mark(base: char, mark: Option<Mark>) -> Option<char> {
    match (base, mark) {
        (base, None) => Some(base),
        ('a', Some(Mark::Circumflex)) => Some('â'),
        ('a', Some(Mark::Breve)) => Some('ă'),
        ('e', Some(Mark::Circumflex)) => Some('ê'),
        ('o', Some(Mark::Circumflex)) => Some('ô'),
        ('o', Some(Mark::Horn)) => Some('ơ'),
        ('u', Some(Mark::Horn)) => Some('ư'),
        ('d', Some(Mark::Stroke)) => Some('đ'),
        _ => None,
    }
}

/// Whether `base` is a letter that can carry `mark`.
pub fn can_take(base: char, mark: Mark) -> bool {
    base_with_mark(base, Some(mark)).is_some()
}

/// The combining character for a tone, or `None` for the level tone (`ngang`),
/// which is written by writing nothing.
pub fn combining(tone: Tone) -> Option<char> {
    match tone {
        Tone::Level => None,
        Tone::Acute => Some('\u{0301}'),
        Tone::Grave => Some('\u{0300}'),
        Tone::HookAbove => Some('\u{0309}'),
        Tone::Tilde => Some('\u{0303}'),
        Tone::UnderDot => Some('\u{0323}'),
    }
}

/// One letter as the user should see it.
///
/// **Fails safe.** A mark the base cannot carry falls back to the bare base
/// rather than panicking or returning nothing: the letter the user typed still
/// reaches the screen, minus a diacritic that was never a letter anyway. The
/// callers in [`super::syllable`] refuse such a combination before it gets
/// here, so this branch should be unreachable — but "should be unreachable"
/// inside a key handler is exactly where a panic would cost the user their
/// whole document.
pub fn render_letter(base: char, mark: Option<Mark>, upper: bool, tone: Tone) -> String {
    let mut text = String::with_capacity(4);
    text.push(base_with_mark(base, mark).unwrap_or(base));
    if let Some(combining) = combining(tone) {
        text.push(combining);
    }
    let text = if upper { text.to_uppercase() } else { text };
    nfc(&text)
}

/// `text` in Normalization Form C.
///
/// Everything this engine hands a host goes through here. An application that
/// receives NFD Vietnamese will render it correctly and then compare, sort and
/// search it wrongly, which is a much worse bug than a visibly broken glyph
/// because nobody sees it happen.
pub fn nfc(text: &str) -> String {
    text.nfc().collect()
}

#[cfg(test)]
mod tests {
    use super::{base_with_mark, can_take, combining, nfc, render_letter};
    use crate::core::grapheme_count;
    use crate::languages::vietnamese::syllable::{Mark, Tone};

    const TONES: [Tone; 6] = [
        Tone::Level,
        Tone::Acute,
        Tone::Grave,
        Tone::HookAbove,
        Tone::Tilde,
        Tone::UnderDot,
    ];

    #[test]
    fn the_alphabet_table_is_the_whole_alphabet() {
        let cases = [
            ('a', Mark::Circumflex, Some('â')),
            ('a', Mark::Breve, Some('ă')),
            ('a', Mark::Horn, None),
            ('e', Mark::Circumflex, Some('ê')),
            ('e', Mark::Horn, None),
            ('e', Mark::Breve, None),
            ('o', Mark::Circumflex, Some('ô')),
            ('o', Mark::Horn, Some('ơ')),
            ('u', Mark::Horn, Some('ư')),
            ('u', Mark::Circumflex, None),
            ('i', Mark::Circumflex, None),
            ('y', Mark::Circumflex, None),
            ('d', Mark::Stroke, Some('đ')),
            ('a', Mark::Stroke, None),
        ];
        for (base, mark, expected) in cases {
            assert_eq!(
                base_with_mark(base, Some(mark)),
                expected,
                "{base} + {mark:?}"
            );
            assert_eq!(can_take(base, mark), expected.is_some());
        }
        // No mark is always the letter itself, for any letter at all.
        for base in 'a'..='z' {
            assert_eq!(base_with_mark(base, None), Some(base));
        }
    }

    /// The reason tones are appended as combining marks instead of looked up:
    /// canonical reordering. A dot below sorts *before* a circumflex, so `â`
    /// plus a dot below has to come out as `ậ`, not as two characters in the
    /// order they were written.
    #[test]
    fn nfc_reorders_a_below_tone_under_an_above_mark() {
        assert_eq!(
            render_letter('a', Some(Mark::Circumflex), false, Tone::UnderDot),
            "ậ"
        );
        assert_eq!(
            render_letter('a', Some(Mark::Breve), false, Tone::UnderDot),
            "ặ"
        );
        assert_eq!(
            render_letter('e', Some(Mark::Circumflex), false, Tone::UnderDot),
            "ệ"
        );
        assert_eq!(
            render_letter('o', Some(Mark::Circumflex), false, Tone::UnderDot),
            "ộ"
        );
        assert_eq!(
            render_letter('o', Some(Mark::Horn), false, Tone::UnderDot),
            "ợ"
        );
        assert_eq!(
            render_letter('u', Some(Mark::Horn), false, Tone::UnderDot),
            "ự"
        );
    }

    #[test]
    fn the_worked_letters_render_as_written() {
        assert_eq!(
            render_letter('e', Some(Mark::Circumflex), false, Tone::Acute),
            "ế"
        );
        assert_eq!(
            render_letter('o', Some(Mark::Horn), false, Tone::Grave),
            "ờ"
        );
        assert_eq!(
            render_letter('a', Some(Mark::Breve), false, Tone::Tilde),
            "ẵ"
        );
        assert_eq!(render_letter('y', None, false, Tone::HookAbove), "ỷ");
        assert_eq!(
            render_letter('d', Some(Mark::Stroke), false, Tone::Level),
            "đ"
        );
        assert_eq!(render_letter('a', None, false, Tone::Level), "a");
    }

    #[test]
    fn uppercase_keeps_every_diacritic() {
        assert_eq!(
            render_letter('e', Some(Mark::Circumflex), true, Tone::UnderDot),
            "Ệ"
        );
        assert_eq!(
            render_letter('d', Some(Mark::Stroke), true, Tone::Level),
            "Đ"
        );
        assert_eq!(render_letter('u', Some(Mark::Horn), true, Tone::Grave), "Ừ");
        assert_eq!(
            render_letter('a', Some(Mark::Breve), true, Tone::Acute),
            "Ắ"
        );
    }

    /// The whole Vietnamese inventory, mechanically: every base-and-mark that
    /// is a letter, crossed with every tone, must come out as exactly one
    /// visible character in NFC, in both cases.
    #[test]
    fn every_letter_and_tone_is_one_nfc_grapheme() {
        let bases = [
            ('a', None),
            ('a', Some(Mark::Circumflex)),
            ('a', Some(Mark::Breve)),
            ('e', None),
            ('e', Some(Mark::Circumflex)),
            ('i', None),
            ('o', None),
            ('o', Some(Mark::Circumflex)),
            ('o', Some(Mark::Horn)),
            ('u', None),
            ('u', Some(Mark::Horn)),
            ('y', None),
        ];
        for (base, mark) in bases {
            for tone in TONES {
                for upper in [false, true] {
                    let rendered = render_letter(base, mark, upper, tone);
                    assert_eq!(
                        grapheme_count(&rendered),
                        1,
                        "{base:?}/{mark:?}/{tone:?}/{upper} rendered {rendered:?}"
                    );
                    assert_eq!(nfc(&rendered), rendered, "{rendered:?} is not NFC");
                    // Vietnamese has a precomposed form for all sixty of these.
                    assert_eq!(rendered.chars().count(), 1, "{rendered:?}");
                }
            }
        }
    }

    /// The unreachable branch, exercised on purpose: a horn on `e` is not a
    /// letter, and the fallback must still give the user their `e` back.
    #[test]
    fn an_impossible_mark_gives_the_bare_letter_rather_than_nothing() {
        assert_eq!(
            render_letter('e', Some(Mark::Horn), false, Tone::Acute),
            "é"
        );
        assert_eq!(
            render_letter('i', Some(Mark::Breve), false, Tone::Level),
            "i"
        );
        assert_eq!(
            render_letter('q', Some(Mark::Stroke), false, Tone::Level),
            "q"
        );
    }

    #[test]
    fn the_level_tone_is_written_by_writing_nothing() {
        assert_eq!(combining(Tone::Level), None);
        for tone in TONES.iter().skip(1) {
            assert!(combining(*tone).is_some(), "{tone:?}");
        }
    }

    #[test]
    fn nfc_composes_decomposed_input() {
        assert_eq!(nfc("e\u{0302}\u{0301}"), "ế");
        assert_eq!(nfc("tie\u{0302}\u{0301}ng"), "tiếng");
        assert_eq!(nfc("tiếng"), "tiếng");
    }
}
