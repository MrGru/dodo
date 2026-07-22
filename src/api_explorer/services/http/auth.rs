//! Turning the Auth tab into an `Authorization` header or a query parameter.
//!
//! This is the only place that knows how a scheme is spelled on the wire. The
//! Auth *view* collects a token and a username; it never builds a header, which
//! is why switching to a scheme with different mechanics later is a change here
//! and nowhere else.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;

use crate::api_explorer::models::auth::{ApiKeyLocation, AuthDraft, AuthType};
use crate::api_explorer::services::http::headers;

/// Applies `auth` to a request under construction.
///
/// Headers are added only when absent, so an `Authorization` row typed in the
/// Headers tab beats the Auth tab rather than being silently replaced. Query
/// parameters are appended to `query` for the caller to merge into the URL —
/// this module has no opinion about URL syntax.
///
/// A scheme with nothing filled in contributes nothing: a half-typed token is
/// a request still being written, not an error to report.
pub fn apply(
    auth: &AuthDraft,
    headers: &mut Vec<(String, String)>,
    query: &mut Vec<(String, String)>,
) {
    match auth.kind {
        // OAuth 2.0 is shown disabled in the tab; contributing nothing here is
        // the same statement made twice, which is what keeps them consistent.
        AuthType::None | AuthType::OAuth2 => {}

        AuthType::Bearer => {
            let token = auth.token.trim();
            if !token.is_empty() {
                headers::set_if_absent(headers, headers::AUTHORIZATION, format!("Bearer {token}"));
            }
        }

        AuthType::Basic => {
            // A blank password is legitimate (many token-as-username APIs use
            // it), so only a blank username means "not filled in yet".
            let username = auth.username.trim();
            if !username.is_empty() {
                let encoded = BASE64.encode(format!("{username}:{}", auth.password));
                headers::set_if_absent(headers, headers::AUTHORIZATION, format!("Basic {encoded}"));
            }
        }

        AuthType::ApiKey => {
            let name = auth.key_name.trim();
            if name.is_empty() {
                return;
            }
            let value = auth.key_value.trim().to_string();
            match auth.key_location {
                ApiKeyLocation::Header => {
                    headers::set_if_absent(headers, name, value);
                }
                // Appended rather than checked for duplicates: a query string
                // may legitimately carry the same key twice, exactly as the
                // Params table already allows.
                ApiKeyLocation::Query => query.push((name.to_string(), value)),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::apply;
    use crate::api_explorer::models::auth::{ApiKeyLocation, AuthDraft, AuthType};

    /// The headers and query pairs one `apply` produced.
    type Applied = (Vec<(String, String)>, Vec<(String, String)>);

    fn applied(auth: &AuthDraft) -> Applied {
        let mut headers = Vec::new();
        let mut query = Vec::new();
        apply(auth, &mut headers, &mut query);
        (headers, query)
    }

    fn bearer(token: &str) -> AuthDraft {
        AuthDraft {
            kind: AuthType::Bearer,
            token: token.into(),
            ..AuthDraft::default()
        }
    }

    #[test]
    fn no_auth_adds_nothing() {
        let (headers, query) = applied(&AuthDraft::default());
        assert!(headers.is_empty());
        assert!(query.is_empty());
    }

    #[test]
    fn oauth2_adds_nothing_because_it_is_not_implemented() {
        let auth = AuthDraft {
            kind: AuthType::OAuth2,
            token: "would-be-token".into(),
            ..AuthDraft::default()
        };
        let (headers, query) = applied(&auth);
        assert!(headers.is_empty());
        assert!(query.is_empty());
    }

    #[test]
    fn a_bearer_token_becomes_an_authorization_header() {
        let (headers, _) = applied(&bearer("abc123"));
        assert_eq!(
            headers,
            [("Authorization".to_string(), "Bearer abc123".to_string())]
        );
    }

    #[test]
    fn a_pasted_token_is_trimmed() {
        let (headers, _) = applied(&bearer("  abc123\n"));
        assert_eq!(headers[0].1, "Bearer abc123");
    }

    #[test]
    fn a_blank_token_contributes_nothing() {
        let (headers, _) = applied(&bearer("   "));
        assert!(headers.is_empty());
    }

    #[test]
    fn an_explicit_authorization_header_wins() {
        let mut headers = vec![(
            "authorization".to_string(),
            "Bearer typed-by-hand".to_string(),
        )];
        let mut query = Vec::new();
        apply(&bearer("from-the-auth-tab"), &mut headers, &mut query);
        assert_eq!(
            headers,
            [(
                "authorization".to_string(),
                "Bearer typed-by-hand".to_string()
            )]
        );
    }

    #[test]
    fn basic_auth_is_base64_of_user_colon_password() {
        let auth = AuthDraft {
            kind: AuthType::Basic,
            username: "aladdin".into(),
            password: "open sesame".into(),
            ..AuthDraft::default()
        };
        let (headers, _) = applied(&auth);
        assert_eq!(headers[0].0, "Authorization");
        assert_eq!(headers[0].1, "Basic YWxhZGRpbjpvcGVuIHNlc2FtZQ==");
    }

    #[test]
    fn basic_auth_allows_an_empty_password() {
        let auth = AuthDraft {
            kind: AuthType::Basic,
            username: "token".into(),
            ..AuthDraft::default()
        };
        let (headers, _) = applied(&auth);
        assert_eq!(headers[0].1, "Basic dG9rZW46");
    }

    #[test]
    fn basic_auth_with_no_username_contributes_nothing() {
        let auth = AuthDraft {
            kind: AuthType::Basic,
            password: "secret".into(),
            ..AuthDraft::default()
        };
        let (headers, _) = applied(&auth);
        assert!(headers.is_empty());
    }

    #[test]
    fn an_api_key_can_ride_as_a_header() {
        let auth = AuthDraft {
            kind: AuthType::ApiKey,
            key_name: "X-Api-Key".into(),
            key_value: "secret".into(),
            key_location: ApiKeyLocation::Header,
            ..AuthDraft::default()
        };
        let (headers, query) = applied(&auth);
        assert_eq!(headers, [("X-Api-Key".to_string(), "secret".to_string())]);
        assert!(query.is_empty());
    }

    #[test]
    fn an_api_key_can_ride_as_a_query_parameter() {
        let auth = AuthDraft {
            kind: AuthType::ApiKey,
            key_name: "api_key".into(),
            key_value: "secret".into(),
            key_location: ApiKeyLocation::Query,
            ..AuthDraft::default()
        };
        let (headers, query) = applied(&auth);
        assert!(headers.is_empty());
        assert_eq!(query, [("api_key".to_string(), "secret".to_string())]);
    }

    #[test]
    fn an_api_key_with_no_name_contributes_nothing() {
        let auth = AuthDraft {
            kind: AuthType::ApiKey,
            key_value: "secret".into(),
            ..AuthDraft::default()
        };
        let (headers, query) = applied(&auth);
        assert!(headers.is_empty());
        assert!(query.is_empty());
    }
}
