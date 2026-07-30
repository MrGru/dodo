//! One walk over a [`RequestSnapshot`] that every emitter starts from.
//!
//! Four targets could each have walked the snapshot themselves, and that is the
//! mistake this module exists to prevent: the moment they do, "does the API key
//! ride in the query" or "is a disabled row sent" has four answers that drift.
//! So the request is flattened **once** into a [`NormalizedRequest`] — a method,
//! one absolute URL with the query already merged, one header list with auth
//! already folded in, and one body in the shape it is actually sent — and each
//! emitter is a pure function from that to text.
//!
//! # It follows `prepare`, deliberately, and reuses its parts
//!
//! The order the pieces defer to each other is `prepare`'s order, because the
//! generated code has to describe the request dodo *would send*:
//!
//! 1. the typed URL and the Params rows,
//! 2. the Auth tab, which may add a query parameter and a header,
//! 3. the Body tab, which may add a `Content-Type`,
//!
//! with 2 and 3 writing headers only through [`headers::set_if_absent`], so a
//! header typed in the Headers tab still wins. [`auth::apply`] and
//! [`effective_pairs`] are called rather than re-implemented, and a method that
//! [`HttpMethod::carries_body`] says carries no body loses it here just as it
//! does on the wire.
//!
//! Two things `prepare` does that this deliberately does **not**:
//!
//! - **No validation.** An illegal header name or an unfetchable scheme is the
//!   send path's business. A user asking to see the code for a half-written
//!   request should get the code, not an error where the code was going to be.
//! - **No file is read.** A multipart file part and a binary body stay *paths*
//!   here, because that is the honest thing to put in a snippet — and it keeps
//!   this whole module pure, so it needs no `Window`, no filesystem and no
//!   background executor.
//!
//! # Secrets
//!
//! Substitution runs through the same [`interpolate`] the send path uses, over
//! whichever [`VariableSet`] the caller hands in. Withholding a secret is
//! therefore not this module's decision at all: `codegen::generate` passes
//! [`VariableSet::with_secrets_masked`] and the reference survives as literal
//! text. See this module's parent for the policy and why it is shaped that way.
//!
//! [`RequestSnapshot`]: crate::api_explorer::models::snapshot::RequestSnapshot
//! [`headers::set_if_absent`]: crate::api_explorer::services::http::headers::set_if_absent
//! [`auth::apply`]: crate::api_explorer::services::http::auth::apply
//! [`effective_pairs`]: crate::api_explorer::models::key_value::effective_pairs
//! [`interpolate`]: crate::api_explorer::models::interpolate::interpolate

use std::path::Path;

use reqwest::Url;

use crate::api_explorer::models::body::{BodyDraft, BodyType};
use crate::api_explorer::models::interpolate::{InterpolationError, interpolate};
use crate::api_explorer::models::key_value::{FieldKind, KeyValue, effective_pairs};
use crate::api_explorer::models::method::HttpMethod;
use crate::api_explorer::models::snapshot::RequestSnapshot;
use crate::api_explorer::models::variables::VariableSet;
use crate::api_explorer::services::codegen::CodegenError;
use crate::api_explorer::services::http::request_body::form_escape;
use crate::api_explorer::services::http::{auth, headers, upload};

/// A request flattened into the four things every emitter needs.
///
/// `PartialEq` because it is what the cURL round-trip property compares: two
/// snapshots are equivalent when they normalize to the same value. That is the
/// property worth asserting — the wire request, not the editor state, which
/// legitimately differs (a Bearer token is an Auth-tab field on the way out and
/// a header on the way back).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NormalizedRequest {
    pub method: HttpMethod,
    /// Absolute, with the Params rows and any query-borne API key merged in.
    pub url: String,
    /// In table order, duplicates preserved, auth appended last.
    pub headers: Vec<(String, String)>,
    pub body: NormalizedBody,
}

/// The body, in the shape the request sends it — not as bytes.
///
/// Bytes would have forced a file read and would have thrown away exactly the
/// structure an emitter needs: `URLSearchParams` and `FormData` are built field
/// by field, and a file part is a path with a comment beside it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NormalizedBody {
    None,
    /// A document typed into the editor: JSON, XML, HTML or plain text, verbatim.
    Text(String),
    /// `application/x-www-form-urlencoded`, as **decoded** pairs. Each emitter
    /// escapes them its own way, and cURL's `--data-urlencode` wants them raw.
    UrlEncoded(Vec<(String, String)>),
    /// `multipart/form-data`. The boundary is absent on purpose: cURL and every
    /// JavaScript client generate their own.
    Multipart(Vec<NormalizedPart>),
    /// A single file, sent as the whole body.
    File {
        path: String,
        /// Sniffed from the extension, exactly as the send path sniffs it.
        media_type: &'static str,
    },
}

/// One multipart part.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NormalizedPart {
    Text { name: String, value: String },
    File { name: String, path: String },
}

/// Flattens `snapshot`, substituting `{{name}}` against `variables`.
pub fn normalize(
    snapshot: &RequestSnapshot,
    variables: &VariableSet,
) -> Result<NormalizedRequest, CodegenError> {
    let mut headers = effective_pairs(&rows(&snapshot.headers, variables)?);

    let mut auth_query = Vec::new();
    let resolved_auth = resolve_auth(snapshot, variables)?;
    auth::apply(&resolved_auth, &mut headers, &mut auth_query);

    let params = effective_pairs(&rows(&snapshot.params, variables)?);
    let url = absolute_url(&text(&snapshot.url, variables)?, &params, &auth_query);

    let body = if snapshot.method.carries_body() {
        normalized_body(&resolve_body(snapshot, variables)?)
    } else {
        NormalizedBody::None
    };
    if let Some(content_type) = implied_content_type(snapshot.body.kind, &body) {
        headers::set_if_absent(&mut headers, headers::CONTENT_TYPE, content_type);
    }

    Ok(NormalizedRequest {
        method: snapshot.method,
        url,
        headers,
        body,
    })
}

/// The media type this body implies, when the user has not written one.
///
/// [`BodyType::content_type`] answers for every kind whose type is a property of
/// the *choice*, and returns `None` for the two where it is a property of the
/// payload: multipart's carries a boundary nothing has laid out yet, and a
/// binary body's is sniffed from the file — so that one is taken from the
/// sniffed value the normalized body already carries.
fn implied_content_type(kind: BodyType, body: &NormalizedBody) -> Option<String> {
    match body {
        NormalizedBody::None => None,
        NormalizedBody::File { media_type, .. } => Some((*media_type).to_string()),
        NormalizedBody::Text(_) | NormalizedBody::UrlEncoded(_) | NormalizedBody::Multipart(_) => {
            kind.content_type().map(str::to_string)
        }
    }
}

/// Builds the absolute URL: a scheme if none was typed, then the params, then
/// any query-borne API key.
///
/// The pairs are appended through [`Url`] when the text parses, so the result is
/// byte-for-byte what `prepare` would put on the wire — including the trailing
/// slash `Url` adds to a bare host. When it does **not** parse the pairs are
/// concatenated by hand with the same encoding, because a URL holding a
/// withheld secret (`https://{{host}}/x`) still deserves a usable snippet rather
/// than one silently missing its query.
fn absolute_url(
    typed: &str,
    params: &[(String, String)],
    auth_query: &[(String, String)],
) -> String {
    let typed = typed.trim();
    let absolute = if typed.contains("://") {
        typed.to_string()
    } else {
        format!("https://{typed}")
    };

    let pairs = params.iter().chain(auth_query);
    match Url::parse(&absolute) {
        Ok(mut url) => {
            for (key, value) in pairs {
                url.query_pairs_mut().append_pair(key, value);
            }
            url.to_string()
        }
        Err(_) => {
            let mut out = absolute;
            for (index, (key, value)) in pairs.enumerate() {
                let separator = if index == 0 && !out.contains('?') {
                    '?'
                } else {
                    '&'
                };
                out.push(separator);
                out.push_str(&form_escape(key));
                out.push('=');
                out.push_str(&form_escape(value));
            }
            out
        }
    }
}

/// Chooses the body shape, dropping the ones that amount to nothing exactly
/// where `request_body::encode` drops them — a blank editor, a form with no
/// filled-in rows, Binary with no file chosen.
fn normalized_body(body: &BodyDraft) -> NormalizedBody {
    match body.kind {
        BodyType::None => NormalizedBody::None,

        BodyType::Json | BodyType::Text | BodyType::Xml | BodyType::Html => {
            if body.text.trim().is_empty() {
                NormalizedBody::None
            } else {
                NormalizedBody::Text(body.text.clone())
            }
        }

        BodyType::UrlEncoded => {
            let pairs = effective_pairs(&body.fields);
            if pairs.is_empty() {
                NormalizedBody::None
            } else {
                NormalizedBody::UrlEncoded(pairs)
            }
        }

        BodyType::FormData => {
            let parts: Vec<NormalizedPart> = body
                .fields
                .iter()
                .filter(|row| row.is_effective())
                .filter_map(|row| {
                    let name = row.key.trim().to_string();
                    match row.kind {
                        FieldKind::Text => Some(NormalizedPart::Text {
                            name,
                            value: row.value.trim().to_string(),
                        }),
                        // A named row with no file chosen is half-written rather
                        // than wrong; the Body tab marks it and the wire skips
                        // it, so the snippet does too.
                        FieldKind::File => {
                            let path = row.file_path.trim();
                            (!path.is_empty()).then(|| NormalizedPart::File {
                                name,
                                path: path.to_string(),
                            })
                        }
                    }
                })
                .collect();
            if parts.is_empty() {
                NormalizedBody::None
            } else {
                NormalizedBody::Multipart(parts)
            }
        }

        BodyType::Binary => {
            let path = body.file_path.trim();
            if path.is_empty() {
                NormalizedBody::None
            } else {
                NormalizedBody::File {
                    path: path.to_string(),
                    media_type: upload::media_type_of(Path::new(path)),
                }
            }
        }
    }
}

/// The auth fields, substituted. `file_path` has no counterpart here; every auth
/// field is typed text.
fn resolve_auth(
    snapshot: &RequestSnapshot,
    variables: &VariableSet,
) -> Result<crate::api_explorer::models::auth::AuthDraft, CodegenError> {
    let auth = &snapshot.auth;
    Ok(crate::api_explorer::models::auth::AuthDraft {
        kind: auth.kind,
        token: text(&auth.token, variables)?,
        username: text(&auth.username, variables)?,
        password: text(&auth.password, variables)?,
        key_name: text(&auth.key_name, variables)?,
        key_value: text(&auth.key_value, variables)?,
        key_location: auth.key_location,
    })
}

/// The body, substituted. File paths are left exactly as chosen, for the reason
/// `http::resolve` gives: a picker returns them, so a brace in one is far more
/// likely to be a filename than a reference.
fn resolve_body(
    snapshot: &RequestSnapshot,
    variables: &VariableSet,
) -> Result<BodyDraft, CodegenError> {
    Ok(BodyDraft {
        kind: snapshot.body.kind,
        text: text(&snapshot.body.text, variables)?,
        fields: rows(&snapshot.body.fields, variables)?,
        file_path: snapshot.body.file_path.clone(),
    })
}

fn text(value: &str, variables: &VariableSet) -> Result<String, CodegenError> {
    interpolate(value, variables).map_err(|error| match error {
        InterpolationError::Unresolved { name } => CodegenError::UnresolvedVariable { name },
        InterpolationError::Recursive { name } => CodegenError::RecursiveVariable { name },
    })
}

/// A key/value table. Switched-off rows are substituted too rather than skipped,
/// matching `http::resolve`: failing over a row that is not sent would be a
/// puzzle with no visible cause.
fn rows(rows: &[KeyValue], variables: &VariableSet) -> Result<Vec<KeyValue>, CodegenError> {
    rows.iter()
        .map(|row| {
            if !row.is_effective() {
                return Ok(row.clone());
            }
            Ok(KeyValue {
                key: text(&row.key, variables)?,
                value: text(&row.value, variables)?,
                enabled: row.enabled,
                kind: row.kind,
                file_path: row.file_path.clone(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{NormalizedBody, NormalizedPart, absolute_url, normalize};
    use crate::api_explorer::models::auth::{ApiKeyLocation, AuthDraft, AuthType};
    use crate::api_explorer::models::body::{BodyDraft, BodyType};
    use crate::api_explorer::models::key_value::KeyValue;
    use crate::api_explorer::models::method::HttpMethod;
    use crate::api_explorer::models::snapshot::RequestSnapshot;
    use crate::api_explorer::models::variables::{Variable, VariableScope, VariableSet};
    use crate::api_explorer::services::codegen::CodegenError;

    fn snapshot(url: &str) -> RequestSnapshot {
        RequestSnapshot {
            url: url.into(),
            ..RequestSnapshot::default()
        }
    }

    fn plain(snapshot: &RequestSnapshot) -> super::NormalizedRequest {
        normalize(snapshot, &VariableSet::default()).expect("normalizes")
    }

    fn header<'a>(request: &'a super::NormalizedRequest, name: &str) -> Option<&'a str> {
        request
            .headers
            .iter()
            .find(|(existing, _)| existing.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    // ---- The URL -----------------------------------------------------------

    #[test]
    fn a_bare_host_gets_https_and_the_params_are_merged() {
        let mut s = snapshot("example.com/search");
        s.params = vec![
            KeyValue::text("q", "rust lang"),
            KeyValue {
                enabled: false,
                ..KeyValue::text("skipped", "yes")
            },
        ];
        assert_eq!(plain(&s).url, "https://example.com/search?q=rust+lang");
    }

    #[test]
    fn a_query_borne_api_key_lands_after_the_param_rows() {
        let mut s = snapshot("https://example.com/search");
        s.params = vec![KeyValue::text("q", "rust")];
        s.auth = AuthDraft {
            kind: AuthType::ApiKey,
            key_name: "api_key".into(),
            key_value: "s e c".into(),
            key_location: ApiKeyLocation::Query,
            ..AuthDraft::default()
        };
        assert_eq!(
            plain(&s).url,
            "https://example.com/search?q=rust&api_key=s+e+c"
        );
        assert!(plain(&s).headers.is_empty());
    }

    #[test]
    fn a_url_holding_a_withheld_secret_still_gets_its_query() {
        // `Url::parse` cannot make sense of a placeholder where the host goes,
        // so the fallback path has to produce something usable rather than
        // dropping the query on the floor.
        assert_eq!(
            absolute_url("https://{{host}}/x", &[("q".into(), "a b".into())], &[]),
            "https://{{host}}/x?q=a+b"
        );
        // …and it appends rather than opening a second query string.
        assert_eq!(
            absolute_url("{{base}}/x?a=1", &[("b".into(), "2".into())], &[]),
            "https://{{base}}/x?a=1&b=2"
        );
    }

    // ---- Headers and auth ---------------------------------------------------

    #[test]
    fn auth_is_folded_into_the_headers_and_never_overwrites_a_typed_one() {
        let mut s = snapshot("https://example.com/");
        s.headers = vec![KeyValue::text("Authorization", "Bearer typed-by-hand")];
        s.auth = AuthDraft {
            kind: AuthType::Bearer,
            token: "from-the-auth-tab".into(),
            ..AuthDraft::default()
        };
        assert_eq!(
            header(&plain(&s), "authorization"),
            Some("Bearer typed-by-hand")
        );
    }

    #[test]
    fn basic_auth_becomes_the_header_it_becomes_on_the_wire() {
        let mut s = snapshot("https://example.com/");
        s.auth = AuthDraft {
            kind: AuthType::Basic,
            username: "aladdin".into(),
            password: "open sesame".into(),
            ..AuthDraft::default()
        };
        assert_eq!(
            header(&plain(&s), "authorization"),
            Some("Basic YWxhZGRpbjpvcGVuIHNlc2FtZQ==")
        );
    }

    #[test]
    fn duplicate_header_rows_survive_in_order() {
        let mut s = snapshot("https://example.com/");
        s.headers = vec![
            KeyValue::text("Accept", "text/html"),
            KeyValue::text("Accept", "application/json"),
        ];
        let request = plain(&s);
        assert_eq!(request.headers.len(), 2);
        assert_eq!(request.headers[1].1, "application/json");
    }

    // ---- Bodies -------------------------------------------------------------

    #[test]
    fn a_method_that_carries_no_body_loses_it_here_as_it_does_on_the_wire() {
        let mut s = snapshot("https://example.com/");
        s.method = HttpMethod::Get;
        s.body = BodyDraft {
            kind: BodyType::Json,
            text: r#"{"a":1}"#.into(),
            ..BodyDraft::default()
        };
        let request = plain(&s);
        assert_eq!(request.body, NormalizedBody::None);
        assert_eq!(header(&request, "content-type"), None);
    }

    #[test]
    fn a_blank_document_is_no_body_at_all() {
        let mut s = snapshot("https://example.com/");
        s.method = HttpMethod::Post;
        s.body = BodyDraft {
            kind: BodyType::Json,
            text: "   \n ".into(),
            ..BodyDraft::default()
        };
        assert_eq!(plain(&s).body, NormalizedBody::None);
    }

    #[test]
    fn a_document_body_declares_the_type_its_kind_implies() {
        let mut s = snapshot("https://example.com/");
        s.method = HttpMethod::Post;
        s.body = BodyDraft {
            kind: BodyType::Json,
            text: r#"{"a":1}"#.into(),
            ..BodyDraft::default()
        };
        assert_eq!(header(&plain(&s), "content-type"), Some("application/json"));

        // …and a `Content-Type` typed in the Headers tab still wins.
        s.headers = vec![KeyValue::text("content-type", "application/vnd.api+json")];
        let request = plain(&s);
        assert_eq!(
            header(&request, "content-type"),
            Some("application/vnd.api+json")
        );
        assert_eq!(
            request
                .headers
                .iter()
                .filter(|(name, _)| name.eq_ignore_ascii_case("content-type"))
                .count(),
            1
        );
    }

    #[test]
    fn a_urlencoded_body_keeps_its_pairs_decoded() {
        let mut s = snapshot("https://example.com/login");
        s.method = HttpMethod::Post;
        s.body = BodyDraft {
            kind: BodyType::UrlEncoded,
            fields: vec![KeyValue::text("user", "ada"), KeyValue::text("pass", "a b")],
            ..BodyDraft::default()
        };
        // Decoded, so each emitter can escape them the way its own client does.
        assert_eq!(
            plain(&s).body,
            NormalizedBody::UrlEncoded(vec![
                ("user".into(), "ada".into()),
                ("pass".into(), "a b".into()),
            ])
        );
        assert_eq!(
            header(&plain(&s), "content-type"),
            Some("application/x-www-form-urlencoded")
        );
    }

    #[test]
    fn a_multipart_body_keeps_file_parts_as_paths_and_skips_unchosen_ones() {
        let mut s = snapshot("https://example.com/upload");
        s.method = HttpMethod::Post;
        s.body = BodyDraft {
            kind: BodyType::FormData,
            fields: vec![
                KeyValue::text("name", "Ada"),
                KeyValue::file("avatar", "/tmp/a.png"),
                KeyValue::file("cv", ""),
            ],
            ..BodyDraft::default()
        };
        let request = plain(&s);
        assert_eq!(
            request.body,
            NormalizedBody::Multipart(vec![
                NormalizedPart::Text {
                    name: "name".into(),
                    value: "Ada".into()
                },
                NormalizedPart::File {
                    name: "avatar".into(),
                    path: "/tmp/a.png".into()
                },
            ])
        );
        // The boundary is the emitter's business, so no type is declared here.
        assert_eq!(header(&request, "content-type"), None);
    }

    #[test]
    fn a_binary_body_carries_the_path_and_the_sniffed_type() {
        let mut s = snapshot("https://example.com/upload");
        s.method = HttpMethod::Put;
        s.body = BodyDraft {
            kind: BodyType::Binary,
            file_path: "/tmp/report.pdf".into(),
            ..BodyDraft::default()
        };
        let request = plain(&s);
        assert_eq!(
            request.body,
            NormalizedBody::File {
                path: "/tmp/report.pdf".into(),
                media_type: "application/pdf",
            }
        );
        assert_eq!(header(&request, "content-type"), Some("application/pdf"));
    }

    #[test]
    fn nothing_here_reads_a_file() {
        // A path that does not exist normalizes fine: unlike `prepare`, this
        // never opens anything, which is what keeps the whole module pure.
        let mut s = snapshot("https://example.com/upload");
        s.method = HttpMethod::Post;
        s.body = BodyDraft {
            kind: BodyType::Binary,
            file_path: "/definitely/not/here.bin".into(),
            ..BodyDraft::default()
        };
        assert!(matches!(plain(&s).body, NormalizedBody::File { .. }));
    }

    #[test]
    fn an_illegal_header_name_is_not_this_layer_s_business() {
        // `prepare` rejects it; showing the code for a half-written request must
        // still show code.
        let mut s = snapshot("https://example.com/");
        s.headers = vec![KeyValue::text("Bad Header", "x")];
        assert_eq!(header(&plain(&s), "Bad Header"), Some("x"));
    }

    // ---- Substitution -------------------------------------------------------

    #[test]
    fn every_field_is_substituted() {
        let mut set = VariableSet::default();
        set.push_layer(
            VariableScope::Environment,
            vec![
                Variable::new("base", "https://example.com"),
                Variable::new("q", "rust"),
                Variable::new("token", "t0k"),
                Variable::new("field", "1"),
            ],
        );

        let mut s = snapshot("{{base}}/v1/things");
        s.method = HttpMethod::Post;
        s.params = vec![KeyValue::text("q", "{{q}}")];
        s.headers = vec![KeyValue::text("X-Q", "{{q}}")];
        s.body = BodyDraft {
            kind: BodyType::Json,
            text: r#"{"f":"{{field}}"}"#.into(),
            ..BodyDraft::default()
        };
        s.auth = AuthDraft {
            kind: AuthType::Bearer,
            token: "{{token}}".into(),
            ..AuthDraft::default()
        };

        let request = normalize(&s, &set).expect("normalizes");
        assert_eq!(request.url, "https://example.com/v1/things?q=rust");
        assert_eq!(header(&request, "X-Q"), Some("rust"));
        assert_eq!(header(&request, "authorization"), Some("Bearer t0k"));
        assert_eq!(request.body, NormalizedBody::Text(r#"{"f":"1"}"#.into()));
    }

    #[test]
    fn an_unresolved_reference_names_the_variable() {
        assert_eq!(
            normalize(&snapshot("https://{{host}}/x"), &VariableSet::default()),
            Err(CodegenError::UnresolvedVariable {
                name: "host".into()
            })
        );
    }

    #[test]
    fn a_recursive_reference_is_reported_rather_than_hanging() {
        let mut set = VariableSet::default();
        set.push_layer(
            VariableScope::Environment,
            vec![Variable::new("loop", "{{loop}}")],
        );
        assert_eq!(
            normalize(&snapshot("{{loop}}"), &set),
            Err(CodegenError::RecursiveVariable {
                name: "loop".into()
            })
        );
    }

    #[test]
    fn a_switched_off_row_cannot_fail_the_generation() {
        let mut s = snapshot("https://example.com/");
        s.headers = vec![KeyValue {
            enabled: false,
            ..KeyValue::text("X-Gone", "{{never-defined}}")
        }];
        assert!(plain(&s).headers.is_empty());
    }
}
