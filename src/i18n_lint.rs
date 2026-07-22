//! A source-level guard against user-facing text that never reaches [`Str`].
//!
//! [`Str`]: crate::i18n::Str
//!
//! `Str::text`'s exhaustive match makes a *missing translation* a compile
//! error. It says nothing about a bare `"Decode JWT"` written straight into a
//! view, because that string never enters the mechanism at all. This module
//! reads the view sources at test time and looks for exactly that.
//!
//! # How it decides
//!
//! By **position, not by content**. Guessing whether `"json"` or `"side-bar"`
//! is prose is hopeless; knowing where a string is *passed* is not. User-facing
//! text reaches the screen through a short, enumerable list of gpui-component
//! sinks — `.child`, `.label`, `.title`, `.description`, `.placeholder`,
//! `SettingItem::new`, … (see `TEXT_SINKS`). A string literal sitting directly
//! in one of those argument slots is user-facing; a string literal anywhere
//! else is not this module's business.
//!
//! That is why element ids (`Button::new("open-settings")`), code-editor
//! language ids (`.code_editor("json")`), theme registry keys, format strings
//! (`format!("{radius}px")`) and developer text (`eprintln!`, `.expect`) do not
//! trip it: none of them is a text sink. No allow-list is needed for them, and
//! the check has zero findings on the tree as written apart from the one
//! documented exception.
//!
//! # What it does not catch
//!
//! It is a guard, not a proof. A literal bound to a variable first
//! (`let msg = "oops"; …child(msg)`), text built with `format!`, or a literal
//! separated from its sink by a comment all slip through. It errs towards
//! silence: every ambiguity resolves to "not a finding", because a check that
//! cries wolf gets deleted. The human rule in the `dodo-i18n-text` skill is
//! what covers the rest.

/// The view sources, embedded at compile time so the test needs no working
/// directory. These are the files that build what the user sees; pure logic
/// modules have no text sinks and are not worth scanning.
const SOURCES: [(&str, &str); 15] = [
    ("src/layout.rs", include_str!("layout.rs")),
    ("src/json_formatter.rs", include_str!("json_formatter.rs")),
    ("src/encoder_decoder.rs", include_str!("encoder_decoder.rs")),
    ("src/settings.rs", include_str!("settings.rs")),
    (
        "src/api_explorer/views/explorer.rs",
        include_str!("api_explorer/views/explorer.rs"),
    ),
    (
        "src/api_explorer/views/collections_panel.rs",
        include_str!("api_explorer/views/collections_panel.rs"),
    ),
    (
        "src/api_explorer/views/request_tabs.rs",
        include_str!("api_explorer/views/request_tabs.rs"),
    ),
    (
        "src/api_explorer/views/request_editor.rs",
        include_str!("api_explorer/views/request_editor.rs"),
    ),
    (
        "src/api_explorer/views/request_body.rs",
        include_str!("api_explorer/views/request_body.rs"),
    ),
    (
        "src/api_explorer/views/request_auth.rs",
        include_str!("api_explorer/views/request_auth.rs"),
    ),
    (
        "src/api_explorer/views/request_scripts.rs",
        include_str!("api_explorer/views/request_scripts.rs"),
    ),
    (
        "src/api_explorer/views/response_viewer.rs",
        include_str!("api_explorer/views/response_viewer.rs"),
    ),
    (
        "src/api_explorer/components/key_value_table.rs",
        include_str!("api_explorer/components/key_value_table.rs"),
    ),
    (
        "src/api_explorer/components/empty_state.rs",
        include_str!("api_explorer/components/empty_state.rs"),
    ),
    (
        "src/api_explorer/components/later_step.rs",
        include_str!("api_explorer/components/later_step.rs"),
    ),
];

/// Calls whose first argument is drawn on screen. Anything reached by another
/// route (ids, keys, values, format arguments) is deliberately absent.
///
/// Add to this when a new widget takes display text at a call site — that
/// widens the guard. Never remove one to silence a finding.
const TEXT_SINKS: [&str; 13] = [
    "child",
    "children",
    "label",
    "title",
    "description",
    "placeholder",
    "tooltip",
    "keywords",
    "text",
    "SidebarMenuItem::new",
    "SidebarGroup::new",
    "SettingPage::new",
    "SettingItem::new",
];

/// Literals that sit in a text sink and are still correct as written.
///
/// The bar is high: a proper noun or a registry key, with a comment saying
/// which. A string a user reads as *language* does not belong here — it belongs
/// in `Str`.
const ALLOWED: [&str; 1] = [
    // The product name. Never translated, in any language.
    "Dodo",
];

/// A string literal in the source, with where it starts.
struct Literal<'a> {
    line: usize,
    text: &'a str,
    /// Byte offset of the opening quote.
    start: usize,
}

/// Everything before the first `#[cfg(test)]` module. Test code may say
/// whatever it likes.
fn without_tests(source: &str) -> &str {
    match source.find("\n#[cfg(test)]") {
        Some(at) => &source[..at],
        None => source,
    }
}

fn is_path_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b':'
}

/// The last non-whitespace byte strictly before `from`.
fn back_over_ws(bytes: &[u8], from: usize) -> Option<usize> {
    bytes[..from]
        .iter()
        .rposition(|byte| !byte.is_ascii_whitespace())
}

/// How many bytes to skip for a `'` at `at`. Distinguishes a char literal from
/// a lifetime tick well enough that neither can hide a `"` from the scanner.
fn char_literal_len(bytes: &[u8], at: usize) -> usize {
    match bytes.get(at + 1) {
        // `'\n'`, `'\''`, `'\u{1f600}'` — run to the closing quote.
        Some(b'\\') => bytes[at + 2..]
            .iter()
            .position(|byte| *byte == b'\'')
            .map_or(1, |offset| offset + 3),
        // `'x'` — a plain ASCII char literal.
        _ if bytes.get(at + 2) == Some(&b'\'') => 3,
        // A lifetime: `&'static`.
        _ => 1,
    }
}

/// Every string literal in `source`, skipping comments and char literals.
///
/// Panics on a raw string rather than guessing at its delimiters — there are
/// none in the view sources today, and a silent misparse would make every
/// finding after it untrustworthy.
fn string_literals(source: &str) -> Vec<Literal<'_>> {
    let bytes = source.as_bytes();
    let mut literals = Vec::new();
    let mut line = 1usize;
    let mut i = 0usize;

    while i < bytes.len() {
        match bytes[i] {
            b'\n' => {
                line += 1;
                i += 1;
            }
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                let mut depth = 1usize;
                i += 2;
                while i < bytes.len() && depth > 0 {
                    match bytes[i] {
                        b'\n' => {
                            line += 1;
                            i += 1;
                        }
                        b'/' if bytes.get(i + 1) == Some(&b'*') => {
                            depth += 1;
                            i += 2;
                        }
                        b'*' if bytes.get(i + 1) == Some(&b'/') => {
                            depth -= 1;
                            i += 2;
                        }
                        _ => i += 1,
                    }
                }
            }
            b'\'' => i += char_literal_len(bytes, i),
            b'"' => {
                assert!(
                    i == 0 || !matches!(bytes[i - 1], b'r' | b'#'),
                    "line {line}: this guard does not understand raw strings; \
                     teach `string_literals` about them before adding one"
                );

                let start = i;
                let opened_on = line;
                i += 1;
                let content = i;
                while i < bytes.len() && bytes[i] != b'"' {
                    match bytes[i] {
                        b'\\' => i += 2,
                        b'\n' => {
                            line += 1;
                            i += 1;
                        }
                        _ => i += 1,
                    }
                }
                literals.push(Literal {
                    line: opened_on,
                    text: &source[content..i.min(bytes.len())],
                    start,
                });
                i += 1;
            }
            _ => i += 1,
        }
    }

    literals
}

/// The call a literal is the first argument of — `"child"` for `.child("x")`,
/// `"SettingItem::new"` for `SettingItem::new("x", f)`. `None` if the literal
/// is not passed directly to anything, as in an array, a `let`, or a `match`.
fn enclosing_call(source: &str, start: usize) -> Option<&str> {
    let bytes = source.as_bytes();

    let mut at = back_over_ws(bytes, start)?;
    // `.keywords(["Foo"])` — one array layer still counts as being passed.
    if bytes[at] == b'[' {
        at = back_over_ws(bytes, at)?;
    }
    if bytes[at] != b'(' {
        return None;
    }

    let end = back_over_ws(bytes, at)? + 1;
    let mut begin = end;
    while begin > 0 && is_path_byte(bytes[begin - 1]) {
        begin -= 1;
    }
    // A macro (`format!(`) or an expression (`)(`) ends in no path bytes.
    (begin < end).then(|| &source[begin..end])
}

/// Literals in a text sink that are neither allowed nor routed through `Str`.
fn findings() -> Vec<String> {
    let mut findings = Vec::new();

    for (path, source) in SOURCES {
        let source = without_tests(source);

        for literal in string_literals(source) {
            // Separators, punctuation and spacing carry no language.
            if !literal.text.chars().any(|c| c.is_ascii_alphabetic()) {
                continue;
            }
            if ALLOWED.contains(&literal.text) {
                continue;
            }
            let Some(call) = enclosing_call(source, literal.start) else {
                continue;
            };
            if !TEXT_SINKS.contains(&call) {
                continue;
            }
            findings.push(format!(
                "{path}:{} — {call}(\"{}\")",
                literal.line, literal.text
            ));
        }
    }

    findings
}

#[cfg(test)]
mod tests {
    use super::{enclosing_call, findings, string_literals};

    /// The guard itself: no view may draw a string that did not come from
    /// `Str`.
    ///
    /// A failure here is a missing translation, not a false alarm. Add a `Str`
    /// variant and call `t(Str::Foo, cx)`; see the `dodo-i18n-text` skill.
    #[test]
    fn view_code_draws_no_untranslated_literals() {
        let findings = findings();
        assert!(
            findings.is_empty(),
            "{} user-facing string literal(s) bypass `Str`:\n  {}",
            findings.len(),
            findings.join("\n  ")
        );
    }

    /// Keeps `SOURCES` honest: `include_str!` would happily embed a file that
    /// no longer builds any UI.
    ///
    /// A file builds UI if it implements `Render`, implements `RenderOnce`,
    /// opens a dialog, or returns an element from a builder function. The last
    /// three forms were added for the API Explorer's `components/` and the
    /// per-region `impl ApiExplorer` blocks, which draw translated text without
    /// implementing `Render` themselves; `AnyElement` is in the list because a
    /// region renderer whose result outlives the `cx` borrow has to return it
    /// boxed. The guard that actually matters —
    /// `view_code_draws_no_untranslated_literals` — is unchanged.
    #[test]
    fn scanned_sources_are_the_view_sources() {
        for (path, source) in super::SOURCES {
            assert!(
                source.contains("impl Render for")
                    || source.contains("impl RenderOnce for")
                    || source.contains("open_dialog")
                    || source.contains("-> impl IntoElement")
                    || source.contains("-> gpui::AnyElement"),
                "{path} no longer renders anything — it does not belong in SOURCES"
            );
        }
    }

    /// The classifier is the whole guard; if it drifts, the guard silently
    /// passes forever. These are the shapes that actually occur in the tree.
    #[test]
    fn enclosing_call_reads_the_argument_slot() {
        let cases = [
            (r#".child("x")"#, Some("child")),
            (r#".label("x")"#, Some("label")),
            (".child(\n    \"x\",\n)", Some("child")),
            (r#".keywords(["x"])"#, Some("keywords")),
            (r#"SettingItem::new("x", f)"#, Some("SettingItem::new")),
            // Not text: an element id, an editor language, a format string, a
            // developer message, an array element, a binding.
            (r#"Button::new("x")"#, Some("Button::new")),
            (r#".code_editor("x")"#, Some("code_editor")),
            (r#"format!("x")"#, None),
            (r#"eprintln!("x")"#, None),
            (r#"[ "x", "y" ]"#, None),
            (r#"let a = "x";"#, None),
        ];

        for (source, expected) in cases {
            let literal = &string_literals(source)[0];
            assert_eq!(
                enclosing_call(source, literal.start),
                expected,
                "misread the argument slot in `{source}`"
            );
        }
    }

    /// The scanner must not see text inside comments or char literals; both
    /// appear in the view sources and both could otherwise raise phantom
    /// findings.
    #[test]
    fn scanner_ignores_comments_and_char_literals() {
        let source = r#"
// .child("commented out")
/* .label("block commented") */
fn f<'a>(c: char) { let _ = c == '"'; }
.child("real")
"#;
        let found: Vec<&str> = string_literals(source)
            .iter()
            .map(|literal| literal.text)
            .collect();
        assert_eq!(found, ["real"]);
    }
}
