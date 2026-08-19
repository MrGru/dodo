//! The canvas's actions and [`init`] — the few lines that turn
//! [`commands::keys`](crate::commands::keys)'s table into real GPUI bindings.
//!
//! # Why so little is here
//!
//! Which keystroke means undo on which platform is decided in
//! `commands/keys.rs`, below the UI-framework line, so that every platform's
//! answer is asserted from any machine — dodo's root `AGENTS.md` explains why
//! that matters here more than it looks. This file is only the part that
//! genuinely needs `gpui`: an action type per row, and the `bind_keys` call.
//!
//! # One action carrying a tool, rather than eight actions
//!
//! §45's eight tools are one [`SelectTool`] action with a
//! [`CanvasTool`](crate::interaction::CanvasTool) payload, not eight unit
//! actions. GPUI actions may carry data — `actions!` is only the shorthand for
//! the ones that do not — and the difference is what keeps "the tools" a single
//! table: adding a tool is a row in `commands::keys` and a variant in
//! `interaction::tool`, and **this file does not change**. Eight unit actions
//! would mean a ninth arm here for every new tool, in a file whose whole
//! argument is that it should be too small to hold a decision.
//!
//! The payload is why [`SelectTool`] uses `#[derive(Action)]` rather than
//! `actions!`, which only builds unit structs. `no_json` comes with it: without
//! it the derive also asks for `serde::Deserialize` and `schemars::JsonSchema`
//! so that a keymap *file* can name the action with its argument, and dodo
//! loads no keymap files. §26's configurability is
//! [`commands::keys`](crate::commands::keys)'s table, which is Rust rather than
//! JSON, so buying a JSON schema here would be paying for a road nothing
//! drives on.
//!
//! # The context, and why it is not optional
//!
//! Every binding is scoped to [`FlowView::KEY_CONTEXT`](super::flow::KEY_CONTEXT),
//! which the canvas sets on its focused root. Without it, `cmd-z` inside the
//! canvas would be `cmd-z` inside dodo's JSON formatter as well — the same
//! scoping every other dodo tool uses, and the reason
//! `crates/dodo-database/src/lib.rs` binds `cmd-c` the way it does.
//!
//! **The tool letters make that scoping load-bearing rather than tidy.** `r`
//! and `l` are bare letters; bound with no context they would be swallowed
//! before every text field in dodo — `gpui-component-recipes` records that a
//! context-less binding is treated as the *deepest* match and therefore wins
//! over an input's own. Scoped to the canvas, and with the canvas focused only
//! while it is being used, they reach nothing else.
//!
//! # When to call it
//!
//! After `gpui_component::init`, exactly once per process. The launcher does
//! it; the app's `tools!` row will do it in Phase 8. Calling it twice binds the
//! same keystrokes twice, which GPUI resolves to the same action and which
//! nothing depends on.

use gpui::{Action, App, KeyBinding, actions};

use crate::{commands::keys, interaction::CanvasTool, views::flow::KEY_CONTEXT};

actions!(
    flow,
    [
        /// §30's undo, scoped to the canvas.
        Undo,
        /// §30's redo.
        Redo,
        /// Removes the selection. Bound to both delete keys — see
        /// [`keys::EditAction::Delete`].
        ///
        /// It is also the action the palette's Delete button names in its
        /// tooltip, which is what puts the real keystroke beside the label
        /// without anything having to spell it: `gpui-component`'s `Tooltip`
        /// looks the binding up from this action and the canvas's context, so a
        /// rebind changes the hint and nothing has to be kept in step.
        Delete,
        /// §45's tool lock.
        ToggleToolLock
    ]
);

/// §45's tool activation, as one action carrying which tool. See the module
/// doc for why this is not eight actions.
#[derive(Debug, Clone, PartialEq, Eq, Action)]
#[action(namespace = flow, no_json)]
pub struct SelectTool {
    pub tool: CanvasTool,
}

/// Builds one row of [`keys`]'s table as a GPUI binding.
///
/// Free-standing so that [`init`] and the test below construct bindings the
/// same way — the test's whole value is that it builds *every* host's rows, and
/// it would prove nothing if it built them differently.
fn binding(row: keys::Binding) -> KeyBinding {
    match row.action {
        keys::EditAction::Undo => KeyBinding::new(row.keystroke, Undo, Some(KEY_CONTEXT)),
        keys::EditAction::Redo => KeyBinding::new(row.keystroke, Redo, Some(KEY_CONTEXT)),
        keys::EditAction::Delete => KeyBinding::new(row.keystroke, Delete, Some(KEY_CONTEXT)),
        keys::EditAction::ToggleToolLock => {
            KeyBinding::new(row.keystroke, ToggleToolLock, Some(KEY_CONTEXT))
        }
        keys::EditAction::Tool(tool) => {
            KeyBinding::new(row.keystroke, SelectTool { tool }, Some(KEY_CONTEXT))
        }
    }
}

/// Registers the canvas's key bindings for this host.
pub fn init(cx: &mut App) {
    cx.bind_keys(keys::defaults().into_iter().map(binding));
}

#[cfg(test)]
mod tests {
    use super::binding;
    use crate::commands::keys;
    use dodo_paths::HostOs;

    /// **Every host's bindings, actually built.**
    ///
    /// `KeyBinding::new` parses the keystroke and the context predicate and
    /// panics on either being malformed, and [`init`](super::init) only ever
    /// builds *this* host's row — so a typo in the Windows keystroke would ship
    /// and only be found by someone running Windows. Building all three here
    /// costs microseconds and needs no `App`.
    #[test]
    fn every_host_s_bindings_parse() {
        for host in [HostOs::MacOs, HostOs::Windows, HostOs::Unix] {
            for row in keys::for_host(host) {
                let _ = binding(row);
            }
        }
    }
}
