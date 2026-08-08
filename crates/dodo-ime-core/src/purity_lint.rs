//! A source-level guard that this crate depends on nothing but `std` and
//! `unicode-normalization`.
//!
//! # Why this exists
//!
//! Three native hosts are coming — a macOS InputMethodKit `.app`, a Windows TSF
//! DLL, an IBus engine — and every one of them is a *separate process* loaded
//! into somebody else's application. They must link this engine; they must not
//! link gpui, or `rust-embed`, or bollard, or dodo's settings. An input method
//! that dragged a UI framework into every text field on the machine would be
//! unacceptable even if it worked.
//!
//! **That boundary is now a real crate boundary, not an aspirational one.** This
//! code was `src/input_method/` for one round, compiled into the `dodo` binary,
//! where a single `use crate::i18n::Str` would have compiled perfectly well and
//! nothing but this file would have objected. It is its own crate now, so the
//! compiler objects first: `crate::` reaches only into this crate, and `use
//! gpui::…` fails to resolve because `Cargo.toml` does not name it.
//!
//! # What `Cargo.toml` cannot say, and this still can
//!
//! Two things, which is why the file survived the extraction rather than being
//! deleted with the problem it was written for:
//!
//! - **A dependency is one line.** Adding `gpui = …` to this crate's manifest
//!   compiles; nothing warns. The allow-list below turns that one line into a
//!   failing test, so growing this crate's dependency list is a decision someone
//!   makes on purpose rather than a change that slips through review. The rule
//!   holds for any crate, including ones nobody thought to forbid by name —
//!   [`ALLOWED_ROOTS`] is an allow-list, not a block-list.
//! - **Sideways is the workspace's shape, not cargo's.** `crates/` will hold the
//!   hosts, and the engine must never depend on one of *them* either. `dodo::`
//!   and any future sibling are caught by the same allow-list.
//!
//! [`tests::the_scan_covers_every_file`] is the third: it proves the check
//! actually reads every file, which no manifest can.
//!
//! # What is allowed
//!
//! - `std` — the whole point is that this is plain Rust.
//! - `crate`, `super`, `self` — paths within this crate.
//! - `unicode_normalization` — the single external crate, and the one thing NFC
//!   cannot be done correctly without. See
//!   [`unicode`](crate::languages::vietnamese::unicode).
//!
//! Everything else fails, and the failure message says which file and which
//! line.
//!
//! # How it decides
//!
//! By reading the source, like dodo's own `i18n_lint` — the module this is
//! modelled on. It looks at the root segment of every `use`, at `extern crate`,
//! and at a short list of names that would be a violation even without a `use`
//! (a fully-qualified `gpui::px(…)` needs no import). It is a guard, not a
//! proof: a macro could still smuggle a path in. Nothing here uses one.

/// Every source file of the crate, embedded so the check needs no working
/// directory. [`tests::the_scan_covers_every_file`] keeps the list complete.
const SOURCES: [(&str, &str); 18] = [
    ("lib.rs", include_str!("lib.rs")),
    ("testing.rs", include_str!("testing.rs")),
    ("core/mod.rs", include_str!("core/mod.rs")),
    ("core/action.rs", include_str!("core/action.rs")),
    ("core/candidate.rs", include_str!("core/candidate.rs")),
    ("core/composition.rs", include_str!("core/composition.rs")),
    ("core/engine.rs", include_str!("core/engine.rs")),
    ("core/event.rs", include_str!("core/event.rs")),
    ("core/language.rs", include_str!("core/language.rs")),
    ("languages/mod.rs", include_str!("languages/mod.rs")),
    (
        "languages/vietnamese/mod.rs",
        include_str!("languages/vietnamese/mod.rs"),
    ),
    (
        "languages/vietnamese/rules.rs",
        include_str!("languages/vietnamese/rules.rs"),
    ),
    (
        "languages/vietnamese/syllable.rs",
        include_str!("languages/vietnamese/syllable.rs"),
    ),
    (
        "languages/vietnamese/telex.rs",
        include_str!("languages/vietnamese/telex.rs"),
    ),
    (
        "languages/vietnamese/tone.rs",
        include_str!("languages/vietnamese/tone.rs"),
    ),
    (
        "languages/vietnamese/unicode.rs",
        include_str!("languages/vietnamese/unicode.rs"),
    ),
    (
        "languages/vietnamese/vni.rs",
        include_str!("languages/vietnamese/vni.rs"),
    ),
    (
        "languages/vietnamese/word_boundary.rs",
        include_str!("languages/vietnamese/word_boundary.rs"),
    ),
];

/// Files that are part of the crate but not of the shipped engine, so they are
/// listed for completeness and skipped by the import check.
///
/// Two files qualify, and both are entirely `#[cfg(test)]`: the word corpus and
/// the engine's behaviour tables. Every other file's tests *are* scanned — a
/// test module is still code in the crate, and an import it drags in is one a
/// `dev-dependency` would have to carry, which the OS hosts would then have to
/// keep out of their own builds.
const TEST_ONLY: [&str; 2] = [
    "languages/vietnamese/corpus.rs",
    "languages/vietnamese/tests.rs",
];

/// The root segment of a path that may be imported. An allow-list, so a crate
/// nobody thought to forbid is forbidden anyway.
///
/// `crate`, `super` and `self` are all *within* this crate now — they were the
/// module-relative forms before the extraction, and the `crate::input_method::…`
/// prefix the old version had to special-case is simply gone.
const ALLOWED_ROOTS: [&str; 5] = ["std", "crate", "super", "self", "unicode_normalization"];

/// Names that are a violation wherever they appear, `use` or not — a
/// fully-qualified `gpui::px(1.)` imports nothing.
///
/// Not exhaustive and not trying to be: every one of these is caught by the
/// `use` check as well. They are here to catch the fully-qualified spelling,
/// which is the one shape that would otherwise slip past. `dodo::` is the
/// sideways case: the workspace is where a sibling crate could be reached for.
const FORBIDDEN_NAMES: [&str; 7] = [
    "gpui::",
    "gpui_component",
    "dodo::",
    "dodo_ime_",
    "serde::",
    "bollard",
    "rust_embed",
];

/// Everything after `use ` or `pub use `, up to the first `;`, per line.
fn imported_path(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    let rest = trimmed
        .strip_prefix("pub use ")
        .or_else(|| trimmed.strip_prefix("use "))?;
    Some(rest.trim_end_matches(';').trim())
}

/// The first `::`-separated segment of an import path, with any leading `::`
/// removed.
fn root_of(path: &str) -> &str {
    path.trim_start_matches("::")
        .split("::")
        .next()
        .unwrap_or(path)
        .trim()
}

/// Import violations in one file.
fn findings_in(path: &str, source: &str) -> Vec<String> {
    let mut findings = Vec::new();

    for (index, line) in source.lines().enumerate() {
        let number = index + 1;
        // Doc comments talk about all of these by name.
        if line.trim_start().starts_with("//") {
            continue;
        }

        if line.trim_start().starts_with("extern crate") {
            findings.push(format!("{path}:{number} — `extern crate` is not allowed"));
        }

        if let Some(imported) = imported_path(line) {
            let root = root_of(imported);
            if !ALLOWED_ROOTS.contains(&root) {
                findings.push(format!("{path}:{number} — `use {imported};`"));
            }
        }

        for name in FORBIDDEN_NAMES {
            if line.contains(name) {
                findings.push(format!("{path}:{number} — names `{name}`"));
            }
        }
    }

    findings
}

/// Every import violation in the crate.
fn findings() -> Vec<String> {
    SOURCES
        .iter()
        .flat_map(|(path, source)| findings_in(path, source))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{SOURCES, TEST_ONLY, findings, findings_in, imported_path, root_of};

    /// The guard itself.
    ///
    /// A failure here means this crate has grown a dependency — on a sibling in
    /// the workspace, or on a crate somebody just added to `Cargo.toml`. The fix
    /// is never to widen `ALLOWED_ROOTS`: pass the value in from the caller
    /// instead, at the boundary where the OS host or the settings layer already
    /// is.
    #[test]
    fn the_engine_depends_on_nothing_but_std() {
        let findings = findings();
        assert!(
            findings.is_empty(),
            "{} forbidden dependency reference(s) in `dodo-ime-core`:\n  {}",
            findings.len(),
            findings.join("\n  ")
        );
    }

    /// `include_str!` happily embeds a stale list, so the directory is walked
    /// at test time and compared. A new file that nobody added here would
    /// otherwise be exempt from the whole check.
    #[test]
    fn the_scan_covers_every_file() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut found: Vec<String> = Vec::new();
        let mut stack = vec![root.clone()];
        while let Some(directory) = stack.pop() {
            for entry in std::fs::read_dir(&directory).expect("the crate source is readable") {
                let path = entry.expect("a readable directory entry").path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().is_some_and(|ext| ext == "rs") {
                    let relative = path
                        .strip_prefix(&root)
                        .expect("walked from the root")
                        .to_string_lossy()
                        .replace('\\', "/");
                    found.push(relative);
                }
            }
        }
        found.sort();

        let mut listed: Vec<String> = SOURCES
            .iter()
            .map(|(path, _)| (*path).to_string())
            .chain(TEST_ONLY.iter().map(|path| (*path).to_string()))
            // This file is the check; scanning itself would find every name it
            // exists to forbid.
            .chain(std::iter::once("purity_lint.rs".to_string()))
            .collect();
        listed.sort();

        assert_eq!(
            found, listed,
            "the file list in `purity_lint` no longer matches the crate's src/"
        );
    }

    #[test]
    fn the_import_reader_finds_the_path() {
        assert_eq!(imported_path("use std::fmt;"), Some("std::fmt"));
        assert_eq!(imported_path("    use super::rules;"), Some("super::rules"));
        assert_eq!(
            imported_path("pub use tone::TonePlacement;"),
            Some("tone::TonePlacement")
        );
        assert_eq!(imported_path("let used = 1;"), None);
        // A commented-out import is not an import; `findings_in` skips the
        // line before it ever gets here.
        assert_eq!(imported_path("// use gpui::*;"), None);

        assert_eq!(root_of("crate::core"), "crate");
        assert_eq!(root_of("::std::fmt"), "std");
        assert_eq!(root_of("super::super::core"), "super");
    }

    /// The check has to actually fail on the things it claims to catch,
    /// otherwise it is decoration.
    #[test]
    fn the_forbidden_shapes_are_caught() {
        let cases = [
            "use gpui::*;",
            "use gpui_component::Icon;",
            // Sideways: a sibling crate in the workspace, which `Cargo.toml`
            // would happily accept as one added line.
            "use dodo::i18n::Str;",
            "use dodo_ime_macos::Host;",
            "use serde::Serialize;",
            "use regex::Regex;",
            "use std::collections::HashMap;\nextern crate alloc;",
            "let size = gpui::px(4.);",
            "#[derive(serde::Serialize)]",
        ];
        for source in cases {
            assert!(
                !findings_in("probe.rs", source).is_empty(),
                "not caught: {source}"
            );
        }
    }

    #[test]
    fn the_allowed_shapes_are_not_caught() {
        let cases = [
            "use std::ops::Range;",
            "use super::syllable::Syllable;",
            "use super::super::core::KeyEvent;",
            "use crate::core::{EngineAction, KeyEvent};",
            "use unicode_normalization::UnicodeNormalization;",
            "use unicode_normalization::char::is_combining_mark;",
            "//! mentions crate::i18n and gpui:: in prose",
            "// use gpui::*; in a commented-out line",
        ];
        for source in cases {
            assert!(
                findings_in("probe.rs", source).is_empty(),
                "false alarm: {source} -> {:?}",
                findings_in("probe.rs", source)
            );
        }
    }
}
