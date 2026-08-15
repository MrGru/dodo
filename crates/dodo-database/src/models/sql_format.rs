//! The query editor's Format action.
//!
//! # Why a crate here and a hand-rolled re-indenter for JavaScript
//!
//! The API Explorer's `models::script_format` is deliberately *not* a
//! JavaScript formatter: a real one (`dprint-plugin-typescript`) measured at
//! +2,829,280 bytes — 12.7% of the binary — and was rejected, so that module
//! only re-indents and normalises blank lines. The same reasoning points the
//! other way here, and the ratio is why: `sqlformat`'s whole subtree is four
//! crates, two of which dodo already links, and it measured at +165,232 bytes —
//! **5.8% of what the JavaScript formatter cost**. Formatting JavaScript needs
//! a parser and an AST printer; formatting SQL the way developers actually want
//! it is tokenize-and-reindent, so the crate that does it is small.
//!
//! `database::models::sql_format` is the only module allowed to name
//! `sqlformat`, the same containment rule the driver crates follow.
//!
//! # What it does not do
//!
//! It does not decide whether the SQL is valid, and it must not: the editor
//! holds statements for servers dodo has never met, and a formatter that
//! rearranged something it misread would be worse than one that did nothing.
//! `sqlformat` is a tokenizer, so text it does not recognise passes through.
//! Formatting is therefore always safe to offer and never reports an error —
//! the worst case is that the text comes back much as it went in.

use sqlformat::{Dialect, FormatOptions, Indent, QueryParams};

use super::engine::Engine;

/// The editor's text, reformatted for `engine`'s dialect.
///
/// Keyword case is left **exactly as written** (`uppercase: None`). Rewriting
/// a user's `select` to `SELECT` is a style opinion, not formatting, and it is
/// the one thing a formatter can do that makes a person stop using it.
///
/// Whitespace-only input comes back unchanged rather than as an empty string,
/// so pressing Format on an empty editor is a no-op instead of a deletion.
///
/// Everything not named below is `sqlformat`'s own default, spread in rather
/// than restated, so a new option in a later version arrives at its default
/// instead of failing the build.
pub fn format(sql: &str, engine: Engine) -> String {
    if sql.trim().is_empty() {
        return sql.to_string();
    }

    let options = FormatOptions {
        indent: Indent::Spaces(2),
        uppercase: None,
        dialect: dialect(engine),
        ..FormatOptions::default()
    };

    sqlformat::format(sql, &QueryParams::None, &options)
}

/// The formatter's dialect for an engine.
///
/// PostgreSQL gets its own because array syntax (`a[1]`) and its operators are
/// otherwise not recognised and get spaced apart. SQLite has no entry in this
/// crate and is closest to `Generic`, which is also the honest answer for any
/// engine added later until somebody checks.
fn dialect(engine: Engine) -> Dialect {
    match engine {
        Engine::PostgreSql => Dialect::PostgreSql,
        Engine::Sqlite | Engine::MySql | Engine::Redis => Dialect::Generic,
    }
}

#[cfg(test)]
mod tests {
    use super::format;
    use crate::models::engine::Engine;

    fn pg(sql: &str) -> String {
        format(sql, Engine::PostgreSql)
    }

    #[test]
    fn a_flat_statement_is_broken_onto_its_clauses() {
        let formatted = pg("select id, name from users where id = 1 order by name");
        assert!(
            formatted.lines().count() > 1,
            "nothing was reformatted:\n{formatted}"
        );
        assert!(formatted.contains("from"), "the clause words survive");
    }

    /// The one opinion this deliberately does not have.
    #[test]
    fn keyword_case_is_left_exactly_as_written() {
        assert!(pg("select 1").starts_with("select"));
        assert!(pg("SELECT 1").starts_with("SELECT"));
    }

    #[test]
    fn an_empty_or_blank_editor_comes_back_unchanged() {
        for engine in Engine::ALL {
            assert_eq!(format("", engine), "");
            assert_eq!(format("   \n ", engine), "   \n ");
        }
    }

    #[test]
    fn formatting_is_idempotent_for_every_engine() {
        for engine in Engine::ALL {
            let once = format("select a,b from t where a=1 and b=2", engine);
            assert_eq!(
                format(&once, engine),
                once,
                "formatting twice changed the text again for {}",
                engine.display_name()
            );
        }
    }

    /// The editor holds SQL for servers dodo has never met, so unrecognised
    /// text must survive rather than be rejected or mangled away.
    #[test]
    fn text_the_formatter_does_not_understand_still_comes_back() {
        assert!(pg("VACUUM ANALYZE some_table").contains("some_table"));
        assert!(pg("this is not sql at all").contains("not sql at all"));
    }

    #[test]
    fn a_string_literals_contents_are_not_reformatted() {
        let formatted = pg("select 'select from where' as s from t");
        assert!(
            formatted.contains("'select from where'"),
            "the literal was rewritten:\n{formatted}"
        );
    }

    #[test]
    fn multibyte_text_survives_formatting() {
        assert!(pg("select 'xin chào' from t").contains("xin chào"));
    }

    #[test]
    fn every_engine_formats_without_losing_the_statement() {
        for engine in Engine::ALL {
            let formatted = format("select 1 from t where a = 2", engine);
            assert!(formatted.contains('1'));
            assert!(formatted.to_lowercase().contains("from"));
        }
    }
}
