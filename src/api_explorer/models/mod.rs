//! Plain data shared by the state, service and view layers.
//!
//! Nothing here builds an element or touches a `Window`, so all of it is unit
//! testable without a GPUI app.

pub mod auth;
pub mod body;
pub mod collection;
pub mod exchange;
pub mod json_tree;
pub mod key_value;
pub mod method;
pub mod request;
pub mod snapshot;
