//! The one trait in this module, and the result type every engine answers with.
//!
//! [`LanguageEngine`] is a trait because Korean, Japanese and Chinese are a
//! real substitution at this seam: each is a genuinely different algorithm
//! behind an identical engine protocol, and a listener must be able to hold
//! whichever one the user selected without knowing which. Nothing *else* in
//! this crate is a trait — the Telex/VNI split inside Vietnamese is an
//! enum, because those two share a state machine rather than replacing one.
//!
//! # Can this API carry Korean, Japanese and Chinese?
//!
//! Checked on paper before the API was frozen, because discovering the answer
//! is "no" after multiple listeners depend on it is a rewrite. Round 1 implements
//! **none** of these and adds no dictionary; this is a design proof, not a plan
//! of work.
//!
//! ## Korean — jamo to Hangul
//!
//! `ㄱ` `ㅏ` `ㄴ` compose to `간`, and the next consonant may either finish the
//! current syllable block or start the next one, so the composition is
//! continuously rewritten in place. That is [`EngineAction::SetComposition`]
//! per keystroke with a one-syllable composition, and a commit when the block
//! closes — the same shape Vietnamese already uses, with no candidates at all.
//! **Fits as-is.**
//!
//! ## Japanese — romaji to kana to conversion
//!
//! Three layers on one composition. `k` `a` becomes `か` (composition rewrite);
//! Space converts the kana run to kanji and opens a candidate list; the arrow
//! keys walk the candidates; the composition splits into *clauses*, one of them
//! active, and shift-arrow resizes the active clause while the candidate list
//! refills for it. Every piece of that is expressible:
//!
//! - the active clause is [`Composition::selection`], which exists for exactly
//!   this and is why it is not an afterthought;
//! - the candidate list refilling while the composition text does not change is
//!   why [`EngineResult::candidates`] is its own field with `None` meaning
//!   *unchanged*, rather than being folded into an action;
//! - `Space`, `Escape`, `Enter`, `ArrowUp`/`ArrowDown` and `ArrowLeft`/
//!   `ArrowRight` are all distinct [`Key`](super::Key) values, so conversion,
//!   cancel, commit and navigation each have a key to hang off.
//!
//! **Fits as-is.** The one adjustment made for it was giving `Space` its own
//! `Key` variant instead of leaving it as `Key::Character(' ')`; an engine
//! should not have to pattern-match a character to find its conversion key.
//!
//! ## Chinese — pinyin to candidates, paged
//!
//! `zhong` opens a list of dozens of characters shown nine at a time, `-`/`=`
//! or PageUp/PageDown turn the page, a number key picks one from the page, and
//! the chosen text commits while the *rest* of the pinyin stays in composition
//! for the next character. Pagination lives in [`CandidateList`] rather than in
//! each host; [`CandidateList::select_on_page`] is page-relative because that
//! is what a number key means; and partial commit is just a `SetComposition`
//! with the remainder followed by the host inserting the chosen text.
//! [`KeyEvent::digit`](super::KeyEvent::digit) exists so candidate selection
//! and VNI's tone digits read the same field. **Fits as-is.**
//!
//! ## What the check changed
//!
//! Three cheap things, all already in the code above: `Space` as its own key,
//! `candidates` as an independent `Option` on the result, and `page`/
//! `page_size` on the candidate list. Everything else the three engines need
//! was already there for Vietnamese.

use super::{CandidateList, Composition, EngineAction, KeyEvent, LanguageId};

/// What one keystroke did.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct EngineResult {
    /// `false` means the application should see the key. Derived from
    /// `actions` by [`EngineResult::from_actions`], never set independently, so
    /// it cannot contradict them.
    pub handled: bool,
    /// What the host should do, in order.
    pub actions: Vec<EngineAction>,
    /// The new candidate list, or `None` for *unchanged*.
    ///
    /// Deliberately not an action: Japanese refills the candidate list while
    /// the composition text stays put, and Chinese leaves the list alone while
    /// the composition changes. The two move independently, so they are two
    /// channels.
    pub candidates: Option<CandidateList>,
}

impl EngineResult {
    /// The result of doing nothing at all.
    pub fn ignored() -> EngineResult {
        EngineResult {
            handled: false,
            actions: Vec::new(),
            candidates: None,
        }
    }

    /// Wrap a list of actions, deciding `handled` from whether any of them
    /// hands the key back.
    ///
    /// An empty list is *not* handled. An engine that asks the host for nothing
    /// has not dealt with the key, and claiming otherwise is how a keystroke
    /// disappears.
    pub fn from_actions(actions: Vec<EngineAction>) -> EngineResult {
        let handled = !actions.is_empty() && !actions.iter().any(EngineAction::passes_through);
        EngineResult {
            handled,
            actions,
            candidates: None,
        }
    }

    pub fn with_candidates(mut self, candidates: CandidateList) -> EngineResult {
        self.candidates = Some(candidates);
        self
    }
}

/// One language's answer to "what should happen when this key is pressed".
///
/// # Contract
///
/// - `process_key` must return for **every** key it is handed, and a key it
///   does not want must come back as [`EngineAction::PassThrough`]. Returning
///   an empty action list for a key that types something loses the keystroke,
///   which is the worst thing an input method can do.
/// - `process_key` must never panic on user input. There is no keystroke
///   sequence a user can produce that is the engine's business to reject.
/// - `commit` accepts whatever is in flight, `reset` abandons it. A host calls
///   `commit` when focus leaves the field and `reset` when the composition is
///   invalidated from outside (the user clicked elsewhere, the application
///   cleared the field).
/// - Neither may keep any record of what was typed once it has returned. See
///   the module docs on [`crate`].
pub trait LanguageEngine {
    /// Which engine this is. Not a user-facing setting — see [`LanguageId`].
    fn language(&self) -> LanguageId;

    /// Decide what one keystroke does.
    fn process_key(&mut self, event: &KeyEvent) -> EngineResult;

    /// What is currently being composed. Empty when nothing is in flight.
    fn composition(&self) -> &Composition;

    /// Abandon whatever is in flight, inserting nothing.
    fn reset(&mut self) -> EngineResult;

    /// Accept whatever is in flight as final text.
    fn commit(&mut self) -> EngineResult;
}

#[cfg(test)]
mod tests {
    use super::{EngineAction, EngineResult};
    use crate::core::{Candidate, CandidateList};

    #[test]
    fn handled_is_derived_from_the_actions() {
        assert!(EngineResult::from_actions(vec![EngineAction::CommitComposition]).handled);
        assert!(!EngineResult::from_actions(vec![EngineAction::PassThrough]).handled);
        // A commit followed by a pass-through is the word-boundary shape: the
        // syllable lands, and the space still reaches the application.
        assert!(
            !EngineResult::from_actions(vec![
                EngineAction::CommitComposition,
                EngineAction::PassThrough,
            ])
            .handled
        );
        // Doing nothing is not handling it.
        assert!(!EngineResult::from_actions(Vec::new()).handled);
    }

    #[test]
    fn ignoring_a_key_asks_the_host_for_nothing() {
        let result = EngineResult::ignored();
        assert!(!result.handled);
        assert!(result.actions.is_empty());
        assert_eq!(result.candidates, None);
    }

    /// The independence the CJK check turns on: a result can carry a new
    /// candidate list without any composition action, and vice versa.
    #[test]
    fn candidates_travel_beside_the_actions_not_inside_them() {
        let list = CandidateList::new(vec![Candidate::new("中")], 9);
        let result = EngineResult::from_actions(Vec::new()).with_candidates(list.clone());
        assert!(result.actions.is_empty());
        assert_eq!(result.candidates, Some(list));

        let text_only = EngineResult::from_actions(vec![EngineAction::CommitComposition]);
        assert_eq!(text_only.candidates, None);
    }
}
