//! The HTTP implementation of [`Transport`](crate::api_explorer::services::Transport).
//!
//! Split so that the parts with no network in them — building a request,
//! encoding a body, applying an authorization scheme, deciding what a response
//! body is, naming a failure — are unit testable without one: only `client`
//! touches the wire.
//!
//! `upload` is the one module here that touches the filesystem, because a
//! multipart file part and a binary body have to come from somewhere. Its
//! module doc states where on the thread map that is allowed to happen.
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
pub mod upload;

pub use client::HttpTransport;
