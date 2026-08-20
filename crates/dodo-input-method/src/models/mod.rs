//! Plain data and rules, with no gpui and no filesystem.
//!
//! [`live_switch`] owns the language-switch rules, [`browser_rewrite`] adjusts
//! direct output for browser address bars, and [`settings`] is the persisted
//! application state shared by the two in-process platform listeners.

pub mod browser_rewrite;
pub mod direct_output;
pub mod event_tap;
pub mod keyboard_hook;
pub mod live_switch;
pub mod settings;
