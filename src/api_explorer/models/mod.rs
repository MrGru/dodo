//! Plain data shared by the state, service and view layers.
//!
//! Nothing here builds an element or touches a `Window`, so all of it is unit
//! testable without a GPUI app.

pub mod exchange;
pub mod key_value;
pub mod method;
pub mod request;
