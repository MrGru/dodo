//! The protocol layer the views talk to.
//!
//! # Why a trait
//!
//! Phase 1 speaks HTTP and nothing else, but the point of this module is that
//! the views cannot tell. They hold an `Arc<dyn Transport>` and pass it a
//! [`PreparedRequest`]; they never name `reqwest`, a URL type, or an HTTP
//! status type. Adding GraphQL later is a second [`Transport`] implementation
//! and a [`Protocol`] variant — no view changes.
//!
//! Streaming protocols (WebSocket, gRPC) do not fit a call that returns once,
//! and pretending otherwise now would cost more than it saves. They get a
//! sibling trait in this module returning a channel of frames, selected through
//! the same [`Protocol`] discriminant, and the request/response views keep
//! compiling untouched.
//!
//! # Threading
//!
//! [`Transport::execute`] is **blocking by contract**. Every caller runs it on
//! GPUI's background executor (see `views::explorer::ApiExplorer::send`), never
//! on the UI thread. Making it blocking keeps implementations honest and
//! testable — a fake transport in a unit test is a struct with one method — and
//! keeps a second async runtime out of the render path.

pub mod http;

use std::sync::Arc;
use std::time::Duration;

use crate::api_explorer::models::exchange::Exchange;
use crate::api_explorer::models::method::HttpMethod;
use crate::i18n::Str;

/// Which protocol a request speaks. Phase 1 ships one; the discriminant exists
/// so that adding another is a data change rather than a structural one.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Protocol {
    #[default]
    Http,
}

/// A request that has been validated and is ready to go on the wire.
///
/// Building one is [`http::prepare`]'s job and can fail; executing one cannot
/// fail for reasons the user could have fixed in the editor.
#[derive(Debug)]
pub struct PreparedRequest {
    pub method: HttpMethod,
    /// Absolute, with query parameters already merged in.
    pub url: String,
    /// In table order, duplicates preserved.
    pub headers: Vec<(String, String)>,
    /// Phase 2 fills this from the Body tab; the plumbing is here so that doing
    /// so touches only `prepare` and the new tab.
    pub body: Option<Vec<u8>>,
    pub timeout: Duration,
}

/// Everything that can go wrong before or during a round trip, in the terms a
/// user can act on.
///
/// An HTTP error *status* is not in here: 404 and 500 are ordinary responses
/// and are rendered as such. This is only for "no response arrived".
///
/// Each variant maps to exactly one [`Str`], so the banner is translated and
/// re-translates live when the language changes.
#[derive(Debug)]
pub enum TransportError {
    /// The URL could not be parsed at all.
    InvalidUrl { detail: String },
    /// Parsed, but not something this transport can fetch (`ftp://`, `file://`).
    UnsupportedScheme { scheme: String },
    /// A header name or value that cannot go on the wire.
    InvalidHeader { name: String },
    /// No response within the deadline.
    Timeout { seconds: u64 },
    /// The host name did not resolve.
    Dns { host: String },
    /// TCP never came up: refused, unreachable, network down.
    Connect { detail: String },
    /// The TLS handshake or certificate validation failed.
    Tls { detail: String },
    /// Bytes arrived but are not text this viewer can show.
    BodyNotText { detail: String },
    /// Anything the classifier could not place. The underlying library's own
    /// English wording is kept verbatim inside a translated frame, the same
    /// convention `i18n.rs` documents for serde_json and base64.
    Unexpected { detail: String },
}

impl TransportError {
    /// The message shown in the error banner.
    pub fn message(&self) -> Str {
        match self {
            TransportError::InvalidUrl { detail } => Str::HttpInvalidUrl(detail.clone()),
            TransportError::UnsupportedScheme { scheme } => {
                Str::HttpUnsupportedScheme(scheme.clone())
            }
            TransportError::InvalidHeader { name } => Str::HttpInvalidHeader(name.clone()),
            TransportError::Timeout { seconds } => Str::HttpTimeout(*seconds),
            TransportError::Dns { host } => Str::HttpDnsFailure(host.clone()),
            TransportError::Connect { detail } => Str::HttpConnectFailure(detail.clone()),
            TransportError::Tls { detail } => Str::HttpTlsFailure(detail.clone()),
            TransportError::BodyNotText { detail } => Str::HttpBodyNotText(detail.clone()),
            TransportError::Unexpected { detail } => Str::HttpUnexpected(detail.clone()),
        }
    }
}

/// A protocol backend.
///
/// Implementations perform blocking IO and are always invoked from a background
/// task — never from the UI thread. `Send + Sync + 'static` is what lets one be
/// shared as an `Arc` across those tasks.
pub trait Transport: Send + Sync + 'static {
    /// Which protocol this backend speaks, so a registry can pick it.
    fn protocol(&self) -> Protocol;

    /// Perform one round trip. Blocking.
    fn execute(&self, request: PreparedRequest) -> Result<Exchange, TransportError>;
}

/// The backends available to the app, looked up by [`Protocol`].
///
/// This is the seam a second protocol arrives through: register another
/// `Transport` here and the views keep asking the registry for the protocol
/// their request declares, unchanged.
pub struct TransportRegistry {
    transports: Vec<Arc<dyn Transport>>,
}

impl TransportRegistry {
    /// The registry the app runs with.
    pub fn with_defaults() -> Self {
        Self {
            transports: vec![Arc::new(http::HttpTransport::new())],
        }
    }

    /// The backend for `protocol`, if one is registered.
    pub fn get(&self, protocol: Protocol) -> Option<Arc<dyn Transport>> {
        self.transports
            .iter()
            .find(|transport| transport.protocol() == protocol)
            .cloned()
    }
}
