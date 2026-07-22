//! Small elements the API Explorer's views share.
//!
//! Each one exists because the widget library has no equivalent and at least
//! two places need it. Anything the library already provides — buttons, tabs,
//! inputs, checkboxes, tags, resizable panels — is used directly instead.

pub mod empty_state;
pub mod key_value_table;
pub mod later_step;
pub mod status_tag;
