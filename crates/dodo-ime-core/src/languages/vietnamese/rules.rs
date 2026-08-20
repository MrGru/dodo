//! The shape of a Vietnamese syllable: where the initial ends, where the
//! nucleus is, and whether the whole thing is a syllable at all.
//!
//! Every Vietnamese syllable is
//! `[initial consonant] [vowel nucleus] [final consonant]`, and the vowels form
//! exactly one run — so the split is found by locating that run rather than
//! being tracked as the user types. That is deliberate: a derived split cannot
//! drift out of step with the letters, and it re-derives correctly after a
//! backspace, after a mark is undone, and after a letter is inserted in the
//! middle of what was already a valid syllable.
//!
//! # The two glides that belong to the initial
//!
//! `qu` and `gi` are the trap. Both end in a vowel letter that is not part of
//! the nucleus, and both change where the tone goes:
//!
//! - **`quả`, not `qủa`.** The `u` of `qu` is part of the initial, so the
//!   nucleus of `qua` is just `a`.
//! - **`giả`, not `gỉa`.** Same for the `i` of `gi` — but only when another
//!   vowel follows it. In `gì` ("what") there is nothing after the `i`, so the
//!   `i` *is* the nucleus and `g` is the initial on its own.
//!
//! [`parts`] absorbs the glide only when at least one vowel remains behind it,
//! which is the rule that gets both `giếng` (nucleus `ê`) and `gìn` (nucleus
//! `i`) right.
//!
//! # Validity is structural, not a word list
//!
//! [`is_valid_syllable`] checks that the initial is a real Vietnamese initial
//! cluster, that there is a nucleus of at most three vowels, and that the final
//! is a real final cluster. It does **not** check the nucleus against a list of
//! attested vowel combinations. A whitelist would be more precise and would
//! also silently mangle the first unusual-but-real syllable it had not heard
//! of; the structural check errs the other way, which is the right direction
//! for a rule whose only job is deciding when to stop interfering.
//!
//! [`viability`] adds the incremental distinction the final validity answer
//! cannot provide: `th` is a useful prefix, while `br` is impossible. The
//! engine uses that answer to restore trustworthy foreign input immediately and
//! keep later Telex controls literal until the boundary. Tone keys and commit
//! fallback still ask final validity, and [`allows_tone`] adds the compact
//! checked-coda rule that is not a word-list question.

use super::syllable::{Letter, Mark, Tone};
use super::unicode::base_with_mark;

/// The vowel letters, before any diacritic. `y` counts: Vietnamese uses it as a
/// full vowel (`yêu`, `mỹ`, `thuý`), not as a consonant.
pub fn is_vowel_base(base: char) -> bool {
    matches!(base, 'a' | 'e' | 'i' | 'o' | 'u' | 'y')
}

/// Every consonant cluster a Vietnamese syllable may begin with, plus the empty
/// one (`ăn`, `yêu`, `ước` all start with a vowel).
///
/// `f`, `j`, `w` and `z` are absent because they are not Vietnamese letters —
/// which is exactly why an English word beginning with one of them passes
/// straight through untransformed.
pub const INITIALS: [&str; 29] = [
    "", "b", "c", "ch", "d", "đ", "g", "gh", "gi", "h", "k", "kh", "l", "m", "n", "ng", "ngh",
    "nh", "p", "ph", "q", "qu", "r", "s", "t", "th", "tr", "v", "x",
];

/// Every consonant cluster a Vietnamese syllable may end with, plus the empty
/// one.
///
/// Short by design: the offglides that look like finals (`ai`, `ao`, `ơi`) are
/// vowels and live in the nucleus.
pub const FINALS: [&str; 9] = ["", "c", "ch", "m", "n", "ng", "nh", "p", "t"];

/// The longest a Vietnamese vowel nucleus gets — `uyê` in `khuyên`, `ươi` in
/// `người`, `oai` in `ngoài`.
pub const MAX_NUCLEUS: usize = 3;

/// Whether the current letters can still become a Vietnamese syllable.
///
/// This is deliberately smaller than a dictionary. It distinguishes an
/// unfinished onset from a shape that no later letter can repair, while the
/// semantic syllable promotes a pending `uơ`/`ươ` reading to [`Ambiguous`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Viability {
    /// An incomplete onset such as `th` or `ngh`.
    Prefix,
    /// A structurally complete Vietnamese syllable.
    Valid,
    /// More than one semantic reading is still useful.
    Ambiguous,
    /// No continuation can repair the onset/nucleus/coda split.
    Impossible,
}

/// Where the vowel run is, and therefore where everything else is.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Parts {
    /// The vowel nucleus, as an index range into the letters. Empty when no
    /// vowel has been typed yet.
    pub nucleus: std::ops::Range<usize>,
    total: usize,
}

impl Parts {
    /// The initial consonant cluster, including an absorbed `qu`/`gi` glide.
    pub fn initial(&self) -> std::ops::Range<usize> {
        0..self.nucleus.start
    }

    /// The final consonant cluster.
    pub fn coda(&self) -> std::ops::Range<usize> {
        self.nucleus.end..self.total
    }

    pub fn has_nucleus(&self) -> bool {
        !self.nucleus.is_empty()
    }

    /// Whether a consonant follows the vowels. The single most important input
    /// to tone placement: a final consonant pulls the tone onto the last vowel.
    pub fn has_coda(&self) -> bool {
        self.nucleus.end < self.total
    }
}

/// Split `letters` into initial, nucleus and final.
pub fn parts(letters: &[Letter]) -> Parts {
    let total = letters.len();
    let Some(first) = letters.iter().position(|l| is_vowel_base(l.base)) else {
        return Parts {
            nucleus: total..total,
            total,
        };
    };

    let mut end = first;
    while end < total && is_vowel_base(letters[end].base) {
        end += 1;
    }

    let mut start = first;
    // `qu` and `gi`: the glide joins the initial, but only if a vowel is left
    // over to be the nucleus. `gia` -> nucleus `a`; `gì` -> nucleus `i`.
    if end - start >= 2 && start == 1 && letters[0].mark.is_none() && letters[start].mark.is_none()
    {
        let absorbed = matches!(
            (letters[0].base, letters[start].base),
            ('q', 'u') | ('g', 'i')
        );
        if absorbed {
            start += 1;
        }
    }

    Parts {
        nucleus: start..end,
        total,
    }
}

/// A run of letters as a plain lowercase string, diacritics applied — `"đ"`,
/// `"ngh"`, `"ch"`.
///
/// Case is dropped because [`INITIALS`] and [`FINALS`] are about spelling, not
/// capitalization: `NGH` and `ngh` are the same cluster.
pub fn cluster(letters: &[Letter], range: std::ops::Range<usize>) -> String {
    letters[range]
        .iter()
        .map(|letter| base_with_mark(letter.base, letter.mark).unwrap_or(letter.base))
        .collect()
}

/// Whether these letters could be a Vietnamese syllable.
///
/// See the module docs for why this is structural rather than a word list.
pub fn is_valid_syllable(letters: &[Letter]) -> bool {
    if letters.is_empty() {
        return false;
    }
    let parts = parts(letters);
    if !parts.has_nucleus() || parts.nucleus.len() > MAX_NUCLEUS {
        return false;
    }
    INITIALS.contains(&cluster(letters, parts.initial()).as_str())
        && FINALS.contains(&cluster(letters, parts.coda()).as_str())
}

/// Classify an incremental letter sequence without pretending that every
/// incomplete sequence is invalid.
pub fn viability(letters: &[Letter]) -> Viability {
    if is_valid_syllable(letters) {
        return Viability::Valid;
    }

    let split = parts(letters);
    if split.has_nucleus() {
        return Viability::Impossible;
    }

    let prefix = cluster(letters, split.initial());
    if INITIALS.iter().any(|initial| initial.starts_with(&prefix)) {
        Viability::Prefix
    } else {
        Viability::Impossible
    }
}

/// Whether `tone` is phonotactically possible with the current coda.
///
/// Syllables closed by `c`, `ch`, `p`, or `t` carry only sắc or nặng when a
/// tone key is explicit. The level state remains usable while typing because a
/// later tone key can still complete it.
pub fn allows_tone(letters: &[Letter], tone: Tone) -> bool {
    if matches!(tone, Tone::Level | Tone::Acute | Tone::UnderDot) {
        return true;
    }
    let split = parts(letters);
    !matches!(
        cluster(letters, split.coda()).as_str(),
        "c" | "ch" | "p" | "t"
    )
}

/// Whether one horn key over bare `uo` has a useful open `uơ` reading.
///
/// In `thuo`, the open nucleus can still become `thuở`, while a following
/// vowel or coda resolves the same key as `ươ`. Keeping this question in the
/// phonotactic layer lets the syllable retain provenance without learning a
/// word-specific exception.
pub fn provisional_uo_horn(letters: &[Letter], first: usize, second: usize) -> bool {
    let split = parts(letters);
    split.nucleus == (first..second + 1)
        && !split.has_coda()
        && cluster(letters, split.initial()) == "th"
}

/// Whether a targetless Telex `w` can begin the true nucleus as `ư`.
///
/// Usually that requires no nucleus yet. The derived `gi` split is the useful
/// exception: appending `ư` turns the current `i` into the initial's glide, so
/// `giw` can become `giư`/`giữ` without licensing synthetic vowels after
/// ordinary nuclei such as `ne` or `ti`.
pub fn can_append_u_horn(letters: &[Letter]) -> bool {
    let before = parts(letters);
    if !before.has_nucleus() {
        return true;
    }

    let mut candidate = letters.to_vec();
    candidate.push(Letter::new('u', false).with_mark(Some(Mark::Horn)));
    let after = parts(&candidate);
    after.nucleus.start > before.nucleus.start && is_valid_syllable(&candidate)
}

#[cfg(test)]
mod tests {
    use super::{
        FINALS, INITIALS, Viability, allows_tone, can_append_u_horn, cluster, is_valid_syllable,
        is_vowel_base, parts, provisional_uo_horn, viability,
    };
    use crate::languages::vietnamese::syllable::{Letter, Mark, Tone};

    /// Build letters from a spelling, `^` marking the mark on the letter before
    /// it: `d^` is `đ`, `e^` is `ê`. Enough to state the structural cases
    /// without dragging a parser in.
    fn letters(spelling: &str) -> Vec<Letter> {
        let mut out: Vec<Letter> = Vec::new();
        for ch in spelling.chars() {
            match ch {
                'â' => out.push(Letter::new('a', false).with_mark(Some(Mark::Circumflex))),
                'ă' => out.push(Letter::new('a', false).with_mark(Some(Mark::Breve))),
                'ê' => out.push(Letter::new('e', false).with_mark(Some(Mark::Circumflex))),
                'ô' => out.push(Letter::new('o', false).with_mark(Some(Mark::Circumflex))),
                'ơ' => out.push(Letter::new('o', false).with_mark(Some(Mark::Horn))),
                'ư' => out.push(Letter::new('u', false).with_mark(Some(Mark::Horn))),
                'đ' => out.push(Letter::new('d', false).with_mark(Some(Mark::Stroke))),
                ch => out.push(Letter::new(ch, ch.is_uppercase())),
            }
        }
        out
    }

    fn nucleus_of(spelling: &str) -> String {
        let letters = letters(spelling);
        let parts = parts(&letters);
        cluster(&letters, parts.nucleus)
    }

    #[test]
    fn y_is_a_vowel() {
        for base in ['a', 'e', 'i', 'o', 'u', 'y'] {
            assert!(is_vowel_base(base), "{base}");
        }
        for base in ['b', 'd', 'g', 'n', 'q', 'w'] {
            assert!(!is_vowel_base(base), "{base}");
        }
    }

    #[test]
    fn the_split_is_found_not_tracked() {
        let cases = [
            ("nghieng", "", "ngh", "ie", "ng"),
            ("ban", "", "b", "a", "n"),
            ("an", "", "", "a", "n"),
            ("yeu", "", "", "yeu", ""),
            ("truong", "", "tr", "uo", "ng"),
            ("ngoai", "", "ng", "oai", ""),
        ];
        for (spelling, _, initial, nucleus, coda) in cases {
            let letters = letters(spelling);
            let parts = parts(&letters);
            assert_eq!(cluster(&letters, parts.initial()), initial, "{spelling}");
            assert_eq!(
                cluster(&letters, parts.nucleus.clone()),
                nucleus,
                "{spelling}"
            );
            assert_eq!(cluster(&letters, parts.coda()), coda, "{spelling}");
        }
    }

    /// `quả`, not `qủa` — the `u` of `qu` is a consonant's tail, not a vowel.
    #[test]
    fn qu_absorbs_its_glide() {
        assert_eq!(nucleus_of("qua"), "a");
        assert_eq!(nucleus_of("quan"), "a");
        assert_eq!(nucleus_of("quy"), "y");
        assert_eq!(nucleus_of("quyên"), "yê");
        assert_eq!(nucleus_of("quôc"), "ô");
        assert_eq!(nucleus_of("quê"), "ê");
        // Nothing to absorb into: `qu` alone keeps its `u` as the nucleus.
        assert_eq!(nucleus_of("qu"), "u");
        // Not a `qu`: the `u` here is the nucleus.
        assert_eq!(nucleus_of("tu"), "u");
    }

    /// `giả`, not `gỉa`; but `gìn`, not `g` + nothing.
    #[test]
    fn gi_absorbs_its_glide_only_when_a_vowel_is_left_over() {
        assert_eq!(nucleus_of("gia"), "a");
        assert_eq!(nucleus_of("giêng"), "ê");
        assert_eq!(nucleus_of("giư"), "ư");
        assert_eq!(nucleus_of("giai"), "ai");
        assert_eq!(nucleus_of("giơi"), "ơi");
        // `gì`: the `i` is all there is, so it is the nucleus.
        assert_eq!(nucleus_of("gi"), "i");
        assert_eq!(nucleus_of("gin"), "i");
        // `ghi` is a different initial and absorbs nothing.
        assert_eq!(nucleus_of("ghi"), "i");
    }

    #[test]
    fn a_syllable_with_no_vowel_has_an_empty_nucleus() {
        let letters = letters("ngh");
        let parts = parts(&letters);
        assert!(!parts.has_nucleus());
        assert!(!parts.has_coda());
        assert_eq!(cluster(&letters, parts.initial()), "ngh");
    }

    #[test]
    fn a_coda_is_only_a_coda_when_consonants_follow_the_vowels() {
        assert!(parts(&letters("tiêng")).has_coda());
        assert!(!parts(&letters("tôi")).has_coda());
        assert!(!parts(&letters("hoa")).has_coda());
    }

    #[test]
    fn real_syllables_are_valid() {
        for spelling in [
            "tiêng", "viêt", "đăng", "đương", "nguyên", "chuyên", "quôc", "ngươi", "khoe", "thuy",
            "gi", "an", "yêu", "ươc", "nghiêng", "quyên", "giêng", "hoa", "ngoai", "khuyu",
        ] {
            assert!(is_valid_syllable(&letters(spelling)), "{spelling}");
        }
    }

    /// The other half of the job: knowing when to leave the keystrokes alone.
    #[test]
    fn non_vietnamese_shapes_are_not_syllables() {
        let cases = [
            ("", "nothing typed"),
            ("ngh", "no vowel"),
            ("spo", "sp is not an initial"),
            ("cas", "s is not a final"),
            ("hello", "vowels on both sides of the consonants"),
            ("zoo", "z is not a Vietnamese letter"),
            ("world", "w is not a Vietnamese letter"),
            ("aeiou", "a nucleus of five"),
            ("tenth", "nth is not a final"),
        ];
        for (spelling, why) in cases {
            assert!(!is_valid_syllable(&letters(spelling)), "{spelling}: {why}");
        }
    }

    #[test]
    fn incremental_viability_distinguishes_prefixes_from_impossible_runs() {
        for spelling in ["", "t", "th", "ngh"] {
            assert_eq!(
                viability(&letters(spelling)),
                Viability::Prefix,
                "{spelling}"
            );
        }
        for spelling in ["ta", "thuơ", "thươn", "giư"] {
            assert_eq!(
                viability(&letters(spelling)),
                Viability::Valid,
                "{spelling}"
            );
        }
        for spelling in ["f", "br", "new", "world"] {
            assert_eq!(
                viability(&letters(spelling)),
                Viability::Impossible,
                "{spelling}"
            );
        }
    }

    #[test]
    fn targetless_w_and_uo_horn_are_contextual() {
        for spelling in ["", "t", "th", "gi"] {
            assert!(can_append_u_horn(&letters(spelling)), "{spelling}");
        }
        for spelling in ["ti", "ne", "vie"] {
            assert!(!can_append_u_horn(&letters(spelling)), "{spelling}");
        }

        assert!(provisional_uo_horn(&letters("thuo"), 2, 3));
        assert!(!provisional_uo_horn(&letters("duo"), 1, 2));
        assert!(!provisional_uo_horn(&letters("thuon"), 2, 3));
    }

    #[test]
    fn stop_codas_accept_only_checked_tones() {
        let cat = letters("cat");
        assert!(allows_tone(&cat, Tone::Acute));
        assert!(allows_tone(&cat, Tone::UnderDot));
        assert!(!allows_tone(&cat, Tone::Grave));
        assert!(!allows_tone(&cat, Tone::HookAbove));
        assert!(!allows_tone(&cat, Tone::Tilde));
        assert!(allows_tone(&letters("can"), Tone::Grave));
    }

    #[test]
    fn the_cluster_tables_hold_no_duplicates_and_no_foreign_letters() {
        for table in [INITIALS.to_vec(), FINALS.to_vec()] {
            let mut sorted = table.clone();
            sorted.sort_unstable();
            let count = sorted.len();
            sorted.dedup();
            assert_eq!(sorted.len(), count, "duplicate cluster in {table:?}");
        }
        for initial in INITIALS {
            assert!(
                !initial.contains(['f', 'j', 'w', 'z']),
                "{initial} is not Vietnamese"
            );
        }
    }
}
