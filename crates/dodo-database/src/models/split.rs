//! Splitting an editor buffer into the statements Execute will run.
//!
//! # This is a tokenizer, and deliberately not a parser
//!
//! dodo runs SQL it cannot parse — every vendor extension, every version of
//! every dialect, every statement written after this code was. A parser that
//! rejected valid input would be a bug factory, so nothing here understands
//! what a statement *means*. It only knows where one ends: at a `;` that is not
//! inside a string, an identifier, a comment, a dollar-quoted body or a
//! `BEGIN … END` block.
//!
//! What that costs and what it buys is worth being exact about, because
//! "splitting on semicolons" sounds like a one-liner and the one-liner is
//! wrong. Each of these is a real statement that a naive split breaks:
//!
//! ```sql
//! SELECT 'a;b';                       -- a semicolon inside a literal
//! SELECT "odd;name" FROM t;           -- and inside a quoted identifier
//! SELECT 1; -- trailing; comment      -- and inside a line comment
//! CREATE FUNCTION f() RETURNS int AS $$ BEGIN RETURN 1; END $$ LANGUAGE plpgsql;
//! CREATE TRIGGER t AFTER INSERT ON x BEGIN UPDATE y SET n = 1; END;
//! ```
//!
//! # Why `BEGIN … END` is tracked and `CASE … END` with it
//!
//! A trigger or procedure body holds semicolons that do not end the outer
//! statement, and — unlike PL/pgSQL, which is dollar-quoted and therefore
//! already handled — SQLite writes that body in the open. So `BEGIN` opens a
//! block and `END` closes one.
//!
//! `CASE` then has to count as an opener too, and that is not decoration: a
//! `CASE … END` *inside* a trigger body would otherwise close the block early
//! and split the statement in the middle. Counting both keeps them balanced.
//!
//! `BEGIN` is not always a block, though — `BEGIN;`, `BEGIN TRANSACTION`,
//! `BEGIN DEFERRED` start a transaction and open nothing. [`begins_a_block`]
//! is where that distinction lives, and it is the part most likely to need a
//! new word one day.

/// Every statement in `source`, in order, trimmed, with the empty ones (a
/// trailing `;`, a buffer of only comments) dropped.
///
/// The returned text is the statement **as the user wrote it**, comments
/// included: it is what dodo sends, and it is what the result footer shows, and
/// those two must be the same string.
pub fn split_statements(source: &str) -> Vec<String> {
    let bytes = source.as_bytes();
    let mut statements = Vec::new();
    let mut start = 0usize;
    let mut i = 0usize;
    // How many `BEGIN`/`CASE` blocks are open. Clamped at zero, so a stray
    // `END` — which a dialect this has never met might allow — cannot make the
    // rest of the buffer one statement.
    let mut depth = 0usize;

    while i < bytes.len() {
        match bytes[i] {
            b'-' if bytes.get(i + 1) == Some(&b'-') => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                i = skip_block_comment(bytes, i);
            }
            b'\'' => i = skip_quoted(bytes, i, b'\''),
            b'"' => i = skip_quoted(bytes, i, b'"'),
            b'`' => i = skip_quoted(bytes, i, b'`'),
            b'$' => match dollar_tag(bytes, i) {
                Some(tag_end) => i = skip_dollar_quoted(bytes, i, tag_end),
                None => i += 1,
            },
            b';' if depth == 0 => {
                push_statement(&mut statements, &source[start..i]);
                i += 1;
                start = i;
            }
            byte if byte.is_ascii_alphabetic() => {
                let word_end = word_end(bytes, i);
                match &bytes[i..word_end] {
                    word if eq_ignore_case(word, b"case") => depth += 1,
                    word if eq_ignore_case(word, b"begin") && begins_a_block(bytes, word_end) => {
                        depth += 1;
                    }
                    word if eq_ignore_case(word, b"end") => depth = depth.saturating_sub(1),
                    _ => {}
                }
                i = word_end;
            }
            _ => i += 1,
        }
    }

    push_statement(&mut statements, &source[start..]);
    statements
}

fn push_statement(statements: &mut Vec<String>, text: &str) {
    let trimmed = text.trim();
    // A run of only comments and whitespace is not a statement. Sending one is
    // harmless — every server here answers an empty query with an empty result
    // — but it would make "Execute" on a buffer of notes report a successful
    // run of nothing, where the honest answer is that there was nothing to run.
    if !trimmed.is_empty() && !is_only_trivia(trimmed) {
        statements.push(trimmed.to_string());
    }
}

/// Whether `text` is nothing but whitespace and comments.
fn is_only_trivia(text: &str) -> bool {
    let bytes = text.as_bytes();
    skip_trivia(bytes, 0) >= bytes.len()
}

fn eq_ignore_case(word: &[u8], lower: &[u8]) -> bool {
    word.len() == lower.len() && word.eq_ignore_ascii_case(lower)
}

/// The end of the identifier starting at `at`.
fn word_end(bytes: &[u8], at: usize) -> usize {
    let mut end = at;
    while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
        end += 1;
    }
    end
}

/// Whether the `BEGIN` ending at `after` opens a block rather than a
/// transaction.
///
/// The transaction forms are a closed list of words; anything else — a bare
/// `BEGIN` followed by a statement, or `BEGIN ATOMIC` — is a body. Erring this
/// way round is deliberate: mistaking a block for a transaction splits a
/// statement in half and the error is baffling, while mistaking a transaction
/// for a block merely runs `BEGIN; …; COMMIT;` as one string, which every
/// server here accepts.
fn begins_a_block(bytes: &[u8], after: usize) -> bool {
    let at = skip_trivia(bytes, after);
    if at >= bytes.len() {
        // `BEGIN` at the very end of the buffer opens nothing worth tracking.
        return false;
    }
    if bytes[at] == b';' {
        return false;
    }
    let end = word_end(bytes, at);
    let word = &bytes[at..end];
    const TRANSACTION_WORDS: [&[u8]; 7] = [
        b"transaction",
        b"work",
        b"isolation",
        b"deferred",
        b"immediate",
        b"exclusive",
        b"read",
    ];
    !TRANSACTION_WORDS
        .iter()
        .any(|candidate| eq_ignore_case(word, candidate))
}

/// Past whitespace and comments, to the next thing that matters.
fn skip_trivia(bytes: &[u8], mut at: usize) -> usize {
    loop {
        while at < bytes.len() && bytes[at].is_ascii_whitespace() {
            at += 1;
        }
        if bytes.get(at) == Some(&b'-') && bytes.get(at + 1) == Some(&b'-') {
            while at < bytes.len() && bytes[at] != b'\n' {
                at += 1;
            }
            continue;
        }
        if bytes.get(at) == Some(&b'/') && bytes.get(at + 1) == Some(&b'*') {
            at = skip_block_comment(bytes, at);
            continue;
        }
        return at;
    }
}

/// Past a `/* … */`, honouring the nesting PostgreSQL allows. Ends at the byte
/// after the comment, or at the end of the buffer if it was never closed.
fn skip_block_comment(bytes: &[u8], at: usize) -> usize {
    let mut i = at + 2;
    let mut depth = 1usize;
    while i < bytes.len() && depth > 0 {
        if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'*') {
            depth += 1;
            i += 2;
        } else if bytes[i] == b'*' && bytes.get(i + 1) == Some(&b'/') {
            depth -= 1;
            i += 2;
        } else {
            i += 1;
        }
    }
    i
}

/// Past a `'…'`, `"…"` or `` `…` ``.
///
/// The doubled delimiter (`''`) is the SQL standard escape and is what all
/// three of these use. A backslash is **not** treated as an escape: with
/// `standard_conforming_strings` on — the default everywhere since PostgreSQL
/// 9.1 — it is an ordinary character, and treating it as an escape would make
/// `'C:\'` swallow the rest of the buffer.
fn skip_quoted(bytes: &[u8], at: usize, delimiter: u8) -> usize {
    let mut i = at + 1;
    while i < bytes.len() {
        if bytes[i] == delimiter {
            if bytes.get(i + 1) == Some(&delimiter) {
                i += 2;
                continue;
            }
            return i + 1;
        }
        i += 1;
    }
    i
}

/// If a dollar-quote opens at `at`, the byte after its opening tag.
///
/// `$$`, `$body$`, `$1$` — but not `$1` (a bind parameter) and not a `$` in the
/// middle of an identifier, both of which appear in perfectly ordinary SQL.
fn dollar_tag(bytes: &[u8], at: usize) -> Option<usize> {
    let mut i = at + 1;
    while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
        i += 1;
    }
    (bytes.get(i) == Some(&b'$')).then_some(i + 1)
}

/// Past a `$tag$ … $tag$` body. `tag_end` is the byte after the opening tag.
fn skip_dollar_quoted(bytes: &[u8], at: usize, tag_end: usize) -> usize {
    let tag = &bytes[at..tag_end];
    let mut i = tag_end;
    while i + tag.len() <= bytes.len() {
        if &bytes[i..i + tag.len()] == tag {
            return i + tag.len();
        }
        i += 1;
    }
    bytes.len()
}

#[cfg(test)]
mod tests {
    use super::split_statements;

    fn split(source: &str) -> Vec<String> {
        split_statements(source)
    }

    #[test]
    fn an_empty_buffer_has_no_statements() {
        assert!(split("").is_empty());
        assert!(split("   \n\t ").is_empty());
        assert!(split(";;;").is_empty());
    }

    #[test]
    fn a_single_statement_survives_with_and_without_a_terminator() {
        assert_eq!(split("SELECT 1"), ["SELECT 1"]);
        assert_eq!(split("SELECT 1;"), ["SELECT 1"]);
        assert_eq!(split("  SELECT 1 ;  "), ["SELECT 1"]);
    }

    #[test]
    fn statements_are_separated_and_trimmed() {
        assert_eq!(split("SELECT 1;\n\nSELECT 2;\n"), ["SELECT 1", "SELECT 2"]);
    }

    #[test]
    fn a_semicolon_inside_a_literal_does_not_split() {
        assert_eq!(split("SELECT 'a;b';"), ["SELECT 'a;b'"]);
        assert_eq!(
            split("SELECT 'it''s; fine', 2;"),
            ["SELECT 'it''s; fine', 2"]
        );
    }

    /// A Windows path in a literal. Treating `\` as an escape here would make
    /// the closing quote invisible and swallow the rest of the buffer.
    #[test]
    fn a_backslash_is_an_ordinary_character_inside_a_literal() {
        assert_eq!(
            split(r"SELECT 'C:\'; SELECT 2;"),
            [r"SELECT 'C:\'", "SELECT 2"]
        );
    }

    #[test]
    fn a_semicolon_inside_a_quoted_identifier_does_not_split() {
        assert_eq!(
            split(r#"SELECT "odd;name" FROM t;"#),
            [r#"SELECT "odd;name" FROM t"#]
        );
        assert_eq!(split("SELECT `a;b` FROM t;"), ["SELECT `a;b` FROM t"]);
    }

    #[test]
    fn a_semicolon_inside_a_comment_does_not_split() {
        assert_eq!(
            split("SELECT 1; -- and; then\nSELECT 2;"),
            ["SELECT 1", "-- and; then\nSELECT 2"]
        );
        assert_eq!(
            split("SELECT /* a;b */ 1; SELECT 2;"),
            ["SELECT /* a;b */ 1", "SELECT 2"]
        );
    }

    /// PostgreSQL allows nested block comments; an unnested skip would end the
    /// comment at the first `*/` and split on the `;` that follows.
    #[test]
    fn block_comments_nest() {
        assert_eq!(
            split("SELECT /* a /* b; */ c; */ 1; SELECT 2;"),
            ["SELECT /* a /* b; */ c; */ 1", "SELECT 2"]
        );
    }

    #[test]
    fn a_dollar_quoted_body_is_one_statement() {
        let source = "CREATE FUNCTION f() RETURNS int AS $$ BEGIN RETURN 1; END $$ LANGUAGE plpgsql;\nSELECT f();";
        assert_eq!(
            split(source),
            [
                "CREATE FUNCTION f() RETURNS int AS $$ BEGIN RETURN 1; END $$ LANGUAGE plpgsql",
                "SELECT f()"
            ]
        );
    }

    #[test]
    fn a_tagged_dollar_quote_is_matched_by_its_own_tag() {
        let source = "SELECT $body$ a; $$ still inside; $body$; SELECT 2;";
        assert_eq!(
            split(source),
            ["SELECT $body$ a; $$ still inside; $body$", "SELECT 2"]
        );
    }

    /// `$1` is a bind parameter, not the start of a dollar quote. Reading it as
    /// one would swallow everything to the next `$`.
    #[test]
    fn a_bind_parameter_is_not_a_dollar_quote() {
        assert_eq!(
            split("SELECT * FROM t WHERE id = $1; SELECT 2;"),
            ["SELECT * FROM t WHERE id = $1", "SELECT 2"]
        );
    }

    #[test]
    fn a_sqlite_trigger_body_is_one_statement() {
        let source = "CREATE TRIGGER t AFTER INSERT ON x BEGIN UPDATE y SET n = 1; DELETE FROM z; END;\nSELECT 1;";
        assert_eq!(
            split(source),
            [
                "CREATE TRIGGER t AFTER INSERT ON x BEGIN UPDATE y SET n = 1; DELETE FROM z; END",
                "SELECT 1"
            ]
        );
    }

    /// The reason `CASE` counts as an opener: without it, the `END` of this
    /// `CASE` closes the trigger's block and the statement splits in half.
    #[test]
    fn a_case_inside_a_block_does_not_close_the_block() {
        let source = "CREATE TRIGGER t AFTER INSERT ON x BEGIN \
                      UPDATE y SET n = CASE WHEN 1 THEN 2 ELSE 3 END; DELETE FROM z; END; SELECT 1;";
        let statements = split(source);
        assert_eq!(statements.len(), 2, "got {statements:?}");
        assert!(statements[0].starts_with("CREATE TRIGGER"));
        assert!(statements[0].ends_with("END"));
        assert_eq!(statements[1], "SELECT 1");
    }

    #[test]
    fn a_case_expression_on_its_own_still_ends_its_statement() {
        assert_eq!(
            split("SELECT CASE WHEN a THEN 1 ELSE 2 END FROM t; SELECT 2;"),
            ["SELECT CASE WHEN a THEN 1 ELSE 2 END FROM t", "SELECT 2"]
        );
    }

    #[test]
    fn begin_that_starts_a_transaction_opens_no_block() {
        assert_eq!(
            split("BEGIN; UPDATE t SET a = 1; COMMIT;"),
            ["BEGIN", "UPDATE t SET a = 1", "COMMIT"]
        );
        assert_eq!(
            split("BEGIN TRANSACTION; SELECT 1; COMMIT;"),
            ["BEGIN TRANSACTION", "SELECT 1", "COMMIT"]
        );
        assert_eq!(
            split("begin deferred; select 1; commit;"),
            ["begin deferred", "select 1", "commit"]
        );
    }

    #[test]
    fn keyword_matching_ignores_case_and_does_not_match_a_longer_word() {
        // `ending` and `beginning` are ordinary identifiers, not keywords.
        assert_eq!(
            split("SELECT ending, beginning FROM t; SELECT 2;"),
            ["SELECT ending, beginning FROM t", "SELECT 2"]
        );
        assert_eq!(
            split("select case when a then 1 end from t; select 2;"),
            ["select case when a then 1 end from t", "select 2"]
        );
    }

    /// A stray `END` must not put the tokenizer into a state it never leaves.
    #[test]
    fn an_unbalanced_end_does_not_swallow_the_rest_of_the_buffer() {
        assert_eq!(split("END; SELECT 1;"), ["END", "SELECT 1"]);
    }

    #[test]
    fn an_unterminated_literal_or_comment_ends_at_the_buffer() {
        assert_eq!(split("SELECT 'unclosed"), ["SELECT 'unclosed"]);
        assert_eq!(split("SELECT 1 /* unclosed"), ["SELECT 1 /* unclosed"]);
        assert_eq!(split("SELECT $$unclosed"), ["SELECT $$unclosed"]);
    }

    #[test]
    fn a_buffer_of_only_comments_has_no_statements() {
        assert!(split("-- nothing here\n/* nor here */").is_empty());
    }

    #[test]
    fn multibyte_text_is_carried_through_unharmed() {
        assert_eq!(
            split("SELECT 'xin chào'; SELECT 'こんにちは';"),
            ["SELECT 'xin chào'", "SELECT 'こんにちは'"]
        );
    }
}
