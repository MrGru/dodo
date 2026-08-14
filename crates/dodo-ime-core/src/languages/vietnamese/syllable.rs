//! The syllable being composed, held as meaning rather than as text.
//!
//! # `ế` is three facts, not a character
//!
//! A [`Letter`] is a base vowel, an optional [`Mark`], and a case; the
//! [`Tone`] belongs to the [`Syllable`] as a whole. `ế` is therefore *base `e`,
//! circumflex, acute, lowercase* — never the result of rewriting `e` into `ê`
//! into `ế`. Everything awkward about Vietnamese input falls out of that:
//!
//! - **The tone moves by itself.** Typing `toas` gives `toá`; the `n` that
//!   follows makes it `toán` because [`tone::placement`] is asked again, not
//!   because anything searched the string for a mark to relocate.
//! - **Undo is exact.** A second `s` clears [`Syllable::tone`]; a third `a`
//!   clears one [`Letter::mark`]. Neither has to find a diacritic in rendered
//!   text and guess which key put it there — and where a whole letter came from
//!   one key, the letter records that key, which is how `ư` the user typed and
//!   `ư` a `w` rendered stay two distinguishable states. See
//!   [`MarkOutcome::SourceCancelled`] and [`MarkOutcome::SourceRestored`].
//! - **Telex and VNI cannot drift apart.** They disagree about which key means
//!   *circumflex*; they agree completely about what a circumflex is, because
//!   the answer is this file and neither of them owns a copy of it.
//!
//! Rendering happens once, at the end, in [`super::unicode`].
//!
//! # The raw keys, and why they are kept
//!
//! [`Syllable::raw`] records the characters that were typed, so that a word
//! which turns out not to be Vietnamese can be handed back exactly as it was
//! entered — `where` rather than `ưhere`. See
//! [`spell_check`](super::VietnameseConfig::spell_check) for when that applies
//! and [`Syllable::raw_is_trustworthy`] for the one case where it does not.
//! Nothing else reads it, it never leaves the syllable, and it dies with the
//! syllable at the next word boundary.

use super::rules;
use super::tone::{self, TonePlacement};
use super::unicode;
use super::word_boundary;

/// A diacritic that changes which *letter* this is, as opposed to which tone.
///
/// Vietnamese treats these as separate letters of the alphabet — `ă` is not an
/// `a` with an accent, it is its own letter — which is precisely why they are
/// modelled apart from [`Tone`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mark {
    /// `â`, `ê`, `ô`.
    Circumflex,
    /// `ă`.
    Breve,
    /// `ơ`, `ư`.
    Horn,
    /// `đ`. The only one that lands on a consonant.
    Stroke,
}

/// One of the six Vietnamese tones.
///
/// [`Tone::Level`] (*ngang*) is a real tone that happens to be written with no
/// mark at all, which is why it is a variant rather than `Option::None` — "this
/// syllable has no tone" and "this syllable has not been given a tone yet" are
/// the same state in Vietnamese, and pretending otherwise invites an
/// `Option<Option<Tone>>`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Tone {
    /// *ngang* — `ma`.
    #[default]
    Level,
    /// *sắc* — `má`.
    Acute,
    /// *huyền* — `mà`.
    Grave,
    /// *hỏi* — `mả`.
    HookAbove,
    /// *ngã* — `mã`.
    Tilde,
    /// *nặng* — `mạ`.
    UnderDot,
}

/// One letter of a syllable.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Letter {
    /// The undecorated ASCII letter: `a`, `e`, `d`, `n`. Always lowercase —
    /// case lives in [`Letter::upper`], so every rule in [`super::rules`] and
    /// [`super::tone`] can be written once instead of twice.
    pub base: char,
    pub mark: Option<Mark>,
    pub upper: bool,
    /// The physical key that produced this whole marked letter, rather than
    /// merely decorating a letter already in the syllable. It is how a second
    /// matching source cancels `ư` back to literal `w` without treating its
    /// synthetic `u` as independently typed.
    source: Option<(Mark, char)>,
    /// The case this letter had before a **re-typed** modifier key overrode it
    /// — see `Syllable::retypes_last_letter`.
    ///
    /// `Some` implies [`Letter::mark`] is `Some`, because it is written and
    /// cleared with the override itself. Taking that mark off puts the user's
    /// own keystroke back rather than leaving a case no key is still asking
    /// for: `Ddd` is `Dd`, not `dd`.
    overridden_case: Option<bool>,
}

impl Letter {
    pub fn new(base: char, upper: bool) -> Letter {
        Letter {
            base: base.to_ascii_lowercase(),
            mark: None,
            upper,
            source: None,
            overridden_case: None,
        }
    }

    pub fn with_mark(mut self, mark: Option<Mark>) -> Letter {
        self.mark = mark;
        self
    }

    fn with_source(mut self, mark: Option<Mark>, source: Option<char>) -> Letter {
        self.source = mark.zip(source);
        self
    }

    /// The key that made this whole letter, when `source` is that same key
    /// again asking for `mark`.
    ///
    /// The answer is the character *as it was typed*, not the character now
    /// being pressed, so undoing `Wi` + `w` puts the user's own `W` back.
    fn self_mark_source(&self, mark: Mark, source: char) -> Option<char> {
        self.source
            .filter(|(source_mark, key)| *source_mark == mark && key.eq_ignore_ascii_case(&source))
            .map(|(_, key)| key)
    }
}

/// What applying a mark did.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MarkOutcome {
    /// The mark landed on a letter.
    Applied,
    /// A one-key transformed letter was cancelled, so the whole letter went
    /// away before the caller types this key literally (`ww` becomes `w`).
    SourceCancelled,
    /// A one-key transformed letter that is no longer the last letter was put
    /// back as the key that made it, in the place it occupies. The caller still
    /// types the current key as itself, at the end, because the two presses are
    /// not one gesture — `windo` (`ưindo`) plus `w` is `window`, not `windo`.
    SourceRestored,
    /// The letter already had that mark, so it was taken off again — the
    /// second `aa` in `aaa`, the second `w` in `aww`. The caller then types the
    /// key literally, which is what makes the undo produce `aa` rather than
    /// `a`.
    Reverted,
    /// No letter in this syllable can carry that mark. The key was never a mark
    /// key here, so it must reach the user as itself.
    NoTarget,
}

/// The syllable currently being composed.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Syllable {
    letters: Vec<Letter>,
    tone: Tone,
    raw: String,
    raw_trusted: bool,
}

impl Default for Syllable {
    fn default() -> Syllable {
        Syllable {
            letters: Vec::new(),
            tone: Tone::Level,
            raw: String::new(),
            raw_trusted: true,
        }
    }
}

impl Syllable {
    pub fn new() -> Syllable {
        Syllable::default()
    }

    pub fn is_empty(&self) -> bool {
        self.letters.is_empty()
    }

    pub fn letters(&self) -> &[Letter] {
        &self.letters
    }

    pub fn tone(&self) -> Tone {
        self.tone
    }

    /// The characters typed into this syllable, in order.
    pub fn raw(&self) -> &str {
        &self.raw
    }

    /// Whether [`Syllable::raw`] still describes what is on screen.
    ///
    /// A backspace removes a rendered letter, which may have taken any number
    /// of raw keys to produce (`đ` took two; `ế` took three). Reconstructing
    /// which keys to drop would be guesswork, so the raw record is simply
    /// abandoned instead — the spell-check fallback stops applying for the rest
    /// of this syllable, and the rendered text is committed as it stands. That
    /// is the conservative direction: at worst the user keeps a syllable they
    /// can see, rather than having it silently swapped for something else.
    pub fn raw_is_trustworthy(&self) -> bool {
        self.raw_trusted
    }

    /// Stop claiming that [`Syllable::raw`] describes the screen.
    ///
    /// Called when the user *undoes* a diacritic or a tone — `aww`, `cass`,
    /// `a11`. An undo is an explicit statement that the literal reading is what
    /// was wanted, so the rendered text stands from then on and the spell-check
    /// fallback stops second-guessing it. Without this, `aww` would come back as
    /// `aww` rather than `aw`, because `aw` is not a Vietnamese syllable and the
    /// fallback would helpfully undo the undo.
    pub fn distrust_raw(&mut self) {
        self.raw_trusted = false;
    }

    /// Record a key as having been typed into this syllable.
    ///
    /// Called for every key the engine consumes, including the ones that turn
    /// into marks and tones rather than letters.
    pub fn record_key(&mut self, key: char) {
        self.raw.push(key);
    }

    pub fn push_letter(&mut self, base: char, upper: bool) {
        self.letters.push(Letter::new(base, upper));
    }

    pub fn push_marked_letter(
        &mut self,
        base: char,
        mark: Option<Mark>,
        upper: bool,
        source: Option<char>,
    ) {
        self.letters.push(
            Letter::new(base, upper)
                .with_mark(mark)
                .with_source(mark, source),
        );
    }

    /// Remove the last letter, as a backspace would.
    ///
    /// Also drops the tone when the tone was being drawn on that letter, which
    /// is what makes this match what the user sees: every letter renders as
    /// exactly one visible character, so removing the last letter removes the
    /// last visible character. `hoà` backspaces to `ho` (the `à` went);
    /// `tiếng` backspaces to `tiến` (the `g` went, and the tone was never on
    /// it).
    pub fn pop_letter(&mut self, style: TonePlacement) -> bool {
        let Some(last) = self.letters.len().checked_sub(1) else {
            return false;
        };
        if tone::placement(&self.letters, style) == Some(last) {
            self.tone = Tone::Level;
        }
        self.letters.truncate(last);
        self.distrust_raw();
        true
    }

    pub fn set_tone(&mut self, tone: Tone) {
        self.tone = tone;
    }

    pub fn clear_tone(&mut self) {
        self.tone = Tone::Level;
    }

    /// Which letter a mark would land on, if any.
    ///
    /// The rightmost letter of the **nucleus** that could carry it, so that
    /// `dduwow` puts the second horn on the `o` rather than back on the `ư`.
    /// [`Mark::Stroke`] is the exception in every way: it lands on `d`, which
    /// is a consonant and sits in the initial.
    pub fn mark_target(&self, mark: Mark) -> Option<usize> {
        if mark == Mark::Stroke {
            return match self.letters.first() {
                Some(letter) if letter.base == 'd' => Some(0),
                _ => None,
            };
        }
        let nucleus = rules::parts(&self.letters).nucleus;
        self.letters[nucleus.clone()]
            .iter()
            .rposition(|letter| unicode::can_take(letter.base, mark))
            .map(|at| nucleus.start + at)
    }

    /// The `u` and `o` of a bare `uo`, at the end of the nucleus.
    ///
    /// `ươ` is the most common two-diacritic nucleus in the language —
    /// `đường`, `người`, `nước`, `được`, `trường` — and both Telex and VNI let
    /// one key mark the pair, so `uow` and `uo7` give `ươ` rather than `uơ`.
    /// The pair only counts when neither vowel is already marked, which is what
    /// keeps `uwow` (`ư` then `ơ`, marked one at a time) working identically.
    fn bare_uo_pair(&self) -> Option<(usize, usize)> {
        let nucleus = rules::parts(&self.letters).nucleus;
        if nucleus.len() < 2 {
            return None;
        }
        let second = nucleus.end - 1;
        let first = second - 1;
        let matches = self.letters[first].base == 'u'
            && self.letters[first].mark.is_none()
            && self.letters[second].base == 'o'
            && self.letters[second].mark.is_none();
        matches.then_some((first, second))
    }

    /// Whether `source` is another press of the very letter it is marking.
    ///
    /// Two things have to hold, and both are load-bearing:
    ///
    /// - **The key spells the letter.** `dd`, `aa`, `ee` and `oo` are the whole
    ///   set; Telex's `w` and every VNI digit fail here, which is what keeps
    ///   `Aw` an `Ă` and `D9` a `Đ`. A shifted `W` is how a typist reaches a
    ///   modifier in a caps-locked word, not an opinion about the vowel.
    /// - **Nothing was typed since.** The two presses are one gesture only when
    ///   the target is still the last letter — the same adjacency
    ///   `Syllable::undo_self_mark` uses. A stroke that reaches back over a
    ///   word (`Did`, `dungd`) is a modifier applied from a distance, and the
    ///   case of that letter was settled when the user typed it.
    fn retypes_last_letter(&self, at: usize, source: char) -> bool {
        at + 1 == self.letters.len()
            && self
                .letters
                .get(at)
                .is_some_and(|letter| source.eq_ignore_ascii_case(&letter.base))
    }

    /// Put `mark` on the letter that should have it, or take it off again.
    ///
    /// The single place either input scheme's diacritic keys end up, which is
    /// what "Telex and VNI share one semantic engine" means concretely: `aa`,
    /// `a6`, `w`, `7` and `9` all arrive here as a [`Mark`], and every rule
    /// about where a mark may live is stated once, below.
    pub fn apply_mark(&mut self, mark: Mark) -> MarkOutcome {
        self.apply_mark_from(mark, None)
    }

    /// As [`Syllable::apply_mark`], retaining the physical key that supplied
    /// the mark. The engine uses this to distinguish a literal `u` decorated by
    /// `w` (`uww` → `uw`) from `ư` that the first `w` created by itself (`ww` →
    /// `w`).
    ///
    /// The source is also what decides the marked letter's **case**, in the one
    /// situation where the key has an opinion about it: see
    /// `Syllable::retypes_last_letter`, and this module's parent docs for the
    /// rule stated whole.
    pub fn apply_mark_from(&mut self, mark: Mark, source: Option<char>) -> MarkOutcome {
        if mark == Mark::Horn
            && let Some((first, second)) = self.bare_uo_pair()
        {
            self.letters[first].mark = Some(Mark::Horn);
            self.letters[second].mark = Some(Mark::Horn);
            return MarkOutcome::Applied;
        }

        let Some(at) = self.mark_target(mark) else {
            return MarkOutcome::NoTarget;
        };
        if self.letters[at].mark == Some(mark) {
            if let Some(source) = source
                && let Some(typed) = self.letters[at].self_mark_source(mark, source)
            {
                return self.undo_self_mark(at, typed);
            }
            let letter = &mut self.letters[at];
            letter.mark = None;
            // The key that overrode the case has just been taken back, so the
            // case goes back with it.
            if let Some(previous) = letter.overridden_case.take() {
                letter.upper = previous;
            }
            MarkOutcome::Reverted
        } else {
            let retyped = source.is_some_and(|source| self.retypes_last_letter(at, source));
            let letter = &mut self.letters[at];
            // A different mark is replaced rather than refused: `oow` is `ô`
            // corrected to `ơ`, which is a typist changing their mind, not an
            // error. Its old source no longer describes the letter.
            letter.mark = Some(mark);
            letter.source = None;
            match source.filter(|_| retyped) {
                // The latest press of this letter is the latest word on its
                // case: `dD` is `Đ` and `Dd` is `đ`.
                Some(source) => {
                    letter.overridden_case = Some(letter.upper);
                    letter.upper = source.is_ascii_uppercase();
                }
                // A modifier that is not this letter says nothing about it, and
                // a replaced mark carries no earlier override forward.
                None => letter.overridden_case = None,
            }
            MarkOutcome::Applied
        }
    }

    /// Take back the whole letter one source key made, now that the same key
    /// has been pressed again.
    ///
    /// # Adjacency decides which shape this takes
    ///
    /// The two presses are **one gesture** only when nothing was typed between
    /// them, and that is the whole difference between `ww` and `window`:
    ///
    /// - `ww` — the `ư` is still the last letter, so the pair collapses to the
    ///   single literal `w` the caller is about to type. Two keys, one letter,
    ///   which is the undo rule every other modifier obeys (`aaa` → `aa`).
    /// - `w i n d o w` — four letters arrived after the `ư`, so the second `w`
    ///   is not correcting the first, it is the next letter of a word. The `ư`
    ///   goes back to being the `w` that made it, **where it stands**, and the
    ///   caller types the current key at the end: `window`. Appending the
    ///   restored key instead is what used to produce `indow`.
    ///
    /// Both readings are derived from provenance alone — which key made this
    /// letter, and how many letters have arrived since — so no word list and no
    /// inspection of the rendered text is involved.
    fn undo_self_mark(&mut self, at: usize, typed: char) -> MarkOutcome {
        let adjacent = at + 1 == self.letters.len();
        // A bracket shortcut cannot stand in the syllable as itself, so its
        // source can only ever be taken back the collapsing way.
        if adjacent || !word_boundary::is_syllable_letter(typed) {
            self.letters.remove(at);
            return MarkOutcome::SourceCancelled;
        }
        self.letters[at] = Letter::new(typed, typed.is_ascii_uppercase());
        MarkOutcome::SourceRestored
    }

    /// Cancel the whole letter a direct marked-letter key made, if this is that
    /// same key again with nothing typed since.
    ///
    /// It asks the **last** letter rather than [`Syllable::mark_target`],
    /// because a direct marked letter is pushed at the end and a mark target is
    /// a question about the nucleus — two different letters as soon as the
    /// syllable stops looking Vietnamese. `windoư` has nucleus `i`, which can
    /// carry no horn at all, so the target was `None` and a second `w` used to
    /// append a second `ư`. Bracket shortcuts are direct marked letters too and
    /// share the rule.
    pub fn cancel_self_mark(&mut self, mark: Mark, source: char) -> bool {
        let Some(at) = self.letters.len().checked_sub(1) else {
            return false;
        };
        if self.letters[at].self_mark_source(mark, source).is_some() {
            self.letters.remove(at);
            true
        } else {
            false
        }
    }

    /// The syllable as the user should see it, in NFC.
    pub fn render(&self, style: TonePlacement) -> String {
        let carrier = if self.tone == Tone::Level {
            None
        } else {
            tone::placement(&self.letters, style)
        };
        let mut text = String::with_capacity(self.letters.len() * 3);
        for (at, letter) in self.letters.iter().enumerate() {
            let tone = if Some(at) == carrier {
                self.tone
            } else {
                Tone::Level
            };
            text.push_str(&unicode::render_letter(
                letter.base,
                letter.mark,
                letter.upper,
                tone,
            ));
        }
        text
    }

    /// Whether these letters could be a Vietnamese syllable. See
    /// [`rules::is_valid_syllable`].
    pub fn is_valid(&self) -> bool {
        rules::is_valid_syllable(&self.letters)
    }

    pub fn clear(&mut self) {
        self.letters.clear();
        self.tone = Tone::Level;
        self.raw.clear();
        self.raw_trusted = true;
    }
}

#[cfg(test)]
mod tests {
    use super::{Letter, Mark, MarkOutcome, Syllable, Tone};
    use crate::languages::vietnamese::tone::TonePlacement;

    const MODERN: TonePlacement = TonePlacement::Modern;

    fn make(spelling: &str) -> Syllable {
        let mut syllable = Syllable::new();
        for ch in spelling.chars() {
            syllable.push_letter(ch, ch.is_uppercase());
            syllable.record_key(ch);
        }
        syllable
    }

    #[test]
    fn a_new_syllable_is_empty_and_level() {
        let syllable = Syllable::new();
        assert!(syllable.is_empty());
        assert_eq!(syllable.tone(), Tone::Level);
        assert_eq!(syllable.render(MODERN), "");
        assert!(!syllable.is_valid());
        assert!(syllable.raw_is_trustworthy());
    }

    /// The point of the whole file: the tone is a fact about the syllable, so a
    /// letter arriving afterwards changes where it is drawn.
    #[test]
    fn a_final_consonant_moves_the_tone_without_touching_it() {
        let mut syllable = make("toa");
        syllable.set_tone(Tone::Acute);
        assert_eq!(syllable.render(MODERN), "toá");

        syllable.push_letter('n', false);
        assert_eq!(syllable.render(MODERN), "toán");
        assert_eq!(syllable.tone(), Tone::Acute);
    }

    #[test]
    fn a_mark_lands_on_the_rightmost_nucleus_letter_that_can_take_it() {
        let mut syllable = make("tie");
        assert_eq!(syllable.apply_mark(Mark::Circumflex), MarkOutcome::Applied);
        assert_eq!(syllable.render(MODERN), "tiê");

        // The horn goes on the `o`, not back on the `ư`.
        let mut syllable = make("duo");
        assert_eq!(syllable.apply_mark(Mark::Horn), MarkOutcome::Applied);
        assert_eq!(syllable.render(MODERN), "dươ");
    }

    #[test]
    fn a_second_application_takes_the_mark_off_again() {
        let mut syllable = make("ta");
        assert_eq!(syllable.apply_mark(Mark::Circumflex), MarkOutcome::Applied);
        assert_eq!(syllable.render(MODERN), "tâ");
        assert_eq!(syllable.apply_mark(Mark::Circumflex), MarkOutcome::Reverted);
        assert_eq!(syllable.render(MODERN), "ta");
    }

    #[test]
    fn only_a_repeated_self_marked_source_removes_the_whole_letter() {
        let mut sourced = Syllable::new();
        sourced.push_marked_letter('u', Some(Mark::Horn), false, Some('w'));
        assert_eq!(
            sourced.apply_mark_from(Mark::Horn, Some('W')),
            MarkOutcome::SourceCancelled
        );
        assert_eq!(sourced.render(MODERN), "");

        let mut literal = make("u");
        assert_eq!(
            literal.apply_mark_from(Mark::Horn, Some('w')),
            MarkOutcome::Applied
        );
        assert_eq!(
            literal.apply_mark_from(Mark::Horn, Some('w')),
            MarkOutcome::Reverted
        );
        assert_eq!(literal.render(MODERN), "u");
    }

    /// With letters typed in between, the two presses are not one gesture: the
    /// source key goes back where its letter stands and the caller still types
    /// the current key. Removing the letter and letting the caller append the
    /// literal is what turned `ưindo` + `w` into `indow`.
    #[test]
    fn a_repeat_with_letters_in_between_restores_the_source_key_in_place() {
        let mut syllable = Syllable::new();
        syllable.push_marked_letter('u', Some(Mark::Horn), false, Some('w'));
        syllable.push_letter('i', false);
        assert_eq!(syllable.render(MODERN), "ưi");
        assert_eq!(
            syllable.apply_mark_from(Mark::Horn, Some('w')),
            MarkOutcome::SourceRestored
        );
        assert_eq!(syllable.render(MODERN), "wi");

        // What went back is an ordinary letter with no provenance, so a third
        // `w` has nothing of its own left to cancel.
        assert_eq!(
            syllable.apply_mark_from(Mark::Horn, Some('w')),
            MarkOutcome::NoTarget
        );
    }

    /// The key that is put back is the one the user typed, not the one they are
    /// typing now — the two differ in case whenever either was shifted.
    #[test]
    fn the_restored_key_keeps_the_case_it_was_typed_with() {
        let mut syllable = Syllable::new();
        syllable.push_marked_letter('u', Some(Mark::Horn), true, Some('W'));
        syllable.push_letter('i', false);
        assert_eq!(
            syllable.apply_mark_from(Mark::Horn, Some('w')),
            MarkOutcome::SourceRestored
        );
        assert_eq!(syllable.render(MODERN), "Wi");
    }

    /// A direct marked letter is pushed at the end, so its cancel asks the last
    /// letter. Asking [`Syllable::mark_target`] instead misses it entirely once
    /// the syllable stops looking Vietnamese: `windoư` has nucleus `i`, which
    /// can carry no horn at all, so there is no target to ask.
    #[test]
    fn a_direct_marked_letter_is_cancelled_from_the_end_not_from_the_mark_target() {
        let mut syllable = make("windo");
        syllable.push_marked_letter('u', Some(Mark::Horn), false, Some('w'));
        assert_eq!(syllable.render(MODERN), "windoư");
        assert_eq!(syllable.mark_target(Mark::Horn), None);
        assert!(syllable.cancel_self_mark(Mark::Horn, 'w'));
        assert_eq!(syllable.render(MODERN), "windo");

        // A letter typed after it ends the gesture, so the same key is a new
        // letter rather than an undo.
        let mut later = make("windo");
        later.push_marked_letter('u', Some(Mark::Horn), false, Some('w'));
        later.push_letter('n', false);
        assert!(!later.cancel_self_mark(Mark::Horn, 'w'));
        assert_eq!(later.render(MODERN), "windoưn");
    }

    #[test]
    fn a_mark_no_letter_can_carry_has_no_target() {
        let mut syllable = make("ti");
        assert_eq!(syllable.apply_mark(Mark::Circumflex), MarkOutcome::NoTarget);
        assert_eq!(syllable.apply_mark(Mark::Horn), MarkOutcome::NoTarget);
        assert_eq!(syllable.apply_mark(Mark::Breve), MarkOutcome::NoTarget);
        assert_eq!(syllable.render(MODERN), "ti");

        // A stroke needs a `d`, and needs it first.
        let mut syllable = make("ba");
        assert_eq!(syllable.apply_mark(Mark::Stroke), MarkOutcome::NoTarget);
        let mut syllable = make("ad");
        assert_eq!(syllable.apply_mark(Mark::Stroke), MarkOutcome::NoTarget);
    }

    #[test]
    fn a_different_mark_replaces_rather_than_being_refused() {
        let mut syllable = make("to");
        assert_eq!(syllable.apply_mark(Mark::Circumflex), MarkOutcome::Applied);
        assert_eq!(syllable.render(MODERN), "tô");
        assert_eq!(syllable.apply_mark(Mark::Horn), MarkOutcome::Applied);
        assert_eq!(syllable.render(MODERN), "tơ");
    }

    /// `ươ` in one key, because it is the most common two-diacritic nucleus in
    /// the language.
    #[test]
    fn one_horn_marks_a_bare_uo_pair() {
        let mut syllable = make("duo");
        assert_eq!(syllable.apply_mark(Mark::Horn), MarkOutcome::Applied);
        assert_eq!(syllable.render(MODERN), "dươ");

        // Marked one at a time, the pair rule stays out of the way and the
        // result is identical.
        let mut syllable = make("du");
        assert_eq!(syllable.apply_mark(Mark::Horn), MarkOutcome::Applied);
        syllable.push_letter('o', false);
        assert_eq!(syllable.apply_mark(Mark::Horn), MarkOutcome::Applied);
        assert_eq!(syllable.render(MODERN), "dươ");
    }

    #[test]
    fn the_stroke_lands_on_an_initial_d_in_either_case() {
        let mut syllable = make("d");
        assert_eq!(syllable.apply_mark(Mark::Stroke), MarkOutcome::Applied);
        assert_eq!(syllable.render(MODERN), "đ");
        assert_eq!(syllable.apply_mark(Mark::Stroke), MarkOutcome::Reverted);
        assert_eq!(syllable.render(MODERN), "d");

        let mut syllable = make("D");
        assert_eq!(syllable.apply_mark(Mark::Stroke), MarkOutcome::Applied);
        assert_eq!(syllable.render(MODERN), "Đ");
    }

    /// Backspace removes one visible character, which is the same thing as one
    /// letter — including when the tone was riding on it.
    #[test]
    fn popping_a_letter_matches_what_the_user_sees() {
        let mut syllable = make("hoa");
        syllable.set_tone(Tone::Grave);
        assert_eq!(syllable.render(MODERN), "hoà");
        assert!(syllable.pop_letter(MODERN));
        assert_eq!(syllable.render(MODERN), "ho");
        assert_eq!(syllable.tone(), Tone::Level);

        let mut syllable = make("tie");
        syllable.apply_mark(Mark::Circumflex);
        syllable.push_letter('n', false);
        syllable.push_letter('g', false);
        syllable.set_tone(Tone::Acute);
        assert_eq!(syllable.render(MODERN), "tiếng");
        assert!(syllable.pop_letter(MODERN));
        assert_eq!(syllable.render(MODERN), "tiến");
        assert_eq!(syllable.tone(), Tone::Acute);
    }

    #[test]
    fn popping_an_empty_syllable_reports_that_it_did_nothing() {
        let mut syllable = Syllable::new();
        assert!(!syllable.pop_letter(MODERN));
        assert!(syllable.is_empty());
    }

    /// The raw record only claims to describe the screen until a backspace
    /// makes it a guess.
    #[test]
    fn a_backspace_abandons_the_raw_record() {
        let mut syllable = make("hoa");
        assert!(syllable.raw_is_trustworthy());
        assert_eq!(syllable.raw(), "hoa");
        syllable.pop_letter(MODERN);
        assert!(!syllable.raw_is_trustworthy());
    }

    #[test]
    fn record_key_keeps_the_keys_not_the_letters() {
        let mut syllable = Syllable::new();
        for key in "tieengs".chars() {
            syllable.record_key(key);
        }
        assert_eq!(syllable.raw(), "tieengs");
        // Nothing was rendered: recording a key is not typing it.
        assert!(syllable.is_empty());
    }

    #[test]
    fn clearing_returns_it_to_a_fresh_syllable() {
        let mut syllable = make("hoa");
        syllable.set_tone(Tone::Grave);
        syllable.pop_letter(MODERN);
        syllable.clear();
        assert_eq!(syllable, Syllable::new());
    }

    #[test]
    fn case_lives_on_the_letter_and_survives_every_mark() {
        let mut syllable = Syllable::new();
        for (base, upper) in [('V', true), ('I', true), ('E', true)] {
            syllable.push_letter(base, upper);
            assert_eq!(
                base.to_ascii_lowercase(),
                syllable.letters().last().unwrap().base
            );
            let _ = upper;
        }
        syllable.apply_mark(Mark::Circumflex);
        syllable.push_letter('T', true);
        syllable.set_tone(Tone::UnderDot);
        assert_eq!(syllable.render(MODERN), "VIỆT");
    }

    #[test]
    fn a_letter_lowercases_its_base_and_remembers_the_case_separately() {
        let letter = Letter::new('E', true);
        assert_eq!(letter.base, 'e');
        assert!(letter.upper);
        assert_eq!(letter.mark, None);
    }
}
