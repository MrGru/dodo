//! Which vowel carries the tone mark.
//!
//! The tone belongs to the **syllable**, not to a letter — that is the single
//! design decision this whole engine is built on. It is stored once on
//! [`Syllable`](super::syllable::Syllable) and its position is worked out here,
//! at render time, from the letters as they currently stand. So typing `toans`
//! puts the mark on `a` (`toán`) and typing `toas` then `n` does too, because
//! the arriving `n` changes the answer rather than requiring the mark to be
//! moved.
//!
//! # The rules, in the order they are applied
//!
//! 1. **A vowel wearing a diacritic takes the tone**, and if two do, the last
//!    one does. This one rule settles `ê`, `ô`, `ơ`, `ư`, `â` and `ă` outright:
//!    `tiếng`, `chuối`, `người`, `cứu`, `tuấn`, `hoặc`. The "last" clause is
//!    what makes `ươ` come out as `ườ` and not `ừơ` — `người`, `được`, `rượu`.
//! 2. **Otherwise**, on plain vowels:
//!    - one vowel: it takes the tone (`cá`, `mẹ`);
//!    - three vowels: the middle one (`ngoài`, `xoáy`, `khuỷu`) — the outer two
//!      are glides;
//!    - two vowels **with a final consonant**: the second (`toán`, `hoàn`,
//!      `Huỳnh`). A glide cannot stand before a final consonant, so the second
//!      vowel is always the main one here;
//!    - two vowels **with no final consonant**: the first (`cái`, `báo`, `kéo`,
//!      `múa`, `bìa`) — except for the three pairs below.
//!
//! # `hoà` or `hòa`: the one genuinely contested case
//!
//! Exactly three open two-vowel nuclei begin with a glide: **`oa`, `oe`, `uy`**.
//! For these the two conventions disagree, and both are in daily use:
//!
//! | | `oa` | `oe` | `uy` |
//! |---|---|---|---|
//! | [`TonePlacement::Modern`] (default) | ho**à** | kho**ẻ** | thu**ỷ** |
//! | [`TonePlacement::Traditional`] | h**ò**a | kh**ỏ**e | th**ủ**y |
//!
//! Modern placement marks the main vowel; traditional placement marks the
//! visual middle of the syllable. dodo defaults to modern, matching Unikey's
//! own default and current Vietnamese teaching practice, and the alternative is
//! a field on [`VietnameseConfig`](super::VietnameseConfig) rather than a
//! constant — a settings page for it is a later round.
//!
//! No other nucleus is affected. `oai`, `uôi`, `ươi`, `iêu`, `uyê` and the rest
//! place identically under both conventions, which is why the switch is one
//! `match` arm and not a second placement table.

use super::rules::{self, Parts};
use super::syllable::{Letter, Mark};

/// Where a tone mark goes when a nucleus could take it in two places.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum TonePlacement {
    /// `hoà`, `khoẻ`, `thuỷ` — the mark goes on the main vowel.
    #[default]
    Modern,
    /// `hòa`, `khỏe`, `thủy` — the mark goes on the first vowel of the pair.
    Traditional,
}

/// The three open nuclei whose first vowel is a glide, and the only place the
/// two conventions differ.
fn is_glide_pair(first: char, second: char) -> bool {
    matches!((first, second), ('o', 'a') | ('o', 'e') | ('u', 'y'))
}

/// Index into `letters` of the vowel that carries the tone, or `None` when
/// there is no vowel to carry one.
///
/// `None` is not an error: it is the state of a half-typed syllable (`ngh`), and
/// the caller renders every letter without a tone mark, which is exactly right.
pub fn placement(letters: &[Letter], style: TonePlacement) -> Option<usize> {
    let parts = rules::parts(letters);
    placement_in(letters, &parts, style)
}

/// [`placement`], for a caller that has already split the syllable.
pub fn placement_in(letters: &[Letter], parts: &Parts, style: TonePlacement) -> Option<usize> {
    if !parts.has_nucleus() {
        return None;
    }
    let nucleus = &letters[parts.nucleus.clone()];

    // 1. A diacritic wins, and the last diacritic wins over an earlier one.
    let marked = nucleus.iter().rposition(|letter| {
        matches!(
            letter.mark,
            Some(Mark::Circumflex | Mark::Breve | Mark::Horn)
        )
    });
    if let Some(at) = marked {
        return Some(parts.nucleus.start + at);
    }

    // 2. Plain vowels.
    let offset = match nucleus.len() {
        1 => 0,
        2 if parts.has_coda() => 1,
        2 if is_glide_pair(nucleus[0].base, nucleus[1].base) => match style {
            TonePlacement::Modern => 1,
            TonePlacement::Traditional => 0,
        },
        2 => 0,
        // Three vowels: the outer two are glides, so the middle one is the
        // nucleus proper. Longer than three is not a Vietnamese nucleus at all
        // and is only reachable through a syllable that has already failed
        // `is_valid_syllable`; the middle is as good an answer as any and, more
        // to the point, is in range.
        _ => nucleus.len() / 2,
    };
    Some(parts.nucleus.start + offset)
}

#[cfg(test)]
mod tests {
    use super::{TonePlacement, placement};
    use crate::languages::vietnamese::syllable::{Letter, Mark};

    fn letters(spelling: &str) -> Vec<Letter> {
        spelling
            .chars()
            .map(|ch| match ch {
                'â' => Letter::new('a', false).with_mark(Some(Mark::Circumflex)),
                'ă' => Letter::new('a', false).with_mark(Some(Mark::Breve)),
                'ê' => Letter::new('e', false).with_mark(Some(Mark::Circumflex)),
                'ô' => Letter::new('o', false).with_mark(Some(Mark::Circumflex)),
                'ơ' => Letter::new('o', false).with_mark(Some(Mark::Horn)),
                'ư' => Letter::new('u', false).with_mark(Some(Mark::Horn)),
                'đ' => Letter::new('d', false).with_mark(Some(Mark::Stroke)),
                ch => Letter::new(ch, false),
            })
            .collect()
    }

    /// The letter the tone lands on, as a character — easier to read in a
    /// failure message than an index.
    fn carrier(spelling: &str, style: TonePlacement) -> Option<char> {
        let letters = letters(spelling);
        let at = placement(&letters, style)?;
        Some(spelling.chars().nth(at).expect("index is within the word"))
    }

    fn modern(spelling: &str) -> Option<char> {
        carrier(spelling, TonePlacement::Modern)
    }

    #[test]
    fn a_diacritic_takes_the_tone() {
        let cases = [
            ("tiêng", 'ê'),
            ("chuôi", 'ô'),
            ("cưu", 'ư'),
            ("tuân", 'â'),
            ("hoăc", 'ă'),
            ("nguyên", 'ê'),
            ("yêu", 'ê'),
            ("ươc", 'ơ'),
        ];
        for (spelling, expected) in cases {
            assert_eq!(modern(spelling), Some(expected), "{spelling}");
        }
    }

    /// `ườ`, never `ừơ`: two horns, and the second one takes the tone. This is
    /// the rule that gets `người`, `được`, `rượu` and `đường` right.
    #[test]
    fn the_last_diacritic_wins_which_is_what_uo_horn_needs() {
        assert_eq!(modern("ngươi"), Some('ơ'));
        assert_eq!(modern("đươc"), Some('ơ'));
        assert_eq!(modern("rươu"), Some('ơ'));
        assert_eq!(modern("đương"), Some('ơ'));
    }

    #[test]
    fn one_vowel_takes_its_own_tone() {
        assert_eq!(modern("ca"), Some('a'));
        assert_eq!(modern("me"), Some('e'));
        assert_eq!(modern("nghi"), Some('i'));
        assert_eq!(modern("quan"), Some('a'));
    }

    #[test]
    fn three_vowels_mark_the_middle_one() {
        assert_eq!(modern("ngoai"), Some('a'));
        assert_eq!(modern("xoay"), Some('a'));
        assert_eq!(modern("khuyu"), Some('y'));
        assert_eq!(modern("thoai"), Some('a'));
    }

    #[test]
    fn a_final_consonant_pulls_the_tone_onto_the_second_vowel() {
        assert_eq!(modern("toan"), Some('a'));
        assert_eq!(modern("hoan"), Some('a'));
        assert_eq!(modern("huynh"), Some('y'));
        assert_eq!(modern("loan"), Some('a'));
    }

    #[test]
    fn an_open_pair_marks_the_first_vowel_unless_it_is_a_glide() {
        for (spelling, expected) in [
            ("cai", 'a'),
            ("bao", 'a'),
            ("keo", 'e'),
            ("mua", 'u'),
            ("bia", 'i'),
            ("tui", 'u'),
            ("cau", 'a'),
            ("cay", 'a'),
            ("diu", 'i'),
        ] {
            assert_eq!(modern(spelling), Some(expected), "{spelling}");
        }
    }

    /// The one contested case, both ways.
    #[test]
    fn oa_oe_and_uy_are_where_the_two_conventions_part() {
        for (spelling, modern_carrier, traditional_carrier) in
            [("hoa", 'a', 'o'), ("khoe", 'e', 'o'), ("thuy", 'y', 'u')]
        {
            assert_eq!(
                carrier(spelling, TonePlacement::Modern),
                Some(modern_carrier),
                "{spelling}, modern"
            );
            assert_eq!(
                carrier(spelling, TonePlacement::Traditional),
                Some(traditional_carrier),
                "{spelling}, traditional"
            );
        }
    }

    /// Everything the two conventions agree on — which is everything else. If a
    /// future change makes the style switch reach further than these three
    /// nuclei, this is what notices.
    #[test]
    fn the_style_switch_touches_nothing_but_those_three_nuclei() {
        for spelling in [
            "tiêng", "ngươi", "chuôi", "ngoai", "toan", "cai", "bao", "mua", "bia", "khuyu",
            "nguyên", "yêu", "quan", "quy", "gia", "cưu", "huynh", "thoai", "đương", "hoăc",
        ] {
            assert_eq!(
                carrier(spelling, TonePlacement::Modern),
                carrier(spelling, TonePlacement::Traditional),
                "{spelling} should not depend on the placement style"
            );
        }
    }

    /// `quy` and `gia` place their tone through the initial-glide rule in
    /// `rules::parts`, so `quý` never comes out as `qúy` whichever convention
    /// is selected.
    #[test]
    fn an_absorbed_glide_is_not_a_tone_carrier() {
        assert_eq!(modern("quy"), Some('y'));
        assert_eq!(modern("qua"), Some('a'));
        assert_eq!(modern("gia"), Some('a'));
        assert_eq!(modern("giai"), Some('a'));
        assert_eq!(carrier("quy", TonePlacement::Traditional), Some('y'));
    }

    #[test]
    fn a_syllable_with_no_vowel_yet_carries_no_tone() {
        assert_eq!(modern("ngh"), None);
        assert_eq!(modern(""), None);
        assert_eq!(modern("tr"), None);
    }
}
