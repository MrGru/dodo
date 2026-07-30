//! Plain data shared by the state, service and view layers.
//!
//! Nothing here builds an element or touches a `Window`, so all of it is unit
//! testable without a GPUI app.

pub mod auth;
pub mod body;
pub mod codegen;
pub mod collection;
pub mod console;
pub mod exchange;
pub mod interpolate;
pub mod json_tree;
pub mod key_value;
pub mod method;
pub mod request;
pub mod script;
pub mod script_consent;
pub mod script_format;
pub mod script_template;
pub mod snapshot;
pub mod tab_title;
pub mod test_result;
pub mod variables;
