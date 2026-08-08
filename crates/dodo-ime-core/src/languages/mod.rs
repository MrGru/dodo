//! One module per language engine.
//!
//! Vietnamese is the only one round 1 ships. Korean, Japanese and Chinese would
//! be siblings here, each implementing
//! [`LanguageEngine`](super::core::LanguageEngine) and sharing nothing but that
//! trait — see [`super::core::engine`] for the check that the trait is wide
//! enough to carry them.

pub mod vietnamese;
