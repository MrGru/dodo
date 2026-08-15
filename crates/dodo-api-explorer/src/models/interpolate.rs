//! `{{name}}` substitution: one pure function over a string and a
//! [`VariableSet`].
//!
//! Kept out of the service layer deliberately. Substitution has no IO, no
//! ordering constraint and a large number of edge cases, so it belongs where it
//! can be exhaustively table-tested without a `Window`, a transport or a
//! filesystem. `services::http::resolve` is the thin pass that walks a
//! [`RequestDraft`] and calls this on each field.
//!
//! # An unresolved reference fails the request
//!
//! `{{missing}}` is an **error**, not text on the wire and not an empty string.
//! Three reasons, in the order they mattered:
//!
//! 1. There is nowhere to report a warning in this round. Diagnostics live on
//!    the Console tab, which is a later round's work; a diagnostic with no
//!    surface is a silent failure.
//! 2. Every other unsendable request already stops here.
//!    `prepare` refuses an empty URL, an unfetchable scheme and an illegal
//!    header by name, and the error banner is where users already look. An
//!    unresolved variable is the same class of mistake and gets the same
//!    treatment, naming the variable.
//! 3. Both alternatives fail worse. Substituting empty turns a missing API key
//!    into a puzzling 401 from the server; sending `{{apiKey}}` literally turns
//!    it into a puzzling 401 *and* leaks the template into someone's access log.
//!
//! A later round relaxes exactly one thing about this: a pre-request script may
//! set a value, so the check moves to *after* the script hook rather than
//! changing its verdict.
//!
//! # Escaping
//!
//! A backslash is special **only** immediately before `{{`:
//!
//! - `\{{a}}` → the literal text `{{a}}`; no lookup happens. The escaping
//!   backslash is consumed, as an escape character always is.
//! - `\\{{a}}` → one literal backslash, then `a`'s value.
//! - `\\\{{a}}` → one literal backslash, then the literal `{{a}}`. It is the
//!   *run* of backslashes before `{{` that is counted: each pair is one
//!   backslash, and an odd one left over does the escaping.
//! - `a\b`, `C:\Users\ada`, `\d+\s*` → unchanged. Backslashes anywhere else are
//!   ordinary characters, which is what keeps Windows paths and regexes in a
//!   body from needing to be doubled.
//!
//! # Nesting and the recursion guard
//!
//! A resolved value is itself interpolated, so `base = {{scheme}}://{{host}}`
//! works. A cycle (`a = {{b}}`, `b = {{a}}`) and a chain deeper than
//! [`MAX_DEPTH`] are both reported as [`InterpolationError::Recursive`] naming
//! the variable the expansion re-entered, rather than hanging or overflowing
//! the stack.
//!
//! [`VariableSet`]: crate::models::variables::VariableSet
//! [`RequestDraft`]: crate::models::request::RequestDraft

use crate::models::variables::VariableSet;

/// How deep a chain of variables referring to variables may go.
///
/// Generous enough that no honest configuration reaches it (`{{base}}` built
/// from `{{scheme}}` and `{{host}}` is depth 2), small enough that a pathological
/// file fails immediately.
pub const MAX_DEPTH: usize = 16;

/// Why a string could not be interpolated.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InterpolationError {
    /// No enabled variable in any scope carries this name.
    Unresolved { name: String },
    /// The expansion re-entered a variable, or ran deeper than [`MAX_DEPTH`].
    Recursive { name: String },
}

/// Replaces every `{{name}}` in `text` with its value.
///
/// A string with no `{{` at all is returned as an untouched copy; callers that
/// care about the difference should check [`has_reference`] first.
pub fn interpolate(text: &str, variables: &VariableSet) -> Result<String, InterpolationError> {
    let mut chain = Vec::new();
    expand(text, variables, &mut chain)
}

/// Whether `text` contains anything this function would substitute.
///
/// Used by the views to decide whether to draw the resolved-value preview at
/// all, so a request with no variables in it costs nothing.
pub fn has_reference(text: &str) -> bool {
    text.contains("{{")
}

/// The names `text` refers to, in order of first appearance, without resolving
/// them.
///
/// Escaped references are excluded, since they are literal text. Used by the
/// preview to name what is missing when resolution fails.
pub fn references(text: &str) -> Vec<String> {
    let mut names = Vec::new();
    scan(text, |piece| {
        if let Piece::Reference(name) = piece {
            let name = name.trim();
            if !names.iter().any(|existing| existing == name) {
                names.push(name.to_string());
            }
        }
    });
    names
}

/// One resolved value with the scope it came from, for the preview.
///
/// A miss is `None` rather than an error because the preview describes the
/// current state rather than judging it.
pub fn resolve_all(text: &str, variables: &VariableSet) -> Vec<(String, Option<String>)> {
    references(text)
        .into_iter()
        .map(|name| {
            let value = variables.lookup(&name).map(|(_, value)| value.to_string());
            (name, value)
        })
        .collect()
}

/// A piece of a scanned string: literal text, or a reference to substitute.
enum Piece<'a> {
    Literal(&'a str),
    Reference(&'a str),
}

/// Expands `text`, guarding against a variable that expands back into itself.
///
/// `chain` holds the names currently being expanded; its length is the depth.
fn expand(
    text: &str,
    variables: &VariableSet,
    chain: &mut Vec<String>,
) -> Result<String, InterpolationError> {
    let mut out = String::with_capacity(text.len());
    let mut error = None;

    scan(text, |piece| {
        if error.is_some() {
            return;
        }
        match piece {
            Piece::Literal(literal) => out.push_str(literal),
            Piece::Reference(name) => {
                let name = name.trim();
                let Some((_, value)) = variables.lookup(name) else {
                    error = Some(InterpolationError::Unresolved {
                        name: name.to_string(),
                    });
                    return;
                };

                if chain.iter().any(|held| held == name) || chain.len() >= MAX_DEPTH {
                    error = Some(InterpolationError::Recursive {
                        name: name.to_string(),
                    });
                    return;
                }

                // Owned before recursing: `value` borrows the set, and the
                // recursive call needs it again.
                let value = value.to_string();
                chain.push(name.to_string());
                match expand(&value, variables, chain) {
                    Ok(expanded) => out.push_str(&expanded),
                    Err(inner) => error = Some(inner),
                }
                chain.pop();
            }
        }
    });

    match error {
        Some(error) => Err(error),
        None => Ok(out),
    }
}

/// Splits `text` into literals and references, honouring the escape rule.
///
/// One scanner shared by expansion and by [`references`], so the preview can
/// never disagree with the substitution about what counts as a reference.
fn scan(text: &str, mut emit: impl FnMut(Piece<'_>)) {
    let bytes = text.as_bytes();
    let mut cursor = 0;
    let mut literal_start = 0;

    while cursor < bytes.len() {
        match bytes[cursor] {
            b'\\' => {
                let run_start = cursor;
                while cursor < bytes.len() && bytes[cursor] == b'\\' {
                    cursor += 1;
                }
                if !bytes[cursor..].starts_with(b"{{") {
                    // Not before a reference: an ordinary run of characters.
                    continue;
                }
                let run = cursor - run_start;

                emit(Piece::Literal(&text[literal_start..run_start]));
                // Each pair of backslashes is one literal backslash.
                for _ in 0..run / 2 {
                    emit(Piece::Literal("\\"));
                }
                if run % 2 == 1 {
                    // An odd run leaves one backslash escaping the braces: emit
                    // them as text and carry on *inside*, so the name and the
                    // closing braces come out verbatim too.
                    emit(Piece::Literal("{{"));
                    cursor += 2;
                }
                literal_start = cursor;
            }
            b'{' if bytes[cursor..].starts_with(b"{{") => {
                let after = cursor + 2;
                let Some(end) = find(&bytes[after..], b"}}") else {
                    // No closing braces anywhere: the rest is literal text.
                    break;
                };
                let name = &text[after..after + end];
                if name.trim().is_empty() {
                    // `{{}}` is not a reference to anything; leave it as typed.
                    cursor = after + end + 2;
                    continue;
                }
                emit(Piece::Literal(&text[literal_start..cursor]));
                emit(Piece::Reference(name));
                cursor = after + end + 2;
                literal_start = cursor;
            }
            _ => cursor += 1,
        }
    }

    if literal_start < text.len() {
        emit(Piece::Literal(&text[literal_start..]));
    }
}

/// The first index of `needle` in `haystack`. `slice::windows` would allocate
/// nothing but reads worse than naming it.
fn find(haystack: &[u8], needle: &[u8; 2]) -> Option<usize> {
    haystack
        .windows(2)
        .position(|window| window == needle.as_slice())
}

#[cfg(test)]
mod tests {
    use super::{
        InterpolationError, MAX_DEPTH, has_reference, interpolate, references, resolve_all,
    };
    use crate::models::variables::{Variable, VariableScope, VariableSet};

    /// A set whose environment layer shadows the collection layer, which is the
    /// arrangement every precedence assertion below is about.
    fn set(pairs: &[(&str, &str)]) -> VariableSet {
        let mut set = VariableSet::default();
        set.push_layer(
            VariableScope::Environment,
            pairs
                .iter()
                .map(|(key, value)| Variable::new(*key, *value))
                .collect(),
        );
        set
    }

    fn interpolated(text: &str, pairs: &[(&str, &str)]) -> String {
        interpolate(text, &set(pairs)).expect("should interpolate")
    }

    fn error(text: &str, pairs: &[(&str, &str)]) -> InterpolationError {
        interpolate(text, &set(pairs)).expect_err("should not interpolate")
    }

    #[test]
    fn a_string_with_no_reference_is_returned_unchanged() {
        for text in ["", "https://example.com/things", "a } b { c", "}}{{"] {
            assert_eq!(interpolated(text, &[]), text, "changed {text:?}");
        }
    }

    #[test]
    fn a_reference_is_replaced_wherever_it_appears() {
        assert_eq!(
            interpolated(
                "{{scheme}}://{{host}}/v1/{{host}}",
                &[("scheme", "https"), ("host", "example.com"),]
            ),
            "https://example.com/v1/example.com"
        );
    }

    #[test]
    fn surrounding_whitespace_inside_the_braces_is_ignored() {
        assert_eq!(interpolated("{{  host  }}", &[("host", "x")]), "x");
    }

    #[test]
    fn a_value_may_be_empty() {
        assert_eq!(interpolated("a{{blank}}b", &[("blank", "")]), "ab");
    }

    #[test]
    fn the_higher_scope_wins() {
        let mut set = VariableSet::default();
        set.push_layer(
            VariableScope::Collection,
            vec![Variable::new("host", "low")],
        );
        set.push_layer(
            VariableScope::Environment,
            vec![Variable::new("host", "high")],
        );
        assert_eq!(interpolate("{{host}}", &set).expect("interpolates"), "high");
    }

    #[test]
    fn an_unresolved_reference_names_the_variable() {
        assert_eq!(
            error("https://{{host}}/x", &[]),
            InterpolationError::Unresolved {
                name: "host".into()
            }
        );
        // A disabled variable is not a definition.
        let mut set = VariableSet::default();
        set.push_layer(
            VariableScope::Environment,
            vec![Variable {
                enabled: false,
                ..Variable::new("host", "x")
            }],
        );
        assert_eq!(
            interpolate("{{host}}", &set),
            Err(InterpolationError::Unresolved {
                name: "host".into()
            })
        );
    }

    #[test]
    fn the_first_unresolved_reference_is_the_one_reported() {
        assert_eq!(
            error("{{a}}{{b}}", &[]),
            InterpolationError::Unresolved { name: "a".into() }
        );
    }

    #[test]
    fn an_unclosed_reference_is_literal_text() {
        assert_eq!(
            interpolated("https://{{host/x", &[("host", "y")]),
            "https://{{host/x"
        );
        assert_eq!(interpolated("{{", &[]), "{{");
    }

    #[test]
    fn empty_braces_are_not_a_reference() {
        assert_eq!(interpolated("a{{}}b", &[]), "a{{}}b");
        assert_eq!(interpolated("a{{   }}b", &[]), "a{{   }}b");
    }

    #[test]
    fn a_backslash_escapes_a_reference() {
        assert_eq!(interpolated(r"\{{host}}", &[("host", "x")]), "{{host}}");
        // …and the escape holds even when the name is not defined at all, which
        // is the point: escaping is how you send literal braces.
        assert_eq!(interpolated(r"\{{nope}}", &[]), "{{nope}}");
    }

    #[test]
    fn a_doubled_backslash_is_one_backslash_and_still_interpolates() {
        assert_eq!(interpolated(r"\\{{host}}", &[("host", "x")]), r"\x");
        assert_eq!(interpolated(r"\\\{{host}}", &[("host", "x")]), r"\{{host}}");
        assert_eq!(interpolated(r"\\\\{{host}}", &[("host", "x")]), r"\\x");
    }

    #[test]
    fn backslashes_that_are_not_before_a_reference_are_ordinary_characters() {
        assert_eq!(interpolated(r"C:\Users\ada", &[]), r"C:\Users\ada");
        assert_eq!(interpolated(r"\d+\s*", &[]), r"\d+\s*");
        // Only the run immediately before the braces is an escape, and it is
        // consumed like any escape character.
        assert_eq!(
            interpolated(r"C:\Users\{{who}}", &[("who", "ada")]),
            r"C:\Users{{who}}"
        );
    }

    #[test]
    fn a_value_that_is_itself_a_reference_is_expanded() {
        assert_eq!(
            interpolated(
                "{{base}}/things",
                &[
                    ("base", "{{scheme}}://{{host}}"),
                    ("scheme", "https"),
                    ("host", "example.com"),
                ]
            ),
            "https://example.com/things"
        );
    }

    #[test]
    fn a_nested_value_that_is_missing_is_reported_by_its_own_name() {
        assert_eq!(
            error("{{base}}", &[("base", "{{host}}/x")]),
            InterpolationError::Unresolved {
                name: "host".into()
            }
        );
    }

    #[test]
    fn a_direct_cycle_is_reported_rather_than_hanging() {
        assert_eq!(
            error("{{a}}", &[("a", "{{a}}")]),
            InterpolationError::Recursive { name: "a".into() }
        );
    }

    #[test]
    fn an_indirect_cycle_is_reported_too() {
        assert_eq!(
            error("{{a}}", &[("a", "-{{b}}-"), ("b", "={{a}}=")]),
            InterpolationError::Recursive { name: "a".into() }
        );
    }

    #[test]
    fn a_chain_longer_than_the_depth_limit_stops() {
        // v0 → v1 → … → vN, each a plain reference to the next, with no cycle.
        let pairs: Vec<(String, String)> = (0..MAX_DEPTH + 4)
            .map(|n| (format!("v{n}"), format!("{{{{v{}}}}}", n + 1)))
            .collect();
        let mut set = VariableSet::default();
        set.push_layer(
            VariableScope::Environment,
            pairs
                .iter()
                .map(|(key, value)| Variable::new(key.clone(), value.clone()))
                .collect(),
        );
        assert!(matches!(
            interpolate("{{v0}}", &set),
            Err(InterpolationError::Recursive { .. })
        ));
    }

    #[test]
    fn the_same_variable_twice_in_one_string_is_not_recursion() {
        // The chain unwinds between siblings; only re-entering while expanding
        // is a cycle.
        assert_eq!(interpolated("{{a}}-{{a}}", &[("a", "x")]), "x-x");
        assert_eq!(
            interpolated("{{pair}}", &[("pair", "{{a}}{{a}}"), ("a", "x")]),
            "xx"
        );
    }

    #[test]
    fn an_escaped_reference_inside_a_value_stays_literal() {
        assert_eq!(
            interpolated("{{tmpl}}", &[("tmpl", r"\{{inner}}")]),
            "{{inner}}"
        );
    }

    #[test]
    fn has_reference_only_fires_on_an_opening_pair() {
        assert!(has_reference("a{{b}}"));
        assert!(has_reference("{{"));
        assert!(!has_reference("a{b}c"));
        assert!(!has_reference(""));
    }

    #[test]
    fn references_lists_each_name_once_in_order_and_skips_escaped_ones() {
        assert_eq!(
            references(r"{{b}}/{{a}}/{{b}}/\{{c}}"),
            vec!["b".to_string(), "a".to_string()]
        );
        assert!(references("no references here").is_empty());
    }

    #[test]
    fn resolve_all_reports_a_miss_as_none_rather_than_failing() {
        assert_eq!(
            resolve_all("{{host}}/{{gone}}", &set(&[("host", "x")])),
            vec![
                ("host".to_string(), Some("x".to_string())),
                ("gone".to_string(), None),
            ]
        );
    }
}
