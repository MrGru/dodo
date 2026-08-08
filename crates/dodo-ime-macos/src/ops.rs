//! What the engine asked for, in the calls an `IMKTextInput` client actually
//! has.
//!
//! An [`EngineAction`] is written for three operating systems at once and says
//! nothing about `NSRange`. A [`ClientOp`] is one Objective-C message with its
//! arguments already computed — so [`translate`] is where the whole macOS
//! mapping lives, in ordinary Rust that a unit test can read, and
//! [`client`](crate::client) is left with nothing to decide.
//!
//! # The mapping, and the two entries that are not one-to-one
//!
//! | [`EngineAction`] | [`ClientOp`] |
//! |---|---|
//! | `SetComposition` | `SetMarked`, with the caret converted to UTF-16 |
//! | `CommitComposition` | **two** ops: `ClearMarked`, then `Insert` |
//! | `ClearComposition` | `ClearMarked` |
//! | `InsertText` | `Insert` |
//! | `ReplaceBeforeCursor` | `ReplaceBefore` |
//! | `DeleteBackward` | `ReplaceBefore` with empty text |
//! | `PassThrough` | `PassThrough` |
//! | `ShowCandidates` / `HideCandidates` | nothing — Vietnamese has no candidates |
//!
//! **A commit is two calls, in that order.** The investigation found that
//! committing while marked text was still live left duplicated glyphs, so the
//! preedit is cleared first and the text inserted second. Nothing in
//! InputMethodKit does this in one step.
//!
//! **A commit carries text the action does not.** `EngineAction::CommitComposition`
//! means "accept what you are showing", and only the host knows what that is —
//! it is whatever the last `SetComposition` said. [`Pending`] is that one string,
//! and it is the only thing in this crate that outlives a keystroke.
//!
//! **There is no delete primitive.** `IMKTextInput` has no "backspace n
//! characters"; the header offers only `insertText:replacementRange:`, so
//! deleting is replacing a span with nothing.
//!
//! # `ReplaceBefore` is real and unreachable, and both halves matter
//!
//! It is only ever emitted in [`OutputMode::Direct`](dodo_ime_core::OutputMode),
//! and the macOS host never selects that mode
//! ([`DEFAULT_CONFIG`](crate::DEFAULT_CONFIG)) because every macOS client has a
//! marked-text channel — all six the investigation probed accepted
//! `setMarkedText:`, including the two whose `selectedRange` is `NSNotFound` and
//! for which `ReplaceBefore` is therefore impossible. Composing is better in
//! every way: no intermediate state in the document, no polluted undo history,
//! nothing for a change-listener to fire on.
//!
//! It is implemented anyway, and correctly, because the translation and its
//! UTF-16 arithmetic are the part that would be got wrong under pressure later —
//! and because `ClientOp` is the protocol a Windows or Linux host will be read
//! against.

use dodo_ime_core::EngineAction;

use crate::text::utf16_offset_of_grapheme;

/// One message to send to the `IMKTextInput` client.
///
/// Ranges are already in UTF-16 code units, because that is the only unit an
/// `NSRange` has ever been in.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ClientOp {
    /// `setMarkedText:selectionRange:replacementRange:` at the insertion point.
    ///
    /// `selection` is `(location, length)` **relative to `text`**, not to the
    /// document — `IMKInputSession.h` is explicit about that, and it is why the
    /// caret can be converted here without knowing anything about the client.
    SetMarked {
        text: String,
        selection: (usize, usize),
    },
    /// `setMarkedText:` with an empty string: drop the preedit, insert nothing.
    ClearMarked,
    /// `insertText:replacementRange:` with `{NSNotFound, NSNotFound}` — at the
    /// caret, replacing nothing.
    Insert(String),
    /// `insertText:replacementRange:` over the `graphemes` graphemes before the
    /// caret, which the client must be asked to measure first. Empty `text`
    /// deletes them.
    ReplaceBefore { graphemes: usize, text: String },
    /// Return `NO` from `inputText:…`, so the application types the key itself.
    PassThrough,
}

/// The text the client is currently showing as marked.
///
/// It exists because a commit does not carry its own text, and it is the only
/// state in this crate that survives a keystroke. It holds at most one syllable
/// and is emptied by every commit, every clear and every deactivation — see the
/// privacy note on [`the crate`](crate).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Pending(String);

impl Pending {
    pub fn new() -> Pending {
        Pending::default()
    }

    pub fn text(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Forget everything. Called on deactivation, and after every commit.
    pub fn clear(&mut self) {
        self.0.clear();
    }
}

/// Turn one keystroke's worth of engine actions into client messages.
///
/// `pending` is read *and* written: it tracks what the client is showing, so a
/// later `CommitComposition` knows what to insert.
pub fn translate(actions: &[EngineAction], pending: &mut Pending) -> Vec<ClientOp> {
    let mut ops = Vec::with_capacity(actions.len() + 1);

    for action in actions {
        match action {
            EngineAction::SetComposition {
                text,
                cursor,
                selection,
            } => {
                let (start, end) = match selection {
                    Some(range) => (range.start, range.end),
                    None => (*cursor, *cursor),
                };
                let location = utf16_offset_of_grapheme(text, start);
                let length = utf16_offset_of_grapheme(text, end).saturating_sub(location);
                pending.0 = text.clone();
                ops.push(ClientOp::SetMarked {
                    text: text.clone(),
                    selection: (location, length),
                });
            }

            EngineAction::CommitComposition => {
                // Clearing first is not tidiness: committing over live marked
                // text duplicated glyphs when the investigation tried it.
                ops.push(ClientOp::ClearMarked);
                if !pending.is_empty() {
                    ops.push(ClientOp::Insert(std::mem::take(&mut pending.0)));
                }
                pending.clear();
            }

            EngineAction::ClearComposition => {
                pending.clear();
                ops.push(ClientOp::ClearMarked);
            }

            EngineAction::InsertText(text) => ops.push(ClientOp::Insert(text.clone())),

            EngineAction::DeleteBackward(graphemes) => ops.push(ClientOp::ReplaceBefore {
                graphemes: *graphemes,
                text: String::new(),
            }),

            EngineAction::ReplaceBeforeCursor {
                grapheme_count,
                text,
            } => ops.push(ClientOp::ReplaceBefore {
                graphemes: *grapheme_count,
                text: text.clone(),
            }),

            EngineAction::PassThrough => ops.push(ClientOp::PassThrough),

            // Vietnamese emits neither, and macOS's candidate panel is a later
            // round's problem. Dropping them is not a lost keystroke: they never
            // travel alone, and `Response::handled` refuses to claim a key whose
            // whole action list translated to nothing.
            EngineAction::ShowCandidates | EngineAction::HideCandidates => {}
        }
    }

    ops
}

#[cfg(test)]
mod tests {
    use super::{ClientOp, Pending, translate};
    use dodo_ime_core::EngineAction;

    fn composition(text: &str, cursor: usize) -> EngineAction {
        EngineAction::SetComposition {
            text: text.to_string(),
            cursor,
            selection: None,
        }
    }

    #[test]
    fn a_composition_carries_the_caret_in_utf16_units() {
        let mut pending = Pending::new();
        let ops = translate(&[composition("tiế", 3)], &mut pending);
        assert_eq!(
            ops,
            vec![ClientOp::SetMarked {
                text: "tiế".into(),
                selection: (3, 0),
            }]
        );
        assert_eq!(pending.text(), "tiế");

        // The same three visible characters, decomposed: five units, and the
        // caret belongs at the end of all of them.
        let mut pending = Pending::new();
        let ops = translate(&[composition("tie\u{302}\u{301}", 3)], &mut pending);
        assert_eq!(
            ops,
            vec![ClientOp::SetMarked {
                text: "tie\u{302}\u{301}".into(),
                selection: (5, 0),
            }]
        );
    }

    /// The selection range exists for Japanese clause conversion, which no
    /// engine emits yet. Converting both ends now costs one line and is the
    /// difference between a protocol and a Vietnamese-shaped hole.
    #[test]
    fn a_selection_converts_both_of_its_ends() {
        let mut pending = Pending::new();
        let ops = translate(
            &[EngineAction::SetComposition {
                text: "tie\u{302}\u{301}ng".into(),
                cursor: 5,
                selection: Some(1..3),
            }],
            &mut pending,
        );
        // Graphemes 1..3 are `i` and `ế`: one unit in, three units long.
        assert_eq!(
            ops,
            vec![ClientOp::SetMarked {
                text: "tie\u{302}\u{301}ng".into(),
                selection: (1, 4),
            }]
        );
    }

    /// The commit shape, in the order the investigation measured: clear the
    /// preedit, then insert.
    #[test]
    fn a_commit_clears_the_preedit_before_inserting_it() {
        let mut pending = Pending::new();
        translate(&[composition("tiếng", 5)], &mut pending);
        let ops = translate(&[EngineAction::CommitComposition], &mut pending);
        assert_eq!(
            ops,
            vec![ClientOp::ClearMarked, ClientOp::Insert("tiếng".into())]
        );
        assert!(pending.is_empty(), "a commit forgets what it committed");
    }

    /// A commit with nothing pending still clears the preedit and inserts
    /// nothing — this is the `commitComposition:` a client sends unprompted, and
    /// inserting a stale string there would type a word into a field the user
    /// had already left.
    #[test]
    fn committing_nothing_inserts_nothing() {
        let mut pending = Pending::new();
        let ops = translate(&[EngineAction::CommitComposition], &mut pending);
        assert_eq!(ops, vec![ClientOp::ClearMarked]);
        assert!(pending.is_empty());
    }

    #[test]
    fn a_cleared_composition_forgets_the_text_it_was_showing() {
        let mut pending = Pending::new();
        translate(&[composition("việt", 4)], &mut pending);
        let ops = translate(&[EngineAction::ClearComposition], &mut pending);
        assert_eq!(ops, vec![ClientOp::ClearMarked]);
        assert!(pending.is_empty());

        // And a commit afterwards must not resurrect it.
        let ops = translate(&[EngineAction::CommitComposition], &mut pending);
        assert_eq!(ops, vec![ClientOp::ClearMarked]);
    }

    /// The word-boundary shape, end to end: the syllable is rewritten one last
    /// time, committed, and the space still reaches the application.
    #[test]
    fn the_word_boundary_shape_survives_translation() {
        let mut pending = Pending::new();
        translate(&[composition("tiên", 4)], &mut pending);
        let ops = translate(
            &[
                composition("tiến", 4),
                EngineAction::CommitComposition,
                EngineAction::PassThrough,
            ],
            &mut pending,
        );
        assert_eq!(
            ops,
            vec![
                ClientOp::SetMarked {
                    text: "tiến".into(),
                    selection: (4, 0),
                },
                ClientOp::ClearMarked,
                ClientOp::Insert("tiến".into()),
                ClientOp::PassThrough,
            ]
        );
    }

    /// `IMKTextInput` has no delete call, so a deletion is a replacement with
    /// nothing.
    #[test]
    fn deleting_is_replacing_a_span_with_nothing() {
        let mut pending = Pending::new();
        let ops = translate(&[EngineAction::DeleteBackward(2)], &mut pending);
        assert_eq!(
            ops,
            vec![ClientOp::ReplaceBefore {
                graphemes: 2,
                text: String::new(),
            }]
        );
    }

    #[test]
    fn a_direct_mode_replacement_keeps_its_grapheme_count() {
        let mut pending = Pending::new();
        let ops = translate(
            &[EngineAction::ReplaceBeforeCursor {
                grapheme_count: 4,
                text: "tiến".into(),
            }],
            &mut pending,
        );
        assert_eq!(
            ops,
            vec![ClientOp::ReplaceBefore {
                graphemes: 4,
                text: "tiến".into(),
            }]
        );
        // Direct mode is not a composition: nothing is pending afterwards.
        assert!(pending.is_empty());
    }

    #[test]
    fn candidate_actions_translate_to_nothing_at_all() {
        let mut pending = Pending::new();
        let ops = translate(
            &[EngineAction::ShowCandidates, EngineAction::HideCandidates],
            &mut pending,
        );
        assert!(ops.is_empty());
    }
}
