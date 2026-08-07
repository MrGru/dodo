//! What an engine asks its host to do about a keystroke.
//!
//! An engine changes nothing itself. It returns a list of these, in order, and
//! the host performs them against whatever it has — a marked-text range on
//! macOS, a TSF composition on Windows, a preedit string on IBus, or, in the
//! tests, a `String`.
//!
//! # Two ways to put text on screen, and why both exist
//!
//! A real input method shows a *composition*: underlined text the application
//! knows is provisional. That is [`EngineAction::SetComposition`] and
//! [`EngineAction::CommitComposition`], and it is what every OS host will use.
//!
//! But some places cannot show marked text at all — a host with no TSF sink, a
//! terminal, an application that rejects the protocol. There the only honest
//! move is to type the text for real and rewrite it as the syllable evolves,
//! which is [`EngineAction::ReplaceBeforeCursor`] and
//! [`EngineAction::DeleteBackward`]. Both are real paths through the Vietnamese
//! engine, selected by
//! [`OutputMode`](crate::input_method::languages::vietnamese::OutputMode) — see
//! its docs for the trade.
//!
//! # Spans are graphemes
//!
//! `DeleteBackward(1)` after `ế` deletes the whole letter, not its tone mark,
//! whichever normalization form the document happens to hold. See
//! [`grapheme_count`](super::grapheme_count).

/// One instruction to the host.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum EngineAction {
    /// The engine wants nothing to do with this key: let the application have
    /// it exactly as pressed.
    ///
    /// This is also every fallback's destination. Losing a keystroke is the
    /// worst failure an input method can have, so anything the engine cannot
    /// represent ends here rather than being dropped.
    PassThrough,

    /// Insert text at the caret, as if typed.
    InsertText(String),

    /// Delete this many graphemes immediately before the caret.
    DeleteBackward(usize),

    /// Replace the `grapheme_count` graphemes before the caret with `text`.
    ///
    /// The direct-typing counterpart of `SetComposition`: it is how a syllable
    /// grows in a host that cannot underline provisional text.
    ReplaceBeforeCursor { grapheme_count: usize, text: String },

    /// Show `text` as provisional, underlined text with the caret at `cursor`
    /// and, for clause conversion, `selection` marking the active span.
    ///
    /// Positions are grapheme offsets into `text`.
    SetComposition {
        text: String,
        cursor: usize,
        selection: Option<std::ops::Range<usize>>,
    },

    /// Turn the current composition into real text in the document.
    CommitComposition,

    /// Throw the current composition away without inserting anything.
    ClearComposition,

    /// Display the candidate list carried on the same
    /// [`EngineResult`](super::EngineResult).
    ///
    /// Never emitted in round 1 — Vietnamese has no candidates, and the three
    /// engines that do (Korean is the exception; Japanese and Chinese are not)
    /// are later rounds. It is here so that the host protocol is settled before
    /// those engines are written, rather than being widened under them.
    ShowCandidates,

    /// Hide the candidate list.
    ///
    /// Unemitted in round 1 for the same reason as [`EngineAction::ShowCandidates`].
    HideCandidates,
}

impl EngineAction {
    /// Whether this action leaves the key for the application.
    ///
    /// [`EngineResult::handled`](super::EngineResult::handled) is derived from
    /// this rather than stated separately, so the two can never disagree.
    pub fn passes_through(&self) -> bool {
        matches!(self, EngineAction::PassThrough)
    }

    /// The smallest action that turns `before` graphemes of already-typed text
    /// into `text`.
    ///
    /// Direct-typing hosts go through here so the three shapes — first
    /// insertion, rewrite, deletion — are chosen in one place instead of at
    /// every call site.
    pub fn replacement(before: usize, text: String) -> Option<EngineAction> {
        match (before, text.is_empty()) {
            (0, true) => None,
            (0, false) => Some(EngineAction::InsertText(text)),
            (n, true) => Some(EngineAction::DeleteBackward(n)),
            (n, false) => Some(EngineAction::ReplaceBeforeCursor {
                grapheme_count: n,
                text,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::EngineAction;

    #[test]
    fn only_pass_through_leaves_the_key_to_the_application() {
        assert!(EngineAction::PassThrough.passes_through());
        assert!(!EngineAction::CommitComposition.passes_through());
        assert!(!EngineAction::InsertText("a".into()).passes_through());
    }

    #[test]
    fn replacement_picks_the_smallest_shape() {
        assert_eq!(EngineAction::replacement(0, String::new()), None);
        assert_eq!(
            EngineAction::replacement(0, "ti".into()),
            Some(EngineAction::InsertText("ti".into()))
        );
        assert_eq!(
            EngineAction::replacement(3, String::new()),
            Some(EngineAction::DeleteBackward(3))
        );
        assert_eq!(
            EngineAction::replacement(2, "tiế".into()),
            Some(EngineAction::ReplaceBeforeCursor {
                grapheme_count: 2,
                text: "tiế".into(),
            })
        );
    }
}
