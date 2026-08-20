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
//! | `dd` | stroke — `đ` (on the syllable's initial `d`, so `did` is `đi` too) |
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
//! Case is the shared layer's too. A doubled letter key (`dd`, `aa`, `ee`,
//! `oo`) states the case of the letter it marks and `w` does not, but neither
//! rule is written below: [`Transform::Mark`] carries only the literal, and
//! `Syllable::retypes_last_letter` reads its case. See the parent module docs.
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
//! The bare `w` is the one key that reaches that layer as a whole *letter*
//! rather than as a mark, and it undoes itself the same way — but the shared
//! layer has to find the letter it made first, and a mark target is the wrong
//! question for that. `windoư` has a nucleus of `i`, which can carry no horn at
//! all, so there is no target; `ưindo` has one, five letters back. The source
//! key recorded on the letter answers both, which is why `window` survives
//! being read as Telex. See
//! [`SourceCancelled`](super::syllable::MarkOutcome::SourceCancelled) and
//! [`SourceRestored`](super::syllable::MarkOutcome::SourceRestored) for the two
//! shapes that takes.
//!
//! # Three judgement calls
//!
//! - **`w` before a nucleus types `ư`.** With no vowel yet, `tw` and `tuw` both
//!   give `tư`. After an existing nucleus with no horn target it stays literal,
//!   which is what keeps `new` and `view` intact. `gi` is derived rather than
//!   excepted: adding `ư` moves its `i` into the initial glide, so `giw` remains
//!   a valid way to begin `giữ`.
//! - **One `w` over `uo` owns the pair.** It normally marks both vowels and a
//!   repeat undoes both. Open `thuo` first displays the viable `uơ` reading;
//!   later nucleus/coda context resolves the same semantic command to `ươ`.
//! - **`[` and `]` type `ơ` and `ư` outright.** Unikey's own shortcut, on by
//!   default there and here
//!   ([`bracket_shortcuts`](super::VietnameseConfig::bracket_shortcuts)). It is
//!   the explicit, non-provisional way to reach `uơ` (`thu[r` gives `thuở`) and
//!   remains needed for forms such as `huơ`. Turn it off and a bracket is an
//!   ordinary bracket.

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
    if lower == 'd' && strokes_the_initial_d(syllable) {
        return Some(Transform::Mark {
            mark: Mark::Stroke,
            literal: key,
        });
    }
    if matches!(lower, 'a' | 'e' | 'o') && repeats_nucleus_vowel(syllable, lower) {
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
/// the vowel it finds. It asks the syllable's target rather than the last
/// vowel typed: `hoiw` reaches back over the `i` to make `hơi`.
///
/// With no target, `w` creates `ư` only before a nucleus. Once a nucleus already
/// exists, appending a second synthetic vowel is not a useful Vietnamese
/// reading (`new`, `view`), so the key remains literal.
fn horn_or_breve(syllable: &Syllable, literal: char, upper: bool) -> Transform {
    let parts = rules::parts(syllable.letters());
    match syllable.letters()[parts.nucleus.clone()]
        .iter()
        .rev()
        .find(|letter| matches!(letter.base, 'a' | 'o' | 'u'))
        .map(|letter| letter.base)
    {
        Some('a') => Transform::Mark {
            mark: Mark::Breve,
            literal,
        },
        Some('o' | 'u') => Transform::Mark {
            mark: Mark::Horn,
            literal,
        },
        _ if rules::can_append_u_horn(syllable.letters()) => Transform::Letter {
            base: 'u',
            mark: Some(Mark::Horn),
            upper,
        },
        _ => Transform::Letter {
            base: 'w',
            mark: None,
            upper,
        },
    }
}

/// The stroke key marks the syllable's **initial** `d`, wherever in the word it
/// is typed — `ddi` and `did` both give `đi`, and `add` stays `add` because its
/// first letter is an `a`.
///
/// This asks [`Syllable::mark_target`] rather than looking at the letters
/// itself, so the position rule is stated once and both schemes obey it: `9` in
/// VNI has always reached back this way. The test is deliberately *not* "and it
/// is not already stroked" — a second stroke key is a
/// [`Reverted`](super::syllable::MarkOutcome::Reverted) mark, which is how every
/// other modifier here undoes itself (`noww` types `now`, `didd` types `did`).
///
/// Whether the resulting `đ` is capital is not decided here either. The shared
/// layer takes the case from this key when it is adjacent to the `d` it marks
/// (`dD` is `Đ`, `Dd` is `đ`) and leaves it alone when the key reached back
/// over a word (`Did` is `Đi`).
fn strokes_the_initial_d(syllable: &Syllable) -> bool {
    syllable.mark_target(Mark::Stroke).is_some()
}

/// `aa`, `ee`, `oo` — the repeated nucleus vowel, where the later key is a
/// circumflex. The vowel need not be adjacent: `thienej` and `thieenj` both
/// mean `thiện`. The current nucleus target keeps unrelated vowels out, so
/// `taoa` remains plain text rather than marking its `o`.
fn repeats_nucleus_vowel(syllable: &Syllable, lower: char) -> bool {
    syllable.is_valid()
        && syllable
            .mark_target(Mark::Circumflex)
            .is_some_and(|at| syllable.letters()[at].base == lower)
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
        for (key, spelling) in [
            ('a', "ta"),
            ('e', "tie"),
            ('o', "kho"),
            // The doubling is on the base letter, so case matters to neither
            // half of it: `Aa`, `aA` and `AA` are all circumflexes.
            ('a', "tA"),
            ('A', "ta"),
            ('A', "tA"),
            ('E', "TIE"),
            ('O', "KHo"),
        ] {
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

    /// The current nucleus decides a repeated vowel, not the nearest key.
    #[test]
    fn a_vowel_outside_the_current_nucleus_is_just_a_letter() {
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
        assert_eq!(
            read('e', "thien"),
            Some(Transform::Mark {
                mark: Mark::Circumflex,
                literal: 'e'
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

    /// A `Mark` carries no case field, only the literal to type if it turns out
    /// not to apply. Whether that literal's own shift reaches the letter is the
    /// shared layer's decision and is asserted in this crate's engine tests;
    /// all this file promises is that a shifted key reads as the same mark.
    #[test]
    fn a_shifted_modifier_key_is_the_same_mark_on_the_same_letter() {
        assert_eq!(
            read('W', "DA"),
            Some(Transform::Mark {
                mark: Mark::Breve,
                literal: 'W'
            })
        );
        for spelling in ["do", "DU", "hoi"] {
            assert_eq!(
                read('W', spelling),
                Some(Transform::Mark {
                    mark: Mark::Horn,
                    literal: 'W'
                }),
                "{spelling}"
            );
        }
        assert_eq!(
            read('D', "d"),
            Some(Transform::Mark {
                mark: Mark::Stroke,
                literal: 'D'
            })
        );
        assert_eq!(
            read('d', "D"),
            Some(Transform::Mark {
                mark: Mark::Stroke,
                literal: 'd'
            })
        );
    }

    /// `tw` and `tuw` both give `tư`, but a nucleus that cannot take a horn
    /// makes the key literal rather than inventing another vowel.
    #[test]
    fn w_without_a_target_depends_on_whether_a_nucleus_exists() {
        for spelling in ["", "t", "th"] {
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
        // And when `w` is the letter, its own shift is the only case there is:
        // `W` alone is `Ư`.
        for spelling in ["", "T", "TH"] {
            assert_eq!(
                read('W', spelling),
                Some(Transform::Letter {
                    base: 'u',
                    mark: Some(Mark::Horn),
                    upper: true
                }),
                "{spelling}"
            );
        }
        for spelling in ["ti", "ne", "vie"] {
            assert_eq!(
                read('w', spelling),
                Some(Transform::Letter {
                    base: 'w',
                    mark: None,
                    upper: false
                }),
                "{spelling}"
            );
        }
    }

    /// The stroke asks about the syllable's **first** letter, not about how
    /// many letters there are — so it reaches back over what has been typed
    /// since, exactly as `w` and the tone keys do.
    #[test]
    fn the_stroke_key_marks_the_syllables_initial_d_from_anywhere_in_the_word() {
        for spelling in ["d", "di", "die", "dươn", "D", "dI"] {
            assert_eq!(
                read('d', spelling),
                Some(Transform::Mark {
                    mark: Mark::Stroke,
                    literal: 'd'
                }),
                "{spelling}d"
            );
        }
        // `add` is a word, not `ađ`: the `d` is not the initial letter.
        for spelling in ["ad", "sd", "ADD", "and"] {
            assert_eq!(
                read('d', spelling),
                Some(Transform::Letter {
                    base: 'd',
                    mark: None,
                    upper: false
                }),
                "{spelling}d"
            );
        }
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

    /// The explicit way to type `uơ`, and off when the setting is off.
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
        assert_eq!(
            read('}', "T"),
            Some(Transform::Letter {
                base: 'u',
                mark: Some(Mark::Horn),
                upper: true
            })
        );

        for key in ['[', ']', '{', '}'] {
            assert_eq!(interpret(key, &syllable("thu"), false), None, "{key}");
        }
    }
}
