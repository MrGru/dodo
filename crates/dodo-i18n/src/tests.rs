//! What these tests protect
//! ------------------------
//!
//! The `match` in each area's language file already makes a *missing*
//! translation a compile error, and the `samples!` macro makes a variant with
//! no sample one too. Three things neither can catch, and that these tests do:
//!
//! 1. A language arm that is present but empty, or whitespace only.
//! 2. A parameterized arm that forgot its `{placeholder}`, so the runtime value
//!    (a line number, a parser's message) silently never reaches the screen.
//! 3. A language arm that was filled in by pasting the English text. Asserting
//!    "every language differs" would be false — `Hex`, `Header` and `Payload`
//!    are the same word in both languages by design — so every variant declares
//!    which it is via [`Expect`], and the test holds it to that declaration in
//!    *both* directions.

use super::encoder_decoder::{JwtPart, en, vi};
use super::{Language, Str};

/// Stands in for a third-party parser's own message. Deliberately unlike any
/// word in the catalogue so `contains` cannot match by accident.
pub(crate) const DETAIL: &str = "<<detail-sentinel>>";
/// Ditto for numeric values: no catalogue string contains this digit run.
pub(crate) const NUMBER: usize = 4242;
pub(crate) const NUMBER_TEXT: &str = "4242";

/// Whether a variant is expected to read differently in each language.
#[derive(Clone, Copy)]
pub(crate) enum Expect {
    /// Prose. Every language must produce its own wording.
    Translated,
    /// A term of art that is the same word in every language we ship.
    /// Asserted as equality, so "translating" one later fails here and forces
    /// the declaration to be updated rather than quietly diverging.
    SameEverywhere,
}

pub(crate) struct Sample {
    pub(crate) text: Str,
    /// Runtime values the rendered text must surface, in every language.
    pub(crate) parts: &'static [&'static str],
    pub(crate) expect: Expect,
}

/// Prose: every language must word it itself.
pub(crate) fn plain(text: impl Into<Str>) -> Sample {
    Sample {
        text: text.into(),
        parts: &[],
        expect: Expect::Translated,
    }
}

/// A term of art that is deliberately identical in every language.
pub(crate) fn term(text: impl Into<Str>) -> Sample {
    Sample {
        text: text.into(),
        parts: &[],
        expect: Expect::SameEverywhere,
    }
}

/// Prose carrying runtime values every language must surface.
pub(crate) fn with(text: impl Into<Str>, parts: &'static [&'static str]) -> Sample {
    Sample {
        text: text.into(),
        parts,
        expect: Expect::Translated,
    }
}

#[test]
fn every_area_contributes_its_samples() {
    let samples = Str::samples();
    assert_eq!(
        samples.len(),
        947,
        "the number of localized strings changed; update this count deliberately \
         so a whole area silently dropping out of `areas!` cannot pass"
    );
}

#[test]
fn every_language_renders_every_string() {
    for sample in Str::samples() {
        let english = sample.text.clone().text(Language::English).into_owned();

        for language in Language::ALL {
            let text = sample.text.clone().text(language).into_owned();
            let code = language.code();

            assert!(
                !text.trim().is_empty(),
                "{code} translation of \"{english}\" is empty"
            );
            for part in sample.parts {
                assert!(
                    text.contains(part),
                    "{code} translation of \"{english}\" dropped the runtime value \
                     `{part}`; it rendered as \"{text}\""
                );
            }
        }
    }
}

#[test]
fn translations_match_their_declared_kind() {
    for sample in Str::samples() {
        let english = sample.text.clone().text(Language::English).into_owned();

        for language in Language::ALL {
            if language == Language::English {
                continue;
            }
            let text = sample.text.clone().text(language).into_owned();
            let code = language.code();

            match sample.expect {
                Expect::Translated => assert_ne!(
                    text, english,
                    "{code} still shows the English text for \"{english}\" — translate it, \
                     or declare it with term() if it really is the same word"
                ),
                Expect::SameEverywhere => assert_eq!(
                    text, english,
                    "\"{english}\" is declared as a term of art that is identical in every \
                     language, but {code} differs — declare it with plain() instead"
                ),
            }
        }
    }
}

/// The JWT part names are the one piece of text that is not a [`Str`] variant:
/// they read mid-sentence inside the messages above, so each language file
/// carries its own table.
#[test]
fn every_language_names_every_jwt_part() {
    for part in [JwtPart::Header, JwtPart::Payload] {
        for language in Language::ALL {
            let name = match language {
                Language::English => en::jwt_part(part),
                Language::Vietnamese => vi::jwt_part(part),
            };
            assert!(
                !name.trim().is_empty(),
                "{} has no name for a JWT part",
                language.code()
            );
        }
    }
}
