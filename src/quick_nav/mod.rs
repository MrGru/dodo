//! Quick navigation: paste the clipboard into whichever tool can read it.
//!
//! A vim-shaped idea. While **no input is focused**, `Cmd+V` / `Ctrl+V` — or
//! plain `p` — reads the clipboard, works out what the text is, and jumps to the
//! tool that handles it with the text already loaded. `Esc` inside an input
//! leaves it, which is how you get back to that mode.
//!
//! The judgement lives in [`models::detect`] and is a pure function; this file
//! is only the wiring: the global that holds the settings, the two key bindings,
//! and the seam onto `quick-nav.json`. Read [`models::detect`]'s module doc
//! before changing what gets detected — it carries the detection order and the
//! reasoning for every position in it.
//!
//! # Normal mode is derived, never tracked
//!
//! There is no mode flag anywhere in dodo, deliberately. "Normal mode" is
//! exactly *no input has focus*, and the only way to keep that from drifting out
//! of step with reality is to ask the real focus state — so the bindings do,
//! through their key context:
//!
//! ```text
//! Dodo && !Input
//! ```
//!
//! `Dodo` is [`KEY_CONTEXT`], which `Layout::render` puts on the pane; `Input`
//! is the context every `gpui_component` text field and code editor renders
//! itself in. gpui's `Not` predicate is evaluated against the **whole dispatch
//! path**, not just the node it is tested at, so `!Input` means "no input
//! anywhere between the focused element and the root" — which is the definition,
//! stated once, in the only place that can enforce it.
//!
//! Three consequences worth knowing:
//!
//! - **Ordinary paste is untouched.** With an input focused the predicate is
//!   false, so `Cmd+V` never reaches this feature and the library's own `Paste`
//!   handles it exactly as before.
//! - **Typing `p` still types `p`.** Same reason, and it is why the predicate
//!   cannot be a plain `Some("Dodo")` relying on precedence: no input binds `p`
//!   to anything, so a binding that merely sat *above* the input would win and
//!   the letter would vanish from every text field in the app.
//! - **A dialog keeps Escape.** Dialogs are painted by
//!   `Root::render_dialog_layer`, which `DodoApp::render` mounts as a **sibling**
//!   of the pane — so a dialog's dispatch path does not contain `Dodo` at all
//!   and none of these bindings can match while one is open.
//!
//! # What `Esc` does now, and what it did before
//!
//! Before: nothing dodo owned. Escape reached `InputState::escape`, which
//! consumes it for an open completion or an IME composition and otherwise calls
//! `cx.propagate()`, and it reached the library's `Dialog`, `Popover`, `Select`,
//! `PopupMenu`, `DataTable` and `List` contexts, each of which dismisses itself.
//!
//! Now, one binding is added at the `Dodo` context: [`LeaveInsertMode`], which
//! moves focus back to the pane. It is bound **shallower** than every one of
//! those, and gpui tries matched bindings deepest-first, stopping at the first
//! that does not propagate — so every existing Escape still wins, and this one
//! runs only once they have all declined. A dialog dismisses; a popup closes; a
//! text field with a completion open closes the completion. Only a plain focused
//! input, which used to let Escape fall through to nothing, now leaves insert
//! mode.
//!
//! # Threading
//!
//! Detection runs on the UI thread, from a key handler, and is bounded there —
//! see [`models::detect::MAX_INPUT_BYTES`] and [`models::pattern`]. Loading and
//! saving `quick-nav.json` never do: both go through the background executor.

pub mod models;
pub mod services;

use std::sync::Arc;
use std::time::Duration;

use gpui::{App, BorrowAppContext as _, Global, KeyBinding, Task, actions};

use crate::i18n::Str;
use crate::quick_nav::models::config::QuickNavDocument;
use crate::quick_nav::models::detect::{self, Detector, Patterns};
use crate::quick_nav::models::pattern::PatternError;
use crate::quick_nav::models::route::Route;
use crate::quick_nav::services::config_store::{
    DiskQuickNavConfigStore, QuickNavConfigStore, QuickNavStoreError,
};

/// The key context `Layout::render` puts on the pane, and the left half of the
/// normal-mode predicate.
pub const KEY_CONTEXT: &str = "Dodo";

/// **Normal mode, as a key-binding context.** See the module doc; this string is
/// the whole definition and `tests::normal_mode_is_exactly_no_input_focused`
/// holds it to it.
pub const NORMAL_MODE: &str = "Dodo && !Input";

/// How long a settings change waits before it is written.
///
/// The library's string field emits a change per keystroke, so without this a
/// user typing a pattern would write the file once per character. Each edit
/// replaces the pending save — a `Task` cancels when it is dropped — so only the
/// last one in a burst ever reaches the disk.
const SAVE_DELAY: Duration = Duration::from_millis(600);

actions!(dodo, [QuickNavigate, LeaveInsertMode]);

/// The live quick-navigation settings: what is saved, the patterns compiled
/// from it, and the seam onto the file.
pub struct QuickNav {
    document: QuickNavDocument,
    patterns: Patterns,
    store: Arc<dyn QuickNavConfigStore>,
    /// The pending coalesced save, if any. Dropping it cancels it.
    save: Option<Task<()>>,
    /// What went wrong reading or writing the file, for the Settings dialog to
    /// show. `None` in the ordinary case, including a first run with no file.
    store_error: Option<QuickNavStoreError>,
}

impl Global for QuickNav {}

impl QuickNav {
    fn new(store: Arc<dyn QuickNavConfigStore>) -> Self {
        let document = QuickNavDocument::default();
        Self {
            patterns: Patterns::compile(&document),
            document,
            store,
            save: None,
            store_error: None,
        }
    }

    /// Whether the feature is on. Read before anything else happens, so a user
    /// who turned it off pays nothing — not even a clipboard read.
    pub fn enabled(cx: &App) -> bool {
        cx.try_global::<QuickNav>()
            .is_none_or(|state| state.document.enabled)
    }

    pub fn set_enabled(enabled: bool, cx: &mut App) {
        Self::edit(cx, |document| document.enabled = enabled);
    }

    /// The user's raw pattern for `detector` — what the settings field shows.
    pub fn pattern(detector: Detector, cx: &App) -> String {
        cx.try_global::<QuickNav>()
            .map(|state| state.document.pattern(detector).to_owned())
            .unwrap_or_default()
    }

    pub fn set_pattern(detector: Detector, source: impl Into<String>, cx: &mut App) {
        let source = source.into();
        Self::edit(cx, move |document| document.set_pattern(detector, source));
    }

    /// What was wrong with the user's pattern for `detector`, if anything. The
    /// detector is meanwhile running on its built-in default — an unreadable
    /// pattern narrows nothing and switches nothing off.
    pub fn pattern_error(detector: Detector, cx: &App) -> Option<PatternError> {
        cx.try_global::<QuickNav>()
            .and_then(|state| state.patterns.error(detector).cloned())
    }

    /// What went wrong with `quick-nav.json`, if anything.
    pub fn store_error(cx: &App) -> Option<Str> {
        cx.try_global::<QuickNav>()
            .and_then(|state| state.store_error.as_ref().map(QuickNavStoreError::message))
    }

    /// Where this text should go, or `None` for "do nothing".
    ///
    /// Returns `None` immediately when the feature is off, so the caller does
    /// not have to remember to ask twice.
    ///
    /// `allowed` is the detectors whose tool the sidebar still lists — the
    /// caller's, because only `Layout` knows which tool each detector answers
    /// for. A detector left out of it is not tried, so a paste falls through to
    /// the next one rather than switching a tool back on behind the user; see
    /// [`detect::detect_among`], which also states why `allowed` cannot reorder
    /// anything.
    pub fn detect(text: &str, allowed: &[Detector], cx: &App) -> Option<Route> {
        let state = cx.try_global::<QuickNav>()?;
        if !state.document.enabled {
            return None;
        }
        detect::detect_among(text, &state.patterns, allowed)
    }

    /// Applies one change: edits the document, recompiles the patterns from it,
    /// and schedules the save.
    ///
    /// One `update_global` for all three, because nesting a second one inside it
    /// would panic on a global that is already leased out.
    fn edit(cx: &mut App, change: impl FnOnce(&mut QuickNavDocument)) {
        if cx.try_global::<QuickNav>().is_none() {
            return;
        }
        cx.update_global::<QuickNav, _>(|state, cx| {
            change(&mut state.document);
            state.patterns = Patterns::compile(&state.document);

            let store = state.store.clone();
            let document = state.document.clone();
            state.save = Some(cx.spawn(async move |cx| {
                cx.background_executor().timer(SAVE_DELAY).await;
                let result = cx
                    .background_executor()
                    .spawn(async move { store.persist(&document) })
                    .await;

                if let Err(error) = result {
                    cx.update(|cx| Self::report(error, cx));
                }
            }));
        });
        cx.refresh_windows();
    }

    fn report(error: QuickNavStoreError, cx: &mut App) {
        eprintln!("quick-nav.json: {error:?}");
        if cx.try_global::<QuickNav>().is_some() {
            cx.update_global::<QuickNav, _>(|state, _| state.store_error = Some(error));
        }
        cx.refresh_windows();
    }

    /// Adopts what the store read at launch.
    fn adopt(document: QuickNavDocument, cx: &mut App) {
        cx.update_global::<QuickNav, _>(|state, _| {
            state.patterns = Patterns::compile(&document);
            state.document = document;
            state.store_error = None;
        });
        cx.refresh_windows();
    }
}

/// Registers the two key bindings and starts the settings load.
///
/// Must run after `gpui_component::init`, like `settings::init`,
/// `api_explorer::init`, `docker::init` and `database::init`: a binding
/// registered later wins a tie at equal context depth, and [`LeaveInsertMode`]
/// depends on being registered after the library's own `escape` bindings — not
/// to beat them, but so the ordering is the one the module doc describes.
pub fn init(cx: &mut App) {
    cx.bind_keys([
        // The clipboard chord on each platform. Both are bound everywhere:
        // in normal mode neither is claimed by anything else, and a Linux user
        // on a Mac keyboard is not an interesting mistake to punish.
        KeyBinding::new("cmd-v", QuickNavigate, Some(NORMAL_MODE)),
        KeyBinding::new("ctrl-v", QuickNavigate, Some(NORMAL_MODE)),
        // vim's own key for the same act.
        KeyBinding::new("p", QuickNavigate, Some(NORMAL_MODE)),
        // Deliberately *not* under `NORMAL_MODE`: this is the way back into it,
        // so it has to fire while an input is focused.
        KeyBinding::new("escape", LeaveInsertMode, Some(KEY_CONTEXT)),
    ]);

    cx.set_global(QuickNav::new(Arc::new(DiskQuickNavConfigStore::new())));

    let store = cx.global::<QuickNav>().store.clone();
    cx.spawn(async move |cx| {
        let loaded = cx
            .background_executor()
            .spawn(async move { store.load() })
            .await;

        cx.update(|cx| match loaded {
            Ok(document) => QuickNav::adopt(document, cx),
            // A settings file this build cannot read leaves the defaults in
            // place — the feature on, every detector at its built-in behaviour —
            // rather than turning quick navigation off. The Settings dialog says
            // so; `services::config_store`'s module doc says why that is the
            // safe end.
            Err(error) => QuickNav::report(error, cx),
        });
    })
    .detach();
}

#[cfg(test)]
mod tests {
    use gpui::{KeyBindingContextPredicate, KeyContext};

    use super::{KEY_CONTEXT, NORMAL_MODE};

    fn context(name: &str) -> KeyContext {
        KeyContext::parse(name).expect("a bare identifier is a valid key context")
    }

    fn matches(predicate: &str, path: &[&str]) -> bool {
        let path: Vec<KeyContext> = path.iter().copied().map(context).collect();
        KeyBindingContextPredicate::parse(predicate)
            .expect("the predicate has to parse, or `KeyBinding::new` panics at startup")
            .depth_of(&path)
            .is_some()
    }

    /// `crates/dodo-input-method` asserts that a key pressed at its shortcut
    /// recorder is *recorded* and never also obeyed, which means naming the
    /// binding set that would otherwise take `⌘V`, `p` and `Esc` — and a crate
    /// cannot read a `const` of this binary. It mirrors both strings instead;
    /// this is the guard that keeps the two spellings one answer, the same one
    /// `paths` keeps for the platform. A drift here would not fail to compile:
    /// the crate's test would quietly assert something about a predicate dodo
    /// no longer binds.
    #[test]
    fn the_input_method_crate_mirrors_this_binarys_key_contexts() {
        assert_eq!(KEY_CONTEXT, dodo_input_method::QUICK_NAV_KEY_CONTEXT);
        assert_eq!(NORMAL_MODE, dodo_input_method::QUICK_NAV_NORMAL_MODE);
    }

    /// The whole definition of normal mode, checked without a window.
    ///
    /// `KeyBinding::new` unwraps its predicate, so a typo here is a panic during
    /// `main`; that alone is worth a test. The rest is the behaviour the feature
    /// stands on: the pane in the path and no input anywhere in it.
    #[test]
    fn normal_mode_is_exactly_no_input_focused() {
        // Nothing focused but the pane, and anything focusable that is not a
        // text field: normal mode.
        assert!(matches(NORMAL_MODE, &["Root", KEY_CONTEXT]));
        assert!(matches(
            NORMAL_MODE,
            &["Root", KEY_CONTEXT, "DatabaseResult"]
        ));
        assert!(matches(
            NORMAL_MODE,
            &["Root", KEY_CONTEXT, "DatabaseResult", "DataTable"]
        ));

        // An input anywhere in the path — however deep — is insert mode, and
        // this is what keeps `p` typing a `p` and `Cmd+V` pasting text.
        assert!(!matches(NORMAL_MODE, &["Root", KEY_CONTEXT, "Input"]));
        assert!(!matches(
            NORMAL_MODE,
            &["Root", KEY_CONTEXT, "ApiExplorer", "Input"]
        ));
        assert!(!matches(
            NORMAL_MODE,
            &["Root", KEY_CONTEXT, "Input", "SearchPanel"],
        ));

        // Outside the pane entirely — which is where a dialog's focus path sits,
        // because `DodoApp::render` mounts the dialog layer as a sibling.
        assert!(!matches(NORMAL_MODE, &["Root", "Dialog"]));
        assert!(!matches(NORMAL_MODE, &["Root", "Dialog", "Input"]));
    }

    /// Escape is bound at the pane, not at normal mode: it is the way *back*
    /// into normal mode, so it has to fire with an input focused.
    #[test]
    fn the_escape_binding_reaches_a_focused_input_but_not_a_dialog() {
        assert!(matches(KEY_CONTEXT, &["Root", KEY_CONTEXT, "Input"]));
        assert!(matches(KEY_CONTEXT, &["Root", KEY_CONTEXT]));
        assert!(!matches(KEY_CONTEXT, &["Root", "Dialog", "Input"]));
    }

    /// Depth is what decides which binding gpui tries first, and every library
    /// Escape has to be tried before ours. `Input` sits at the end of the path,
    /// so it matches deeper than the pane does.
    #[test]
    fn a_focused_inputs_own_escape_is_tried_before_the_panes() {
        let path: Vec<KeyContext> = ["Root", KEY_CONTEXT, "Input"]
            .into_iter()
            .map(context)
            .collect();
        let depth = |predicate: &str| {
            KeyBindingContextPredicate::parse(predicate)
                .expect("parses")
                .depth_of(&path)
                .expect("matches")
        };

        assert!(
            depth("Input") > depth(KEY_CONTEXT),
            "the input's own escape must be dispatched first, so a completion \
             popup or an IME composition still wins",
        );
    }
}
