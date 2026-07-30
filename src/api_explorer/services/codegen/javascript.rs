//! The three JavaScript targets: `fetch`, `axios` and `XMLHttpRequest`.
//!
//! One module rather than three, because what they share is most of the file:
//! string literals, the headers object, the body-building lines and — the part
//! worth reading before changing anything here — what to do about the two bodies
//! JavaScript cannot express.
//!
//! # A file upload cannot be written down
//!
//! cURL takes a path (`-F 'file=@/tmp/a.png'`) and reads it. JavaScript has no
//! such thing: `FormData.append` wants a `File`, which comes from a file input or
//! a drop, and a browser cannot open `/tmp/a.png` at all. The same goes for
//! [`NormalizedBody::File`].
//!
//! Three ways to handle that, and only one of them is honest:
//!
//! - Emit nothing for the part. The snippet then silently sends a *different*
//!   request from the one on screen, which is the failure mode this whole feature
//!   exists to avoid.
//! - Emit a value that exists — `null`, `""`, the path as a string. Worse: it
//!   runs, the server accepts it, and the upload is empty.
//! - Emit an **undeclared identifier**, with a comment above it naming the field
//!   and the path dodo would have sent. The snippet is complete and readable, and
//!   running it unmodified throws a `ReferenceError` on that exact line instead
//!   of quietly sending half a request.
//!
//! The third is what this does. The identifier is `file1`, `file2`, … rather than
//! something derived from the field name, so it can never collide with a
//! variable the reader already has and never needs sanitizing into a legal
//! identifier.
//!
//! # Duplicate header names
//!
//! HTTP allows them and dodo's Headers table allows them; a JavaScript object
//! literal holds a name once. `fetch` can take an **array of pairs** instead, so
//! it switches to one when a name repeats and keeps the more readable object
//! otherwise. `XMLHttpRequest` has no problem at all — `setRequestHeader` appends
//! — so it is one call per row. `axios` takes only an object, so the duplicate is
//! emitted with a comment saying which value actually goes on the wire. In all
//! three the reader can see what happened.
//!
//! # Why the comments are English
//!
//! They are part of a code snippet, like `models::script_template`'s bodies: the
//! surrounding UI is translated and the code is not, for the same reason a header
//! token or an editor language id is not. Nothing here goes through [`Str`].
//!
//! [`Str`]: crate::i18n::Str

use crate::api_explorer::services::codegen::{NormalizedBody, NormalizedPart, NormalizedRequest};

/// `await fetch(...)`, async/await throughout.
pub fn fetch(request: &NormalizedRequest) -> String {
    let mut out = String::new();
    let body = body_lines(request, "body");
    out.push_str(&body.preamble);

    out.push_str(&format!(
        "const response = await fetch({}, {{\n  method: {},\n",
        string(&request.url),
        string(request.method.as_str())
    ));
    if !request.headers.is_empty() {
        out.push_str(&format!("  headers: {},\n", fetch_headers(request)));
    }
    if let Some(expression) = &body.expression {
        out.push_str(&format!("  body: {expression},\n"));
    }
    out.push_str("});\n\n");
    out.push_str("const data = await response.text();\nconsole.log(response.status, data);\n");
    out
}

/// `await axios.request(...)`, the config-object form so every field is named.
pub fn axios(request: &NormalizedRequest) -> String {
    let mut out = String::from("import axios from \"axios\";\n\n");
    let body = body_lines(request, "data");
    out.push_str(&body.preamble);

    out.push_str(&format!(
        "const response = await axios.request({{\n  method: {},\n  url: {},\n",
        // axios lowercases the method itself; lowercase reads as the idiom.
        string(&request.method.as_str().to_ascii_lowercase()),
        string(&request.url)
    ));
    if !request.headers.is_empty() {
        out.push_str(&object_headers(request, "  "));
    }
    if let Some(expression) = &body.expression {
        out.push_str(&format!("  data: {expression},\n"));
    }
    out.push_str("});\n\nconsole.log(response.status, response.data);\n");
    out
}

/// `new XMLHttpRequest()`, callback style — the API has no promise form.
pub fn xhr(request: &NormalizedRequest) -> String {
    let mut out = String::new();
    let body = body_lines(request, "body");
    out.push_str(&body.preamble);

    out.push_str("const xhr = new XMLHttpRequest();\n");
    out.push_str(&format!(
        "xhr.open({}, {});\n",
        string(request.method.as_str()),
        string(&request.url)
    ));
    for (name, value) in &request.headers {
        // One call per row, duplicates included: `setRequestHeader` appends to
        // an existing name rather than replacing it, which is what the Headers
        // table means by two rows.
        out.push_str(&format!(
            "xhr.setRequestHeader({}, {});\n",
            string(name),
            string(value)
        ));
    }
    out.push_str(
        "\nxhr.onload = () => console.log(xhr.status, xhr.responseText);\n\
         xhr.onerror = () => console.error(\"The request failed.\");\n",
    );
    match &body.expression {
        Some(expression) => out.push_str(&format!("xhr.send({expression});\n")),
        None => out.push_str("xhr.send();\n"),
    }
    out
}

/// The body, split into the statements that build it and the expression that
/// refers to it.
struct Body {
    /// Lines emitted before the request itself. Empty for a body that is a
    /// literal, and for no body at all.
    preamble: String,
    /// What to pass — `None` when there is no body.
    expression: Option<String>,
}

/// Builds the body for one target. `name` is the identifier the preamble
/// declares, which differs only so that each snippet reads naturally next to the
/// field it is assigned to.
fn body_lines(request: &NormalizedRequest, name: &str) -> Body {
    match &request.body {
        NormalizedBody::None => Body {
            preamble: String::new(),
            expression: None,
        },

        NormalizedBody::Text(text) => Body {
            preamble: String::new(),
            expression: Some(literal(text)),
        },

        NormalizedBody::UrlEncoded(fields) => {
            let mut preamble = format!("const {name} = new URLSearchParams();\n");
            for (key, value) in fields {
                preamble.push_str(&format!(
                    "{name}.append({}, {});\n",
                    string(key),
                    string(value)
                ));
            }
            preamble.push('\n');
            Body {
                preamble,
                expression: Some(name.to_string()),
            }
        }

        NormalizedBody::Multipart(parts) => {
            let mut preamble = format!("const {name} = new FormData();\n");
            let mut files = 0usize;
            for part in parts {
                match part {
                    NormalizedPart::Text { name: key, value } => preamble.push_str(&format!(
                        "{name}.append({}, {});\n",
                        string(key),
                        string(value)
                    )),
                    NormalizedPart::File { name: key, path } => {
                        files += 1;
                        let identifier = format!("file{files}");
                        preamble.push_str(&file_comment(&identifier, Some(key), path));
                        preamble
                            .push_str(&format!("{name}.append({}, {identifier});\n", string(key)));
                    }
                }
            }
            preamble.push('\n');
            Body {
                preamble,
                expression: Some(name.to_string()),
            }
        }

        NormalizedBody::File { path, .. } => Body {
            preamble: format!("{}\n", file_comment("file1", None, path)),
            expression: Some("file1".to_string()),
        },
    }
}

/// The comment that stands where a file would be.
///
/// It names the identifier, the field (for a multipart part) and the path, and
/// says outright that the identifier is undeclared on purpose — see this
/// module's doc for why that is the honest option.
fn file_comment(identifier: &str, field: Option<&str>, path: &str) -> String {
    let what = match field {
        Some(field) => format!("the \"{field}\" part"),
        None => "the request body".to_string(),
    };
    format!(
        "// dodo sends {path} as {what}. JavaScript cannot open a path: assign a\n\
         // File (from an <input type=\"file\">, a drop, or Node's fs) to `{identifier}`.\n\
         // Left undeclared on purpose, so running this unchanged throws rather than\n\
         // sending an incomplete request.\n"
    )
}

/// `fetch`'s `headers` value: an object when every name is distinct, an array of
/// pairs when one repeats.
fn fetch_headers(request: &NormalizedRequest) -> String {
    if !has_duplicate_names(&request.headers) {
        // Indented for a value sitting at one level inside the options object,
        // which is where the caller puts it.
        return object_literal(&request.headers, "    ", "  ");
    }
    let mut out = String::from("[\n");
    for (name, value) in &request.headers {
        out.push_str(&format!("    [{}, {}],\n", string(name), string(value)));
    }
    out.push_str("  ]");
    out
}

/// The `headers:` line of an axios config, with the duplicate caveat when it
/// applies.
fn object_headers(request: &NormalizedRequest, indent: &str) -> String {
    let mut out = String::new();
    for name in duplicate_names(&request.headers) {
        out.push_str(&format!(
            "{indent}// dodo sends \"{name}\" more than once. An object holds it once, so only\n\
             {indent}// the last value below is sent; use `new Headers()` if you need both.\n"
        ));
    }
    out.push_str(&format!(
        "{indent}headers: {},\n",
        object_literal(&request.headers, &format!("{indent}  "), indent)
    ));
    out
}

/// `{ "a": "1" }` over as many lines as it has entries.
fn object_literal(pairs: &[(String, String)], indent: &str, closing_indent: &str) -> String {
    let mut out = String::from("{\n");
    for (key, value) in pairs {
        out.push_str(&format!("{indent}{}: {},\n", string(key), string(value)));
    }
    out.push_str(closing_indent);
    out.push('}');
    out
}

fn has_duplicate_names(headers: &[(String, String)]) -> bool {
    !duplicate_names(headers).is_empty()
}

/// The header names that appear more than once, matched the way HTTP matches
/// them, each reported once and in first-appearance order.
fn duplicate_names(headers: &[(String, String)]) -> Vec<String> {
    let mut repeated: Vec<String> = Vec::new();
    for (index, (name, _)) in headers.iter().enumerate() {
        let appears_again = headers[index + 1..]
            .iter()
            .any(|(other, _)| other.eq_ignore_ascii_case(name));
        let already_reported = repeated
            .iter()
            .any(|other| other.eq_ignore_ascii_case(name));
        if appears_again && !already_reported {
            repeated.push(name.clone());
        }
    }
    repeated
}

/// A body document as a JavaScript expression.
///
/// A template literal for anything spanning lines, because a pretty-printed JSON
/// body escaped onto one line is unreadable — and unreadable generated code gets
/// rewritten by hand, which defeats the point. Only when the text contains a
/// backtick or a `${` (which a template literal would interpolate) does it fall
/// back to a quoted string, where nothing can be misread.
fn literal(text: &str) -> String {
    if text.contains('\n') && !text.contains('`') && !text.contains("${") {
        return format!("`{text}`");
    }
    string(text)
}

/// A double-quoted JavaScript string literal.
///
/// Everything outside printable ASCII stays as it is — a JavaScript source file
/// is UTF-8, so an accented character or an emoji in a header value is legible
/// rather than a `\u` run. Only what would end the literal or break the line is
/// escaped, plus the C0 controls, which are invisible and would otherwise be
/// copied as raw bytes.
fn string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::{axios, duplicate_names, fetch, literal, string, xhr};
    use crate::api_explorer::models::auth::{AuthDraft, AuthType};
    use crate::api_explorer::models::body::{BodyDraft, BodyType};
    use crate::api_explorer::models::key_value::KeyValue;
    use crate::api_explorer::models::method::HttpMethod;
    use crate::api_explorer::models::snapshot::RequestSnapshot;
    use crate::api_explorer::models::variables::VariableSet;
    use crate::api_explorer::services::codegen::{NormalizedRequest, normalize};

    /// The request the acceptance criteria name — the same one the cURL tests
    /// use, so the four snippets can be read side by side.
    fn everything() -> RequestSnapshot {
        RequestSnapshot {
            method: HttpMethod::Post,
            url: "https://api.example.com/v2/orders".into(),
            params: vec![
                KeyValue::text("status", "open"),
                KeyValue::text("limit", "50"),
            ],
            headers: vec![KeyValue::text("Accept", "application/json")],
            body: BodyDraft {
                kind: BodyType::Json,
                text: r#"{"sku":"A-1","qty":2}"#.into(),
                ..BodyDraft::default()
            },
            auth: AuthDraft {
                kind: AuthType::Bearer,
                token: "eyJhbGciOi.J9".into(),
                ..AuthDraft::default()
            },
            ..RequestSnapshot::default()
        }
    }

    fn of(snapshot: &RequestSnapshot) -> NormalizedRequest {
        normalize(snapshot, &VariableSet::default()).expect("normalizes")
    }

    // ---- The three snippets, whole -----------------------------------------

    #[test]
    fn fetch_names_every_part_of_the_request() {
        assert_eq!(
            fetch(&of(&everything())),
            "const response = await fetch(\"https://api.example.com/v2/orders?status=open&limit=50\", {\n  \
               method: \"POST\",\n  \
               headers: {\n    \
                 \"Accept\": \"application/json\",\n    \
                 \"Authorization\": \"Bearer eyJhbGciOi.J9\",\n    \
                 \"Content-Type\": \"application/json\",\n  \
               },\n  \
               body: \"{\\\"sku\\\":\\\"A-1\\\",\\\"qty\\\":2}\",\n\
             });\n\n\
             const data = await response.text();\n\
             console.log(response.status, data);\n"
        );
    }

    #[test]
    fn axios_uses_the_config_object_form() {
        assert_eq!(
            axios(&of(&everything())),
            "import axios from \"axios\";\n\n\
             const response = await axios.request({\n  \
               method: \"post\",\n  \
               url: \"https://api.example.com/v2/orders?status=open&limit=50\",\n  \
               headers: {\n    \
                 \"Accept\": \"application/json\",\n    \
                 \"Authorization\": \"Bearer eyJhbGciOi.J9\",\n    \
                 \"Content-Type\": \"application/json\",\n  \
               },\n  \
               data: \"{\\\"sku\\\":\\\"A-1\\\",\\\"qty\\\":2}\",\n\
             });\n\n\
             console.log(response.status, response.data);\n"
        );
    }

    #[test]
    fn xhr_sets_one_header_per_call() {
        assert_eq!(
            xhr(&of(&everything())),
            "const xhr = new XMLHttpRequest();\n\
             xhr.open(\"POST\", \"https://api.example.com/v2/orders?status=open&limit=50\");\n\
             xhr.setRequestHeader(\"Accept\", \"application/json\");\n\
             xhr.setRequestHeader(\"Authorization\", \"Bearer eyJhbGciOi.J9\");\n\
             xhr.setRequestHeader(\"Content-Type\", \"application/json\");\n\n\
             xhr.onload = () => console.log(xhr.status, xhr.responseText);\n\
             xhr.onerror = () => console.error(\"The request failed.\");\n\
             xhr.send(\"{\\\"sku\\\":\\\"A-1\\\",\\\"qty\\\":2}\");\n"
        );
    }

    #[test]
    fn a_request_with_no_headers_and_no_body_omits_both() {
        let snapshot = RequestSnapshot {
            url: "https://example.com/things".into(),
            ..RequestSnapshot::default()
        };
        let request = of(&snapshot);
        assert_eq!(
            fetch(&request),
            "const response = await fetch(\"https://example.com/things\", {\n  \
               method: \"GET\",\n\
             });\n\n\
             const data = await response.text();\n\
             console.log(response.status, data);\n"
        );
        assert!(xhr(&request).contains("xhr.send();"));
        assert!(!axios(&request).contains("headers"));
    }

    // ---- Bodies -------------------------------------------------------------

    #[test]
    fn a_urlencoded_body_becomes_url_search_params_in_every_target() {
        let mut snapshot = everything();
        snapshot.headers.clear();
        snapshot.body = BodyDraft {
            kind: BodyType::UrlEncoded,
            fields: vec![KeyValue::text("user", "ada"), KeyValue::text("pass", "a b")],
            ..BodyDraft::default()
        };
        let request = of(&snapshot);

        for code in [fetch(&request), axios(&request), xhr(&request)] {
            assert!(code.contains("new URLSearchParams()"), "{code}");
            // Unescaped: `URLSearchParams` does the escaping, so pre-escaping
            // here would send `a+b` as a literal plus sign.
            assert!(code.contains(r#".append("pass", "a b")"#), "{code}");
        }
        assert!(fetch(&request).contains("body: body,"));
        assert!(axios(&request).contains("data: data,"));
        assert!(xhr(&request).contains("xhr.send(body);"));
    }

    #[test]
    fn a_multipart_text_field_is_appended_for_real() {
        let mut snapshot = everything();
        snapshot.body = BodyDraft {
            kind: BodyType::FormData,
            fields: vec![KeyValue::text("name", "Ada")],
            ..BodyDraft::default()
        };
        let code = fetch(&of(&snapshot));
        assert!(code.contains("const body = new FormData();"));
        assert!(code.contains(r#"body.append("name", "Ada")"#));
        // The browser sets `multipart/form-data` with its own boundary, so
        // nothing here declares one.
        assert!(!code.contains("multipart/form-data"));
    }

    #[test]
    fn a_multipart_file_part_is_an_undeclared_identifier_with_the_path_named() {
        let mut snapshot = everything();
        snapshot.headers.clear();
        snapshot.body = BodyDraft {
            kind: BodyType::FormData,
            fields: vec![
                KeyValue::text("name", "Ada"),
                KeyValue::file("avatar", "/Users/ada/a.png"),
                KeyValue::file("cv", "/Users/ada/cv.pdf"),
            ],
            ..BodyDraft::default()
        };

        for code in [
            fetch(&of(&snapshot)),
            axios(&of(&snapshot)),
            xhr(&of(&snapshot)),
        ] {
            // The path is named, so the reader knows which file to supply…
            assert!(code.contains("/Users/ada/a.png"), "{code}");
            assert!(code.contains("/Users/ada/cv.pdf"), "{code}");
            // …the identifiers are distinct and never declared…
            assert!(code.contains(r#".append("avatar", file1)"#), "{code}");
            assert!(code.contains(r#".append("cv", file2)"#), "{code}");
            assert!(!code.contains("const file1"), "{code}");
            // …and the snippet says why, rather than looking like an oversight.
            assert!(code.contains("Left undeclared on purpose"), "{code}");
            // The path must never be passed as a string, which would run and
            // upload the text of the path.
            assert!(!code.contains(r#""/Users/ada/a.png""#), "{code}");
        }
    }

    #[test]
    fn a_binary_body_gets_the_same_treatment() {
        let mut snapshot = everything();
        snapshot.headers.clear();
        snapshot.body = BodyDraft {
            kind: BodyType::Binary,
            file_path: "/tmp/payload.pdf".into(),
            ..BodyDraft::default()
        };
        let code = fetch(&of(&snapshot));
        assert!(code.contains("/tmp/payload.pdf"));
        assert!(code.contains("as the request body"));
        assert!(code.contains("body: file1,"));
        assert!(!code.contains("const file1"));
        // The sniffed type is still declared, because that is what dodo sends.
        assert!(code.contains(r#""Content-Type": "application/pdf""#));
    }

    #[test]
    fn a_multi_line_body_becomes_a_template_literal_and_stays_readable() {
        let mut snapshot = everything();
        snapshot.body = BodyDraft {
            kind: BodyType::Json,
            text: "{\n  \"sku\": \"A-1\"\n}".into(),
            ..BodyDraft::default()
        };
        assert!(fetch(&of(&snapshot)).contains("body: `{\n  \"sku\": \"A-1\"\n}`,"));
    }

    #[test]
    fn a_body_a_template_literal_would_interpolate_falls_back_to_a_quoted_string() {
        assert_eq!(literal("a\nb"), "`a\nb`");
        assert_eq!(literal("a\n${b}"), "\"a\\n${b}\"");
        assert_eq!(literal("a\n`b`"), "\"a\\n`b`\"");
        // Single-line text was never a candidate.
        assert_eq!(literal("plain"), "\"plain\"");
    }

    // ---- Duplicate header names ---------------------------------------------

    #[test]
    fn duplicate_names_are_matched_case_insensitively_and_reported_once() {
        let headers = |pairs: &[(&str, &str)]| -> Vec<(String, String)> {
            pairs
                .iter()
                .map(|(a, b)| ((*a).to_string(), (*b).to_string()))
                .collect()
        };
        assert_eq!(
            duplicate_names(&headers(&[
                ("Accept", "a"),
                ("X", "1"),
                ("accept", "b"),
                ("Accept", "c")
            ])),
            ["Accept".to_string()]
        );
        assert!(duplicate_names(&headers(&[("Accept", "a"), ("X", "1")])).is_empty());
    }

    #[test]
    fn fetch_switches_to_an_array_of_pairs_when_a_name_repeats() {
        let mut snapshot = everything();
        snapshot.headers = vec![
            KeyValue::text("Accept", "text/html"),
            KeyValue::text("Accept", "application/json"),
        ];
        let code = fetch(&of(&snapshot));
        assert!(code.contains("headers: [\n"), "{code}");
        assert!(code.contains(r#"["Accept", "text/html"],"#), "{code}");
        assert!(
            code.contains(r#"["Accept", "application/json"],"#),
            "{code}"
        );
    }

    #[test]
    fn axios_says_which_duplicate_value_actually_goes_on_the_wire() {
        let mut snapshot = everything();
        snapshot.headers = vec![
            KeyValue::text("Accept", "text/html"),
            KeyValue::text("Accept", "application/json"),
        ];
        let code = axios(&of(&snapshot));
        assert!(
            code.contains("only\n  // the last value below is sent"),
            "{code}"
        );
        // Both rows are still shown: hiding one would be the silent lie.
        assert!(code.contains(r#""Accept": "text/html","#), "{code}");
        assert!(code.contains(r#""Accept": "application/json","#), "{code}");
    }

    #[test]
    fn xhr_needs_no_caveat_because_the_api_appends() {
        let mut snapshot = everything();
        snapshot.headers = vec![
            KeyValue::text("Accept", "text/html"),
            KeyValue::text("Accept", "application/json"),
        ];
        let code = xhr(&of(&snapshot));
        assert!(code.contains(r#"xhr.setRequestHeader("Accept", "text/html");"#));
        assert!(code.contains(r#"xhr.setRequestHeader("Accept", "application/json");"#));
        // No caveat comment: the // inside the URL is the only one in the file.
        assert!(
            !code.lines().any(|line| line.trim_start().starts_with("//")),
            "{code}"
        );
    }

    // ---- Escaping ------------------------------------------------------------

    #[test]
    fn a_string_escapes_only_what_would_break_it() {
        assert_eq!(string(r#"a"b\c"#), r#""a\"b\\c""#);
        assert_eq!(string("line\nbreak\ttab"), r#""line\nbreak\ttab""#);
        // An invisible C0 control becomes a visible escape rather than a raw
        // byte nobody can see in the copied text.
        assert_eq!(string("bell\u{0007}"), r#""bell\u0007""#);
        // Non-ASCII stays legible: the file is UTF-8.
        assert_eq!(string("chào"), "\"chào\"");
    }
}
