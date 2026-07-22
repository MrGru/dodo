//! Turning what is on screen into a request that is safe to send.
//!
//! Everything that can be wrong with a request *as typed* is caught here, so
//! the user sees "that URL has no host" rather than a network error thirty
//! seconds later.

use std::time::Duration;

use reqwest::Url;
use reqwest::header::{HeaderName, HeaderValue};

use crate::api_explorer::models::key_value::effective_pairs;
use crate::api_explorer::models::request::RequestDraft;
use crate::api_explorer::services::{PreparedRequest, TransportError};

/// How long a request may take in total before it is abandoned.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// How long the TCP/TLS handshake alone may take. Shorter than the total, so an
/// unreachable host fails quickly instead of burning the whole budget.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// The schemes this transport can fetch.
const SUPPORTED_SCHEMES: [&str; 2] = ["http", "https"];

/// Validates a draft and merges the enabled params into the URL's query string.
///
/// A URL typed without a scheme gets `https://`, which is what every HTTP
/// client does and what people expect when they paste a bare host. Existing
/// query parameters in the typed URL are kept, and the enabled param rows are
/// appended after them.
pub fn prepare(draft: &RequestDraft) -> Result<PreparedRequest, TransportError> {
    let typed = draft.url.trim();
    if typed.is_empty() {
        return Err(TransportError::InvalidUrl {
            detail: String::new(),
        });
    }

    let absolute = if typed.contains("://") {
        typed.to_string()
    } else {
        format!("https://{typed}")
    };

    let mut url = Url::parse(&absolute).map_err(|err| TransportError::InvalidUrl {
        detail: err.to_string(),
    })?;

    if !SUPPORTED_SCHEMES.contains(&url.scheme()) {
        return Err(TransportError::UnsupportedScheme {
            scheme: url.scheme().to_string(),
        });
    }

    // `Url::parse` accepts "https://" with nothing after it; a request needs a
    // host to connect to.
    if url.host_str().is_none_or(str::is_empty) {
        return Err(TransportError::InvalidUrl {
            detail: absolute.clone(),
        });
    }

    for (key, value) in effective_pairs(&draft.params) {
        url.query_pairs_mut().append_pair(&key, &value);
    }

    let headers = effective_pairs(&draft.headers);
    // Validated here rather than at send time so an unsendable header name is
    // reported as the editing mistake it is.
    for (name, value) in &headers {
        validate_header(name, value)?;
    }

    Ok(PreparedRequest {
        method: draft.method,
        url: url.to_string(),
        headers,
        body: None,
        timeout: REQUEST_TIMEOUT,
    })
}

/// Checks that a header pair can go on the wire, naming the offending header so
/// the message can point at a row.
fn validate_header(name: &str, value: &str) -> Result<(), TransportError> {
    HeaderName::from_bytes(name.as_bytes()).map_err(|_| TransportError::InvalidHeader {
        name: name.to_string(),
    })?;
    HeaderValue::from_str(value).map_err(|_| TransportError::InvalidHeader {
        name: name.to_string(),
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::prepare;
    use crate::api_explorer::models::key_value::KeyValue;
    use crate::api_explorer::models::method::HttpMethod;
    use crate::api_explorer::models::request::RequestDraft;
    use crate::api_explorer::services::TransportError;

    fn row(enabled: bool, key: &str, value: &str) -> KeyValue {
        KeyValue {
            enabled,
            key: key.into(),
            value: value.into(),
        }
    }

    fn draft(url: &str) -> RequestDraft {
        RequestDraft {
            method: HttpMethod::Get,
            url: url.into(),
            params: Vec::new(),
            headers: Vec::new(),
        }
    }

    #[test]
    fn a_bare_host_gets_https() {
        let prepared = prepare(&draft("example.com/things")).expect("should prepare");
        assert_eq!(prepared.url, "https://example.com/things");
    }

    #[test]
    fn an_explicit_scheme_is_left_alone() {
        let prepared = prepare(&draft("http://example.com/")).expect("should prepare");
        assert_eq!(prepared.url, "http://example.com/");
    }

    #[test]
    fn enabled_params_are_appended_to_the_query() {
        let mut d = draft("https://example.com/search");
        d.params = vec![
            row(true, "q", "rust"),
            row(false, "skipped", "yes"),
            row(true, "page", "2"),
        ];
        let prepared = prepare(&d).expect("should prepare");
        assert_eq!(prepared.url, "https://example.com/search?q=rust&page=2");
    }

    #[test]
    fn params_are_appended_after_a_query_already_in_the_url() {
        let mut d = draft("https://example.com/search?existing=1");
        d.params = vec![row(true, "added", "2")];
        let prepared = prepare(&d).expect("should prepare");
        assert_eq!(prepared.url, "https://example.com/search?existing=1&added=2");
    }

    #[test]
    fn param_values_are_percent_encoded() {
        let mut d = draft("https://example.com/");
        d.params = vec![row(true, "q", "a b&c")];
        let prepared = prepare(&d).expect("should prepare");
        assert_eq!(prepared.url, "https://example.com/?q=a+b%26c");
    }

    #[test]
    fn duplicate_header_keys_survive() {
        let mut d = draft("https://example.com/");
        d.headers = vec![
            row(true, "Accept", "text/html"),
            row(true, "Accept", "application/json"),
        ];
        let prepared = prepare(&d).expect("should prepare");
        assert_eq!(prepared.headers.len(), 2);
        assert_eq!(prepared.headers[0].1, "text/html");
        assert_eq!(prepared.headers[1].1, "application/json");
    }

    #[test]
    fn an_empty_url_is_rejected() {
        assert!(matches!(
            prepare(&draft("   ")),
            Err(TransportError::InvalidUrl { .. })
        ));
    }

    #[test]
    fn a_non_http_scheme_is_rejected_by_name() {
        let error = prepare(&draft("ftp://example.com")).expect_err("ftp is not fetchable");
        match error {
            TransportError::UnsupportedScheme { scheme } => assert_eq!(scheme, "ftp"),
            other => panic!("expected UnsupportedScheme, got {other:?}"),
        }
    }

    #[test]
    fn a_url_with_no_host_is_rejected() {
        assert!(matches!(
            prepare(&draft("https://")),
            Err(TransportError::InvalidUrl { .. })
        ));
    }

    #[test]
    fn an_unsendable_header_name_is_reported_with_the_name() {
        let mut d = draft("https://example.com/");
        d.headers = vec![row(true, "Bad Header", "x")];
        let error = prepare(&d).expect_err("a space is not legal in a header name");
        match error {
            TransportError::InvalidHeader { name } => assert_eq!(name, "Bad Header"),
            other => panic!("expected InvalidHeader, got {other:?}"),
        }
    }

    #[test]
    fn a_newline_in_a_header_value_is_rejected() {
        let mut d = draft("https://example.com/");
        d.headers = vec![row(true, "X-Note", "line\nbreak")];
        assert!(matches!(
            prepare(&d),
            Err(TransportError::InvalidHeader { .. })
        ));
    }
}
