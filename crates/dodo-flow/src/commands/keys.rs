//! §26's *"configurable bindings rather than deeply hard-coding platform
//! keys"*, as a table this side of the UI-framework line.
//!
//! # Why the table is here and not in the view
//!
//! A binding is two facts: a keystroke, and the thing it does. Only the first
//! is the UI framework's. Keeping both in `views/` would mean the answer to
//! "what does Ctrl+Z do on Windows?" could only be obtained by building for
//! Windows — and dodo's root `AGENTS.md` names that exact trap: **a
//! platform-conditional answer is a value chosen by [`HostOs`] or `cfg!`, never
//! an item behind `#[cfg]`**, because two of dodo's four release targets cannot
//! be built from a Mac at all. [`for_host`] is a total function of a `HostOs`,
//! so every platform's bindings are asserted from any machine, and the tests at
//! the bottom of this file do exactly that.
//!
//! `views::init` is the only thing that turns this table into real bindings,
//! and it is four lines long. Replacing the defaults with a user's own
//! preferences later is a different source for the same `&[Binding]`, and needs
//! no change in the view.
//!
//! # What is bound, and what is not
//!
//! Undo, redo, delete, the tool lock, and §45's eight tools. §26's remaining
//! list — nudge, duplicate, copy/paste, zoom-to-fit, zoom-to-selection — is
//! more rows over the same type, and each needs a command the engine can
//! perform; the ones that exist today have no gesture asking for them yet. A
//! row here that no code reads would be a binding that silently does nothing,
//! which is worse than an absent one.
//!
//! # The tool letters are the same on every host, and that is a decision
//!
//! Undo and redo differ per platform because the platforms differ about them.
//! A bare letter has no platform convention to follow — `r` is the rectangle
//! tool in Excalidraw, Figma and Sketch alike on every operating system — so
//! [`TOOLS`] is one table shared by all three hosts rather than three identical
//! ones. It is still reached through [`for_host`], so replacing it with a
//! user's own preferences later is one source change and no view change.
//!
//! **`Esc` is deliberately not here.** The canvas already handles it as a raw
//! key, because it means two things in sequence — abandon whatever is in
//! progress, *then* return to the Select tool — and the interaction machine's
//! contract is one effect per event. `views::flow`'s `on_key_down` sends the
//! two events; a binding would have had to invent a compound action for the
//! one keystroke that needs it.
//!
//! # Redo has two bindings outside macOS, on purpose
//!
//! Windows and most Linux desktops answer both `Ctrl+Y` and `Ctrl+Shift+Z`, and
//! people arrive with either in their fingers. macOS has settled on
//! `Cmd+Shift+Z` and does not use `Cmd+Y` for anything, but adding it there
//! would be inventing a platform convention rather than following one.
//!
//! **This file names no UI framework.**

use dodo_paths::HostOs;

use crate::interaction::CanvasTool;

/// What a binding does. One variant per command the canvas can be driven to
/// perform from the keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EditAction {
    Undo,
    Redo,
    /// **Removes the selection** — [`FlowEditor::delete_selection`](crate::commands::FlowEditor::delete_selection).
    ///
    /// Two keystrokes on every host rather than one, because the two are the
    /// same key to most people: `Backspace` is the key labelled *delete* on an
    /// Apple keyboard, and `Delete` is the forward-delete every PC keyboard
    /// has. Binding only one of them means a user whose muscle memory has the
    /// other presses it and nothing happens.
    Delete,
    /// **The tool lock**: whether finishing a drawing keeps the tool or returns
    /// to Select. Like [`EditAction::Tool`] it is not an edit — it changes what
    /// the *next* gesture means and touches no document.
    ToggleToolLock,
    /// §45's tool activation. **Not an edit**, despite the type's name: picking
    /// a tool changes what the next press means and touches no document. It is
    /// here because this is the binding table, and a second table for one kind
    /// of row would be a second place for a keystroke collision to hide — which
    /// [`no_keystroke_is_bound_to_two_actions`](tests::no_keystroke_is_bound_to_two_actions)
    /// is able to rule out precisely because there is only one.
    Tool(CanvasTool),
}

impl EditAction {
    /// A short stable name, for a test or a debug overlay. Not user-facing —
    /// see [`crate::commands::edit::EditCommand::kind`].
    pub fn name(self) -> &'static str {
        match self {
            EditAction::Undo => "undo",
            EditAction::Redo => "redo",
            EditAction::Delete => "delete",
            EditAction::ToggleToolLock => "toggle-tool-lock",
            EditAction::Tool(tool) => tool.name(),
        }
    }
}

/// One keystroke and the action it performs.
///
/// The keystroke is GPUI's own syntax (`"cmd-shift-z"`) because that is what it
/// is ultimately handed to, and translating between two spellings of the same
/// thing would be a second place for a typo to hide.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Binding {
    pub keystroke: &'static str,
    pub action: EditAction,
}

const MACOS: &[Binding] = &[
    Binding {
        keystroke: "cmd-z",
        action: EditAction::Undo,
    },
    Binding {
        keystroke: "cmd-shift-z",
        action: EditAction::Redo,
    },
];

/// The rows that are the same on every host and carry no modifier: the two
/// delete keys, and Excalidraw's own `q` for the tool lock.
///
/// Beside [`TOOLS`] rather than inside it because these are not tools, and
/// apart from [`MACOS`] and [`PC`] because they follow no platform convention:
/// `Backspace` deletes the selection in every canvas editor on every operating
/// system, and inventing a `Cmd+Backspace` for macOS would be following a
/// convention that does not exist.
const UNIVERSAL: &[Binding] = &[
    Binding {
        keystroke: "backspace",
        action: EditAction::Delete,
    },
    Binding {
        keystroke: "delete",
        action: EditAction::Delete,
    },
    Binding {
        keystroke: "q",
        action: EditAction::ToggleToolLock,
    },
];

/// §45's tools, one letter each and the same on every host — see the module
/// doc.
///
/// The letters are the ones every canvas editor already uses, so a user
/// arriving from one of them is not retrained: `v` select, `h` hand, `r`
/// rectangle, `o` ellipse (*not* `e`, which is the eraser everywhere), `d`
/// diamond, `a` arrow, `l` line, `n` node.
const TOOLS: &[Binding] = &[
    Binding {
        keystroke: "v",
        action: EditAction::Tool(CanvasTool::Select),
    },
    Binding {
        keystroke: "h",
        action: EditAction::Tool(CanvasTool::Hand),
    },
    Binding {
        keystroke: "r",
        action: EditAction::Tool(CanvasTool::Rectangle),
    },
    Binding {
        keystroke: "d",
        action: EditAction::Tool(CanvasTool::Diamond),
    },
    Binding {
        keystroke: "o",
        action: EditAction::Tool(CanvasTool::Ellipse),
    },
    Binding {
        keystroke: "a",
        action: EditAction::Tool(CanvasTool::Arrow),
    },
    Binding {
        keystroke: "l",
        action: EditAction::Tool(CanvasTool::Line),
    },
    Binding {
        keystroke: "n",
        action: EditAction::Tool(CanvasTool::GraphNode),
    },
];

const PC: &[Binding] = &[
    Binding {
        keystroke: "ctrl-z",
        action: EditAction::Undo,
    },
    Binding {
        keystroke: "ctrl-shift-z",
        action: EditAction::Redo,
    },
    Binding {
        keystroke: "ctrl-y",
        action: EditAction::Redo,
    },
];

/// **The canvas's default bindings on a given host.** A total function, so the
/// Windows and Linux answers are asserted from a Mac.
///
/// Two tables joined rather than one written out three times: the editing rows
/// follow a platform convention and the tool rows do not.
pub fn for_host(host: HostOs) -> Vec<Binding> {
    let editing = match host {
        HostOs::MacOs => MACOS,
        // Windows and every Linux desktop dodo targets use the same three, and
        // splitting them would be two identical tables to keep in step.
        HostOs::Windows | HostOs::Unix => PC,
    };

    editing
        .iter()
        .chain(UNIVERSAL)
        .chain(TOOLS)
        .copied()
        .collect()
}

/// The bindings for the machine this was compiled for.
pub fn defaults() -> Vec<Binding> {
    for_host(crate::budgets::current_host())
}

#[cfg(test)]
mod tests {
    use super::{Binding, EditAction, defaults, for_host};
    use crate::interaction::CanvasTool;
    use dodo_paths::HostOs;

    const EVERY_HOST: [HostOs; 3] = [HostOs::MacOs, HostOs::Windows, HostOs::Unix];

    /// The point of the table: an undo and a redo exist everywhere, and both
    /// answers are checked from whichever machine happens to run the tests.
    #[test]
    fn every_host_can_undo_and_redo() {
        for host in EVERY_HOST {
            let bindings = for_host(host);
            for action in [EditAction::Undo, EditAction::Redo] {
                assert!(
                    bindings.iter().any(|binding| binding.action == action),
                    "{host:?} has no binding for {}",
                    action.name()
                );
            }
        }
    }

    /// A keystroke bound to two different actions is a binding the user cannot
    /// predict; whichever one GPUI resolves, the other looks broken.
    #[test]
    fn no_keystroke_is_bound_to_two_actions() {
        for host in EVERY_HOST {
            let bindings = for_host(host);
            for (index, binding) in bindings.iter().enumerate() {
                for other in &bindings[index + 1..] {
                    assert!(
                        binding.keystroke != other.keystroke || binding.action == other.action,
                        "{host:?} binds {} to two actions",
                        binding.keystroke
                    );
                }
            }
        }
    }

    /// The platform convention each host actually has. Written out rather than
    /// derived, because the whole value of the table is that the wrong answer
    /// is visible in a diff.
    ///
    /// Only *undo and redo* are platform-shaped. The tool letters, the two
    /// delete keys and the lock have no convention to follow and are excluded
    /// by name rather than by position — which is why this filter lists what it
    /// wants instead of subtracting what it does not.
    #[test]
    fn each_host_gets_its_own_modifier() {
        let editing = |host| {
            for_host(host)
                .into_iter()
                .filter(|binding: &Binding| {
                    matches!(binding.action, EditAction::Undo | EditAction::Redo)
                })
                .collect::<Vec<_>>()
        };

        assert!(
            editing(HostOs::MacOs)
                .iter()
                .all(|binding| binding.keystroke.starts_with("cmd-"))
        );
        for host in [HostOs::Windows, HostOs::Unix] {
            assert!(
                editing(host)
                    .iter()
                    .all(|binding| binding.keystroke.starts_with("ctrl-"))
            );
        }
    }

    /// **Every tool is reachable from the keyboard, on every host.** A palette
    /// entry with no binding is the sort of half-wired control §26 exists to
    /// prevent, and it is invisible until somebody reaches for the key.
    #[test]
    fn every_tool_has_a_binding_on_every_host() {
        for host in EVERY_HOST {
            let bindings = for_host(host);
            for tool in CanvasTool::ALL {
                assert!(
                    bindings
                        .iter()
                        .any(|binding| binding.action == EditAction::Tool(*tool)),
                    "{host:?} cannot reach the {} tool",
                    tool.name()
                );
            }
        }
    }

    /// A tool letter must not carry a modifier: `cmd-r` is reload in half the
    /// world's applications and `ctrl-l` is the address bar in the other half.
    #[test]
    fn a_tool_is_a_bare_letter() {
        for binding in for_host(HostOs::MacOs) {
            if matches!(binding.action, EditAction::Tool(_)) {
                assert_eq!(
                    binding.keystroke.len(),
                    1,
                    "{} is not a bare letter",
                    binding.keystroke
                );
            }
        }
    }

    /// `Ctrl+Y` is the second redo people arrive with on a PC, and macOS
    /// deliberately does not get a `Cmd+Y` invented for it.
    #[test]
    fn a_pc_answers_both_redo_conventions_and_macos_answers_one() {
        let redos = |host| {
            for_host(host)
                .into_iter()
                .filter(|binding: &Binding| binding.action == EditAction::Redo)
                .count()
        };

        assert_eq!(redos(HostOs::MacOs), 1);
        assert_eq!(redos(HostOs::Windows), 2);
        assert_eq!(redos(HostOs::Unix), 2);
    }

    /// **Both delete keys reach the same action, on every host.** The key an
    /// Apple keyboard labels *delete* reports as `backspace`, and a user
    /// pressing it must not find that the canvas only answers the other one.
    #[test]
    fn both_delete_keys_delete_on_every_host() {
        for host in EVERY_HOST {
            let bindings = for_host(host);
            for keystroke in ["backspace", "delete"] {
                assert!(
                    bindings.iter().any(|binding| binding.keystroke == keystroke
                        && binding.action == EditAction::Delete),
                    "{host:?} does not delete on {keystroke}"
                );
            }
        }
    }

    /// The lock is reachable from the keyboard everywhere, on the letter
    /// Excalidraw uses.
    #[test]
    fn the_tool_lock_is_bound_on_every_host() {
        for host in EVERY_HOST {
            assert!(
                for_host(host)
                    .iter()
                    .any(|binding| binding.action == EditAction::ToggleToolLock
                        && binding.keystroke == "q"),
                "{host:?} cannot toggle the tool lock"
            );
        }
    }

    /// Deleting must not need a modifier, and must not accidentally acquire
    /// one: `Cmd+Backspace` is "delete to start of line" in every text field
    /// and would be a different gesture wearing the same name.
    #[test]
    fn deleting_carries_no_modifier() {
        for host in EVERY_HOST {
            for binding in for_host(host) {
                if binding.action == EditAction::Delete {
                    assert!(
                        !binding.keystroke.contains('-'),
                        "{host:?} binds delete to {}, which carries a modifier",
                        binding.keystroke
                    );
                }
            }
        }
    }

    #[test]
    fn the_compiled_defaults_are_this_host_s_row() {
        assert_eq!(defaults(), for_host(crate::budgets::current_host()));
    }
}
