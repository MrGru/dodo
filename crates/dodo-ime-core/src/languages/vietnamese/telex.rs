//! Telex: the diacritics are spelled with letters.
//!
//! Telex has no keys of its own. It reuses `w` and the five letters Vietnamese
//! does not otherwise need — `f`, `j`, `s`, `x`, `z` — plus the trick of typing
//! a vowel twice, so that a Vietnamese typist never leaves the letter keys.
//!
//! | keys | meaning |
//! |---|---|
//! | `aa` `ee` `oo` | circumflex — `â` `ê` `ô` |
//! | `aw` `ow` `uw` | breve on `a`, horn on `o`/`u` — `ă` `ơ` `ư` |
//! | `dd` | stroke — `đ` |
//! | `s` `f` `r` `x` `j` | sắc, huyền, hỏi, ngã, nặng |
//! | `z` | remove the tone |
//! | `[` `]` | `ơ` and `ư` outright — see below |
//!
//! # This file decides *which key*, and nothing else
//!
//! Every entry above turns into a [`Transform`] — a [`Mark`], a [`Tone`], or a
//! plain letter — and every rule about what a mark or a tone then *does* lives
//! in [`super::syllable`] and [`super::tone`]. That is the whole separation
//! between Telex and VNI: two files this size, one shared state machine, and no
//! syllable logic duplicated between them. If a rule about Vietnamese ever
//! needs writing here, the boundary has been drawn in the wrong place.
//!
//! # Undo comes from the shared layer, not from here
//!
//! `aaa` types `aa`, and `ss` types `s`. Neither is special-cased below: the
//! key is reported as a mark or a tone every time, and
//! [`MarkOutcome::Reverted`](super::syllable::MarkOutcome::Reverted) — or a
//! tone that is already set — is what tells the engine to take the diacritic
//! off and type the key as itself. So the undo rule is stated once and both
//! schemes get it.
//!
//! # Three judgement calls
//!
//! - **`w` on its own types `ư`.** With no vowel to put a horn on, Unikey types
//!   `ư`, so `tw` and `tuw` both give `tư`. Kept, because it is what Vietnamese
//!   typists have in their fingers — and because `w` is not a Vietnamese letter,
//!   so nothing is lost.
//! - **`uo` + `w` marks both vowels.** `ươ` is the most common two-diacritic
//!   nucleus in the language, so `duowc` and `duwowc` both reach `được`. The
//!   cost is that `uơ` (`thuở`, `huơ`) cannot be typed with `w`, which is the
//!   next point.
//! - **`[` and `]` type `ơ` and `ư` outright.** Unikey's own shortcut, on by
//!   default there and here
//!   ([`bracket_shortcuts`](super::VietnameseConfig::bracket_shortcuts)). It is
//!   the only way to reach `uơ`: `thu[r` gives `thuở`. Turn it off and a
//!   bracket is an ordinary bracket.

use super::syllable::{Mark, Syllable, Tone};
use super::{Transform, rules};

/// What the next key means in Telex, or `None` when it is not a Telex key at
/// all and therefore ends the syllable.
pub fn interpret(key: char, syllable: &Syllable, bracket_shortcuts: bool) -> Option<Transform> {
    if bracket_shortcuts {
        match key {
            '[' => {
                return Some(Transform::Letter {
                    base: 'o',
                    mark: Some(Mark::Horn),
                    upper: false,
                });
            }
            ']' => {
                return Some(Transform::Letter {
                    base: 'u',
                    mark: Some(Mark::Horn),
                    upper: false,
                });
            }
            '{' => {
                return Some(Transform::Letter {
                    base: 'o',
                    mark: Some(Mark::Horn),
                    upper: true,
                });
            }
            '}' => {
                return Some(Transform::Letter {
                    base: 'u',
                    mark: Some(Mark::Horn),
                    upper: true,
                });
            }
            _ => {}
        }
    }

    if !key.is_ascii_alphabetic() {
        return None;
    }
    let upper = key.is_ascii_uppercase();
    let lower = key.to_ascii_lowercase();

    if let Some(tone) = tone_key(lower) {
        return Some(Transform::Tone { tone, literal: key });
    }
    if lower == 'z' {
        return Some(Transform::ClearTone { literal: key });
    }
    if lower == 'w' {
        return Some(horn_or_breve(syllable, key, upper));
    }
    if lower == 'd' && doubles_into_stroke(syllable) {
        return Some(Transform::Mark {
            mark: Mark::Stroke,
            literal: key,
        });
    }
    if matches!(lower, 'a' | 'e' | 'o') && repeats_last_vowel(syllable, lower) {
        return Some(Transform::Mark {
            mark: Mark::Circumflex,
            literal: key,
        });
    }

    Some(Transform::Letter {
        base: lower,
        mark: None,
        upper,
    })
}

fn tone_key(lower: char) -> Option<Tone> {
    match lower {
        's' => Some(Tone::Acute),
        'f' => Some(Tone::Grave),
        'r' => Some(Tone::HookAbove),
        'x' => Some(Tone::Tilde),
        'j' => Some(Tone::UnderDot),
        _ => None,
    }
}

/// `w` is a breve on `a` and a horn on `o`/`u`, so which mark it is depends on
/// the vowel it finds. With no vowel to decorate it types `ư`.
fn horn_or_breve(syllable: &Syllable, literal: char, upper: bool) -> Transform {
    let last_vowel = syllable
        .letters()
        .iter()
        .rev()
        .find(|letter| rules::is_vowel_base(letter.base));
    match last_vowel.map(|letter| letter.base) {
        Some('a') => Transform::Mark {
            mark: Mark::Breve,
            literal,
        },
        Some('o' | 'u') => Transform::Mark {
            mark: Mark::Horn,
            literal,
        },
        _ => Transform::Letter {
            base: 'u',
            mark: Some(Mark::Horn),
            upper,
        },
    }
}

/// `dd` is a stroke only at the very start of a syllable, so `add` stays `add`
/// and `ddi` becomes `đi`.
fn doubles_into_stroke(syllable: &Syllable) -> bool {
    matches!(syllable.letters(), [only] if only.base == 'd')
}

/// `aa`, `ee`, `oo` — the doubled vowel, where the second one is a circumflex.
///
/// The test is on the **last letter typed**, not on the nucleus, which is what
/// keeps `taoa` from turning into `taô`: the two letters have to be adjacent.
fn repeats_last_vowel(syllable: &Syllable, lower: char) -> bool {
    matches!(syllable.letters().last(), Some(last) if last.base == lower)
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
        interpret(key, &syllable(spelling), true)
    }

    #[test]
    fn the_five_tone_letters_and_the_undo_letter() {
        for (key, tone) in [
            ('s', Tone::Acute),
            ('f', Tone::Grave),
            ('r', Tone::HookAbove),
            ('x', Tone::Tilde),
            ('j', Tone::UnderDot),
        ] {
            assert_eq!(
                read(key, "ta"),
                Some(Transform::Tone { tone, literal: key }),
                "{key}"
            );
        }
        assert_eq!(read('z', "ta"), Some(Transform::ClearTone { literal: 'z' }));
    }

    /// A tone key is reported as a tone key wherever it appears. Whether it is
    /// *allowed* there is the engine's decision, not this file's — which is
    /// what keeps the "does this look like Vietnamese" rule in one place.
    #[test]
    fn a_tone_letter_is_a_tone_letter_even_with_nothing_to_put_it_on() {
        assert!(matches!(read('s', ""), Some(Transform::Tone { .. })));
        assert!(matches!(read('r', "sp"), Some(Transform::Tone { .. })));
    }

    #[test]
    fn a_doubled_vowel_is_a_circumflex() {
        for (key, spelling) in [('a', "ta"), ('e', "tie"), ('o', "kho")] {
            assert_eq!(
                read(key, spelling),
                Some(Transform::Mark {
                    mark: Mark::Circumflex,
                    literal: key
                }),
                "{spelling}{key}"
            );
        }
    }

    /// `taoa` is not `taô`: the doubled letters have to be adjacent.
    #[test]
    fn a_vowel_that_does_not_repeat_the_last_letter_is_just_a_letter() {
        assert_eq!(
            read('a', "tao"),
            Some(Transform::Letter {
                base: 'a',
                mark: None,
                upper: false
            })
        );
        assert_eq!(
            read('e', "ta"),
            Some(Transform::Letter {
                base: 'e',
                mark: None,
                upper: false
            })
        );
    }

    #[test]
    fn w_is_a_breve_on_a_and_a_horn_on_o_or_u() {
        assert_eq!(
            read('w', "da"),
            Some(Transform::Mark {
                mark: Mark::Breve,
                literal: 'w'
            })
        );
        for spelling in ["do", "du"] {
            assert_eq!(
                read('w', spelling),
                Some(Transform::Mark {
                    mark: Mark::Horn,
                    literal: 'w'
                }),
                "{spelling}"
            );
        }
    }

    /// `tw` and `tuw` both give `tư`.
    #[test]
    fn w_with_no_vowel_to_decorate_types_u_horn() {
        for spelling in ["", "t", "th", "ti"] {
            assert_eq!(
                read('w', spelling),
                Some(Transform::Letter {
                    base: 'u',
                    mark: Some(Mark::Horn),
                    upper: false
                }),
                "{spelling}"
            );
        }
        assert_eq!(
            read('W', "t"),
            Some(Transform::Letter {
                base: 'u',
                mark: Some(Mark::Horn),
                upper: true
            })
        );
    }

    #[test]
    fn dd_is_a_stroke_only_at_the_start_of_a_syllable() {
        assert_eq!(
            read('d', "d"),
            Some(Transform::Mark {
                mark: Mark::Stroke,
                literal: 'd'
            })
        );
        // `add` is a word, not `ađ`.
        assert_eq!(
            read('d', "ad"),
            Some(Transform::Letter {
                base: 'd',
                mark: None,
                upper: false
            })
        );
        assert_eq!(
            read('d', ""),
            Some(Transform::Letter {
                base: 'd',
                mark: None,
                upper: false
            })
        );
    }

    #[test]
    fn case_is_carried_through_every_reading() {
        assert_eq!(
            read('D', "D"),
            Some(Transform::Mark {
                mark: Mark::Stroke,
                literal: 'D'
            })
        );
        assert_eq!(
            read('J', "VIET"),
            Some(Transform::Tone {
                tone: Tone::UnderDot,
                literal: 'J'
            })
        );
        assert_eq!(
            read('B', ""),
            Some(Transform::Letter {
                base: 'b',
                mark: None,
                upper: true
            })
        );
    }

    #[test]
    fn a_non_letter_is_not_a_telex_key() {
        for key in [' ', '.', ',', '1', '\u{e9}'] {
            assert_eq!(read(key, "ta"), None, "{key:?}");
        }
    }

    /// The only way to reach `uơ`, and off when the setting is off.
    #[test]
    fn brackets_type_o_horn_and_u_horn_when_enabled() {
        assert_eq!(
            read('[', "thu"),
            Some(Transform::Letter {
                base: 'o',
                mark: Some(Mark::Horn),
                upper: false
            })
        );
        assert_eq!(
            read(']', "t"),
            Some(Transform::Letter {
                base: 'u',
                mark: Some(Mark::Horn),
                upper: false
            })
        );
        assert_eq!(
            read('{', "TH"),
            Some(Transform::Letter {
                base: 'o',
                mark: Some(Mark::Horn),
                upper: true
            })
        );

        for key in ['[', ']', '{', '}'] {
            assert_eq!(interpret(key, &syllable("thu"), false), None, "{key}");
        }
    }
}
