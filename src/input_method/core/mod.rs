//! The vocabulary every language engine and every OS host shares.
//!
//! Four small data types and one trait. A keystroke arrives as a [`KeyEvent`],
//! an engine answers with an [`EngineResult`] carrying [`EngineAction`]s, and
//! whatever is mid-composition is a [`Composition`] with, for the conversion
//! languages, a [`CandidateList`] beside it.
//!
//! Nothing here knows about Vietnamese, and nothing here knows about AppKit,
//! TSF or IBus. That is the whole point of the layer: it is the only thing a
//! macOS input-method bundle and a Windows DLL would both link against.

pub mod action;
pub mod candidate;
pub mod composition;
pub mod engine;
pub mod event;
pub mod language;

pub use self::action::EngineAction;
pub use self::candidate::{Candidate, CandidateList};
// `truncate_graphemes` is what a host needs to perform `DeleteBackward` and
// `ReplaceBeforeCursor`; only the test host does today. Remove this the round a
// real OS host lands.
#[allow(unused_imports)]
pub use self::composition::{Composition, grapheme_count, truncate_graphemes};
pub use self::engine::{EngineResult, LanguageEngine};
pub use self::event::{Key, KeyEvent, Modifiers};
pub use self::language::LanguageId;
