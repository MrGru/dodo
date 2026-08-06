//! Quick navigation's plain data: the decision, the settings, and the pattern
//! machinery in between.
//!
//! No GPUI, no clipboard, no window — [`detect::detect`] is a `&str` in and an
//! `Option<Route>` out, which is what lets every routing decision the feature
//! makes be a unit test. [`detect`] is where the ordering and the
//! parser-versus-pattern rule are written down.

pub mod config;
pub mod detect;
pub mod pattern;
pub mod route;
