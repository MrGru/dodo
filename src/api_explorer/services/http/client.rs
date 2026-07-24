//! The HTTP backend: the one place in the app that knows about `reqwest`.

use std::io::Read as _;
use std::sync::OnceLock;
use std::time::Instant;

use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};

use crate::api_explorer::models::exchange::Exchange;
use crate::api_explorer::models::method::HttpMethod;
use crate::api_explorer::services::http::{body, classify, prepare};
use crate::api_explorer::services::{PreparedRequest, Protocol, Transport, TransportError};

/// The most of a response body that is read into memory.
///
/// A cap rather than a stream because the viewer shows text: past a few
/// megabytes there is nothing a person can read, and an uncapped read is how a
/// desktop app gets killed by a misdirected file download. What was cut is
/// reported in the response footer rather than hidden.
const MAX_BODY_BYTES: usize = 4 * 1024 * 1024;

/// Sent so that servers and logs can tell what is calling them. Not localized:
/// a User-Agent is a wire token.
const USER_AGENT: &str = concat!("dodo/", env!("CARGO_PKG_VERSION"));

/// HTTP/HTTPS, over `reqwest` with rustls.
///
/// The underlying client is built once on first use rather than at
/// construction: `reqwest::blocking::Client` starts a runtime thread, and a
/// user who never opens the API Explorer should not pay for one.
#[derive(Default)]
pub struct HttpTransport {
    /// `Err` holds the build failure so that it is reported once per attempt
    /// instead of being retried into the same failure forever.
    client: OnceLock<Result<Client, String>>,
}

impl HttpTransport {
    pub fn new() -> Self {
        Self::default()
    }

    fn client(&self) -> Result<&Client, TransportError> {
        self.client
            .get_or_init(|| {
                Client::builder()
                    .timeout(prepare::REQUEST_TIMEOUT)
                    .connect_timeout(prepare::CONNECT_TIMEOUT)
                    .user_agent(USER_AGENT)
                    .build()
                    .map_err(|err| err.to_string())
            })
            .as_ref()
            .map_err(|detail| TransportError::Unexpected {
                detail: detail.clone(),
            })
    }
}

impl Transport for HttpTransport {
    fn protocol(&self) -> Protocol {
        Protocol::Http
    }

    fn execute(&self, request: PreparedRequest) -> Result<Exchange, TransportError> {
        let client = self.client()?;
        let started = Instant::now();

        let mut builder = client
            .request(method_of(request.method), &request.url)
            .timeout(request.timeout)
            .headers(header_map(&request.headers)?);

        if let Some(body) = request.body {
            builder = builder.body(body);
        }

        let response = builder.send().map_err(|err| classify::classify(&err))?;

        let status = response.status();
        let headers: Vec<(String, String)> = response
            .headers()
            .iter()
            .map(|(name, value)| {
                (
                    name.to_string(),
                    // A header value is bytes on the wire; the rare non-ASCII
                    // one is shown escaped rather than dropped.
                    value
                        .to_str()
                        .map(str::to_string)
                        .unwrap_or_else(|_| format!("{value:?}")),
                )
            })
            .collect();
        let content_type = headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("content-type"))
            .map(|(_, value)| value.clone());

        // Read one byte past the cap so that hitting it exactly is not
        // misreported as truncation.
        let mut buffer = Vec::new();
        response
            .take(MAX_BODY_BYTES as u64 + 1)
            .read_to_end(&mut buffer)
            .map_err(|err| TransportError::Connect {
                detail: err.to_string(),
            })?;

        let truncated = buffer.len() > MAX_BODY_BYTES;
        buffer.truncate(MAX_BODY_BYTES);

        let elapsed = started.elapsed();
        let size_bytes = buffer.len();
        let kind = body::kind_of(content_type.as_deref());
        let text = body::decode(&buffer, content_type.as_deref());

        Ok(Exchange {
            status: status.as_u16(),
            headers,
            body: text,
            kind,
            size_bytes,
            truncated,
            elapsed,
        })
    }
}

fn method_of(method: HttpMethod) -> reqwest::Method {
    match method {
        HttpMethod::Get => reqwest::Method::GET,
        HttpMethod::Post => reqwest::Method::POST,
        HttpMethod::Put => reqwest::Method::PUT,
        HttpMethod::Patch => reqwest::Method::PATCH,
        HttpMethod::Delete => reqwest::Method::DELETE,
        HttpMethod::Options => reqwest::Method::OPTIONS,
        HttpMethod::Head => reqwest::Method::HEAD,
        HttpMethod::Connect => reqwest::Method::CONNECT,
        HttpMethod::Trace => reqwest::Method::TRACE,
    }
}

/// Builds the header map, keeping duplicate names.
///
/// `prepare` has already validated every pair, so a failure here would be a
/// bug rather than user error; it is still reported instead of unwrapped.
fn header_map(headers: &[(String, String)]) -> Result<HeaderMap, TransportError> {
    let mut map = HeaderMap::with_capacity(headers.len());
    for (name, value) in headers {
        let name = HeaderName::from_bytes(name.as_bytes())
            .map_err(|_| TransportError::InvalidHeader { name: name.clone() })?;
        let value = HeaderValue::from_str(value).map_err(|_| TransportError::InvalidHeader {
            name: name.to_string(),
        })?;
        // `append`, not `insert`: a second `Accept` must not replace the first.
        map.append(name, value);
    }
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::{header_map, method_of};
    use crate::api_explorer::models::method::HttpMethod;

    #[test]
    fn every_method_maps_to_its_wire_token() {
        for method in HttpMethod::ALL {
            assert_eq!(method_of(method).as_str(), method.as_str());
        }
    }

    #[test]
    fn duplicate_header_names_are_appended_not_replaced() {
        let headers = [
            ("Accept".to_string(), "text/html".to_string()),
            ("Accept".to_string(), "application/json".to_string()),
        ];
        let map = header_map(&headers).expect("both are valid");
        assert_eq!(map.get_all("accept").iter().count(), 2);
    }

    #[test]
    fn an_invalid_header_name_is_an_error_not_a_panic() {
        let headers = [("Bad Name".to_string(), "x".to_string())];
        assert!(header_map(&headers).is_err());
    }
}
