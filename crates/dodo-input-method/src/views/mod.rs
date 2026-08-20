//! The Input method tool's pane.
//!
//! One view, present where dodo has an in-process listener: Event Tap on macOS
//! and Keyboard Hook on Windows. Linux remains hidden until it has an implementation.

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub mod input_method_view;

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub use input_method_view::InputMethodView;
