//! VNI: the diacritics are spelled with digits.
//!
//! | key | meaning |
//! |---|---|
//! | `1` `2` `3` `4` `5` | sắc, huyền, hỏi, ngã, nặng |
//! | `0` | remove the tone |
//! | `6` | circumflex — `â` `ê` `ô` |
//! | `7` | horn — `ơ` `ư` |
//! | `8` | breve — `ă` |
//! | `9` | stroke — `đ` |
//!
//! # The same transforms Telex produces, from a different keyboard
//!
//! This file is the second half of the answer to "Telex and VNI share one
//! semantic engine". It maps ten digits onto exactly the [`Transform`] values
//! [`super::telex`] maps letters onto, and then stops. There is no syllable
//! rule here, no tone placement, no `uo` special case, no undo logic — all of
//! that is in [`super::syllable`] and runs identically whichever scheme
//! produced the transform. `tie6ng1` and `tieengs` reach `tiếng` down the same
//! path.
//!
//! # Digits only count when they have somewhere to go
//!
//! Unlike Telex's letters, VNI's keys are characters people also type as
//! themselves. `7` is a horn when there is an `o` or a `u` to put it on, and
//! otherwise it is the number seven — so `iphone 7` types the seven, and `a1b2`
//! does not turn into an accented mess. That check is here rather than in the
//! engine because it is scheme-specific: Telex's `s` is a tone key whether or
//! not it can land, because `s` is never a digit somebody meant literally.
//!
//! The tone digits are the exception: `1`–`5` are reported as tone keys
//! whenever any vowel has been typed, and the engine's own "does this look like
//! Vietnamese" test decides whether the tone may actually land. That keeps the
//! two schemes agreeing about `cas`/`ca1` rather than each having a private
//! opinion.

use super::syllable::{Mark, Syllable, Tone};
use super::{Transform, rules};

/// What the next key means in VNI, or `None` when it is not a VNI key here and
/// therefore ends the syllable.
pub fn interpret(key: char, syllable: &Syllable) -> Option<Transform> {
    if key.is_ascii_alphabetic() {
        return Some(Transform::Letter {
            base: key.to_ascii_lowercase(),
            mark: None,
            upper: key.is_ascii_uppercase(),
        });
    }

    let has_vowel = syllable
        .letters()
        .iter()
        .any(|letter| rules::is_vowel_base(letter.base));

    match key {
        '1' | '2' | '3' | '4' | '5' if has_vowel => Some(Transform::Tone {
            tone: match key {
                '1' => Tone::Acute,
                '2' => Tone::Grave,
                '3' => Tone::HookAbove,
                '4' => Tone::Tilde,
                _ => Tone::UnderDot,
            },
            literal: key,
        }),
        '0' if has_vowel => Some(Transform::ClearTone { literal: key }),
        '6' => mark_if_it_lands(syllable, Mark::Circumflex, key),
        '7' => mark_if_it_lands(syllable, Mark::Horn, key),
        '8' => mark_if_it_lands(syllable, Mark::Breve, key),
        '9' => mark_if_it_lands(syllable, Mark::Stroke, key),
        _ => None,
    }
}

/// A digit is a diacritic key only when some letter could take that diacritic;
/// otherwise it is the digit the user typed.
fn mark_if_it_lands(syllable: &Syllable, mark: Mark, literal: char) -> Option<Transform> {
    syllable
        .mark_target(mark)
        .map(|_| Transform::Mark { mark, literal })
}

#[cfg(test)]
mod tests {
    use super::interpret;
    use crate::languages::vietnamese::Transform;
    use crate::languages::vietnamese::syllable::{Mark, Syllable, Tone};

    fn syllable(spelling: &str) -> Syllable {
        let mut syllable = Syllable::new();
        for ch in spelling.chars() {
            syllable.push_letter(ch, ch.is_uppercase());
        }
        syllable
    }

    fn read(key: char, spelling: &str) -> Option<Transform> {
        interpret(key, &syllable(spelling))
    }

    #[test]
    fn the_five_tone_digits_and_the_undo_digit() {
        for (key, tone) in [
            ('1', Tone::Acute),
            ('2', Tone::Grave),
            ('3', Tone::HookAbove),
            ('4', Tone::Tilde),
            ('5', Tone::UnderDot),
        ] {
            assert_eq!(
                read(key, "ta"),
                Some(Transform::Tone { tone, literal: key }),
                "{key}"
            );
        }
        assert_eq!(read('0', "ta"), Some(Transform::ClearTone { literal: '0' }));
    }

    #[test]
    fn the_four_diacritic_digits_find_their_letters() {
        assert_eq!(
            read('6', "tie"),
            Some(Transform::Mark {
                mark: Mark::Circumflex,
                literal: '6'
            })
        );
        assert_eq!(
            read('7', "du"),
            Some(Transform::Mark {
                mark: Mark::Horn,
                literal: '7'
            })
        );
        assert_eq!(
            read('8', "da"),
            Some(Transform::Mark {
                mark: Mark::Breve,
                literal: '8'
            })
        );
        assert_eq!(
            read('9', "d"),
            Some(Transform::Mark {
                mark: Mark::Stroke,
                literal: '9'
            })
        );
    }

    /// A digit has no case, so an uppercase syllable reads exactly the same
    /// way — which is why VNI cannot leak a modifier's case onto a letter even
    /// in principle.
    #[test]
    fn a_diacritic_digit_reads_the_same_over_uppercase_letters() {
        for (key, mark, spelling) in [
            ('6', Mark::Circumflex, "TIE"),
            ('7', Mark::Horn, "DU"),
            ('8', Mark::Breve, "DA"),
            ('9', Mark::Stroke, "D"),
        ] {
            assert_eq!(
                read(key, spelling),
                Some(Transform::Mark { mark, literal: key }),
                "{spelling}{key}"
            );
        }
    }

    /// `iphone 7` types a seven.
    #[test]
    fn a_digit_with_nowhere_to_land_is_a_digit() {
        assert_eq!(read('6', "ti"), None);
        assert_eq!(read('7', "ta"), None);
        assert_eq!(read('8', "to"), None);
        assert_eq!(read('9', "ta"), None);
        assert_eq!(read('9', ""), None);
        // No vowel typed yet, so no tone key either.
        assert_eq!(read('1', "ngh"), None);
        assert_eq!(read('0', ""), None);
    }

    #[test]
    fn letters_are_always_just_letters_in_vni() {
        for key in ['a', 'w', 's', 'd', 'z'] {
            assert_eq!(
                read(key, "d"),
                Some(Transform::Letter {
                    base: key,
                    mark: None,
                    upper: false
                }),
                "{key}"
            );
        }
        assert_eq!(
            read('D', ""),
            Some(Transform::Letter {
                base: 'd',
                mark: None,
                upper: true
            })
        );
    }

    #[test]
    fn punctuation_is_not_a_vni_key() {
        for key in [' ', '.', ',', '-', '[', '\u{e9}'] {
            assert_eq!(read(key, "ta"), None, "{key:?}");
        }
    }

    /// The digits are reported as diacritics whether or not the letter already
    /// has one — undo is the shared layer's job, not this file's.
    #[test]
    fn a_digit_over_an_existing_mark_is_still_a_diacritic_key() {
        let mut syllable = syllable("tie");
        syllable.apply_mark(Mark::Circumflex);
        assert_eq!(
            interpret('6', &syllable),
            Some(Transform::Mark {
                mark: Mark::Circumflex,
                literal: '6'
            })
        );
    }
}
