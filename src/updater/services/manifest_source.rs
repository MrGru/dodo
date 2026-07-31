//! Fetching `update.json`.
//!
//! One of the two modules that may name `reqwest` outside
//! `api_explorer::services::http` — see `Cargo.toml`, whose comment on the
//! dependency names both owners.
//!
//! # The client is built lazily, and is its own
//!
//! `reqwest::blocking::Client` starts a background thread with a
//! current-thread runtime on it. A user who has turned automatic checks off
//! should not pay for one, so the client is built on first use behind a
//! `OnceLock` — exactly as `api_explorer::services::http::client::HttpTransport`
//! does, and for the same reason.
//!
//! Sharing the API Explorer's client was considered and rejected: its timeouts
//! are tuned for a request a person is watching, an update download needs a much
//! longer one, and coupling the updater to another tool's transport internals
//! would make either one's timeouts the other one's problem.

use std::sync::OnceLock;
use std::time::Duration;

use reqwest::blocking::Client;

use crate::updater::models::state::UpdateError;
use crate::updater::services::ManifestSource;

/// The manifest is a few kilobytes of JSON. If it has not arrived in this long,
/// the network is not going to produce it, and a background check must never
/// hold a background-executor thread indefinitely.
const MANIFEST_TIMEOUT: Duration = Duration::from_secs(30);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// The most of a manifest that is read. A generous multiple of the real one
/// (about 2 KiB) and still small enough that a redirect to something enormous
/// cannot exhaust memory — this is the one response the updater *does* buffer,
/// because it has to be parsed as a whole.
const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;

/// Sent so a server can tell what is polling it.
const USER_AGENT: &str = concat!("dodo/", env!("CARGO_PKG_VERSION"), " (updater)");

/// `update.json` over HTTPS.
#[derive(Default)]
pub struct HttpManifestSource {
    client: OnceLock<Result<Client, String>>,
}

impl HttpManifestSource {
    pub fn new() -> Self {
        Self::default()
    }

    fn client(&self) -> Result<&Client, UpdateError> {
        self.client
            .get_or_init(|| {
                Client::builder()
                    .timeout(MANIFEST_TIMEOUT)
                    .connect_timeout(CONNECT_TIMEOUT)
                    .user_agent(USER_AGENT)
                    .build()
                    .map_err(|err| err.to_string())
            })
            .as_ref()
            .map_err(|detail| UpdateError::Network(detail.clone()))
    }
}

impl ManifestSource for HttpManifestSource {
    fn fetch(&self, url: &str) -> Result<Vec<u8>, UpdateError> {
        // The URL comes from the config file, which a user may edit. Plaintext
        // is refused here as well as in the manifest's own validation, because
        // this one names *where the digests come from*: fetched over `http://`,
        // the checksums that would catch a tampered archive are themselves
        // tamperable.
        if !url.starts_with("https://") {
            return Err(UpdateError::Manifest(
                crate::updater::models::manifest::ManifestError::InvalidFile {
                    platform: String::new(),
                    detail: crate::i18n::Str::UpdateErrorManifestInsecureUrl(url.to_owned()),
                },
            ));
        }

        let response = self
            .client()?
            .get(url)
            .send()
            .map_err(|err| UpdateError::Network(err.to_string()))?;

        let status = response.status();
        if !status.is_success() {
            // A 404 is the ordinary state of this URL before the first release
            // that publishes a manifest, so it has to be legible rather than a
            // bare number.
            return Err(UpdateError::Network(format!(
                "{} {}",
                status.as_u16(),
                status.canonical_reason().unwrap_or_default()
            )));
        }

        use std::io::Read as _;
        let mut body = Vec::new();
        response
            .take(MAX_MANIFEST_BYTES)
            .read_to_end(&mut body)
            .map_err(|err| UpdateError::Network(err.to_string()))?;
        Ok(body)
    }
}

/// A manifest held in memory, for tests and for driving the pipeline with no
/// network at all. Shaped after `consent_store::InMemoryConsentStore`.
pub struct InMemoryManifestSource {
    /// What every fetch returns, whatever URL it is given — or the failure it
    /// returns instead.
    response: Result<Vec<u8>, UpdateError>,
    /// Every URL that has been asked for, so a test can assert the pipeline
    /// used the configured one.
    requested: std::sync::Mutex<Vec<String>>,
}

impl InMemoryManifestSource {
    pub fn serving(document: impl Into<Vec<u8>>) -> Self {
        Self {
            response: Ok(document.into()),
            requested: std::sync::Mutex::new(Vec::new()),
        }
    }

    pub fn failing(error: UpdateError) -> Self {
        Self {
            response: Err(error),
            requested: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// The URLs fetched so far, oldest first.
    pub fn requested(&self) -> Vec<String> {
        self.requested
            .lock()
            .map(|urls| urls.clone())
            .unwrap_or_default()
    }
}

impl ManifestSource for InMemoryManifestSource {
    fn fetch(&self, url: &str) -> Result<Vec<u8>, UpdateError> {
        if let Ok(mut urls) = self.requested.lock() {
            urls.push(url.to_owned());
        }
        self.response.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::{InMemoryManifestSource, MAX_MANIFEST_BYTES};
    use crate::updater::models::state::UpdateError;
    use crate::updater::services::ManifestSource;

    #[test]
    fn the_in_memory_source_records_what_it_was_asked_for() {
        let source = InMemoryManifestSource::serving(b"{}".to_vec());
        assert_eq!(
            source.fetch("https://example.test/a").unwrap_or_default(),
            b"{}"
        );
        let _ = source.fetch("https://example.test/b");
        assert_eq!(
            source.requested(),
            ["https://example.test/a", "https://example.test/b"]
        );
    }

    #[test]
    fn the_in_memory_source_can_fail_the_way_the_network_does() {
        let source = InMemoryManifestSource::failing(UpdateError::Network("offline".into()));
        assert_eq!(
            source.fetch("https://example.test/x"),
            Err(UpdateError::Network("offline".into()))
        );
    }

    /// The cap is on the *manifest*, which is the one response the updater
    /// buffers whole. It is nowhere near the real document and nowhere near a
    /// size that could exhaust memory.
    #[test]
    fn the_manifest_cap_is_generous_but_bounded() {
        assert!(
            MAX_MANIFEST_BYTES >= 64 * 1024,
            "smaller than a real manifest could grow"
        );
        assert!(MAX_MANIFEST_BYTES <= 8 * 1024 * 1024, "not a bound at all");
    }
}
