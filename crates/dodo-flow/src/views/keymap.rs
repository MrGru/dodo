//! The canvas's actions and [`init`] — the four lines that turn
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
//! # The context, and why it is not optional
//!
//! Every binding is scoped to [`FlowView::KEY_CONTEXT`](super::flow::KEY_CONTEXT),
//! which the canvas sets on its focused root. Without it, `cmd-z` inside the
//! canvas would be `cmd-z` inside dodo's JSON formatter as well — the same
//! scoping every other dodo tool uses, and the reason
//! `crates/dodo-database/src/lib.rs` binds `cmd-c` the way it does.
//!
//! # When to call it
//!
//! After `gpui_component::init`, exactly once per process. The launcher does
//! it; the app's `tools!` row will do it in Phase 8. Calling it twice binds the
//! same keystrokes twice, which GPUI resolves to the same action and which
//! nothing depends on.

use gpui::{App, KeyBinding, actions};

use crate::{commands::keys, views::flow::KEY_CONTEXT};

actions!(
    flow,
    [
        /// §30's undo, scoped to the canvas.
        Undo,
        /// §30's redo.
        Redo
    ]
);

/// Registers the canvas's key bindings for this host.
pub fn init(cx: &mut App) {
    cx.bind_keys(keys::defaults().iter().map(|binding| match binding.action {
        keys::EditAction::Undo => KeyBinding::new(binding.keystroke, Undo, Some(KEY_CONTEXT)),
        keys::EditAction::Redo => KeyBinding::new(binding.keystroke, Redo, Some(KEY_CONTEXT)),
    }));
}

#[cfg(test)]
mod tests {
    use super::{KEY_CONTEXT, Redo, Undo};
    use crate::commands::keys;
    use dodo_paths::HostOs;
    use gpui::KeyBinding;

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
            for binding in keys::for_host(host) {
                let _ = match binding.action {
                    keys::EditAction::Undo => {
                        KeyBinding::new(binding.keystroke, Undo, Some(KEY_CONTEXT))
                    }
                    keys::EditAction::Redo => {
                        KeyBinding::new(binding.keystroke, Redo, Some(KEY_CONTEXT))
                    }
                };
            }
        }
    }
}
