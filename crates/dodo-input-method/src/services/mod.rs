//! Filesystem persistence and the two platform key listeners.

pub mod document;
#[cfg(target_os = "macos")]
pub mod event_tap;
#[cfg(target_os = "windows")]
pub mod keyboard_hook;
pub mod store;
