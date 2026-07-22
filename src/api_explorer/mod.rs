//! The API Explorer: an HTTP client as a dodo tool.
//!
//! The request half is complete: method, URL, query params, headers, a body in
//! seven shapes, and four authorization schemes go up; status, timing, size,
//! headers and a highlighted body come back. The module structure is what keeps
//! that from being one file:
//!
//! - [`models`] — plain data, no GPUI, unit tested.
//! - [`services`] — the `Transport` trait and its HTTP implementation. The one
//!   place that knows about `reqwest`; views cannot reach it, and it is also
//!   where a body becomes bytes and an auth scheme becomes a header.
//! - [`state`] — request, response, collections and layout state, split apart.
//! - [`components`] — the few small elements the widget library does not have.
//! - [`views`] — rendering only.
//!
//! What is deliberately still absent, each said out loud where a user would
//! look for it rather than left to be discovered: a binary body (needs a file
//! picker), OAuth 2.0 (needs a redirect flow and a token store), running the
//! scripts the Scripts tab edits (needs an engine), the Cookies, Tests and
//! Console response tabs, and collections.

pub mod components;
pub mod models;
pub mod services;
pub mod state;
pub mod views;

use gpui::{App, KeyBinding, actions};

pub use views::ApiExplorer;

use crate::api_explorer::views::explorer::KEY_CONTEXT;

actions!(dodo, [SendRequest]);

/// Registers the send shortcut.
///
/// Must run after `gpui_component::init`, which binds the library's own keys:
/// a binding registered later wins a tie at equal context depth, the same
/// ordering `settings::init` depends on. Neither chord is claimed by `Input`,
/// so both fire from inside the URL field.
pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-enter", SendRequest, Some(KEY_CONTEXT)),
        KeyBinding::new("ctrl-enter", SendRequest, Some(KEY_CONTEXT)),
    ]);
}
