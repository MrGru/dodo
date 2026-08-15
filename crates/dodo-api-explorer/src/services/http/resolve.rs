//! The `{{name}}` pass over a draft, run immediately before [`prepare`].
//!
//! # Why before `prepare` and not inside it
//!
//! `prepare` validates *final* text: it parses the URL, percent-encodes the
//! params and checks that every header can go on the wire. Substituting first
//! means all of that still applies to what is actually sent — a `{{host}}` that
//! resolves to something with a space in it fails as an invalid URL, naming the
//! URL, exactly as if it had been typed. Substituting inside `prepare` would
//! have interleaved the two and made the order arbitrary.
//!
//! It also keeps `prepare`'s signature and its whole existing test table
//! untouched: a request with no variables in it goes down the identical path.
//!
//! # What is substituted
//!
//! The URL, param keys and values, header keys and values, the body document,
//! form field keys and values, and every auth field. That is every place a
//! user types text that reaches the wire.
//!
//! **File paths are not substituted.** A file is chosen through the platform
//! picker rather than typed, so a `{{}}` in one is far more likely to be a
//! literal brace in a filename than a reference — and getting that wrong turns
//! into "the upload silently pointed somewhere else", which is the failure
//! `upload` exists to make impossible.
//!
//! # Threading
//!
//! Pure and cheap, but it runs on the **background executor** with `prepare`
//! and the request itself, because that is where `state::tab` already puts the
//! whole job.
//!
//! [`prepare`]: crate::services::http::prepare::prepare

use crate::models::auth::AuthDraft;
use crate::models::body::BodyDraft;
use crate::models::interpolate::{InterpolationError, interpolate};
use crate::models::key_value::KeyValue;
use crate::models::request::RequestDraft;
use crate::models::variables::VariableSet;
use crate::services::TransportError;

/// Returns a copy of `draft` with every `{{name}}` replaced.
///
/// The first unresolved or recursive reference stops the whole request and is
/// named in the error — see the policy note in
/// [`models::interpolate`](crate::models::interpolate).
pub fn resolve(
    draft: &RequestDraft,
    variables: &VariableSet,
) -> Result<RequestDraft, TransportError> {
    Ok(RequestDraft {
        method: draft.method,
        url: text(&draft.url, variables)?,
        params: rows(&draft.params, variables)?,
        headers: rows(&draft.headers, variables)?,
        body: BodyDraft {
            kind: draft.body.kind,
            text: text(&draft.body.text, variables)?,
            fields: rows(&draft.body.fields, variables)?,
            // Not substituted; see this module's doc.
            file_path: draft.body.file_path.clone(),
        },
        auth: AuthDraft {
            kind: draft.auth.kind,
            token: text(&draft.auth.token, variables)?,
            username: text(&draft.auth.username, variables)?,
            password: text(&draft.auth.password, variables)?,
            key_name: text(&draft.auth.key_name, variables)?,
            key_value: text(&draft.auth.key_value, variables)?,
            key_location: draft.auth.key_location,
        },
    })
}

/// One field, with the model's error translated into the transport's.
fn text(value: &str, variables: &VariableSet) -> Result<String, TransportError> {
    interpolate(value, variables).map_err(|error| match error {
        InterpolationError::Unresolved { name } => TransportError::UnresolvedVariable { name },
        InterpolationError::Recursive { name } => TransportError::RecursiveVariable { name },
    })
}

/// A key/value table. Disabled rows are substituted too rather than skipped:
/// `effective_pairs` drops them later, and failing a request over a switched-off
/// row would be a puzzle with no visible cause.
fn rows(rows: &[KeyValue], variables: &VariableSet) -> Result<Vec<KeyValue>, TransportError> {
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
    use super::resolve;
    use crate::models::auth::{AuthDraft, AuthType};
    use crate::models::body::{BodyDraft, BodyType};
    use crate::models::key_value::KeyValue;
    use crate::models::method::HttpMethod;
    use crate::models::request::RequestDraft;
    use crate::models::variables::{Variable, VariableScope, VariableSet};
    use crate::services::TransportError;

    fn variables(pairs: &[(&str, &str)]) -> VariableSet {
        let mut set = VariableSet::default();
        set.push_layer(
            VariableScope::Environment,
            pairs
                .iter()
                .map(|(key, value)| Variable::new(*key, *value))
                .collect(),
        );
        set
    }

    fn draft() -> RequestDraft {
        RequestDraft {
            method: HttpMethod::Post,
            url: "{{baseUrl}}/v1/things".into(),
            params: vec![KeyValue::text("q", "{{query}}")],
            headers: vec![KeyValue::text("X-{{headerName}}", "{{headerValue}}")],
            body: BodyDraft {
                kind: BodyType::Json,
                text: r#"{"host":"{{host}}"}"#.into(),
                fields: vec![KeyValue::text("f", "{{field}}")],
                file_path: "/tmp/{{not}}-substituted.bin".into(),
            },
            auth: AuthDraft {
                kind: AuthType::Bearer,
                token: "{{token}}".into(),
                username: "{{user}}".into(),
                password: "{{pass}}".into(),
                key_name: "{{keyName}}".into(),
                key_value: "{{keyValue}}".into(),
                ..AuthDraft::default()
            },
        }
    }

    fn everything() -> VariableSet {
        variables(&[
            ("baseUrl", "https://example.com"),
            ("query", "rust"),
            ("headerName", "Trace"),
            ("headerValue", "abc"),
            ("host", "example.com"),
            ("field", "1"),
            ("token", "t"),
            ("user", "ada"),
            ("pass", "hunter2"),
            ("keyName", "api_key"),
            ("keyValue", "k"),
        ])
    }

    #[test]
    fn every_substitution_point_is_covered() {
        let resolved = resolve(&draft(), &everything()).expect("resolves");

        assert_eq!(resolved.url, "https://example.com/v1/things");
        assert_eq!(resolved.params[0].value, "rust");
        assert_eq!(resolved.headers[0].key, "X-Trace");
        assert_eq!(resolved.headers[0].value, "abc");
        assert_eq!(resolved.body.text, r#"{"host":"example.com"}"#);
        assert_eq!(resolved.body.fields[0].value, "1");
        assert_eq!(resolved.auth.token, "t");
        assert_eq!(resolved.auth.username, "ada");
        assert_eq!(resolved.auth.password, "hunter2");
        assert_eq!(resolved.auth.key_name, "api_key");
        assert_eq!(resolved.auth.key_value, "k");
    }

    #[test]
    fn a_file_path_is_left_exactly_as_chosen() {
        let resolved = resolve(&draft(), &everything()).expect("resolves");
        assert_eq!(resolved.body.file_path, "/tmp/{{not}}-substituted.bin");
    }

    #[test]
    fn a_draft_with_no_references_is_unchanged() {
        let mut plain = draft();
        plain.url = "https://example.com/x".into();
        plain.params.clear();
        plain.headers.clear();
        plain.body = BodyDraft::default();
        plain.auth = AuthDraft::default();

        let resolved = resolve(&plain, &VariableSet::default()).expect("resolves");
        assert_eq!(resolved.url, "https://example.com/x");
    }

    #[test]
    fn an_unresolved_reference_fails_the_request_and_names_the_variable() {
        let mut d = draft();
        d.params.clear();
        d.headers.clear();
        d.body = BodyDraft::default();
        d.auth = AuthDraft::default();

        match resolve(&d, &VariableSet::default()).expect_err("baseUrl is not defined") {
            TransportError::UnresolvedVariable { name } => assert_eq!(name, "baseUrl"),
            other => panic!("expected UnresolvedVariable, got {other:?}"),
        }
    }

    #[test]
    fn a_recursive_variable_fails_rather_than_hanging() {
        let mut d = draft();
        d.url = "{{loop}}".into();
        d.params.clear();
        d.headers.clear();
        d.body = BodyDraft::default();
        d.auth = AuthDraft::default();

        match resolve(&d, &variables(&[("loop", "{{loop}}")])).expect_err("a cycle") {
            TransportError::RecursiveVariable { name } => assert_eq!(name, "loop"),
            other => panic!("expected RecursiveVariable, got {other:?}"),
        }
    }

    #[test]
    fn a_switched_off_row_cannot_fail_the_request() {
        let mut d = draft();
        d.url = "https://example.com".into();
        d.headers = vec![KeyValue {
            enabled: false,
            ..KeyValue::text("X-Gone", "{{never-defined}}")
        }];
        d.params.clear();
        d.body = BodyDraft::default();
        d.auth = AuthDraft::default();

        let resolved = resolve(&d, &VariableSet::default()).expect("a disabled row is not sent");
        assert_eq!(resolved.headers[0].value, "{{never-defined}}");
    }

    #[test]
    fn the_substituted_text_is_still_validated_by_prepare() {
        use crate::services::http::prepare::prepare;

        let mut d = draft();
        d.url = "{{baseUrl}}".into();
        d.params.clear();
        d.headers.clear();
        d.body = BodyDraft::default();
        d.auth = AuthDraft::default();

        // A variable holding something unsendable fails as the URL error it is,
        // which is the whole reason this pass runs *before* prepare.
        let resolved = resolve(&d, &variables(&[("baseUrl", "ftp://example.com")]))
            .expect("substitution itself succeeds");
        assert!(matches!(
            prepare(&resolved),
            Err(TransportError::UnsupportedScheme { .. })
        ));
    }
}
