//! Graphemes in, UTF-16 code units out — the conversion every `NSRange` needs.
//!
//! # Two units, and a document that will not survive confusing them
//!
//! The engine counts everything in **graphemes**: `ế` is one thing, whether the
//! document holds it as U+1EBF or as `e` + U+0302 + U+0301. `Composition`'s
//! cursor, `EngineAction::ReplaceBeforeCursor`'s span and
//! `EngineAction::DeleteBackward`'s count are all in that unit, and the engine's
//! own docs say so.
//!
//! Every `NSRange` an `IMKTextInput` client accepts or returns is in **UTF-16
//! code units**. The investigation measured it: after committing `"jj" + U+0301`
//! the client reported `selectedRange = {3, 0}` — three units for two visible
//! characters — so "replace one grapheme before the caret" is the range `{2, 1}`
//! and not `{1, 1}`. Passing a grapheme count straight into an `NSRange` cuts a
//! tone mark off its vowel, and the surviving half attaches to whatever letter
//! is now in front of it.
//!
//! # Where the grapheme definition comes from
//!
//! Not from here. [`grapheme_prefix`] is built out of the engine's own
//! [`grapheme_count`] and [`truncate_graphemes`], so this module has no second
//! opinion about where a grapheme ends and cannot drift from the code that
//! produced the counts it is converting. That also means this crate needs no
//! Unicode dependency of its own.
//!
//! # Everything saturates
//!
//! Except [`utf16_len_of_last_graphemes`], which returns `None` when it is asked
//! for more graphemes than the text it was given holds. That case is a client
//! that answered `attributedSubstringFromRange:` with less text than the engine
//! believes it typed, and the honest answer is "I cannot compute this range" —
//! performing a wrong one would eat characters the user did not type. Nothing
//! here panics: a panic inside a key handler takes the user's application down
//! with it.

use dodo_ime_core::core::{grapheme_count, truncate_graphemes};

/// How many UTF-16 code units `text` occupies — the length an `NSString` made
/// from it would report.
pub fn utf16_len(text: &str) -> usize {
    text.encode_utf16().count()
}

/// The first `graphemes` graphemes of `text`.
///
/// Saturates: asking for more than is there returns all of it.
pub fn grapheme_prefix(text: &str, graphemes: usize) -> &str {
    let total = grapheme_count(text);
    if graphemes >= total {
        return text;
    }
    // `truncate_graphemes` cuts from the end and is the engine's own walk, so
    // going through it is what keeps the two definitions identical. It allocates;
    // the length it produces is what we actually want, and it is a byte index
    // into `text` because a prefix's bytes are unchanged by truncation.
    let cut = truncate_graphemes(text, total - graphemes).len();
    &text[..cut]
}

/// The UTF-16 offset of the caret when it sits after `graphemes` graphemes of
/// `text`.
///
/// This is what `setMarkedText:selectionRange:replacementRange:` wants for its
/// `selectionRange`, which the header documents as relative to the string being
/// passed rather than to the document.
pub fn utf16_offset_of_grapheme(text: &str, graphemes: usize) -> usize {
    utf16_len(grapheme_prefix(text, graphemes))
}

/// How many UTF-16 code units the last `graphemes` graphemes of `text` occupy,
/// or `None` if `text` does not have that many.
///
/// `text` is a window the client read back out of its own document, so this is
/// the step that turns `ReplaceBeforeCursor { grapheme_count: 2, .. }` into the
/// `{location, length}` an `insertText:replacementRange:` can use.
pub fn utf16_len_of_last_graphemes(text: &str, graphemes: usize) -> Option<usize> {
    let total = grapheme_count(text);
    if graphemes > total {
        return None;
    }
    Some(utf16_len(text) - utf16_offset_of_grapheme(text, total - graphemes))
}

#[cfg(test)]
mod tests {
    use super::{
        grapheme_prefix, utf16_len, utf16_len_of_last_graphemes, utf16_offset_of_grapheme,
    };

    /// The measurement from the investigation, as a test: two visible
    /// characters, three UTF-16 units, and the range that replaces the last one
    /// is `{2, 1}`.
    #[test]
    fn the_measured_case_that_names_the_whole_problem() {
        let text = "jj\u{301}";
        assert_eq!(utf16_len(text), 3);
        assert_eq!(super::grapheme_count(text), 2);

        let last = utf16_len_of_last_graphemes(text, 1).expect("two graphemes are there");
        assert_eq!(last, 2, "j + combining acute is two units, not one");
        assert_eq!(utf16_len(text) - last, 1, "the replacement starts at {{1}}");
    }

    #[test]
    fn precomposed_and_decomposed_agree_on_graphemes_and_differ_on_units() {
        let precomposed = "ti\u{1ebf}ng";
        let decomposed = "tie\u{302}\u{301}ng";

        assert_eq!(utf16_len(precomposed), 5);
        assert_eq!(utf16_len(decomposed), 7);

        // Three visible characters in from the start is the same place in both.
        assert_eq!(utf16_offset_of_grapheme(precomposed, 3), 3);
        assert_eq!(utf16_offset_of_grapheme(decomposed, 3), 5);

        // And the caret at the end is the whole string, both ways.
        assert_eq!(utf16_offset_of_grapheme(precomposed, 5), 5);
        assert_eq!(utf16_offset_of_grapheme(decomposed, 5), 7);
    }

    #[test]
    fn the_prefix_is_counted_in_visible_characters() {
        assert_eq!(grapheme_prefix("tiếng", 0), "");
        assert_eq!(grapheme_prefix("tiếng", 3), "tiế");
        assert_eq!(grapheme_prefix("tiếng", 5), "tiếng");
        // Saturates rather than panicking.
        assert_eq!(grapheme_prefix("tiếng", 99), "tiếng");
        assert_eq!(grapheme_prefix("", 3), "");

        // A whole decomposed letter comes with its marks or not at all.
        assert_eq!(
            grapheme_prefix("tie\u{302}\u{301}ng", 3),
            "tie\u{302}\u{301}"
        );
    }

    #[test]
    fn the_span_before_the_caret_is_measured_in_units() {
        assert_eq!(utf16_len_of_last_graphemes("tiếng", 0), Some(0));
        assert_eq!(utf16_len_of_last_graphemes("tiếng", 2), Some(2));
        assert_eq!(utf16_len_of_last_graphemes("tiếng", 5), Some(5));
        // Two of these five visible characters cost three units.
        assert_eq!(
            utf16_len_of_last_graphemes("tie\u{302}\u{301}ng", 3),
            Some(5)
        );
    }

    /// A client that reported less text than the engine thinks it typed. The
    /// only safe answer is to refuse the range.
    #[test]
    fn asking_for_more_than_is_there_refuses_rather_than_guessing() {
        assert_eq!(utf16_len_of_last_graphemes("ti", 3), None);
        assert_eq!(utf16_len_of_last_graphemes("", 1), None);
        assert_eq!(utf16_len_of_last_graphemes("", 0), Some(0));
    }

    /// Astral characters are two UTF-16 units each and one grapheme. Nothing
    /// this engine emits is astral, but a *document* can hold one right before
    /// the caret, and that is the window `ReplaceBeforeCursor` reads.
    #[test]
    fn a_surrogate_pair_is_one_visible_character_and_two_units() {
        let text = "a\u{1f423}";
        assert_eq!(utf16_len(text), 3);
        assert_eq!(utf16_len_of_last_graphemes(text, 1), Some(2));
        assert_eq!(utf16_offset_of_grapheme(text, 1), 1);
    }
}
