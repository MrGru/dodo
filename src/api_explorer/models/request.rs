//! The request as plain data, handed from the editor to the service layer.

use crate::api_explorer::models::auth::AuthDraft;
use crate::api_explorer::models::body::BodyDraft;
use crate::api_explorer::models::key_value::KeyValue;
use crate::api_explorer::models::method::HttpMethod;

/// A snapshot of what the request editor currently holds.
///
/// Taken from the live editor state at the moment Send is pressed, so that the
/// background task works on an owned copy and the user can keep typing while
/// the request is in flight.
///
/// The Scripts tab is deliberately absent: nothing executes scripts in this
/// phase, so carrying them here would be data no one reads. They live in
/// `state::request` and travel no further, which is what the tab says on
/// screen.
pub struct RequestDraft {
    pub method: HttpMethod,
    pub url: String,
    pub params: Vec<KeyValue>,
    pub headers: Vec<KeyValue>,
    pub body: BodyDraft,
    pub auth: AuthDraft,
}
