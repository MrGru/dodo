//! The Settings dialog, plus the app-level state it edits.
//!
//! There is deliberately no settings struct of our own for appearance: font
//! size, border radius and colours all live on `gpui_component::Theme`, which
//! is already a global the whole app renders from, so the dialog reads and
//! writes that directly and every change is live. Language is the one setting
//! with no home in `Theme`; it lives in [`crate::i18n::Language`].
//!
//! **Every setting here is persisted across restarts except one**, in
//! `session.json` — see [`crate::session`], which the captain asked for on
//! 2026-08-06. (Quick navigation keeps its own file, `quick-nav.json`, because
//! it was already there and its fields hold text the user typed.)
//!
//! The exception is **Run scripts**, and it is not an omission. `ScriptPolicy`
//! goes back to the cautious `Ask for imported` at every launch because it is
//! the gate in front of running code that arrived inside someone else's
//! collection file, not a preference about how the app looks. The *approvals*
//! it collects are persisted, per script, in `script-consent.json`, which is
//! the right granularity for that memory. [`general::run_scripts_field`] says the same
//! thing at the control; [`crate::session`]'s module doc argues it.
//!
//! The dialog body is [`view::SettingsView`]: a quick-navigation search box above the
//! library's own settings panel. Typing fuzzy-matches every setting and picking
//! a result jumps to it.
//!
//! # One page here is not a setting but an editor
//!
//! **Features** — which tools the sidebar lists and in what order, asked for on
//! 2026-08-06 — is the one page whose state is not a global. It edits `Layout`,
//! because switching a tool off has to move the main pane off it, and that is
//! the pane's business rather than a preference's. [`features::features_page`] carries the
//! consequences: a hand-built row instead of a [`gpui_component::setting::SettingField`], a weak handle
//! to the pane instead of a `&mut App` closure pair, and the reorder rules
//! themselves nowhere near here — they are pure data in
//! [`crate::session::models::features`].

mod appearance;
mod features;
mod general;
mod pages;
mod quick_nav;
mod search;
mod view;

#[cfg(test)]
mod tests;

use gpui::*;
use gpui_component::{ThemeRegistry, WindowExt as _};

use self::view::SettingsView;
use crate::assets::Assets;
use crate::dialog_slot::{self, SingleDialog};
use crate::i18n::{shell, t};
use crate::layout::Layout;
pub use appearance::apply_session;

/// Width of the dialog card, and of the settings panel's own sidebar inside it.
///
/// Named because the row layout depends on what is left over: the card spends
/// 2px on its border and `Dialog`'s own `Edges::all(16)` padding on each side,
/// the settings sidebar takes [`SIDEBAR_WIDTH`] of the rest, and each setting
/// row is what remains less the page's own `px_4`.
/// `tests::row_layout::a_pattern_row_stays_inside_the_card` does that arithmetic
/// against a real frame; see [`quick_nav::pattern_field`] for why it matters.
const DIALOG_WIDTH: Pixels = px(760.);
const SIDEBAR_WIDTH: Pixels = px(200.);

/// Key context of the search box. Escape has to be bound *tighter* than the
/// text input's own Escape, which propagates all the way to the dialog and
/// closes it — see [`view::SettingsView::dismiss_results`].
const SEARCH_CONTEXT: &str = "SettingsSearch";

actions!(dodo, [DismissSettingsResults]);

/// Registers the vendored themes with the library's [`ThemeRegistry`], and the
/// one key binding the search box needs.
///
/// Must run after `gpui_component::init`, which creates the registry and binds
/// the library's own keys — Escape resolves by depth first and registration
/// order second, so ours has to be registered last to win the tie.
pub fn init(cx: &mut App) {
    cx.bind_keys([KeyBinding::new(
        "escape",
        DismissSettingsResults,
        Some(&format!("{SEARCH_CONTEXT} > Input")),
    )]);

    let themes: Vec<_> = Assets::themes().collect();
    let registry = ThemeRegistry::global_mut(cx);

    for (path, data) in themes {
        let Ok(json) = std::str::from_utf8(&data) else {
            eprintln!("theme {path} is not valid UTF-8");
            continue;
        };
        if let Err(err) = registry.load_themes_from_str(json) {
            eprintln!("failed to load theme {path}: {err}");
        }
    }
}

/// The marker that keys this dialog's single slot. See [`crate::dialog_slot`].
struct SettingsDialog;

impl SingleDialog for SettingsDialog {}

/// Opens the Settings dialog. The dialog is dismissed with Escape, the close
/// button, or a click on the overlay.
///
/// **There is only ever one.** Two things open it — the sidebar footer's button
/// and the menu bar item's Settings row — and a dialog layer is a stack, so
/// until [`dialog_slot`](crate::dialog_slot) was put in front of it the two
/// paths put two identical cards on top of each other. A second request is
/// dropped rather than served; `on_close` is what gives the slot back, and it
/// pops nothing itself, so one dismissal stays one dismissal.
///
/// `layout` is the pane the Features page edits, and is the one thing here that
/// is not a global: which tools the sidebar lists is `Layout`'s state, because
/// changing it has to move the main pane off a tool that has just stopped being
/// listed. It is held **weakly** and never read while this runs — `open` is
/// reached from a click listener that has `Layout` leased, so a read here would
/// panic. See `gpui-component-recipes`.
pub fn open(layout: WeakEntity<Layout>, window: &mut Window, cx: &mut App) {
    if !dialog_slot::claim::<SettingsDialog>(window, cx) {
        return;
    }

    let view = cx.new(|cx| SettingsView::new(layout, window, cx));

    window.open_dialog(cx, move |dialog, _, cx| {
        dialog
            .title(t(shell::Text::Settings, cx))
            .w(DIALOG_WIDTH)
            .on_close(|_, _, cx| dialog_slot::release::<SettingsDialog>(cx))
            .child(view.clone())
    });
}
