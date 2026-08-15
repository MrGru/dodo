//! Re-indenting a script, and the deliberate limit on what that means.
//!
//! # Why this is not a JavaScript formatter
//!
//! The Scripts tab wanted the "Format" affordance the JSON formatter and the
//! request body already offer. A *real* JavaScript formatter — one that reflows
//! expressions, breaks long argument lists and normalises quotes — needs a full
//! parser and printer. Both candidates measured for this round
//! (`dprint-plugin-typescript`, `biome_js_formatter`) carry one, and the
//! measurement is recorded in `docs/build-optimization.md`: several megabytes on
//! a 22 MB binary, for a button. dodo's own build documentation treats 2.5% as a
//! lever worth arguing about, so that price was refused.
//!
//! What is here instead does **three** things and says so:
//!
//! 1. Re-indents every line by its bracket depth.
//! 2. Strips trailing whitespace.
//! 3. Collapses a run of blank lines to one, and drops leading and trailing
//!    blank lines.
//!
//! It never reorders, splits or joins anything. That is the safety property the
//! whole design leans on: **the only thing a line can lose is its leading and
//! trailing whitespace**, so even a mis-lexed token can at worst give a line the
//! wrong indent — it can never corrupt code or change a string.
//!
//! # Where a line is left exactly as it was
//!
//! A line that *starts* inside a template literal or a block comment is emitted
//! byte for byte. Its leading whitespace is part of a string or a comment body,
//! not indentation, and re-indenting it would silently rewrite data. The same
//! goes for a line that *ends* inside one: its trailing whitespace is content
//! too, so only the leading whitespace is touched.
//!
//! # The lexer, and what it gets wrong on purpose
//!
//! Brackets are only counted in code, so the scanner has to know about strings,
//! template literals (including `${…}` interpolation, which nests), and both
//! comment forms. The one genuinely ambiguous case in JavaScript is `/`:
//! division or the start of a regular expression. [`Scanner`] decides from the
//! previous significant token, the standard heuristic, and it is wrong on
//! contrived input such as `} / 2`. Two things bound that: a regex literal
//! cannot span a line, so the scanner resets at every newline; and a wrong guess
//! only moves indentation.
//!
//! # One level per line
//!
//! Indentation is **not** raw bracket depth. `pm.test("ok", function () {`
//! leaves three brackets open and one closed, but its body belongs one level in,
//! not two — every JavaScript reader expects that. So a line that opens more
//! than it closes indents the next line by exactly one, however many brackets
//! it opened, and a line that *starts* with closers steps out once per closer,
//! which is what puts `}));` back under the call that opened it.
//!
//! # Idempotence
//!
//! Formatting twice equals formatting once, by construction rather than by
//! luck: an output line is `indent + trimmed`, the trimmed text is unchanged on
//! a second pass, and the indent calculation reads only tokens and the running
//! level — never the whitespace it is about to replace. The tests assert it over
//! every case in the table.

/// One level of indentation. Four spaces, matching the snippets
/// [`ScriptTemplate`](crate::models::script_template::ScriptTemplate)
/// inserts, so a formatted script and a freshly inserted template agree.
const INDENT: &str = "    ";

/// A ceiling on indentation, so a file that opens a thousand brackets does not
/// produce four thousand spaces per line.
const MAX_DEPTH: usize = 32;

/// Re-indents `source`. See this module's doc for exactly what that includes.
pub fn format(source: &str) -> String {
    let mut scanner = Scanner::default();
    let mut out: Vec<String> = Vec::new();
    let mut pending_blank = false;
    let mut level = 0usize;

    for line in source.lines() {
        let started_inside = scanner.inside_multiline();
        // Only meaningful for a line that begins in code; a verbatim line is
        // not re-indented at all.
        let closers = if started_inside {
            0
        } else {
            leading_closers(line)
        };

        let (opens, closes) = scanner.scan(line);

        if started_inside {
            out.push(line.to_string());
            pending_blank = false;
            continue;
        }

        // The closers this line opens with belong to the enclosing block, so it
        // is drawn one level out per closer.
        let indent = level.saturating_sub(closers);
        // What is left open once those are discounted decides the next line, and
        // it is worth at most one level however many brackets it was.
        level = if opens + closers > closes {
            (indent + 1).min(MAX_DEPTH)
        } else {
            indent
        };

        let head = line.trim_start();
        // A line that runs into a template literal or a block comment keeps its
        // trailing whitespace: from here on it is content.
        let body = if scanner.inside_multiline() {
            head
        } else {
            head.trim_end()
        };

        if body.is_empty() {
            // Never emitted before the first real line, so a script cannot
            // acquire a blank first line.
            pending_blank = !out.is_empty();
            continue;
        }

        if pending_blank {
            out.push(String::new());
            pending_blank = false;
        }
        out.push(format!("{}{body}", INDENT.repeat(indent)));
    }

    out.join("\n")
}

/// How many closing brackets a code line opens with.
///
/// They belong to the enclosing block, so the line is drawn one level out per
/// closer — which is what puts `});` under the call that opened it rather than
/// inside it.
fn leading_closers(line: &str) -> usize {
    line.trim_start()
        .chars()
        .take_while(|c| matches!(c, '}' | ']' | ')'))
        .count()
}

/// Where the scanner is in the source.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Code,
    /// `'…'` or `"…"`. Reset at a newline: an unterminated quote is a syntax
    /// error, and confining the damage to one line is better than treating the
    /// rest of the file as a string.
    Quoted(char),
    Template,
    BlockComment,
}

/// Counts brackets through the parts of a script where brackets count.
#[derive(Default)]
struct Scanner {
    mode: ModeState,
    /// Combined `{`, `[` and `(` depth. Not what indentation is drawn from —
    /// see this module's "one level per line" note — but needed to tell the `}`
    /// that closes a `${…}` from the `}` that closes a block.
    depth: usize,
    /// The depth each open `${` was entered at.
    interpolations: Vec<usize>,
    /// Brackets opened and closed on the line being scanned.
    opens: usize,
    closes: usize,
    /// The last non-whitespace, non-comment character, for the `/` decision.
    previous: Option<char>,
    /// The identifier immediately before the cursor, for the same decision:
    /// `return /re/` is a regex, `count /re/` is not.
    word: String,
}

/// `Mode` with a `Default`, kept apart so `Scanner` can derive it.
struct ModeState(Mode);

impl Default for ModeState {
    fn default() -> Self {
        Self(Mode::Code)
    }
}

/// Keywords a `/` may legally follow as a regex.
const REGEX_KEYWORDS: [&str; 12] = [
    "return",
    "typeof",
    "instanceof",
    "in",
    "of",
    "new",
    "delete",
    "void",
    "case",
    "do",
    "else",
    "yield",
];

impl Scanner {
    /// Whether the *next* line begins inside something whose whitespace is
    /// content.
    fn inside_multiline(&self) -> bool {
        matches!(self.mode.0, Mode::Template | Mode::BlockComment)
    }

    /// Consumes one line, leaving `mode` ready for the next, and reports how
    /// many brackets it opened and closed in code.
    fn scan(&mut self, line: &str) -> (usize, usize) {
        self.opens = 0;
        self.closes = 0;
        let chars: Vec<char> = line.chars().collect();
        let mut index = 0;

        while index < chars.len() {
            let c = chars[index];
            let next = chars.get(index + 1).copied();

            match self.mode.0 {
                Mode::BlockComment => {
                    if c == '*' && next == Some('/') {
                        self.mode.0 = Mode::Code;
                        index += 2;
                        continue;
                    }
                }
                Mode::Quoted(quote) => {
                    if c == '\\' {
                        index += 2;
                        continue;
                    }
                    if c == quote {
                        self.mode.0 = Mode::Code;
                        self.previous = Some(quote);
                        self.word.clear();
                    }
                }
                Mode::Template => {
                    if c == '\\' {
                        index += 2;
                        continue;
                    }
                    if c == '`' {
                        self.mode.0 = Mode::Code;
                        self.previous = Some('`');
                        self.word.clear();
                    } else if c == '$' && next == Some('{') {
                        self.interpolations.push(self.depth);
                        self.depth += 1;
                        self.opens += 1;
                        self.mode.0 = Mode::Code;
                        index += 2;
                        continue;
                    }
                }
                Mode::Code => {
                    index = self.scan_code(&chars, index);
                    continue;
                }
            }

            index += 1;
        }

        // A quote or a regex never survives a newline; a template literal and a
        // block comment both do.
        if matches!(self.mode.0, Mode::Quoted(_)) {
            self.mode.0 = Mode::Code;
        }
        self.word.clear();
        (self.opens, self.closes)
    }

    /// One character of code. Returns the index to continue from.
    fn scan_code(&mut self, chars: &[char], index: usize) -> usize {
        let c = chars[index];
        let next = chars.get(index + 1).copied();

        match c {
            '/' if next == Some('/') => return chars.len(),
            '/' if next == Some('*') => {
                self.mode.0 = Mode::BlockComment;
                return index + 2;
            }
            '/' if self.regex_may_start() => return self.skip_regex(chars, index),
            '\'' | '"' => {
                self.mode.0 = Mode::Quoted(c);
                return index + 1;
            }
            '`' => {
                self.mode.0 = Mode::Template;
                return index + 1;
            }
            '{' | '[' | '(' => {
                self.depth += 1;
                self.opens += 1;
            }
            '}' => {
                self.depth = self.depth.saturating_sub(1);
                self.closes += 1;
                if self.interpolations.last() == Some(&self.depth) {
                    self.interpolations.pop();
                    self.mode.0 = Mode::Template;
                    self.previous = Some('}');
                    self.word.clear();
                    return index + 1;
                }
            }
            ']' | ')' => {
                self.depth = self.depth.saturating_sub(1);
                self.closes += 1;
            }
            _ => {}
        }

        if c.is_alphanumeric() || c == '_' || c == '$' {
            self.word.push(c);
        } else {
            self.word.clear();
        }
        if !c.is_whitespace() {
            self.previous = Some(c);
        }
        index + 1
    }

    /// Whether a `/` here starts a regular expression rather than dividing.
    ///
    /// The standard heuristic: a regex may follow an operator, an opening
    /// bracket, a separator or one of [`REGEX_KEYWORDS`], but not a value. `}`
    /// is genuinely ambiguous — it ends a block (regex) or an object literal
    /// (division) — and is read as a block, which is the commoner shape at the
    /// start of a statement.
    fn regex_may_start(&self) -> bool {
        if REGEX_KEYWORDS.contains(&self.word.as_str()) {
            return true;
        }
        if !self.word.is_empty() {
            return false;
        }
        match self.previous {
            None => true,
            Some(c) => matches!(
                c,
                '(' | ','
                    | '='
                    | ':'
                    | '['
                    | '!'
                    | '&'
                    | '|'
                    | '?'
                    | '{'
                    | '}'
                    | ';'
                    | '+'
                    | '-'
                    | '*'
                    | '/'
                    | '%'
                    | '~'
                    | '^'
                    | '<'
                    | '>'
            ),
        }
    }

    /// Walks past a regex literal, including a character class, and stops at the
    /// end of the line if it is unterminated.
    fn skip_regex(&mut self, chars: &[char], index: usize) -> usize {
        let mut cursor = index + 1;
        let mut in_class = false;
        while cursor < chars.len() {
            match chars[cursor] {
                '\\' => cursor += 1,
                '[' => in_class = true,
                ']' => in_class = false,
                '/' if !in_class => {
                    self.previous = Some('/');
                    self.word.clear();
                    return cursor + 1;
                }
                _ => {}
            }
            cursor += 1;
        }
        // Unterminated: treat the rest of the line as part of it and carry on
        // in code on the next line.
        self.previous = Some('/');
        self.word.clear();
        chars.len()
    }
}

#[cfg(test)]
mod tests {
    use super::format;

    /// Formats, asserts the expected text, and asserts that formatting the
    /// result again changes nothing — the stability property the whole design
    /// rests on.
    fn stable(input: &str, expected: &str) {
        let once = format(input);
        assert_eq!(once, expected, "\ninput:\n{input}");
        assert_eq!(format(&once), once, "formatting twice differed\n{once}");
    }

    #[test]
    fn a_block_is_indented_by_its_depth() {
        stable(
            "pm.test(\"ok\", function () {\npm.response.to.have.status(200);\n});",
            "pm.test(\"ok\", function () {\n    pm.response.to.have.status(200);\n});",
        );
    }

    #[test]
    fn over_indented_code_is_pulled_back() {
        stable(
            "if (a) {\n            b();\n                }",
            "if (a) {\n    b();\n}",
        );
    }

    #[test]
    fn a_line_of_closers_steps_out_once_per_closer() {
        stable(
            "call(function () {\nnest(function () {\nwork();\n}));",
            "call(function () {\n    nest(function () {\n        work();\n}));",
        );
    }

    #[test]
    fn an_else_on_a_closing_line_stays_at_the_outer_level() {
        stable(
            "if (a) {\nb();\n} else {\nc();\n}",
            "if (a) {\n    b();\n} else {\n    c();\n}",
        );
    }

    #[test]
    fn braces_inside_strings_and_comments_do_not_count() {
        stable(
            "const a = \"{{\";\n// }\nconst b = 1;",
            "const a = \"{{\";\n// }\nconst b = 1;",
        );
    }

    #[test]
    fn a_template_literals_own_lines_are_left_exactly_as_they_were() {
        // The leading spaces on line 2 are string content. Re-indenting them
        // would rewrite the value.
        stable(
            "if (a) {\nconst t = `first\n    second   \nthird`;\n}",
            "if (a) {\n    const t = `first\n    second   \nthird`;\n}",
        );
    }

    #[test]
    fn an_interpolation_inside_a_template_is_still_code() {
        stable(
            "const url = `${base}/things`;\nif (a) {\nb();\n}",
            "const url = `${base}/things`;\nif (a) {\n    b();\n}",
        );
    }

    #[test]
    fn a_brace_inside_an_interpolation_does_not_leak_out() {
        stable(
            "const t = `${ {a: 1}.a }`;\nnext();",
            "const t = `${ {a: 1}.a }`;\nnext();",
        );
    }

    #[test]
    fn a_block_comments_inner_lines_are_left_alone() {
        stable(
            "/*\n   a { b\n*/\nif (a) {\nb();\n}",
            "/*\n   a { b\n*/\nif (a) {\n    b();\n}",
        );
    }

    #[test]
    fn a_regex_containing_a_brace_is_not_a_block() {
        stable(
            "if (/\\{[a-z]+\\}/.test(url)) {\nfound();\n}",
            "if (/\\{[a-z]+\\}/.test(url)) {\n    found();\n}",
        );
    }

    #[test]
    fn a_division_is_not_read_as_a_regex() {
        stable(
            "const half = total / 2;\nif (a) {\nb();\n}",
            "const half = total / 2;\nif (a) {\n    b();\n}",
        );
    }

    #[test]
    fn trailing_whitespace_goes_and_blank_runs_collapse_to_one() {
        stable(
            "const a = 1;   \n\n\n\nconst b = 2;",
            "const a = 1;\n\nconst b = 2;",
        );
    }

    #[test]
    fn leading_and_trailing_blank_lines_are_dropped() {
        stable("\n\nconst a = 1;\n\n\n", "const a = 1;");
    }

    #[test]
    fn an_empty_script_formats_to_nothing() {
        stable("", "");
        stable("   \n\t\n", "");
    }

    #[test]
    fn a_script_that_is_already_formatted_is_untouched() {
        let source = "// Attach a bearer token to this request.\n\
                      pm.request.headers.add({\n\
                      \x20   key: \"Authorization\",\n\
                      \x20   value: \"Bearer \" + pm.variables.get(\"token\"),\n\
                      });";
        assert_eq!(format(source), source);
    }

    #[test]
    fn every_shipped_template_is_already_formatted() {
        // If a template needs reformatting the moment it is inserted, one of the
        // two is wrong.
        use crate::models::script_template::ScriptTemplate;
        for template in ScriptTemplate::PRE_REQUEST
            .iter()
            .chain(ScriptTemplate::POST_RESPONSE)
        {
            let snippet = template.snippet();
            assert_eq!(
                format(snippet),
                snippet,
                "a shipped template is not in the formatter's own shape"
            );
        }
    }

    #[test]
    fn unbalanced_closers_do_not_run_the_indent_negative() {
        stable("}\n}\n}\nconst a = 1;", "}\n}\n}\nconst a = 1;");
    }

    #[test]
    fn an_unterminated_string_only_affects_its_own_line() {
        stable(
            "const a = \"oops;\nif (b) {\nc();\n}",
            "const a = \"oops;\nif (b) {\n    c();\n}",
        );
    }

    #[test]
    fn deep_nesting_stops_growing_at_the_ceiling() {
        let opens = "{\n".repeat(40);
        let formatted = format(&format!("{opens}x();"));
        let last = formatted.lines().next_back().expect("a line");
        assert_eq!(last.len() - last.trim_start().len(), 32 * 4);
        assert_eq!(format(&formatted), formatted);
    }
}
