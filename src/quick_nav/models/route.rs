//! What quick navigation decided to do, as plain data.
//!
//! A [`Route`] is the whole output of detection: which tool to show and the
//! payload it should already be holding when it appears. It carries no GPUI and
//! no entity, which is what lets every routing decision be a unit test — the
//! only thing left for `layout.rs` to do is hand each variant to the tool that
//! owns it.
//!
//! Two variants carry a value that a **real parser** produced rather than the
//! pasted text: [`Route::Curl`] holds the `RequestSnapshot`
//! `api_explorer::services::curl::parse` built, and [`Route::Database`] holds
//! the `ParsedUri` `database::models::uri::parse` built. Detection and
//! preparation are the same act for those two, so nothing is parsed twice.

use crate::api_explorer::models::snapshot::RequestSnapshot;
use crate::database::models::uri::ParsedUri;

/// A tool to open, and what to put in it.
///
/// Adding a tool to quick navigation means one variant here, one arm in
/// [`Detector::detect`](super::detect::Detector::detect), and one arm in
/// `Layout::apply_route`. Nothing else.
#[derive(Clone, Debug, PartialEq)]
pub enum Route {
    /// The JSON formatter, with this text loaded **and formatted**.
    Json(String),
    /// The Encoder/Decoder's JWT view, with this token decoded.
    Jwt(String),
    /// The Encoder/Decoder, with this text decoded. `url_safe` picks which of
    /// the two Base64 alphabets the dropdown lands on, because the text has
    /// already told us which one it is written in.
    Base64 { text: String, url_safe: bool },
    /// The API Explorer, in a new tab built from this request.
    ///
    /// Boxed: `RequestSnapshot` is by far the largest payload here and an
    /// unboxed variant would make every `Route` that size.
    Curl(Box<RequestSnapshot>),
    /// The Database Explorer, opening or creating the connection this URI
    /// describes. The `id` inside the profile is a placeholder — see
    /// [`Detector::PLACEHOLDER_ID`](super::detect::Detector::PLACEHOLDER_ID).
    Database(Box<ParsedUri>),
}
