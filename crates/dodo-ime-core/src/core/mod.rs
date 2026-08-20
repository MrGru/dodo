//! The vocabulary every language engine and platform input listener shares.
//!
//! Four small data types and one trait. A keystroke arrives as a [`KeyEvent`],
//! an engine answers with an [`EngineResult`] carrying [`EngineAction`]s, and
//! whatever is mid-composition is a [`Composition`] with, for the conversion
//! languages, a [`CandidateList`] beside it.
//!
//! Nothing here knows about Vietnamese or any platform input API. That is the
//! whole point of the layer: listeners normalize platform events into this
//! shared vocabulary.

pub mod action;
pub mod candidate;
pub mod composition;
pub mod engine;
pub mod event;
pub mod language;

pub use self::action::EngineAction;
pub use self::candidate::{Candidate, CandidateList};
// Direct-output listeners use `truncate_graphemes` to perform
// `DeleteBackward` and `ReplaceBeforeCursor`.
#[allow(unused_imports)]
pub use self::composition::{Composition, grapheme_count, grapheme_prefix, truncate_graphemes};
pub use self::engine::{EngineResult, LanguageEngine};
pub use self::event::{Key, KeyEvent, Modifiers};
pub use self::language::{ActiveLanguages, LanguageId};
