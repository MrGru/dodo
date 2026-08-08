//! A pretend host, so a key sequence can be replayed without dodo running.
//!
//! An engine only ever returns [`EngineAction`]s; something has to perform them
//! before there is any text to assert on. On macOS that something is
//! InputMethodKit, on Windows a TSF sink — and in a test it is [`Host`], which
//! is forty lines and keeps a `String`.
//!
//! That makes [`type_keys`] the developer-facing simulate path this round owes:
//!
//! ```ignore
//! let mut engine = VietnameseEngine::default();
//! assert_eq!(type_keys(&mut engine, "tieengs"), "tiếng");
//! ```
//!
//! It is a function rather than a small binary on purpose — dodo has no
//! `[[bin]]` and is not getting one (see `AGENTS.md`), and a test helper reaches
//! the same place with no packaging, no argument parsing and no second target to
//! keep compiling.
//!
//! [`Host`] is also the only thing that checks the *ordering* of the action
//! list. A commit that arrives after its pass-through, or a replacement counted
//! in the wrong unit, produces visibly wrong text here and nowhere else.

use super::core::{EngineAction, KeyEvent, LanguageEngine, truncate_graphemes};

/// A document an engine can type into.
#[derive(Debug, Default)]
pub struct Host {
    /// Text the application has actually received.
    pub document: String,
    /// Provisional text the application is showing but has not accepted.
    pub composition: String,
}

impl Host {
    pub fn new() -> Host {
        Host::default()
    }

    /// Perform one engine result.
    ///
    /// `typed` is the character the key would have produced, needed only for
    /// [`EngineAction::PassThrough`] — which is the whole point of that action:
    /// the host still has the original event and types it unchanged.
    pub fn apply(&mut self, actions: &[EngineAction], typed: Option<char>) {
        for action in actions {
            match action {
                EngineAction::PassThrough => {
                    if let Some(ch) = typed {
                        self.document.push(ch);
                    }
                }
                EngineAction::InsertText(text) => self.document.push_str(text),
                EngineAction::DeleteBackward(count) => {
                    self.document = truncate_graphemes(&self.document, *count);
                }
                EngineAction::ReplaceBeforeCursor {
                    grapheme_count,
                    text,
                } => {
                    self.document = truncate_graphemes(&self.document, *grapheme_count);
                    self.document.push_str(text);
                }
                EngineAction::SetComposition { text, .. } => {
                    self.composition = text.clone();
                }
                EngineAction::CommitComposition => {
                    self.document.push_str(&self.composition);
                    self.composition.clear();
                }
                EngineAction::ClearComposition => self.composition.clear(),
                EngineAction::ShowCandidates | EngineAction::HideCandidates => {}
            }
        }
    }

    /// Everything on screen: accepted text plus whatever is still provisional.
    pub fn visible(&self) -> String {
        format!("{}{}", self.document, self.composition)
    }
}

/// Type `keys` into `engine` and return the resulting text.
///
/// Each character becomes one [`KeyEvent`], with space, tab and newline
/// classified as themselves. The engine is committed at the end, so a sequence
/// that stops mid-syllable still yields what the user would see.
pub fn type_keys(engine: &mut dyn LanguageEngine, keys: &str) -> String {
    let mut host = Host::new();
    for key in keys.chars() {
        let event = KeyEvent::character(key);
        let result = engine.process_key(&event);
        host.apply(&result.actions, event.text);
    }
    let result = engine.commit();
    host.apply(&result.actions, None);
    host.visible()
}

/// Type `keys` and return the text *without* committing at the end, so a test
/// can inspect a half-finished composition.
pub fn type_keys_uncommitted(engine: &mut dyn LanguageEngine, keys: &str) -> String {
    let mut host = Host::new();
    for key in keys.chars() {
        let event = KeyEvent::character(key);
        let result = engine.process_key(&event);
        host.apply(&result.actions, event.text);
    }
    host.visible()
}

/// Send one non-printing key — Backspace, Escape, an arrow — to an engine that
/// a [`Host`] is following.
pub fn press(engine: &mut dyn LanguageEngine, host: &mut Host, key: super::core::Key) {
    let event = KeyEvent::special(key);
    let result = engine.process_key(&event);
    host.apply(&result.actions, event.text);
}

#[cfg(test)]
mod tests {
    use super::{Host, type_keys};
    use crate::core::EngineAction;
    use crate::languages::vietnamese::VietnameseEngine;

    #[test]
    fn the_host_performs_actions_in_order() {
        let mut host = Host::new();
        host.apply(
            &[
                EngineAction::SetComposition {
                    text: "tiếng".into(),
                    cursor: 5,
                    selection: None,
                },
                EngineAction::CommitComposition,
                EngineAction::PassThrough,
            ],
            Some(' '),
        );
        assert_eq!(host.document, "tiếng ");
        assert_eq!(host.composition, "");
    }

    #[test]
    fn a_replacement_is_counted_in_visible_characters() {
        let mut host = Host::new();
        host.apply(&[EngineAction::InsertText("tiến".into())], None);
        host.apply(
            &[EngineAction::ReplaceBeforeCursor {
                grapheme_count: 4,
                text: "tiếng".into(),
            }],
            None,
        );
        assert_eq!(host.document, "tiếng");
        host.apply(&[EngineAction::DeleteBackward(2)], None);
        assert_eq!(host.document, "tiế");
    }

    #[test]
    fn a_cleared_composition_leaves_the_document_alone() {
        let mut host = Host::new();
        host.apply(&[EngineAction::InsertText("xin ".into())], None);
        host.apply(
            &[
                EngineAction::SetComposition {
                    text: "chào".into(),
                    cursor: 4,
                    selection: None,
                },
                EngineAction::ClearComposition,
            ],
            None,
        );
        assert_eq!(host.visible(), "xin ");
    }

    /// The helper the rest of the suite is written against.
    #[test]
    fn type_keys_replays_a_sequence() {
        let mut engine = VietnameseEngine::default();
        assert_eq!(type_keys(&mut engine, "tieengs"), "tiếng");
    }
}
