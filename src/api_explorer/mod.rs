//! The API Explorer: an HTTP client as a dodo tool.
//!
//! A request goes up — method, URL, query params, headers, a body in seven
//! shapes, four authorization schemes — and status, timing, size, headers and a
//! highlighted body come back. Around that: a persistent tree of saved
//! collections, an in-session history, and a response viewer that renders JSON
//! as a tree, HTML as a text preview, and parses Set-Cookie. The module
//! structure is what keeps that from being one file:
//!
//! - [`models`] — plain data, no GPUI, unit tested (request/response, the
//!   collection tree, the JSON tree, a request snapshot).
//! - [`services`] — the `Transport` trait and its HTTP implementation, plus the
//!   `CollectionStore` trait and its disk implementation. The two places that
//!   touch the outside world (`reqwest`, the filesystem); views cannot reach
//!   either.
//! - [`state`] — request, response, collections, history and layout state.
//! - [`components`] — the few small elements the widget library does not have.
//! - [`views`] — rendering only.
//!
//! What is deliberately still absent, each said out loud where a user would look
//! for it rather than left to be discovered: a binary body (needs a file
//! picker), OAuth 2.0 (needs a redirect flow and a token store), running the
//! scripts the Scripts tab edits and the Tests/Console response tabs (need an
//! engine), and drag-and-drop reordering of collections (the model supports it;
//! the gesture is future work).

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
