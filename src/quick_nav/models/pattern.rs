//! Compiling a user-typed pattern without letting it hurt anything.
//!
//! Quick navigation lets the user edit the pattern each detector selects
//! candidates with, which means a keystroke handler runs a **regular expression
//! this program did not write**. Three things could go wrong, and each is closed
//! here rather than at the call sites:
//!
//! 1. **It could be nonsense.** `compile` returns a [`PatternError`] with the
//!    engine's own wording; the settings dialog shows it and the detector falls
//!    back to its built-in default. An unreadable pattern therefore narrows
//!    nothing and disables nothing — see `quick_nav`'s module doc for why that
//!    is the safe end.
//! 2. **It could be slow.** It cannot be, and that is a property of the crate
//!    rather than of this file: `regex` compiles to a finite automaton and does
//!    **no backtracking**, so a match is linear in the length of the input. The
//!    classic `(a+)+$` blow-up has no expression here. The `regex` crate also
//!    rejects look-around and back-references at compile time — the two features
//!    that would require backtracking — so a user cannot reach for them.
//! 3. **It could be enormous.** A pattern is bounded to [`MAX_PATTERN_LEN`]
//!    characters before it is even handed to the engine, and the compiled
//!    program is bounded by [`SIZE_LIMIT`] / [`DFA_SIZE_LIMIT`], so a
//!    pathological but legal expression fails to compile instead of eating
//!    memory. Both limits are generous next to any pattern a person types.
//!
//! Compilation happens **once**, when the settings are loaded or changed, and
//! the compiled program is what the key handler uses — a keystroke never
//! compiles anything.
//!
//! # A pattern is a search, not an anchored match
//!
//! `Regex::is_match` searches, so `curl` matches anywhere in the text. That is
//! the grep convention and the one a person typing a pattern expects. The
//! built-in defaults in [`super::detect`] anchor themselves with `^…$` where
//! being anchored is the point.

use regex::{Regex, RegexBuilder};

use crate::i18n::{Str, quick_nav};

/// The longest pattern accepted, in characters.
///
/// Nothing a person types is near this; it exists so a pasted megabyte never
/// reaches the parser at all.
pub const MAX_PATTERN_LEN: usize = 512;

/// The compiled program's memory ceiling, in bytes. `regex`'s own default is
/// 10 MB, which is far more than a hand-written pattern can need and far more
/// than dodo wants to hand to an untrusted one.
const SIZE_LIMIT: usize = 64 * 1024;
/// The lazy-DFA cache ceiling, in bytes. Same reasoning; the engine falls back
/// to a slower-but-still-linear strategy rather than failing when it is hit.
const DFA_SIZE_LIMIT: usize = 256 * 1024;

/// Why a pattern could not be used.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PatternError {
    /// Longer than [`MAX_PATTERN_LEN`].
    TooLong { length: usize, limit: usize },
    /// The engine refused it. Carries `regex`'s own English message — there is
    /// nothing to translate it with, exactly as with serde_json's and base64's
    /// wording elsewhere in dodo.
    Invalid(String),
}

impl PatternError {
    pub fn message(&self) -> Str {
        match self {
            PatternError::TooLong { length, limit } => quick_nav::Text::PatternTooLong {
                length: *length,
                limit: *limit,
            }
            .into(),
            PatternError::Invalid(detail) => quick_nav::Text::PatternInvalid(detail.clone()).into(),
        }
    }
}

/// Compiles one pattern under the limits above.
///
/// An empty (or whitespace-only) pattern is **not** an error: it means "the user
/// has not set one", and the caller substitutes the detector's default. That is
/// why the signature returns `Result<Option<Regex>, _>` rather than making the
/// caller test the string first — the emptiness rule then lives in one place.
pub fn compile(source: &str) -> Result<Option<Regex>, PatternError> {
    let source = source.trim();
    if source.is_empty() {
        return Ok(None);
    }

    let length = source.chars().count();
    if length > MAX_PATTERN_LEN {
        return Err(PatternError::TooLong {
            length,
            limit: MAX_PATTERN_LEN,
        });
    }

    RegexBuilder::new(source)
        .size_limit(SIZE_LIMIT)
        .dfa_size_limit(DFA_SIZE_LIMIT)
        .build()
        .map(Some)
        .map_err(|err| PatternError::Invalid(err.to_string()))
}

#[cfg(test)]
mod tests {
    use super::{MAX_PATTERN_LEN, PatternError, compile};
    use crate::i18n::Language;

    #[test]
    fn an_empty_pattern_is_absence_rather_than_an_error() {
        assert!(compile("").expect("empty is fine").is_none());
        assert!(compile("   \n ").expect("blank is fine").is_none());
    }

    #[test]
    fn an_ordinary_pattern_compiles_and_searches_unanchored() {
        let regex = compile(r"curl").expect("compiles").expect("some");
        assert!(regex.is_match("curl https://example.com"));
        assert!(
            regex.is_match("please run curl now"),
            "a pattern searches; anchoring is the author's to ask for",
        );

        let anchored = compile(r"^curl\b").expect("compiles").expect("some");
        assert!(!anchored.is_match("please run curl now"));
    }

    #[test]
    fn nonsense_is_refused_with_the_engines_own_wording() {
        let error = compile("(unclosed").expect_err("refused");
        let PatternError::Invalid(detail) = &error else {
            panic!("expected Invalid, got {error:?}");
        };
        assert!(!detail.trim().is_empty(), "the engine says nothing useful");
    }

    /// The two features that would need backtracking are compile errors in this
    /// engine, which is half of why an untrusted pattern is safe to run.
    #[test]
    fn look_around_and_back_references_cannot_be_written_at_all() {
        assert!(compile(r"(?=foo)").is_err());
        assert!(compile(r"(\w)\1").is_err());
    }

    #[test]
    fn an_over_long_pattern_never_reaches_the_engine() {
        // `Regex` is not `PartialEq`, so the `Ok` side cannot be compared; the
        // error is what this is about.
        let source = "a".repeat(MAX_PATTERN_LEN + 1);
        assert_eq!(
            compile(&source).expect_err("refused"),
            PatternError::TooLong {
                length: MAX_PATTERN_LEN + 1,
                limit: MAX_PATTERN_LEN,
            }
        );
        // …and exactly at the limit it is still accepted.
        assert!(compile(&"a".repeat(MAX_PATTERN_LEN)).is_ok());
    }

    /// A legal expression whose compiled program is huge fails to compile rather
    /// than being allowed to allocate. `{n}` repetition is the cheap way to ask
    /// for one.
    #[test]
    fn a_pattern_that_would_compile_huge_is_refused_by_the_size_limit() {
        assert!(
            compile(r"(?:[A-Za-z0-9]{100}){100}").is_err(),
            "the size limit has to bound what an untrusted pattern can allocate",
        );
    }

    #[test]
    fn every_failure_says_something_in_every_language() {
        for error in [
            PatternError::TooLong {
                length: 9_000,
                limit: MAX_PATTERN_LEN,
            },
            PatternError::Invalid("regex parse error".to_owned()),
        ] {
            for language in Language::ALL {
                assert!(!error.message().text(language).trim().is_empty());
            }
        }
    }
}
