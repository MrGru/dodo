//! Emitting a cURL command.
//!
//! # The round-trip property
//!
//! Everything this file emits is chosen so that `services::curl::parse` can read
//! it back, and the tests at the bottom require it: generate, parse, normalize
//! both, compare. That is a strong and cheap check — it catches a quoting bug, a
//! flag whose argument the parser would mistake for the URL, and any drift
//! between the two directions — but the equivalence is over the **wire request**,
//! not the snapshot. Two differences are expected and are why the comparison is
//! not `snapshot == snapshot`:
//!
//! - An **Auth tab** entry comes back as a header. `-H 'Authorization: Bearer x'`
//!   is lifted back into the Auth tab, but `-H 'X-Api-Key: k'` is not (nothing in
//!   the command says it was an API key), and a Basic credential arrives as the
//!   base64 header it is on the wire. Normalizing folds auth into headers on both
//!   sides, so all three agree.
//! - **Params** come back split out of the URL. Normalizing merges them back in.
//!
//! Three cases where even the normalized forms cannot match, all of them the
//! parser's asymmetry rather than this emitter's:
//!
//! 1. **A urlencoded field whose value contains `&`.** `--data-urlencode 'k=a&b'`
//!    sends `k=a%26b`; `parse` splits the argument on `&` into two fields, which
//!    is what its own test table pins. Every other value round-trips, because
//!    `--data-urlencode` is emitted per field with the value *unescaped* — using
//!    `-d` with a pre-escaped document would fail for anything needing escaping
//!    at all, since `parse` does not percent-decode.
//! 2. **A bare host with query parameters and no path.** `Url` re-serializes
//!    `https://example.com?q=1` with a trailing slash on the path, so the second
//!    pass gains one. A path of any length makes this disappear.
//! 3. **A URL that does not parse** — one holding a withheld `{{secret}}` where
//!    the host goes. `parse` cannot split a query out of it, so the parameters
//!    stay in the URL text instead of returning to the Params table. Both sides
//!    still describe the same request; they are not the same *structure*.

use crate::api_explorer::services::codegen::{NormalizedBody, NormalizedPart, NormalizedRequest};

/// The continuation between options: a shell line continuation and two spaces.
///
/// `parse` reads this back through the same tokenizer that copes with a browser's
/// "Copy as cURL", including the case where a single-line paste has eaten the
/// newline and left the backslash against the next line's indent.
const CONTINUE: &str = " \\\n  ";

/// The command for `request`.
pub fn generate(request: &NormalizedRequest) -> String {
    let mut out = format!("curl -X {}", request.method.as_str());
    out.push_str(CONTINUE);
    out.push_str(&quote(&request.url));

    for (name, value) in &request.headers {
        out.push_str(CONTINUE);
        out.push_str("-H ");
        out.push_str(&quote(&format!("{name}: {value}")));
    }

    match &request.body {
        NormalizedBody::None => {}
        // `--data-raw`, not `-d`: it is the one form where a leading `@` is
        // literal, so a body that happens to start with one is not read as a
        // filename by curl *or* by the parser.
        NormalizedBody::Text(text) => {
            out.push_str(CONTINUE);
            out.push_str("--data-raw ");
            out.push_str(&quote(text));
        }
        // One `--data-urlencode` per field, values unescaped: curl does the
        // escaping, which is both the idiomatic command and the only form that
        // survives a round trip (see this module's doc).
        NormalizedBody::UrlEncoded(fields) => {
            for (name, value) in fields {
                out.push_str(CONTINUE);
                out.push_str("--data-urlencode ");
                out.push_str(&quote(&format!("{name}={value}")));
            }
        }
        NormalizedBody::Multipart(parts) => {
            for part in parts {
                out.push_str(CONTINUE);
                match part {
                    // `--form-string`, not `-F`: `-F` would read a leading `@`
                    // as a file and would treat `;type=…` inside the value as a
                    // qualifier, so a text field containing either would change
                    // meaning.
                    NormalizedPart::Text { name, value } => {
                        out.push_str("--form-string ");
                        out.push_str(&quote(&format!("{name}={value}")));
                    }
                    NormalizedPart::File { name, path } => {
                        out.push_str("-F ");
                        out.push_str(&quote(&format!("{name}=@{path}")));
                    }
                }
            }
        }
        NormalizedBody::File { path, .. } => {
            out.push_str(CONTINUE);
            out.push_str("--data-binary ");
            out.push_str(&quote(&format!("@{path}")));
        }
    }

    out
}

/// Wraps `text` in single quotes, the way a POSIX shell wants it.
///
/// A single quote cannot be escaped *inside* single quotes, so the standard trick
/// applies: close the string, emit an escaped quote, reopen. `parse`'s tokenizer
/// reads `'it'\''s'` back as `it's`, which is asserted below.
fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    use super::{generate, quote};
    use crate::api_explorer::models::auth::{ApiKeyLocation, AuthDraft, AuthType};
    use crate::api_explorer::models::body::{BodyDraft, BodyType};
    use crate::api_explorer::models::key_value::KeyValue;
    use crate::api_explorer::models::method::HttpMethod;
    use crate::api_explorer::models::snapshot::RequestSnapshot;
    use crate::api_explorer::models::variables::VariableSet;
    use crate::api_explorer::services::codegen::normalize;
    use crate::api_explorer::services::curl::parse;

    fn command(snapshot: &RequestSnapshot) -> String {
        let request = normalize::normalize(snapshot, &VariableSet::default()).expect("normalizes");
        generate(&request)
    }

    /// Generate, read back, and require both to describe the same request.
    ///
    /// The whole point of this helper: it is the property, not a spot check, so
    /// every case below gets it for free.
    ///
    /// Headers are compared **sorted**, because the one thing that legitimately
    /// moves is where an auth header sits relative to the others: on the way out
    /// `auth::apply` appends it last, and on the way back a `Bearer` token has
    /// returned to the Auth tab so it is appended after the `Content-Type` that
    /// was a header all along. Order between *different* header names carries no
    /// meaning; two rows with the same name still have to both be present, and
    /// still sort into the same sequence on both sides, so a dropped duplicate is
    /// caught.
    fn round_trips(snapshot: &RequestSnapshot) {
        let command = command(snapshot);
        let parsed = parse(&command)
            .unwrap_or_else(|| panic!("the generated command did not parse:\n{command}"));

        let canonical = |snapshot: &RequestSnapshot| {
            let mut request =
                normalize::normalize(snapshot, &VariableSet::default()).expect("normalizes");
            request.headers.sort();
            request
        };
        assert_eq!(
            canonical(&parsed),
            canonical(snapshot),
            "the command did not round-trip:\n{command}"
        );
    }

    /// The request the acceptance criteria name: a method, query params, headers,
    /// auth and a JSON body.
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

    #[test]
    fn the_whole_request_is_one_readable_command() {
        assert_eq!(
            command(&everything()),
            "curl -X POST \\\n  \
             'https://api.example.com/v2/orders?status=open&limit=50' \\\n  \
             -H 'Accept: application/json' \\\n  \
             -H 'Authorization: Bearer eyJhbGciOi.J9' \\\n  \
             -H 'Content-Type: application/json' \\\n  \
             --data-raw '{\"sku\":\"A-1\",\"qty\":2}'"
        );
    }

    // ---- The round-trip property ------------------------------------------

    #[test]
    fn the_whole_request_round_trips() {
        round_trips(&everything());
    }

    #[test]
    fn a_bare_get_round_trips() {
        round_trips(&RequestSnapshot {
            url: "example.com/things".into(),
            ..RequestSnapshot::default()
        });
    }

    #[test]
    fn every_method_round_trips() {
        for method in HttpMethod::ALL {
            let mut snapshot = everything();
            snapshot.method = method;
            round_trips(&snapshot);
        }
    }

    #[test]
    fn basic_and_api_key_auth_round_trip_as_the_headers_they_become() {
        for auth in [
            AuthDraft {
                kind: AuthType::Basic,
                username: "ada".into(),
                password: "l0ve lace".into(),
                ..AuthDraft::default()
            },
            AuthDraft {
                kind: AuthType::ApiKey,
                key_name: "X-Api-Key".into(),
                key_value: "k".into(),
                ..AuthDraft::default()
            },
            AuthDraft {
                kind: AuthType::ApiKey,
                key_name: "api_key".into(),
                key_value: "k".into(),
                key_location: ApiKeyLocation::Query,
                ..AuthDraft::default()
            },
        ] {
            let mut snapshot = everything();
            snapshot.auth = auth;
            round_trips(&snapshot);
        }
    }

    #[test]
    fn awkward_values_round_trip() {
        // A quote, a space, a `#`, a `$`, a backslash and a brace: everything the
        // shell or the tokenizer could mishandle, in the places a user puts text.
        let mut snapshot = everything();
        snapshot.params = vec![KeyValue::text("q", r#"it's "a" $HOME\x #1"#)];
        snapshot.headers = vec![KeyValue::text("X-Note", "a 'quoted' word")];
        snapshot.body = BodyDraft {
            kind: BodyType::Json,
            text: "{\"note\":\"it's\\n\\\"quoted\\\"\"}".into(),
            ..BodyDraft::default()
        };
        round_trips(&snapshot);
    }

    #[test]
    fn a_body_starting_with_an_at_sign_is_not_read_as_a_filename() {
        let mut snapshot = everything();
        snapshot.body = BodyDraft {
            kind: BodyType::Text,
            text: "@not-a-file".into(),
            ..BodyDraft::default()
        };
        round_trips(&snapshot);
        assert!(command(&snapshot).contains("--data-raw '@not-a-file'"));
    }

    #[test]
    fn a_urlencoded_body_round_trips_with_values_curl_has_to_escape() {
        let mut snapshot = everything();
        snapshot.headers.clear();
        snapshot.body = BodyDraft {
            kind: BodyType::UrlEncoded,
            fields: vec![
                KeyValue::text("user", "ada"),
                KeyValue::text("note", "two words+plus"),
            ],
            ..BodyDraft::default()
        };
        round_trips(&snapshot);
        assert!(
            command(&snapshot).contains("--data-urlencode 'note=two words+plus'"),
            "the value was pre-escaped, which is the one thing parse cannot undo"
        );
    }

    #[test]
    fn a_multipart_body_round_trips_including_its_file_part() {
        let mut snapshot = everything();
        snapshot.headers.clear();
        snapshot.body = BodyDraft {
            kind: BodyType::FormData,
            fields: vec![
                // A `;` and a leading `@`, which are exactly what `-F` would
                // misread and `--form-string` does not.
                KeyValue::text("meta", r#"{"title":"Report"};weird"#),
                KeyValue::text("note", "@literal"),
                KeyValue::file("file", "/Users/ada/report.pdf"),
            ],
            ..BodyDraft::default()
        };
        round_trips(&snapshot);
        assert!(command(&snapshot).contains("-F 'file=@/Users/ada/report.pdf'"));
    }

    #[test]
    fn a_binary_body_round_trips_as_a_data_binary_file_argument() {
        let mut snapshot = everything();
        snapshot.headers.clear();
        snapshot.body = BodyDraft {
            kind: BodyType::Binary,
            file_path: "/tmp/payload.pdf".into(),
            ..BodyDraft::default()
        };
        round_trips(&snapshot);
        assert!(command(&snapshot).contains("--data-binary '@/tmp/payload.pdf'"));
    }

    // ---- The documented limits --------------------------------------------

    #[test]
    fn a_withheld_secret_is_emitted_verbatim_and_does_not_run_as_a_shell_variable() {
        use crate::api_explorer::models::codegen::CodeTarget;
        use crate::api_explorer::models::variables::{Variable, VariableScope};

        let mut snapshot = everything();
        snapshot.auth.token = "{{token}}".into();
        let mut set = VariableSet::default();
        set.push_layer(
            VariableScope::Environment,
            vec![Variable::secret("token", "s3cr3t")],
        );

        let generated = crate::api_explorer::services::codegen::generate(
            CodeTarget::Curl,
            &snapshot,
            &set,
            false,
        )
        .expect("generates");
        // Single quotes throughout, so `{{token}}` reaches the reader as typed
        // rather than being expanded or mangled by the shell.
        assert!(
            generated
                .code
                .contains("-H 'Authorization: Bearer {{token}}'"),
            "{}",
            generated.code
        );
    }

    #[test]
    fn a_urlencoded_value_containing_an_ampersand_is_the_one_case_that_cannot() {
        // Pinned rather than hidden: `parse` splits a `--data-urlencode`
        // argument on `&`, so the field comes back as two. The command curl
        // would send is still correct; only the reverse trip is lossy.
        let mut snapshot = everything();
        snapshot.headers.clear();
        snapshot.body = BodyDraft {
            kind: BodyType::UrlEncoded,
            fields: vec![KeyValue::text("q", "a&b")],
            ..BodyDraft::default()
        };
        let parsed = parse(&command(&snapshot)).expect("parses");
        assert_eq!(
            parsed.body.fields,
            [KeyValue::text("q", "a"), KeyValue::text("b", "")]
        );
    }

    // ---- Quoting -----------------------------------------------------------

    #[test]
    fn a_single_quote_is_closed_escaped_and_reopened() {
        assert_eq!(quote("it's"), r"'it'\''s'");
        assert_eq!(quote("plain"), "'plain'");
    }
}
