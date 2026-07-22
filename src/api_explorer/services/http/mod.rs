//! The HTTP implementation of [`Transport`](crate::api_explorer::services::Transport).
//!
//! Split so that the parts with no IO in them — building a request, deciding
//! what a body is, naming a failure — are unit testable without a network:
//! only `client` touches the wire.

pub mod body;
pub mod classify;
pub mod client;
pub mod prepare;

pub use client::HttpTransport;
