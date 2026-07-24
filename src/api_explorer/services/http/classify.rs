//! Turning a `reqwest::Error` into something a person can act on.
//!
//! # Why this is heuristic, and why that is acceptable
//!
//! reqwest exposes precise predicates for only some failures (`is_timeout`,
//! `is_connect`, `is_decode`). The distinctions a user actually cares about —
//! "the name did not resolve" versus "the certificate was rejected" versus
//! "nothing is listening" — live further down the error chain, in types
//! (`rustls::Error`, hyper's connect error) that this crate does not depend on
//! and should not start depending on for a message string.
//!
//! So the chain is walked once: a `std::io::Error` in it gives an authoritative
//! `ErrorKind`, and only when that is absent do the chain's own words decide.
//! Every path ends in a translated sentence, and the unclassified path still
//! surfaces the library's verbatim English detail rather than swallowing it —
//! the same convention `i18n.rs` documents for serde_json and base64.

use std::error::Error as _;
use std::io;

use crate::api_explorer::services::TransportError;

/// Words that only appear when name resolution is what failed.
const DNS_MARKERS: [&str; 4] = [
    "dns error",
    "failed to lookup address",
    "name or service not known",
    "nodename nor servname",
];

/// Words that only appear when the TLS handshake or certificate check failed.
const TLS_MARKERS: [&str; 6] = [
    "certificate",
    "tls handshake",
    "invalid peer",
    "handshake failure",
    "unknown issuer",
    "self-signed",
];

pub fn classify(error: &reqwest::Error) -> TransportError {
    let host = error
        .url()
        .and_then(|url| url.host_str())
        .unwrap_or_default()
        .to_string();

    if error.is_timeout() {
        return TransportError::Timeout {
            seconds: super::prepare::REQUEST_TIMEOUT.as_secs(),
        };
    }

    let chain = chain_text(error);
    let lowercase = chain.to_lowercase();

    if DNS_MARKERS.iter().any(|marker| lowercase.contains(marker)) {
        return TransportError::Dns { host };
    }

    if TLS_MARKERS.iter().any(|marker| lowercase.contains(marker)) {
        return TransportError::Tls { detail: chain };
    }

    // An io::ErrorKind is the one authoritative signal in the chain, so it
    // outranks any word-matching below it.
    if let Some(kind) = io_kind(error) {
        match kind {
            io::ErrorKind::ConnectionRefused
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::ConnectionAborted
            | io::ErrorKind::NotConnected
            | io::ErrorKind::AddrNotAvailable
            | io::ErrorKind::HostUnreachable
            | io::ErrorKind::NetworkUnreachable
            | io::ErrorKind::NetworkDown => {
                return TransportError::Connect { detail: chain };
            }
            io::ErrorKind::TimedOut => {
                return TransportError::Timeout {
                    seconds: super::prepare::REQUEST_TIMEOUT.as_secs(),
                };
            }
            _ => {}
        }
    }

    if error.is_connect() {
        return TransportError::Connect { detail: chain };
    }

    if error.is_decode() || error.is_body() {
        return TransportError::BodyNotText { detail: chain };
    }

    if error.is_builder() {
        return TransportError::InvalidUrl { detail: chain };
    }

    TransportError::Unexpected { detail: chain }
}

/// The first `std::io::Error` kind in the error chain, if there is one.
fn io_kind(error: &reqwest::Error) -> Option<io::ErrorKind> {
    let mut source = error.source();
    while let Some(err) = source {
        if let Some(io_error) = err.downcast_ref::<io::Error>() {
            return Some(io_error.kind());
        }
        source = err.source();
    }
    None
}

/// The whole error chain as one sentence, deepest cause last.
///
/// reqwest's own `Display` is often just "error sending request for url (…)",
/// with the useful part one or two levels down, so the chain is what gets
/// shown rather than the top-level message alone.
fn chain_text(error: &reqwest::Error) -> String {
    let mut parts = vec![error.to_string()];
    let mut source = error.source();
    while let Some(err) = source {
        let text = err.to_string();
        // hyper repeats its child's message verbatim at several levels; showing
        // it three times reads as noise.
        if !parts.iter().any(|part| part == &text) {
            parts.push(text);
        }
        source = err.source();
    }
    parts.join(": ")
}

#[cfg(test)]
mod tests {
    use super::{DNS_MARKERS, TLS_MARKERS};

    /// The markers are matched against a lowercased haystack, so a marker with
    /// an uppercase letter in it could never match.
    #[test]
    fn markers_are_lowercase() {
        for marker in DNS_MARKERS.iter().chain(TLS_MARKERS.iter()) {
            assert_eq!(
                *marker,
                marker.to_lowercase(),
                "`{marker}` would never match a lowercased chain"
            );
        }
    }

    #[test]
    fn markers_are_non_empty() {
        // An empty marker would make `contains` true for every error.
        for marker in DNS_MARKERS.iter().chain(TLS_MARKERS.iter()) {
            assert!(
                !marker.trim().is_empty(),
                "an empty marker matches anything"
            );
        }
    }
}
