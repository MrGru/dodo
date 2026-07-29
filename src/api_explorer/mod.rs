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
//! Scripts run, **both hooks**. The pre-request hook executes before every send
//! and the post-response hook after every response, in a QuickJS sandbox that
//! [`services::script`] is the only module allowed to name, behind one consent
//! gate covering both ([`models::script_consent`]). Their `console` output lands
//! in the Console response tab and their `pm.test` results in the Tests tab
//! ([`models::test_result`]), with a `passed/total` badge on the tab and another
//! in history. The two script editors are JavaScript-highlighted, re-parsed as
//! they are typed by the same engine that will run them, and have a Format
//! action whose deliberately narrow scope [`models::script_format`] argues.
//!
//! What is deliberately still absent, each said out loud where a user would look
//! for it rather than left to be discovered: OAuth 2.0 (needs a redirect flow
//! and a token store), `pm.sendRequest` (denied outright — see
//! `decision-pm-sendrequest-scope`; a script has no network), and drag-and-drop
//! reordering of collections (the model supports it; the gesture is future
//! work).
//!
//! Uploads are here, and there is one rule about them worth stating at this
//! level: **no file is read on the UI thread.** Choosing one goes through
//! [`services::file_picker`] (the `stat` runs on the background executor);
//! reading its bytes happens once, at send time, inside
//! [`services::http::upload`], reached only from `prepare` — which
//! `state::tab::RequestTabState::send` has moved onto the background executor
//! for exactly this reason.

pub mod components;
pub mod models;
pub mod services;
pub mod state;
pub mod views;

use gpui::{App, Global, KeyBinding, actions};

pub use views::ApiExplorer;

use crate::api_explorer::models::script_consent::ConsentPolicy;
use crate::api_explorer::views::explorer::KEY_CONTEXT;

actions!(dodo, [SendRequest]);

/// The active "Run scripts" setting.
///
/// A global rather than page state because the Settings dialog edits it, and a
/// `SettingField` is a pair of closures over `&App` / `&mut App` — the same
/// reason [`Language`](crate::i18n::Language) is one. Like every other dodo
/// setting it is **not persisted**, so each launch starts at
/// [`ConsentPolicy::AskImported`]; that is the safe end, which is what makes
/// not persisting it acceptable here. The *approvals* are persisted, because
/// re-approving every script on every launch would train the user to click
/// through the prompt without reading it.
#[derive(Clone, Copy, Default)]
pub struct ScriptPolicy(ConsentPolicy);

impl Global for ScriptPolicy {}

impl ScriptPolicy {
    pub fn current(cx: &App) -> ConsentPolicy {
        cx.try_global::<ScriptPolicy>()
            .map_or_else(ConsentPolicy::default, |policy| policy.0)
    }

    pub fn set(policy: ConsentPolicy, cx: &mut App) {
        cx.set_global(ScriptPolicy(policy));
    }
}

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
