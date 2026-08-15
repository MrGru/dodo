//! One dialog at a time, for the dialogs that have more than one way in.
//!
//! A dialog layer is a **stack**: `window.open_dialog` pushes, and nothing in
//! the library asks whether the same dialog is already showing. That is right
//! for a confirmation raised over an editor, and wrong for the two dialogs a
//! user can reach from two unrelated places — **Settings** (the sidebar footer
//! and the menu bar item) and the **updater** (the sidebar footer and a
//! background check that found something). Both stacked two identical cards;
//! this is the slot they now share, one per marker type.
//!
//! # Why this is a crate and not `src/dialog_slot.rs`
//!
//! It was that file until 2026-08-15, and it had to stop being one the moment
//! the updater moved out of the binary: the slot is a gpui [`Global`], a
//! `Global` is identified by its **type**, and a copy on each side of a crate
//! boundary is therefore two unrelated slots — the updater would claim one
//! that `src/settings.rs` never reads, and the two identical cards this module
//! exists to prevent would come straight back. There can be exactly one of it,
//! so it has to sit where the binary and every feature crate can both name it.
//! That is the same argument that moved `t()` and the active-language global
//! into `dodo-i18n`. `main.rs` aliases this crate to `crate::dialog_slot`, so
//! every call site is spelled as it was.
//!
//! # The flag is checked against the window, never believed on its own
//!
//! [`claim`] takes the slot and every close path releases it. If a close path
//! were ever missed the flag would outlive its dialog and the dialog could never
//! be opened again for the rest of the session — so [`decide_open`] treats
//! `window.has_active_dialog` as the authority and clears a flag that no dialog
//! backs. That decision is a pure function precisely so the recovery case has a
//! test, and [`claim_with`] keeps the bookkeeping around it testable too — a
//! gpui test window has no platform handle and `Root::new` asks for one, so a
//! real dialog stack is out of reach under `cargo test`.
//!
//! # Releasing is the caller's job, and `on_close` is where it belongs
//!
//! `window.close_dialog` does **not** run a dialog's `on_close` handler — that
//! fires for the close button, the overlay and Escape, each of which already
//! pops exactly one dialog. So a dialog dismissed by its own button has to
//! release the slot beside its own `close_dialog` call, and must not pop a
//! second time: `release` only clears a flag, it never touches the stack.

use std::marker::PhantomData;

use gpui::{App, Global, Window};
use gpui_component::WindowExt as _;

/// A dialog of which there is only ever one on screen.
///
/// Implemented by a marker type per dialog — the type is the key the slot is
/// stored under, so two dialogs cannot accidentally share one flag.
pub trait SingleDialog: 'static {}

/// Whether `K`'s dialog is believed to be on screen. A global rather than a
/// field because the thing being tracked is not a property of any one dialog.
struct OnScreen<K>(bool, PhantomData<fn() -> K>);

impl<K: SingleDialog> Global for OnScreen<K> {}

/// What an open request should do, given what is believed and what is true.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenDecision {
    /// Nothing of ours is showing; open one.
    Open,
    /// Ours is already showing; leave it alone.
    AlreadyShowing,
    /// The flag is stale — no dialog is on screen at all. Clear it and open.
    ClearStaleAndOpen,
}

pub fn decide_open(believed_on_screen: bool, any_dialog_on_screen: bool) -> OpenDecision {
    match (believed_on_screen, any_dialog_on_screen) {
        (false, _) => OpenDecision::Open,
        (true, true) => OpenDecision::AlreadyShowing,
        (true, false) => OpenDecision::ClearStaleAndOpen,
    }
}

/// Takes `K`'s single slot, or reports that it is taken.
///
/// `true` means the caller **must** go on to open the dialog: the slot is
/// already marked taken when this returns. `false` means one is on screen
/// already and the request is to be dropped — the dialog is modal and on top of
/// its own layer, so there is nothing left to focus.
pub fn claim<K: SingleDialog>(window: &mut Window, cx: &mut App) -> bool {
    claim_with::<K>(window.has_active_dialog(cx), cx)
}

/// [`claim`] with the window's answer supplied.
///
/// The seam exists because the window cannot be faked: a gpui test window has no
/// platform handle, and `Root::new` asks for one, so nothing that needs a real
/// dialog stack can run under `cargo test`. Everything this module decides — the
/// flag, its recovery, and giving it back — is therefore reachable from a test
/// with an `App` and no frame at all.
fn claim_with<K: SingleDialog>(any_dialog_on_screen: bool, cx: &mut App) -> bool {
    let believed = cx.try_global::<OnScreen<K>>().is_some_and(|slot| slot.0);
    match decide_open(believed, any_dialog_on_screen) {
        OpenDecision::AlreadyShowing => false,
        OpenDecision::Open | OpenDecision::ClearStaleAndOpen => {
            set_on_screen::<K>(true, cx);
            true
        }
    }
}

/// Releases `K`'s slot. Call this from **every** close path, and never together
/// with a second `close_dialog` — see the module doc.
pub fn release<K: SingleDialog>(cx: &mut App) {
    set_on_screen::<K>(false, cx);
}

fn set_on_screen<K: SingleDialog>(on_screen: bool, cx: &mut App) {
    cx.set_global(OnScreen::<K>(on_screen, PhantomData));
}

#[cfg(test)]
mod tests {
    use gpui::{App, TestAppContext};

    use super::{OpenDecision, SingleDialog, claim_with, decide_open, release};

    /// The reported defect: opening Settings from the tray while the in-app one
    /// was already up put two identical cards on the stack.
    #[test]
    fn a_second_open_request_does_not_stack_another_dialog() {
        assert_eq!(decide_open(true, true), OpenDecision::AlreadyShowing);
    }

    #[test]
    fn the_first_open_request_opens() {
        for on_screen in [false, true] {
            assert_eq!(
                decide_open(false, on_screen),
                OpenDecision::Open,
                "with none of ours on screen, somebody else's dialog must not \
                 block this one"
            );
        }
    }

    /// The recovery case, and the reason the decision is not just the flag: if a
    /// close path ever failed to release the slot, believing the flag would make
    /// the dialog unopenable for the rest of the session. The window is the
    /// authority on what is actually there.
    #[test]
    fn a_flag_that_outlived_its_dialog_is_corrected_rather_than_believed() {
        assert_eq!(decide_open(true, false), OpenDecision::ClearStaleAndOpen);
    }

    struct ProbeDialog;

    impl SingleDialog for ProbeDialog {}

    /// A dialog stack, counted. `Root`'s own is unreachable from a test — see
    /// [`super::claim_with`] — so the caller's half of the protocol is played
    /// out against this instead: claim, and push only if the slot was free.
    #[derive(Default)]
    struct Stack(usize);

    impl Stack {
        fn open(&mut self, cx: &mut App) {
            if !claim_with::<ProbeDialog>(self.0 > 0, cx) {
                return;
            }
            self.0 += 1;
        }

        /// One dismissal, as the close button, the overlay and Escape each do
        /// it: pop once, release once — never pop twice.
        fn dismiss(&mut self, cx: &mut App) {
            self.0 -= 1;
            release::<ProbeDialog>(cx);
        }
    }

    /// The defect end to end: two open requests, one dialog.
    #[gpui::test]
    fn two_open_requests_leave_exactly_one_dialog(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let mut stack = Stack::default();
            stack.open(cx);
            stack.open(cx);
            assert_eq!(stack.0, 1, "the second request must add no dialog");
        });
    }

    /// The other half of the acceptance: the reused dialog closes cleanly on one
    /// dismiss, and the stack is balanced — and empty — afterwards.
    #[gpui::test]
    fn one_dismissal_empties_the_stack_and_frees_the_slot(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let mut stack = Stack::default();
            stack.open(cx);
            stack.open(cx);

            stack.dismiss(cx);
            assert_eq!(stack.0, 0, "one dismissal must empty the stack");

            stack.open(cx);
            assert_eq!(stack.0, 1, "and the slot is free again afterwards");
        });
    }
}
