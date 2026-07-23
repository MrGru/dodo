//! The HTTP implementation of [`Transport`](crate::api_explorer::services::Transport).
//!
//! Split so that the parts with no IO in them — building a request, encoding a
//! body, applying an authorization scheme, deciding what a response body is,
//! naming a failure — are unit testable without a network: only `client`
//! touches the wire.
//!
//! `body` is about the response that came back; `request_body` is about the one
//! being sent. They are separate modules because they answer opposite
//! questions: one decodes bytes into something readable, the other encodes
//! something edited into bytes.

pub mod auth;
pub mod body;
pub mod classify;
pub mod client;
pub mod cookies;
pub mod headers;
pub mod prepare;
pub mod request_body;

pub use client::HttpTransport;
