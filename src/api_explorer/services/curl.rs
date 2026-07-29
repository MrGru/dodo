//! Reading a cURL command back into a request.
//!
//! Every API's documentation, every browser's "Copy as cURL", and every bug
//! report pastes the same artefact: one long shell command. Retyping it into
//! eight fields is the tax this module removes — paste it into the URL box and
//! the whole request appears.
//!
//! # Shape of the problem
//!
//! Two independent grammars sit on top of each other, and they are handled in
//! that order:
//!
//! 1. **Shell quoting** ([`tokenize`]). Single quotes are literal, double
//!    quotes honour a handful of backslash escapes, `$'…'` is ANSI-C quoting,
//!    and a trailing `\` continues the line. This layer knows nothing about
//!    cURL.
//! 2. **cURL's own options** ([`parse`]). Flags this app has no equivalent for
//!    (`--compressed`, `-k`, `-s`, `-L`, `-v`, …) are *ignored*, never fatal:
//!    a command that is 90% understood is worth far more than a refusal.
//!
//! # What it will not do
//!
//! It is not a shell. Command substitution, variables, pipes and redirections
//! are treated as ordinary text, so `curl "$URL"` yields a request whose URL is
//! literally `$URL` — which shows up in the URL box as something to fix, rather
//! than being guessed at.
//!
//! The output is a [`RequestSnapshot`], the same plain-data capture a saved
//! collection entry holds, so applying one to a tab reuses
//! `RequestState::apply_snapshot` rather than a second restore path.

use reqwest::Url;

use crate::api_explorer::models::auth::{AuthDraft, AuthType};
use crate::api_explorer::models::body::{BodyDraft, BodyType};
use crate::api_explorer::models::key_value::KeyValue;
use crate::api_explorer::models::method::HttpMethod;
use crate::api_explorer::models::snapshot::RequestSnapshot;

/// Long options this app has no equivalent for that still take an argument.
///
/// They have to be listed because the argument would otherwise be mistaken for
/// the URL. An unknown flag that is *not* here loses only itself, which is the
/// right trade: dropping one option beats dropping the whole command.
const IGNORED_WITH_ARGUMENT: &[&str] = &[
    "--connect-timeout",
    "--max-time",
    "--retry",
    "--retry-delay",
    "--limit-rate",
    "--proxy",
    "--proxy-user",
    "--cacert",
    "--capath",
    "--cert",
    "--key",
    "--output",
    "--resolve",
    "--interface",
    "--write-out",
    "--dump-header",
    "--range",
    "--continue-at",
    "--cookie-jar",
    "--max-redirs",
];

/// Short options that take an argument and have no dodo equivalent.
const IGNORED_SHORT_WITH_ARGUMENT: &[char] = &['m', 'x', 'o', 'w', 'D', 'r', 'C', 'c', 'y', 'Y'];

/// Whether `text` is worth handing to [`parse`] at all.
///
/// Only the first word is examined, so this stays cheap enough to run on every
/// keystroke in the URL field. `/usr/bin/curl` and `curl.exe` count; a URL that
/// merely contains the word does not.
pub fn looks_like_curl(text: &str) -> bool {
    let trimmed = text.trim_start();
    let Some(first) = trimmed.split_whitespace().next() else {
        return false;
    };
    let command = first
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(first)
        .trim_end_matches(".exe");
    command.eq_ignore_ascii_case("curl") && trimmed.len() > first.len()
}

/// Parses a cURL command into a request, or `None` if it is not one.
///
/// `None` also covers a command with no URL in it: there is nothing useful to
/// put in a tab, so the caller leaves the pasted text alone.
pub fn parse(text: &str) -> Option<RequestSnapshot> {
    if !looks_like_curl(text) {
        return None;
    }
    let tokens = tokenize(text);
    let mut walk = tokens.iter();
    walk.next(); // The `curl` itself.

    let mut url: Option<String> = None;
    let mut method: Option<HttpMethod> = None;
    let mut headers: Vec<KeyValue> = Vec::new();
    let mut form: Vec<KeyValue> = Vec::new();
    let mut data: Vec<DataArgument> = Vec::new();
    let mut auth = AuthDraft::default();
    let mut force_get = false;

    while let Some(token) = walk.next() {
        // `--flag=value` is the same as `--flag value`; splitting here means
        // every long option below is written once.
        let (token, attached) = match token.split_once('=') {
            Some((name, value)) if name.starts_with("--") => (name.to_string(), Some(value)),
            _ => (token.clone(), None),
        };
        match token.as_str() {
            "-X" | "--request" => {
                if let Some(value) = take(attached, &mut walk) {
                    method = method_from(&value);
                }
            }
            "-H" | "--header" => {
                if let Some(value) = take(attached, &mut walk) {
                    push_header(&value, &mut headers, &mut auth);
                }
            }
            "--url" => {
                if let Some(value) = take(attached, &mut walk) {
                    url.get_or_insert(value);
                }
            }
            "-u" | "--user" => {
                if let Some(value) = take(attached, &mut walk) {
                    apply_basic(&value, &mut auth);
                }
            }
            "-b" | "--cookie" => {
                if let Some(value) = take(attached, &mut walk) {
                    // A value with no `=` is a cookie *file* to curl, and this
                    // app has nothing to do with one.
                    if value.contains('=') {
                        headers.push(KeyValue::text("Cookie", value));
                    }
                }
            }
            "-A" | "--user-agent" => {
                if let Some(value) = take(attached, &mut walk) {
                    headers.push(KeyValue::text("User-Agent", value));
                }
            }
            "-e" | "--referer" => {
                if let Some(value) = take(attached, &mut walk) {
                    headers.push(KeyValue::text("Referer", value));
                }
            }
            "-d" | "--data" | "--data-ascii" | "--data-binary" => {
                if let Some(value) = take(attached, &mut walk) {
                    data.push(DataArgument::from_maybe_file(&value));
                }
            }
            "--data-raw" => {
                // `--data-raw` is the one form where a leading `@` is literal.
                if let Some(value) = take(attached, &mut walk) {
                    data.push(DataArgument::Text(value));
                }
            }
            "--data-urlencode" => {
                if let Some(value) = take(attached, &mut walk) {
                    data.push(DataArgument::UrlEncode(value));
                }
            }
            "-F" | "--form" => {
                if let Some(value) = take(attached, &mut walk) {
                    push_form(&value, true, &mut form);
                }
            }
            "--form-string" => {
                if let Some(value) = take(attached, &mut walk) {
                    push_form(&value, false, &mut form);
                }
            }
            "-G" | "--get" => force_get = true,
            other if IGNORED_WITH_ARGUMENT.contains(&other) => {
                let _ = take(attached, &mut walk);
            }
            other if other.starts_with("--") => {}
            other if other.starts_with('-') && other.len() > 1 => {
                short_option(other, &mut walk, &mut |name, value| match name {
                    'X' => method = method_from(value),
                    'H' => push_header(value, &mut headers, &mut auth),
                    'u' => apply_basic(value, &mut auth),
                    'b' if value.contains('=') => headers.push(KeyValue::text("Cookie", value)),
                    'A' => headers.push(KeyValue::text("User-Agent", value)),
                    'e' => headers.push(KeyValue::text("Referer", value)),
                    'd' => data.push(DataArgument::from_maybe_file(value)),
                    'F' => push_form(value, true, &mut form),
                    _ => {}
                });
            }
            _ => {
                url.get_or_insert(token);
            }
        }
    }

    let url = url?;
    let mut snapshot = RequestSnapshot {
        url,
        headers,
        auth,
        ..RequestSnapshot::default()
    };
    split_query(&mut snapshot);

    let content_type = header_value(&snapshot.headers, "content-type");
    if force_get {
        // `-G` turns every data argument into query parameters instead.
        for (key, value) in data.iter().flat_map(DataArgument::as_pairs) {
            snapshot.params.push(KeyValue::text(key, value));
        }
        data.clear();
    }

    snapshot.body = build_body(&data, &form, content_type.as_deref());
    // curl's own rule: a body without an explicit method is a POST. `-G` has
    // already emptied the body, so it lands in the GET branch on its own.
    snapshot.method = method.unwrap_or(if snapshot.body.kind == BodyType::None {
        HttpMethod::Get
    } else {
        HttpMethod::Post
    });

    Some(snapshot)
}

/// The value an option carries: attached with `=`, or the next token.
fn take(attached: Option<&str>, walk: &mut std::slice::Iter<'_, String>) -> Option<String> {
    attached
        .map(str::to_string)
        .or_else(|| walk.next().cloned())
}

/// One `-d`-family argument, kept in the form that says how to read it.
#[derive(Debug, PartialEq, Eq)]
enum DataArgument {
    /// Literal payload text.
    Text(String),
    /// `--data-urlencode name=value`: the value is unencoded and curl would
    /// escape it, so this maps onto a urlencoded form row.
    UrlEncode(String),
    /// `@path`: curl reads the file. Maps onto the Binary body type.
    File(String),
}

impl DataArgument {
    /// Reads a `-d`/`--data-binary` argument, honouring the `@file` form.
    fn from_maybe_file(value: &str) -> Self {
        match value.strip_prefix('@') {
            Some(path) if !path.is_empty() => DataArgument::File(path.to_string()),
            _ => DataArgument::Text(value.to_string()),
        }
    }

    /// The payload as `key=value` pairs, for `-G` and for a urlencoded body.
    /// Text that is not shaped like a form yields one pair with an empty value,
    /// which is what curl itself would send.
    fn as_pairs(&self) -> Vec<(String, String)> {
        match self {
            DataArgument::File(_) => Vec::new(),
            DataArgument::Text(text) | DataArgument::UrlEncode(text) => text
                .split('&')
                .filter(|piece| !piece.is_empty())
                .map(|piece| match piece.split_once('=') {
                    Some((key, value)) => (key.to_string(), value.to_string()),
                    None => (piece.to_string(), String::new()),
                })
                .collect(),
        }
    }

    fn as_text(&self) -> Option<&str> {
        match self {
            DataArgument::Text(text) | DataArgument::UrlEncode(text) => Some(text),
            DataArgument::File(_) => None,
        }
    }
}

/// Chooses the body type the data and form arguments describe.
///
/// Multipart wins outright when `-F` is present — curl refuses to mix the two.
/// Otherwise the `Content-Type` header decides, and with no header the shape of
/// the payload does: `a=1&b=2` is a form, `{…}` is JSON, anything else is raw
/// text.
fn build_body(data: &[DataArgument], form: &[KeyValue], content_type: Option<&str>) -> BodyDraft {
    if !form.is_empty() {
        return BodyDraft {
            kind: BodyType::FormData,
            fields: form.to_vec(),
            ..BodyDraft::default()
        };
    }

    if let Some(DataArgument::File(path)) = data.iter().find(|arg| arg.as_text().is_none()) {
        return BodyDraft {
            kind: BodyType::Binary,
            file_path: path.clone(),
            ..BodyDraft::default()
        };
    }

    let joined = data
        .iter()
        .filter_map(DataArgument::as_text)
        .collect::<Vec<_>>()
        .join("&");
    if joined.is_empty() {
        return BodyDraft::default();
    }

    let declared = content_type.map(str::to_ascii_lowercase);
    let urlencoded = match declared.as_deref() {
        Some(value) if value.contains("x-www-form-urlencoded") => true,
        Some(_) => false,
        // Everything curl sends without an explicit type is urlencoded by
        // default, but only text actually shaped like a form is worth putting
        // in the table rather than the editor.
        None => {
            data.iter()
                .any(|arg| matches!(arg, DataArgument::UrlEncode(_)))
                || looks_like_form(&joined)
        }
    };

    if urlencoded {
        return BodyDraft {
            kind: BodyType::UrlEncoded,
            fields: data
                .iter()
                .flat_map(DataArgument::as_pairs)
                .map(|(key, value)| KeyValue::text(key, value))
                .collect(),
            ..BodyDraft::default()
        };
    }

    let kind = match declared.as_deref() {
        Some(value) if value.contains("json") => BodyType::Json,
        Some(value) if value.contains("xml") => BodyType::Xml,
        Some(value) if value.contains("html") => BodyType::Html,
        Some(_) => BodyType::Text,
        None if joined.starts_with('{') || joined.starts_with('[') => BodyType::Json,
        None => BodyType::Text,
    };
    BodyDraft {
        kind,
        text: joined,
        ..BodyDraft::default()
    }
}

/// Whether a payload is `key=value` pairs and nothing else.
fn looks_like_form(text: &str) -> bool {
    !text.is_empty()
        && !text.contains(char::is_whitespace)
        && text.split('&').all(|piece| {
            piece
                .split_once('=')
                .is_some_and(|(key, _)| !key.is_empty())
        })
}

/// Splits the URL's own query string into parameter rows.
///
/// A URL that does not parse — a `{{variable}}` host, say — keeps its query, on
/// the principle that mangling what was pasted is worse than not splitting it.
fn split_query(snapshot: &mut RequestSnapshot) {
    let Ok(mut parsed) = Url::parse(&snapshot.url) else {
        return;
    };
    let pairs: Vec<KeyValue> = parsed
        .query_pairs()
        .map(|(key, value)| KeyValue::text(key.into_owned(), value.into_owned()))
        .collect();
    if pairs.is_empty() {
        return;
    }
    parsed.set_query(None);
    // `Url` re-serializes a bare host with a trailing slash; the query is what
    // was being removed, so put the text back the way it was typed otherwise.
    snapshot.url = parsed.to_string();
    snapshot.params = pairs;
}

/// Records a `-H` header, lifting the ones the Auth tab expresses better.
fn push_header(value: &str, headers: &mut Vec<KeyValue>, auth: &mut AuthDraft) {
    let Some((name, value)) = value.split_once(':') else {
        // `-H 'X-Trace;'` is curl's way of sending an empty header.
        let name = value.trim_end_matches(';').trim();
        if !name.is_empty() {
            headers.push(KeyValue::text(name, ""));
        }
        return;
    };
    let name = name.trim();
    let value = value.trim();

    if name.eq_ignore_ascii_case("authorization")
        && let Some(token) = value
            .strip_prefix("Bearer ")
            .or(value.strip_prefix("bearer "))
        && auth.kind == AuthType::None
    {
        auth.kind = AuthType::Bearer;
        auth.token = token.trim().to_string();
        return;
    }

    headers.push(KeyValue::text(name, value));
}

/// `-u user:password`, which is the Auth tab's Basic scheme.
fn apply_basic(value: &str, auth: &mut AuthDraft) {
    let (username, password) = match value.split_once(':') {
        Some((username, password)) => (username, password),
        // curl prompts for the password in this case; there is nobody to
        // prompt here, so the field is left blank for the user to fill in.
        None => (value, ""),
    };
    auth.kind = AuthType::Basic;
    auth.username = username.to_string();
    auth.password = password.to_string();
}

/// Records a `-F` form field. `interpret` is false for `--form-string`, where
/// `@` and `<` are literal.
fn push_form(value: &str, interpret: bool, form: &mut Vec<KeyValue>) {
    let Some((name, rest)) = value.split_once('=') else {
        return;
    };
    if !interpret {
        form.push(KeyValue::text(name, rest));
        return;
    }

    // `;type=…` and `;filename=…` qualify the part. dodo sniffs the media type
    // from the extension instead of carrying an explicit one, so the qualifier
    // is parsed off and dropped rather than confusing the path.
    let payload = rest.split(';').next().unwrap_or(rest);
    match payload.strip_prefix('@').or(payload.strip_prefix('<')) {
        Some(path) if !path.is_empty() => form.push(KeyValue::file(name, path)),
        _ => form.push(KeyValue::text(name, payload)),
    }
}

fn header_value(headers: &[KeyValue], name: &str) -> Option<String> {
    headers
        .iter()
        .find(|row| row.key.eq_ignore_ascii_case(name))
        .map(|row| row.value.clone())
}

fn method_from(value: &str) -> Option<HttpMethod> {
    HttpMethod::ALL
        .into_iter()
        .find(|method| method.as_str().eq_ignore_ascii_case(value.trim()))
}

/// Handles a short-option token, including the bundled (`-sSL`) and attached
/// (`-XPOST`) forms.
///
/// Each character is offered to `apply` in turn; the first one that takes an
/// argument consumes the rest of the token, or the next token, and ends the
/// bundle — which is what stops `-o /dev/null` from mistaking the path for the
/// URL.
fn short_option(
    token: &str,
    walk: &mut std::slice::Iter<'_, String>,
    apply: &mut impl FnMut(char, &str),
) {
    const WITH_ARGUMENT: &[char] = &['X', 'H', 'd', 'F', 'u', 'b', 'A', 'e'];

    let chars: Vec<char> = token.chars().skip(1).collect();
    for (index, name) in chars.iter().enumerate() {
        let takes_argument =
            WITH_ARGUMENT.contains(name) || IGNORED_SHORT_WITH_ARGUMENT.contains(name);
        if !takes_argument {
            continue;
        }
        let rest: String = chars[index + 1..].iter().collect();
        let argument = if rest.is_empty() {
            walk.next().cloned()
        } else {
            Some(rest)
        };
        if let Some(argument) = argument
            && WITH_ARGUMENT.contains(name)
        {
            apply(*name, &argument);
        }
        return;
    }
}

/// Splits a command line into words the way a POSIX shell would, for the
/// quoting forms that appear in a pasted cURL command.
///
/// - `'…'` is literal throughout, including backslashes.
/// - `"…"` honours `\"`, `\\`, `` \` ``, `\$` and a line continuation; every
///   other backslash stays as it was typed.
/// - `$'…'` is ANSI-C quoting, so `\n`, `\t`, `\r`, `\\` and `\'` decode.
/// - Outside quotes, a backslash before a newline — or before the run of
///   spaces a pasted newline collapsed into — continues the line and
///   disappears. Elsewhere it escapes the next character.
///
/// A closing quote that never arrives ends the word at the end of the text
/// rather than failing: half a pasted command is still worth parsing.
fn tokenize(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut started = false;
    let mut chars = text.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            c if c.is_whitespace() => {
                if started {
                    tokens.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            '\'' => {
                started = true;
                for c in chars.by_ref() {
                    if c == '\'' {
                        break;
                    }
                    current.push(c);
                }
            }
            '"' => {
                started = true;
                while let Some(c) = chars.next() {
                    match c {
                        '"' => break,
                        '\\' => match chars.peek() {
                            Some('"' | '\\' | '`' | '$') => {
                                current.push(chars.next().expect("peeked"))
                            }
                            Some('\n') => {
                                chars.next();
                            }
                            _ => current.push('\\'),
                        },
                        c => current.push(c),
                    }
                }
            }
            '$' if chars.peek() == Some(&'\'') => {
                started = true;
                chars.next();
                while let Some(c) = chars.next() {
                    match c {
                        '\'' => break,
                        '\\' => match chars.next() {
                            Some('n') => current.push('\n'),
                            Some('t') => current.push('\t'),
                            Some('r') => current.push('\r'),
                            Some('0') => current.push('\0'),
                            Some(other) => current.push(other),
                            None => break,
                        },
                        c => current.push(c),
                    }
                }
            }
            '\\' => match chars.peek() {
                // A line continuation. The single-line URL field strips the
                // newline out of a pasted command, so the backslash is left
                // followed by the next line's indent — both forms end here.
                Some('\n' | '\r') => {
                    chars.next();
                }
                Some(' ' | '\t') if !started => {
                    while matches!(chars.peek(), Some(' ' | '\t')) {
                        chars.next();
                    }
                }
                Some(_) => {
                    started = true;
                    current.push(chars.next().expect("peeked"));
                }
                None => {}
            },
            c => {
                started = true;
                current.push(c);
            }
        }
    }

    if started {
        tokens.push(current);
    }
    tokens
}

#[cfg(test)]
mod tests {
    use super::{looks_like_curl, parse, tokenize};
    use crate::api_explorer::models::auth::AuthType;
    use crate::api_explorer::models::body::BodyType;
    use crate::api_explorer::models::key_value::{FieldKind, KeyValue};
    use crate::api_explorer::models::method::HttpMethod;
    use crate::api_explorer::models::snapshot::RequestSnapshot;

    fn parsed(command: &str) -> RequestSnapshot {
        parse(command).unwrap_or_else(|| panic!("should have parsed: {command}"))
    }

    fn header(snapshot: &RequestSnapshot, name: &str) -> Option<String> {
        snapshot
            .headers
            .iter()
            .find(|row| row.key.eq_ignore_ascii_case(name))
            .map(|row| row.value.clone())
    }

    // ---- Recognition ------------------------------------------------------

    #[test]
    fn only_a_command_that_starts_with_curl_is_one() {
        assert!(looks_like_curl("curl https://example.com"));
        assert!(looks_like_curl("  CURL https://example.com"));
        assert!(looks_like_curl("/usr/bin/curl https://example.com"));
        assert!(looks_like_curl("curl.exe https://example.com"));

        assert!(!looks_like_curl("https://example.com/curl"));
        assert!(!looks_like_curl("curlyurl.example.com"));
        // `curl` on its own has nothing to parse out of it.
        assert!(!looks_like_curl("curl"));
        assert!(!looks_like_curl(""));
    }

    #[test]
    fn a_command_with_no_url_yields_nothing_to_open() {
        assert!(parse("curl -X POST -H 'Accept: */*'").is_none());
        assert!(parse("https://example.com").is_none());
    }

    // ---- Tokenizing -------------------------------------------------------

    #[test]
    fn quotes_group_words_and_disappear() {
        assert_eq!(
            tokenize(r#"curl -H 'Accept: application/json' "https://a.b/c d""#),
            ["curl", "-H", "Accept: application/json", "https://a.b/c d"]
        );
    }

    #[test]
    fn a_single_quote_is_literal_and_a_double_quote_escapes() {
        assert_eq!(tokenize(r#"a 'b\nc'"#), ["a", r"b\nc"]);
        assert_eq!(tokenize(r#"a "b\"c\\d""#), ["a", r#"b"c\d"#]);
        // A backslash with no meaning inside double quotes stays put.
        assert_eq!(tokenize(r#"a "b\nc""#), ["a", r"b\nc"]);
    }

    #[test]
    fn ansi_c_quoting_decodes_its_escapes() {
        assert_eq!(
            tokenize("a $'line\\nbreak\\ttab'"),
            ["a", "line\nbreak\ttab"]
        );
        assert_eq!(tokenize("a $'it\\'s'"), ["a", "it's"]);
    }

    #[test]
    fn a_trailing_backslash_continues_the_line() {
        let command = "curl 'https://a.b' \\\n  -H 'Accept: x' \\\n  -d 'q=1'";
        assert_eq!(
            tokenize(command),
            ["curl", "https://a.b", "-H", "Accept: x", "-d", "q=1"]
        );
    }

    #[test]
    fn a_continuation_survives_the_url_field_eating_the_newline() {
        // A single-line `InputState` strips `\n` out of a paste, leaving the
        // backslash against the next line's indent.
        let pasted = "curl 'https://a.b' \\  -H 'Accept: x' \\-d 'q=1'";
        assert_eq!(
            tokenize(pasted),
            ["curl", "https://a.b", "-H", "Accept: x", "-d", "q=1"]
        );
    }

    #[test]
    fn an_unterminated_quote_still_yields_what_came_before_it() {
        assert_eq!(tokenize("curl 'https://a.b"), ["curl", "https://a.b"]);
    }

    // ---- Real-world commands ----------------------------------------------

    #[test]
    fn the_simplest_command_is_a_get() {
        let snapshot = parsed("curl https://api.example.com/users");
        assert_eq!(snapshot.method, HttpMethod::Get);
        assert_eq!(snapshot.url, "https://api.example.com/users");
        assert!(snapshot.headers.is_empty());
        assert_eq!(snapshot.body.kind, BodyType::None);
    }

    #[test]
    fn an_explicit_method_wins() {
        assert_eq!(
            parsed("curl -X DELETE https://a.b/things/1").method,
            HttpMethod::Delete
        );
        assert_eq!(
            parsed("curl --request patch https://a.b/things/1").method,
            HttpMethod::Patch
        );
        // The attached form browsers emit.
        assert_eq!(parsed("curl -XPUT https://a.b/x").method, HttpMethod::Put);
    }

    #[test]
    fn a_body_without_a_method_is_a_post() {
        assert_eq!(
            parsed("curl -d 'a=1' https://a.b/x").method,
            HttpMethod::Post
        );
        assert_eq!(
            parsed("curl -F 'name=Ada' https://a.b/x").method,
            HttpMethod::Post
        );
        // …and an explicit method still overrides it.
        assert_eq!(
            parsed("curl -X PUT -d 'a=1' https://a.b/x").method,
            HttpMethod::Put
        );
    }

    #[test]
    fn the_query_string_moves_into_the_params_table() {
        let snapshot = parsed("curl 'https://a.b/search?q=rust+lang&page=2'");
        assert_eq!(snapshot.url, "https://a.b/search");
        assert_eq!(
            snapshot.params,
            [
                KeyValue::text("q", "rust lang"),
                KeyValue::text("page", "2")
            ]
        );
    }

    #[test]
    fn a_url_with_no_query_keeps_its_shape() {
        let snapshot = parsed("curl https://a.b/search");
        assert_eq!(snapshot.url, "https://a.b/search");
        assert!(snapshot.params.is_empty());
    }

    #[test]
    fn headers_land_in_the_headers_table() {
        let snapshot = parsed(
            "curl 'https://a.b/x' -H 'Accept: application/json' \
             -H 'X-Trace: abc' --header 'X-Empty;'",
        );
        assert_eq!(
            header(&snapshot, "accept").as_deref(),
            Some("application/json")
        );
        assert_eq!(header(&snapshot, "X-Trace").as_deref(), Some("abc"));
        assert_eq!(header(&snapshot, "X-Empty").as_deref(), Some(""));
    }

    #[test]
    fn a_bearer_token_becomes_the_auth_tab_rather_than_a_header() {
        let snapshot = parsed("curl https://a.b/x -H 'Authorization: Bearer abc.def.ghi'");
        assert_eq!(snapshot.auth.kind, AuthType::Bearer);
        assert_eq!(snapshot.auth.token, "abc.def.ghi");
        assert!(
            header(&snapshot, "authorization").is_none(),
            "the token was left duplicated in the headers table"
        );
    }

    #[test]
    fn an_authorization_scheme_with_no_auth_tab_equivalent_stays_a_header() {
        let snapshot = parsed("curl https://a.b/x -H 'Authorization: Digest xyz'");
        assert_eq!(snapshot.auth.kind, AuthType::None);
        assert_eq!(
            header(&snapshot, "authorization").as_deref(),
            Some("Digest xyz")
        );
    }

    #[test]
    fn basic_auth_fills_the_auth_tab() {
        let snapshot = parsed("curl -u ada:l0velace https://a.b/x");
        assert_eq!(snapshot.auth.kind, AuthType::Basic);
        assert_eq!(snapshot.auth.username, "ada");
        assert_eq!(snapshot.auth.password, "l0velace");

        // curl prompts for a missing password; there is nobody to prompt.
        let snapshot = parsed("curl --user ada https://a.b/x");
        assert_eq!(snapshot.auth.username, "ada");
        assert!(snapshot.auth.password.is_empty());
    }

    #[test]
    fn cookies_become_a_cookie_header() {
        let snapshot = parsed("curl -b 'session=abc; theme=dark' https://a.b/x");
        assert_eq!(
            header(&snapshot, "cookie").as_deref(),
            Some("session=abc; theme=dark")
        );
        // A cookie *file* has no equivalent and is dropped, not misread.
        let snapshot = parsed("curl -b cookies.txt https://a.b/x");
        assert!(header(&snapshot, "cookie").is_none());
        assert_eq!(snapshot.url, "https://a.b/x");
    }

    #[test]
    fn a_json_body_is_read_from_its_content_type() {
        let snapshot = parsed(
            "curl -X POST https://a.b/things -H 'Content-Type: application/json' \
             -d '{\"name\":\"Ada\"}'",
        );
        assert_eq!(snapshot.body.kind, BodyType::Json);
        assert_eq!(snapshot.body.text, r#"{"name":"Ada"}"#);
    }

    #[test]
    fn a_json_body_is_recognised_by_shape_when_no_type_was_given() {
        let snapshot = parsed(r#"curl https://a.b/x --data-raw '{"a":1}'"#);
        assert_eq!(snapshot.body.kind, BodyType::Json);
        assert_eq!(snapshot.body.text, r#"{"a":1}"#);
    }

    #[test]
    fn a_form_shaped_payload_becomes_urlencoded_rows() {
        let snapshot = parsed("curl https://a.b/login -d 'user=ada&pass=secret'");
        assert_eq!(snapshot.body.kind, BodyType::UrlEncoded);
        assert_eq!(
            snapshot.body.fields,
            [
                KeyValue::text("user", "ada"),
                KeyValue::text("pass", "secret")
            ]
        );
    }

    #[test]
    fn repeated_data_arguments_are_joined_the_way_curl_joins_them() {
        let snapshot = parsed("curl https://a.b/x -d 'a=1' -d 'b=2'");
        assert_eq!(snapshot.body.kind, BodyType::UrlEncoded);
        assert_eq!(
            snapshot.body.fields,
            [KeyValue::text("a", "1"), KeyValue::text("b", "2")]
        );
    }

    #[test]
    fn data_urlencode_is_taken_as_written_rather_than_double_escaped() {
        let snapshot = parsed("curl https://a.b/x --data-urlencode 'q=a b&c'");
        assert_eq!(snapshot.body.kind, BodyType::UrlEncoded);
        assert_eq!(
            snapshot.body.fields,
            [KeyValue::text("q", "a b"), KeyValue::text("c", "")]
        );
    }

    #[test]
    fn free_text_with_a_declared_type_stays_raw() {
        let snapshot =
            parsed("curl https://a.b/x -H 'Content-Type: text/plain' -d 'just some words'");
        assert_eq!(snapshot.body.kind, BodyType::Text);
        assert_eq!(snapshot.body.text, "just some words");
    }

    #[test]
    fn an_xml_content_type_picks_the_xml_editor() {
        let snapshot = parsed("curl https://a.b/x -H 'Content-Type: application/xml' -d '<a/>'");
        assert_eq!(snapshot.body.kind, BodyType::Xml);
    }

    #[test]
    fn a_data_file_argument_becomes_a_binary_body() {
        let snapshot = parsed("curl -X POST https://a.b/upload --data-binary @/tmp/payload.pdf");
        assert_eq!(snapshot.body.kind, BodyType::Binary);
        assert_eq!(snapshot.body.file_path, "/tmp/payload.pdf");
    }

    #[test]
    fn data_raw_keeps_a_leading_at_sign_literal() {
        let snapshot = parsed("curl https://a.b/x --data-raw '@not-a-file'");
        assert_eq!(snapshot.body.kind, BodyType::Text);
        assert_eq!(snapshot.body.text, "@not-a-file");
    }

    #[test]
    fn form_arguments_become_typed_multipart_rows() {
        let snapshot = parsed(
            "curl https://a.b/upload -F 'name=Ada' -F 'avatar=@/tmp/a.png' \
             -F 'cv=@/tmp/cv.pdf;type=application/pdf'",
        );
        assert_eq!(snapshot.body.kind, BodyType::FormData);
        assert_eq!(
            snapshot.body.fields,
            [
                KeyValue::text("name", "Ada"),
                KeyValue::file("avatar", "/tmp/a.png"),
                KeyValue::file("cv", "/tmp/cv.pdf"),
            ]
        );
    }

    #[test]
    fn form_string_never_reads_a_file() {
        let snapshot = parsed("curl https://a.b/x --form-string 'note=@literal'");
        assert_eq!(snapshot.body.fields[0].kind, FieldKind::Text);
        assert_eq!(snapshot.body.fields[0].value, "@literal");
    }

    #[test]
    fn the_angle_bracket_form_uploads_a_file_too() {
        let snapshot = parsed("curl https://a.b/x -F 'doc=</tmp/notes.txt'");
        assert_eq!(
            snapshot.body.fields,
            [KeyValue::file("doc", "/tmp/notes.txt")]
        );
    }

    #[test]
    fn flags_with_no_equivalent_are_ignored_rather_than_fatal() {
        let snapshot = parsed(
            "curl --compressed -k -s -L -v -i --http1.1 --retry 3 \
             --connect-timeout 5 -o /dev/null https://a.b/x",
        );
        assert_eq!(snapshot.url, "https://a.b/x");
        assert_eq!(snapshot.method, HttpMethod::Get);
        assert!(snapshot.headers.is_empty());
    }

    #[test]
    fn a_bundle_of_short_flags_does_not_swallow_the_url() {
        let snapshot = parsed("curl -sSL https://a.b/x");
        assert_eq!(snapshot.url, "https://a.b/x");
    }

    #[test]
    fn the_get_flag_turns_data_into_query_parameters() {
        let snapshot = parsed("curl -G https://a.b/search -d 'q=rust' -d 'page=2'");
        assert_eq!(snapshot.method, HttpMethod::Get);
        assert_eq!(snapshot.body.kind, BodyType::None);
        assert_eq!(
            snapshot.params,
            [KeyValue::text("q", "rust"), KeyValue::text("page", "2")]
        );
    }

    #[test]
    fn the_url_flag_names_the_url_explicitly() {
        let snapshot = parsed("curl --url https://a.b/x -X POST");
        assert_eq!(snapshot.url, "https://a.b/x");
        assert_eq!(snapshot.method, HttpMethod::Post);
    }

    #[test]
    fn a_long_option_written_with_an_equals_sign_still_parses() {
        let snapshot = parsed("curl --request=POST --header='Accept: text/csv' https://a.b/x");
        assert_eq!(snapshot.method, HttpMethod::Post);
        assert_eq!(header(&snapshot, "accept").as_deref(), Some("text/csv"));
    }

    #[test]
    fn the_user_agent_and_referer_shortcuts_become_headers() {
        let snapshot = parsed("curl -A 'dodo/1.0' -e 'https://ref.example' https://a.b/x");
        assert_eq!(header(&snapshot, "user-agent").as_deref(), Some("dodo/1.0"));
        assert_eq!(
            header(&snapshot, "referer").as_deref(),
            Some("https://ref.example")
        );
    }

    #[test]
    fn a_copy_as_curl_command_from_a_browser_parses_whole() {
        // The shape Chrome's "Copy as cURL" produces, newlines and all.
        let command = "curl 'https://api.example.com/v2/orders?status=open&limit=50' \\\n\
                       \x20 -X POST \\\n\
                       \x20 -H 'accept: application/json' \\\n\
                       \x20 -H 'authorization: Bearer eyJhbGciOi.J9' \\\n\
                       \x20 -H 'content-type: application/json' \\\n\
                       \x20 --data-raw '{\"sku\":\"A-1\",\"qty\":2}' \\\n\
                       \x20 --compressed";
        let snapshot = parsed(command);

        assert_eq!(snapshot.method, HttpMethod::Post);
        assert_eq!(snapshot.url, "https://api.example.com/v2/orders");
        assert_eq!(
            snapshot.params,
            [
                KeyValue::text("status", "open"),
                KeyValue::text("limit", "50")
            ]
        );
        assert_eq!(
            header(&snapshot, "accept").as_deref(),
            Some("application/json")
        );
        assert_eq!(snapshot.auth.kind, AuthType::Bearer);
        assert_eq!(snapshot.auth.token, "eyJhbGciOi.J9");
        assert_eq!(snapshot.body.kind, BodyType::Json);
        assert_eq!(snapshot.body.text, r#"{"sku":"A-1","qty":2}"#);
    }

    #[test]
    fn a_multipart_upload_command_parses_whole() {
        let command = "curl -X POST https://api.example.com/files \\\n\
                       \x20 -H 'Authorization: Bearer tok' \\\n\
                       \x20 -F 'meta={\"title\":\"Report\"};type=application/json' \\\n\
                       \x20 -F 'file=@/Users/ada/report.pdf' \\\n\
                       \x20 --form-string 'note=see @page 3'";
        let snapshot = parsed(command);

        assert_eq!(snapshot.method, HttpMethod::Post);
        assert_eq!(snapshot.body.kind, BodyType::FormData);
        assert_eq!(
            snapshot.body.fields,
            [
                KeyValue::text("meta", r#"{"title":"Report"}"#),
                KeyValue::file("file", "/Users/ada/report.pdf"),
                KeyValue::text("note", "see @page 3"),
            ]
        );
        assert_eq!(snapshot.auth.token, "tok");
    }

    #[test]
    fn a_variable_where_the_host_goes_is_left_for_the_user_to_fix() {
        let snapshot = parsed("curl '{{base_url}}/api/123' -H 'Accept: */*'");
        assert_eq!(snapshot.url, "{{base_url}}/api/123");
        assert!(snapshot.params.is_empty());
        assert_eq!(header(&snapshot, "accept").as_deref(), Some("*/*"));
    }

    #[test]
    fn a_shell_variable_is_left_as_text_rather_than_guessed_at() {
        let snapshot = parsed("curl \"$BASE/things\"");
        assert_eq!(snapshot.url, "$BASE/things");
    }
}
