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
//! Undo and redo only. §26's full list — nudge, delete, duplicate, copy/paste,
//! zoom-to-fit, zoom-to-selection — is a longer table over the same type, and
//! each row needs a command the engine can perform; the ones that exist today
//! have no gesture asking for them yet. A row here that no code reads would be
//! a binding that silently does nothing, which is worse than an absent one.
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

/// What a binding does. One variant per command the canvas can be driven to
/// perform from the keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EditAction {
    Undo,
    Redo,
}

impl EditAction {
    /// A short stable name, for a test or a debug overlay. Not user-facing —
    /// see [`crate::commands::edit::EditCommand::kind`].
    pub fn name(self) -> &'static str {
        match self {
            EditAction::Undo => "undo",
            EditAction::Redo => "redo",
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
pub const fn for_host(host: HostOs) -> &'static [Binding] {
    match host {
        HostOs::MacOs => MACOS,
        // Windows and every Linux desktop dodo targets use the same three, and
        // splitting them would be two identical tables to keep in step.
        HostOs::Windows | HostOs::Unix => PC,
    }
}

/// The bindings for the machine this was compiled for.
pub fn defaults() -> &'static [Binding] {
    for_host(crate::budgets::current_host())
}

#[cfg(test)]
mod tests {
    use super::{Binding, EditAction, defaults, for_host};
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
    #[test]
    fn each_host_gets_its_own_modifier() {
        assert!(
            for_host(HostOs::MacOs)
                .iter()
                .all(|binding| binding.keystroke.starts_with("cmd-"))
        );
        for host in [HostOs::Windows, HostOs::Unix] {
            assert!(
                for_host(host)
                    .iter()
                    .all(|binding| binding.keystroke.starts_with("ctrl-"))
            );
        }
    }

    /// `Ctrl+Y` is the second redo people arrive with on a PC, and macOS
    /// deliberately does not get a `Cmd+Y` invented for it.
    #[test]
    fn a_pc_answers_both_redo_conventions_and_macos_answers_one() {
        let redos = |host| {
            for_host(host)
                .iter()
                .filter(|binding: &&Binding| binding.action == EditAction::Redo)
                .count()
        };

        assert_eq!(redos(HostOs::MacOs), 1);
        assert_eq!(redos(HostOs::Windows), 2);
        assert_eq!(redos(HostOs::Unix), 2);
    }

    #[test]
    fn the_compiled_defaults_are_this_host_s_row() {
        assert_eq!(defaults(), for_host(crate::budgets::current_host()));
    }
}
