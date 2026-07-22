//! The API Explorer: an HTTP client as a dodo tool.
//!
//! Phase 1 delivers one working request/response loop — method, URL, query
//! params and headers up; status, timing, size, headers and a highlighted body
//! back — plus the module structure the later phases plug into:
//!
//! - [`models`] — plain data, no GPUI, unit tested.
//! - [`services`] — the `Transport` trait and its HTTP implementation. The one
//!   place that knows about `reqwest`; views cannot reach it.
//! - [`state`] — request, response, collections and layout state, split apart.
//! - [`components`] — the few small elements the widget library does not have.
//! - [`views`] — rendering only.
//!
//! Body, Auth and Scripts (request) and Cookies, Tests and Console (response)
//! render an honest placeholder naming the step they arrive in; collections are
//! phase 3.

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
