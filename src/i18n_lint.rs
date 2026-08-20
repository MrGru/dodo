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
const SOURCES: [(&str, &str); 46] = [
    ("src/layout.rs", include_str!("layout.rs")),
    (
        "crates/dodo-json-formatter/src/lib.rs",
        include_str!("../crates/dodo-json-formatter/src/lib.rs"),
    ),
    (
        "crates/dodo-encoder-decoder/src/lib.rs",
        include_str!("../crates/dodo-encoder-decoder/src/lib.rs"),
    ),
    ("src/settings/mod.rs", include_str!("settings/mod.rs")),
    (
        "src/settings/appearance.rs",
        include_str!("settings/appearance.rs"),
    ),
    (
        "src/settings/features.rs",
        include_str!("settings/features.rs"),
    ),
    (
        "src/settings/general.rs",
        include_str!("settings/general.rs"),
    ),
    ("src/settings/pages.rs", include_str!("settings/pages.rs")),
    (
        "src/settings/quick_nav.rs",
        include_str!("settings/quick_nav.rs"),
    ),
    ("src/settings/search.rs", include_str!("settings/search.rs")),
    ("src/settings/view.rs", include_str!("settings/view.rs")),
    (
        "crates/dodo-cleaner/src/views/cleaner_view.rs",
        include_str!("../crates/dodo-cleaner/src/views/cleaner_view.rs"),
    ),
    (
        "crates/dodo-cleaner/src/views/results_table.rs",
        include_str!("../crates/dodo-cleaner/src/views/results_table.rs"),
    ),
    (
        "crates/dodo-cleaner/src/views/uninstall_review_dialog.rs",
        include_str!("../crates/dodo-cleaner/src/views/uninstall_review_dialog.rs"),
    ),
    (
        "crates/dodo-api-explorer/src/views/explorer.rs",
        include_str!("../crates/dodo-api-explorer/src/views/explorer.rs"),
    ),
    (
        "crates/dodo-api-explorer/src/views/collections_panel.rs",
        include_str!("../crates/dodo-api-explorer/src/views/collections_panel.rs"),
    ),
    (
        "crates/dodo-api-explorer/src/views/history_panel.rs",
        include_str!("../crates/dodo-api-explorer/src/views/history_panel.rs"),
    ),
    (
        "crates/dodo-api-explorer/src/views/request_tabs.rs",
        include_str!("../crates/dodo-api-explorer/src/views/request_tabs.rs"),
    ),
    (
        "crates/dodo-api-explorer/src/views/request_editor.rs",
        include_str!("../crates/dodo-api-explorer/src/views/request_editor.rs"),
    ),
    (
        "crates/dodo-api-explorer/src/views/request_body.rs",
        include_str!("../crates/dodo-api-explorer/src/views/request_body.rs"),
    ),
    (
        "crates/dodo-api-explorer/src/views/request_auth.rs",
        include_str!("../crates/dodo-api-explorer/src/views/request_auth.rs"),
    ),
    (
        "crates/dodo-api-explorer/src/views/request_scripts.rs",
        include_str!("../crates/dodo-api-explorer/src/views/request_scripts.rs"),
    ),
    (
        "crates/dodo-api-explorer/src/views/response_viewer.rs",
        include_str!("../crates/dodo-api-explorer/src/views/response_viewer.rs"),
    ),
    // The Generate code dialog. Its *snippets* are code and are deliberately not
    // translated (see `services::codegen::javascript`), but they are built in the
    // service layer, which this never scans — everything a user reads as language
    // in this file is a label, a tab or the secrets notice.
    (
        "crates/dodo-api-explorer/src/views/generate_code.rs",
        include_str!("../crates/dodo-api-explorer/src/views/generate_code.rs"),
    ),
    (
        "crates/dodo-api-explorer/src/components/key_value_table.rs",
        include_str!("../crates/dodo-api-explorer/src/components/key_value_table.rs"),
    ),
    (
        "crates/dodo-api-explorer/src/components/empty_state.rs",
        include_str!("../crates/dodo-api-explorer/src/components/empty_state.rs"),
    ),
    (
        "crates/dodo-api-explorer/src/components/later_step.rs",
        include_str!("../crates/dodo-api-explorer/src/components/later_step.rs"),
    ),
    // The update dialog. Its *release notes* pane is deliberately not
    // translated — that text arrives in `update.json` and belongs to the
    // release, not to dodo — but it reaches the screen through a
    // `SharedString::from(info.notes)`, not a literal, so it is not a finding
    // and needs no exception here.
    (
        "crates/dodo-updater/src/views/dialog.rs",
        include_str!("../crates/dodo-updater/src/views/dialog.rs"),
    ),
    // The Database Explorer. Its result *cells* and tree *labels* are data —
    // a server's own identifiers and values — but they arrive as
    // `SharedString::from(String)`, never as literals, so they are not findings
    // and need no exception. Everything a user reads as language here is a
    // label, a placeholder, a status word or an error.
    (
        "crates/dodo-database/src/views/database.rs",
        include_str!("../crates/dodo-database/src/views/database.rs"),
    ),
    (
        "crates/dodo-database/src/views/connections_panel.rs",
        include_str!("../crates/dodo-database/src/views/connections_panel.rs"),
    ),
    (
        "crates/dodo-database/src/views/connection_form.rs",
        include_str!("../crates/dodo-database/src/views/connection_form.rs"),
    ),
    (
        "crates/dodo-database/src/views/query_pane.rs",
        include_str!("../crates/dodo-database/src/views/query_pane.rs"),
    ),
    (
        "crates/dodo-database/src/views/object_detail.rs",
        include_str!("../crates/dodo-database/src/views/object_detail.rs"),
    ),
    (
        "crates/dodo-database/src/views/result_grid.rs",
        include_str!("../crates/dodo-database/src/views/result_grid.rs"),
    ),
    (
        "crates/dodo-database/src/views/history.rs",
        include_str!("../crates/dodo-database/src/views/history.rs"),
    ),
    (
        "crates/dodo-database/src/views/saved_queries.rs",
        include_str!("../crates/dodo-database/src/views/saved_queries.rs"),
    ),
    (
        "crates/dodo-database/src/views/saved_query_form.rs",
        include_str!("../crates/dodo-database/src/views/saved_query_form.rs"),
    ),
    (
        "crates/dodo-database/src/views/catalog_search.rs",
        include_str!("../crates/dodo-database/src/views/catalog_search.rs"),
    ),
    // The two shared elements. They take text that is already translated, so a
    // literal in one of them would be a new, untranslated string rather than a
    // pass-through — which is exactly what this scan is for.
    (
        "crates/dodo-database/src/components/notice.rs",
        include_str!("../crates/dodo-database/src/components/notice.rs"),
    ),
    (
        "crates/dodo-database/src/components/states.rs",
        include_str!("../crates/dodo-database/src/components/states.rs"),
    ),
    // The Input method tool. Scanned on every platform even though it only
    // *compiles* on macOS: `include_str!` reads the file, not the build, which
    // is exactly what a guard against untranslated text should do — a literal
    // there is no less untranslated for being behind a `cfg`.
    (
        "crates/dodo-input-method/src/views/input_method_view.rs",
        include_str!("../crates/dodo-input-method/src/views/input_method_view.rs"),
    ),
    // The Flow Canvas. Scanned from Phase 9, four phases before its sidebar
    // row: the canvas draws its first translated strings there, and a guard
    // that starts when the tool ships is a guard that misses everything
    // written while it was being built.
    (
        "crates/dodo-flow/src/views/flow.rs",
        include_str!("../crates/dodo-flow/src/views/flow.rs"),
    ),
    (
        "crates/dodo-flow/src/views/palette.rs",
        include_str!("../crates/dodo-flow/src/views/palette.rs"),
    ),
    (
        "crates/dodo-flow/src/views/nodes.rs",
        include_str!("../crates/dodo-flow/src/views/nodes.rs"),
    ),
    // Phase 11's property panel, which is on its own the largest single
    // addition of user-visible text the canvas has had — fifteen section
    // labels and forty tooltips. Added with the file rather than after it, for
    // the reason the comment above gives.
    (
        "crates/dodo-flow/src/views/properties.rs",
        include_str!("../crates/dodo-flow/src/views/properties.rs"),
    ),
    // Phase 12's pictures. It draws no text of its own today — a picture that
    // cannot be decoded is a muted frame rather than a message — and it is
    // scanned anyway, for the reason the two comments above give: a guard added
    // when a file first needs one is a guard that was absent while it was being
    // written.
    (
        "crates/dodo-flow/src/views/images.rs",
        include_str!("../crates/dodo-flow/src/views/images.rs"),
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

/// Lines where a platform `cfg` attribute hides a function returning `Str`.
///
/// Such a function is invisible to the other platforms' type checkers, which
/// lets a bare area `Text` be returned where `Str` was promised. Platform-only
/// API calls belong in an element-building caller; the value-selection helper
/// stays portable and ends with one conversion to `Str`.
fn platform_gated_str_functions(source: &str) -> Vec<usize> {
    let hidden_on_macos = |attribute: &str| {
        let compact: String = attribute.chars().filter(|c| !c.is_whitespace()).collect();
        compact.contains("target_os")
            && (!compact.contains("target_os=\"macos\"")
                || compact.contains("not(target_os=\"macos\")"))
    };
    let lines: Vec<&str> = source.lines().collect();
    let mut findings = Vec::new();
    let mut line = 0;

    while line < lines.len() {
        let trimmed = lines[line].trim();
        if !trimmed.starts_with("#[cfg") {
            line += 1;
            continue;
        }

        let cfg_line = line + 1;
        let mut attribute = String::new();
        loop {
            attribute.push_str(lines[line].trim());
            line += 1;
            if attribute.matches('[').count() == attribute.matches(']').count()
                || line == lines.len()
            {
                break;
            }
        }
        if !hidden_on_macos(&attribute) {
            continue;
        }

        while line < lines.len()
            && (lines[line].trim().is_empty()
                || lines[line].trim().starts_with("//")
                || lines[line].trim().starts_with("#["))
        {
            line += 1;
        }
        if line == lines.len()
            || !lines[line]
                .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                .any(|word| word == "fn")
        {
            continue;
        }

        let mut signature = String::new();
        while line < lines.len() {
            signature.push_str(lines[line]);
            line += 1;
            if signature.contains('{') || signature.contains(';') {
                break;
            }
        }
        let compact: String = signature.chars().filter(|c| !c.is_whitespace()).collect();
        if compact.find("->Str").is_some_and(|at| {
            !compact[at + 5..].starts_with(|c: char| c == '_' || c.is_alphanumeric())
        }) {
            findings.push(cfg_line);
        }
    }

    // A portable `Str` function can hide one branch just as effectively by
    // putting the attribute inside its body. Rustfmt makes the function's
    // closing brace the next `}` at the signature's indentation, so this pass
    // can inspect that body without pretending to parse Rust.
    let mut line = 0;
    while line < lines.len() {
        let indent = lines[line].len() - lines[line].trim_start().len();
        if !lines[line]
            .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
            .any(|word| word == "fn")
        {
            line += 1;
            continue;
        }

        let mut signature = String::new();
        while line < lines.len() {
            signature.push_str(lines[line]);
            line += 1;
            if signature.contains('{') || signature.contains(';') {
                break;
            }
        }
        let compact: String = signature.chars().filter(|c| !c.is_whitespace()).collect();
        if !compact.find("->Str").is_some_and(|at| {
            !compact[at + 5..].starts_with(|c: char| c == '_' || c.is_alphanumeric())
        }) || signature.contains(';')
        {
            continue;
        }

        while line < lines.len() {
            let body_line = lines[line];
            let trimmed = body_line.trim();
            let body_indent = body_line.len() - body_line.trim_start().len();
            if body_indent == indent && trimmed.starts_with('}') {
                break;
            }
            if trimmed.starts_with("#[cfg") {
                let cfg_line = line + 1;
                let mut attribute = String::new();
                loop {
                    attribute.push_str(lines[line].trim());
                    line += 1;
                    if attribute.matches('[').count() == attribute.matches(']').count()
                        || line == lines.len()
                    {
                        break;
                    }
                }
                if hidden_on_macos(&attribute) {
                    findings.push(cfg_line);
                }
                continue;
            }
            line += 1;
        }
    }

    findings.sort_unstable();
    findings.dedup();
    findings
}

#[cfg(test)]
mod tests {
    use super::{enclosing_call, findings, platform_gated_str_functions, string_literals};

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

    /// Keeps the *set* honest. The array's declared length already pins the
    /// count at compile time; this says the number out loud so shrinking the
    /// scan is a deliberate two-line change, and catches a path pasted twice —
    /// which would keep the length while dropping a file from the scan.
    #[test]
    fn the_scan_still_covers_every_source_it_did() {
        assert_eq!(
            super::SOURCES.len(),
            46,
            "the view scan covers fewer files than it did; add the file back, or \
             lower this count deliberately and say why"
        );

        let mut paths: Vec<&str> = super::SOURCES.iter().map(|(path, _)| *path).collect();
        paths.sort_unstable();
        let unique = paths.len();
        paths.dedup();
        assert_eq!(unique, paths.len(), "a source path is listed twice");
    }

    /// Keeps `SOURCES` honest: `include_str!` would happily embed a file that
    /// no longer builds any UI.
    ///
    /// A file builds UI if it implements `Render`, implements `RenderOnce`,
    /// opens a dialog, or returns an element from a builder function. The last
    /// three forms were added for the API Explorer's `components/` and the
    /// per-region `impl ApiExplorer` blocks, which draw translated text without
    /// implementing `Render` themselves; the Settings return types and
    /// `ListDelegate` cover the same shape after that view was split into parts.
    /// `AnyElement` is in the list because a region renderer whose result
    /// outlives the `cx` borrow has to return it boxed, in either its qualified
    /// or its imported spelling. `-> Div` covers the shared elements that hand
    /// an unfinished frame back to their caller. The guard that actually matters —
    /// `view_code_draws_no_untranslated_literals` — is unchanged.
    #[test]
    fn scanned_sources_are_the_view_sources() {
        for (path, source) in super::SOURCES {
            assert!(
                source.contains("impl Render for")
                    || source.contains("impl RenderOnce for")
                    || source.contains("open_dialog")
                    || source.contains("-> impl IntoElement")
                    || source.contains("-> gpui::AnyElement")
                    || source.contains("-> AnyElement")
                    || source.contains("-> Div")
                    || source.contains("-> SettingField")
                    || source.contains("-> SettingPage")
                    || source.contains("-> Vec<SettingPage>")
                    || source.contains("impl ListDelegate for"),
                "{path} no longer renders anything — it does not belong in SOURCES"
            );
        }
    }

    /// Every platform's labels must be type-checked on this Mac. A platform
    /// attribute may still guard callers that use native APIs, but never the
    /// `Str`-returning value-selection helper itself.
    #[test]
    fn no_platform_gate_hides_a_str_returning_function() {
        fn visit(directory: &std::path::Path, root: &std::path::Path, found: &mut Vec<String>) {
            const SKIP: [&str; 2] = [".git", "target"];

            for entry in std::fs::read_dir(directory).expect("the repository is readable") {
                let path = entry.expect("a readable directory entry").path();
                if path.is_dir() {
                    if !path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| SKIP.contains(&name))
                    {
                        visit(&path, root, found);
                    }
                } else if path.extension().is_some_and(|extension| extension == "rs")
                    && path.file_name().is_none_or(|name| name != "i18n_lint.rs")
                {
                    let source = std::fs::read_to_string(&path).expect("Rust source is readable");
                    let relative = path
                        .strip_prefix(root)
                        .expect("walked from the repository root")
                        .display();
                    found.extend(
                        platform_gated_str_functions(&source)
                            .into_iter()
                            .map(|line| format!("{relative}:{line}")),
                    );
                }
            }
        }

        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut found = Vec::new();
        visit(root, root, &mut found);
        assert!(
            found.is_empty(),
            "platform `cfg` hides `Str`-returning function(s):\n  {}\n\
             move native API calls into the gated caller and keep label selection portable",
            found.join("\n  ")
        );
    }

    #[test]
    fn the_platform_gate_classifier_is_positional() {
        let source = concat!(
            "#[",
            "cfg(target_os = \"windows\")]\n",
            "fn hidden() -> ",
            "Str { Text::Windows }\n\n",
            "#[cfg(\n    target_os = \"linux\"\n)]\n",
            "fn multiline(\n) -> ",
            "Str {\n    Text::Linux\n}\n\n",
            "#[",
            "cfg(target_os = \"windows\")]\n{\n    let value = Text::Windows;\n}\n\n",
            "// #[",
            "cfg(target_os = \"windows\")]\n",
            "fn commented_out() -> ",
            "Str { unreachable!() }\n\n",
            "fn portable() -> ",
            "Str {\n    #[",
            "cfg(target_os = \"windows\")]\n    { Text::Windows }\n}\n\n",
            "#[cfg(test)]\nfn ordinary_test() -> ",
            "Str { unreachable!() }\n",
        );
        assert_eq!(platform_gated_str_functions(source), [1, 4, 21]);
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
