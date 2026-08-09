//! The Input method tool's pane.
//!
//! One view, present where a real native host exists: InputMethodKit on macOS
//! and TSF on Windows. Linux remains hidden until it has an IBus host rather
//! than drawing an install button that cannot work.

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub mod input_method_view;

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub use input_method_view::InputMethodView;
