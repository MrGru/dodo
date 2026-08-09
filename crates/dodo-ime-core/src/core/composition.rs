//! The text being composed but not yet committed, plus the two grapheme
//! helpers every host needs.
//!
//! # Positions are graphemes, never scalars and never bytes
//!
//! `ế` is one thing on screen. Depending on where it came from it is one
//! `char` (U+1EBF, NFC) or three (`e` + U+0302 + U+0301, NFD), and in UTF-8 it
//! is three bytes or five. A cursor counted in any of those units drifts the
//! moment a diacritic appears — which, in a Vietnamese input method, is
//! immediately.
//!
//! So [`Composition::cursor`] and [`Composition::selection`] are counted in
//! graphemes, [`grapheme_count`] is the only way this module measures text, and
//! [`crate::core::EngineAction::ReplaceBeforeCursor`] states its
//! span the same way. A host that thinks in UTF-16 code units (Windows) or
//! `NSRange`s (macOS) converts at its own boundary, where it has the string in
//! hand.

use unicode_normalization::char::is_combining_mark;

/// How many user-visible characters `text` has.
///
/// A grapheme cluster is a base character plus the combining marks that attach
/// to it, so this counts the characters that are *not* combining marks. That
/// is exact for everything this module produces (Latin with combining
/// diacritics, precomposed or not) and for the CJK scripts a later round adds.
/// It is not a full UAX #29 implementation: it does not join a regional
/// indicator pair into one flag, or an emoji ZWJ sequence into one glyph. No
/// input method engine here emits either, and buying the full algorithm would
/// mean a new dependency — which this round is not allowed, and would not want
/// for a case it cannot produce.
pub fn grapheme_count(text: &str) -> usize {
    text.chars().filter(|c| !is_combining_mark(*c)).count()
}

/// The first `count` user-visible characters of `text`.
///
/// Saturates at the whole string. Like [`truncate_graphemes`], this is the
/// engine's intentionally small grapheme definition, so direct-output hosts
/// can compare edits without inventing a second one.
pub fn grapheme_prefix(text: &str, count: usize) -> &str {
    if count == 0 {
        return "";
    }
    let mut seen = 0;
    for (at, character) in text.char_indices() {
        if !is_combining_mark(character) {
            if seen == count {
                return &text[..at];
            }
            seen += 1;
        }
    }
    text
}

/// `text` with its last `count` graphemes removed.
///
/// Saturates rather than panicking: asking to remove more than is there
/// empties the string. A host that miscounts loses text it should have kept,
/// which is bad; a panic inside a key handler takes the user's whole
/// application down, which is worse.
pub fn truncate_graphemes(text: &str, count: usize) -> String {
    if count == 0 {
        return text.to_string();
    }
    let mut seen = 0usize;
    // Walk back from the end, counting base characters. The byte index of the
    // `count`-th base character from the end is where the string is cut.
    for (at, ch) in text.char_indices().rev() {
        if !is_combining_mark(ch) {
            seen += 1;
            if seen == count {
                return text[..at].to_string();
            }
        }
    }
    String::new()
}

/// Text an engine is still working on: shown to the user, not yet in the
/// document.
///
/// # The selection is not decoration
///
/// Vietnamese never uses it — a syllable is composed and committed whole. It is
/// here because Japanese conversion cannot be expressed without it: `わたしのなまえ`
/// converts to clauses, one of which is *active* while the arrow keys move
/// between them and the candidate list re-fills for whichever is selected. That
/// is a selection range over the composition, and an API without one forces
/// either a rewrite or a second parallel channel when the Japanese engine
/// lands. Adding the field now costs one `Option`.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Composition {
    text: String,
    /// Caret position, in graphemes from the start.
    cursor: usize,
    /// The active span, in graphemes. `None` when the whole composition is the
    /// unit of attention, which is every Vietnamese case.
    selection: Option<std::ops::Range<usize>>,
}

impl Composition {
    pub fn new() -> Composition {
        Composition::default()
    }

    /// A composition whose caret sits after the last character — the normal
    /// state while someone is typing.
    pub fn at_end(text: impl Into<String>) -> Composition {
        let text = text.into();
        let cursor = grapheme_count(&text);
        Composition {
            text,
            cursor,
            selection: None,
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn selection(&self) -> Option<&std::ops::Range<usize>> {
        self.selection.as_ref()
    }

    pub fn with_selection(mut self, selection: std::ops::Range<usize>) -> Composition {
        self.selection = Some(selection);
        self
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// The length of the composition in graphemes — what a host must replace or
    /// delete to get rid of it.
    pub fn len(&self) -> usize {
        grapheme_count(&self.text)
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
        self.selection = None;
    }
}

#[cfg(test)]
mod tests {
    use super::{Composition, grapheme_count, grapheme_prefix, truncate_graphemes};

    /// The property the whole module is built around: a precomposed and a
    /// decomposed `ế` are one visible character either way.
    #[test]
    fn a_vietnamese_letter_is_one_grapheme_however_it_is_encoded() {
        let precomposed = "\u{1ebf}";
        let decomposed = "e\u{0302}\u{0301}";

        assert_eq!(precomposed.chars().count(), 1);
        assert_eq!(decomposed.chars().count(), 3);
        assert_ne!(precomposed.len(), decomposed.len());

        assert_eq!(grapheme_count(precomposed), 1);
        assert_eq!(grapheme_count(decomposed), 1);
    }

    #[test]
    fn grapheme_count_measures_words_not_bytes() {
        assert_eq!(grapheme_count(""), 0);
        assert_eq!(grapheme_count("tieng"), 5);
        assert_eq!(grapheme_count("tiếng"), 5);
        assert_eq!(grapheme_count("đường"), 5);
        assert_eq!(grapheme_count("VIỆT"), 4);
        // Decomposed throughout: 12 scalars, still five letters.
        assert_eq!(grapheme_count("d\u{0111}u\u{031b}o\u{031b}\u{0300}ng"), 6);
    }

    #[test]
    fn prefix_and_truncate_remove_visible_characters() {
        assert_eq!(grapheme_prefix("tiếng", 0), "");
        assert_eq!(grapheme_prefix("tiếng", 3), "tiế");
        assert_eq!(grapheme_prefix("tiếng", 99), "tiếng");
        assert_eq!(
            grapheme_prefix("e\u{0302}\u{0301}x", 1),
            "e\u{0302}\u{0301}"
        );

        assert_eq!(truncate_graphemes("tiếng", 0), "tiếng");
        assert_eq!(truncate_graphemes("tiếng", 1), "tiến");
        assert_eq!(truncate_graphemes("tiếng", 3), "ti");
        assert_eq!(truncate_graphemes("tiếng", 5), "");
        // A whole letter goes, not just its tone mark.
        assert_eq!(truncate_graphemes("e\u{0302}\u{0301}x", 2), "");
    }

    /// Overshooting is a host bug, and a panic in a key handler would take the
    /// user's application with it.
    #[test]
    fn truncating_past_the_start_empties_rather_than_panics() {
        assert_eq!(truncate_graphemes("việt", 99), "");
        assert_eq!(truncate_graphemes("", 3), "");
    }

    #[test]
    fn at_end_puts_the_caret_after_the_last_visible_character() {
        let composition = Composition::at_end("đường");
        assert_eq!(composition.cursor(), 5);
        assert_eq!(composition.len(), 5);
        assert_eq!(composition.selection(), None);
        assert!(!composition.is_empty());
    }

    #[test]
    fn a_selection_survives_being_set() {
        let composition = Composition::at_end("わたしのなまえ").with_selection(3..5);
        assert_eq!(composition.selection(), Some(&(3..5)));
        assert_eq!(composition.len(), 7);
    }

    #[test]
    fn clearing_resets_every_field() {
        let mut composition = Composition::at_end("việt").with_selection(0..2);
        composition.clear();
        assert!(composition.is_empty());
        assert_eq!(composition.cursor(), 0);
        assert_eq!(composition.selection(), None);
        assert_eq!(composition, Composition::new());
    }
}
