//! The Input method tool's pane.
//!
//! One view, and it is **macOS-only** — the thing it installs is an
//! InputMethodKit bundle, so on Linux and Windows there is no sidebar row for it
//! at all rather than a row whose button could not work. That is the same call
//! the settings page made before this became a tool, and it is why
//! [`crate::input_method`]'s conditional `allow(dead_code)` is still accurate:
//! this module is that module's only caller.

#[cfg(target_os = "macos")]
pub mod input_method_view;

#[cfg(target_os = "macos")]
pub use input_method_view::InputMethodView;
