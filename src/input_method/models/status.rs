//! What the Input method tool says about itself, decided as a pure function.
//!
//! The tool has exactly one sentence of standing state — is it installed, is it
//! running, has the settings change the user just made reached it — and that
//! sentence is chosen by priority rather than from a flag. Priority rules are
//! the kind of thing that goes subtly wrong (an outcome that never wins because
//! a standing state is checked first; a "not applied yet" that never clears),
//! and none of that needs a frame to be wrong, so none of it needs a frame to be
//! tested. [`status_message`] is the whole rule and this module's tests are
//! every branch of it.
//!
//! It lived in `settings::install_status` until the input method became a tool,
//! where it read a global, was `#[cfg(target_os = "macos")]`, and could only be
//! exercised by installing an input method and looking at the dialog.
//!
//! **Nothing here performs a syscall**, which is why liveness arrives as an
//! argument rather than being asked of the [`StatusDocument`]: its
//! `describes_a_live_process` is `kill(pid, 0)`, exists on Unix only, and would
//! make this file both impure and unbuildable on Windows — where the rest of
//! `models/` deliberately still compiles.
//!
//! [`StatusDocument`]: dodo_ime_ipc::status::StatusDocument

use crate::i18n::Str;
use crate::input_method::models::install::{InstallFailure, InstallOutcome, InstallStep};

/// Whether an install is running, and what the last one did.
///
/// Lives beside [`status_message`] rather than in the state layer because it is
/// that function's first argument, and the two are one rule read together.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Install {
    #[default]
    Idle,
    Running,
    Done(InstallOutcome),
}

/// What the status line says, in priority order.
///
/// **The most recent *action* wins over the standing state**: someone who has
/// just pressed the button wants to know what it did, and the install outcome is
/// the only place a refused `TISSelectInputSource` can be reported at all — the
/// bundle is on disk either way, so every standing check below would call it
/// installed and the refusal would never be said.
///
/// `installed` is whether the bundle is in `~/Library/Input Methods`;
/// `settings_applied` is whether the revision dodo last wrote has been echoed
/// back, and `None` when there is nothing to say; `running` is the bundle
/// version of a *live* input-method process, and `None` when the process that
/// wrote the status file has since exited — which is the ordinary state, not a
/// fault.
pub fn status_message(
    install: &Install,
    installed: bool,
    settings_applied: Option<bool>,
    running: Option<&str>,
) -> Str {
    match install {
        Install::Running => Str::InputMethodInstalling,
        Install::Done(InstallOutcome::Ready) => Str::InputMethodInstalled,
        Install::Done(InstallOutcome::Installed { refused, status }) => {
            // The step is not named in the message on purpose: "enable" and
            // "select" are Text Input Sources' words, and what the user has to do
            // about either is the same sentence.
            debug_assert!(matches!(refused, InstallStep::Enable | InstallStep::Select));
            Str::InputMethodInstalledNotActive(*status)
        }
        Install::Done(InstallOutcome::Failed(InstallFailure::NoSourceBundle)) => {
            Str::InputMethodNoBundle
        }
        Install::Done(InstallOutcome::Failed(InstallFailure::Copy { detail })) => {
            Str::InputMethodCopyFailed(detail.clone())
        }
        Install::Done(InstallOutcome::Failed(InstallFailure::InvalidSignature { detail })) => {
            Str::InputMethodInvalidSignature(detail.clone())
        }
        Install::Done(InstallOutcome::Failed(InstallFailure::NeverAppeared { attempts })) => {
            Str::InputMethodNeverAppeared(*attempts)
        }
        // Nothing has been pressed this run, so the state is whatever is on disk.
        Install::Idle if !installed => Str::InputMethodNotInstalled,
        Install::Idle if settings_applied == Some(false) => Str::InputMethodSettingsPending,
        Install::Idle => match running {
            Some(version) => Str::InputMethodRunning(version.to_string()),
            // Installed, and either it has never run or the process that wrote
            // the status file has since exited. macOS stops the agent when
            // nothing is typing at it.
            None => Str::InputMethodInstalledIdle,
        },
    }
}

#[cfg(test)]
mod tests {
    use std::mem::{Discriminant, discriminant};

    use super::{Install, status_message};
    use crate::i18n::Str;
    use crate::input_method::models::install::{InstallFailure, InstallOutcome, InstallStep};

    fn said(
        install: &Install,
        installed: bool,
        applied: Option<bool>,
        running: Option<&str>,
    ) -> Discriminant<Str> {
        discriminant(&status_message(install, installed, applied, running))
    }

    fn is(expected: Str) -> Discriminant<Str> {
        discriminant(&expected)
    }

    /// The whole reason the outcome is checked first. A refused
    /// `TISSelectInputSource` leaves the bundle installed, so every standing
    /// check below it would answer "installed" and the refusal — the only thing
    /// the user can act on — would never be said.
    #[test]
    fn a_just_finished_install_outranks_every_standing_state() {
        let refused = Install::Done(InstallOutcome::Installed {
            refused: InstallStep::Select,
            status: -50,
        });

        // Installed, running, settings applied: everything that could speak
        // instead of the outcome.
        assert_eq!(
            said(&refused, true, Some(true), Some("0.1.0")),
            is(Str::InputMethodInstalledNotActive(-50)),
        );
        assert_eq!(
            said(&Install::Running, true, Some(true), Some("0.1.0")),
            is(Str::InputMethodInstalling),
        );
        assert_eq!(
            said(
                &Install::Done(InstallOutcome::Ready),
                true,
                Some(false),
                None
            ),
            is(Str::InputMethodInstalled),
        );
    }

    /// Every failure says which one it was. They are not interchangeable: one
    /// means this dodo carries no bundle, one means `ditto` refused, and one
    /// means registration was accepted and the source never appeared — which is
    /// the case §2 of `docs/macos-input-method.md` exists for.
    #[test]
    fn each_failure_has_its_own_sentence() {
        for (failure, expected) in [
            (InstallFailure::NoSourceBundle, Str::InputMethodNoBundle),
            (
                InstallFailure::Copy {
                    detail: "ditto: permission denied".into(),
                },
                Str::InputMethodCopyFailed(String::new()),
            ),
            (
                InstallFailure::InvalidSignature {
                    detail: "code object is not signed at all".into(),
                },
                Str::InputMethodInvalidSignature(String::new()),
            ),
            (
                InstallFailure::NeverAppeared { attempts: 5 },
                Str::InputMethodNeverAppeared(0),
            ),
        ] {
            let install = Install::Done(InstallOutcome::Failed(failure));
            assert_eq!(said(&install, false, None, None), is(expected));
        }
    }

    /// `ditto`'s own message is carried through rather than summarised: it is
    /// the only thing that says *why* the copy failed.
    #[test]
    fn a_failed_copy_keeps_the_detail_it_was_given() {
        let install = Install::Done(InstallOutcome::Failed(InstallFailure::Copy {
            detail: "ditto: /Users/x: Permission denied".into(),
        }));
        assert_eq!(
            status_message(&install, false, None, None),
            Str::InputMethodCopyFailed("ditto: /Users/x: Permission denied".into()),
        );
    }

    /// With nothing pressed this run, the four standing states in their own
    /// priority order. "Not installed" comes first because none of the three
    /// below it means anything without a bundle on disk.
    #[test]
    fn the_standing_states_are_ordered_from_the_one_that_makes_the_others_moot() {
        // Not installed wins even over a status file left by an install the user
        // has since deleted by hand.
        assert_eq!(
            said(&Install::Idle, false, Some(true), Some("0.1.0")),
            is(Str::InputMethodNotInstalled),
        );
        // Written but not echoed back: the ordinary state for a moment after a
        // change, and a lasting one when the input method is not running.
        assert_eq!(
            said(&Install::Idle, true, Some(false), Some("0.1.0")),
            is(Str::InputMethodSettingsPending),
        );
        assert_eq!(
            said(&Install::Idle, true, Some(true), Some("0.1.0")),
            is(Str::InputMethodRunning(String::new())),
        );
        // Installed and nothing has ever run: not a fault.
        assert_eq!(
            said(&Install::Idle, true, None, None),
            is(Str::InputMethodInstalledIdle),
        );
    }

    /// "Applied" is only ever reported as *pending*, never as a success
    /// message: `Some(true)` and `None` — the settings have arrived, and there
    /// is nothing to say — read the same, because a row that announced "your
    /// settings are in" on every launch would be noise.
    #[test]
    fn settings_that_have_arrived_say_nothing_more_than_settings_never_changed() {
        assert_eq!(
            said(&Install::Idle, true, Some(true), None),
            said(&Install::Idle, true, None, None),
        );
        assert_ne!(
            said(&Install::Idle, true, Some(false), None),
            said(&Install::Idle, true, None, None),
        );
    }

    /// The running message carries the *bundle's* version, not dodo's. They can
    /// differ — that is the whole point of showing it — and reading dodo's own
    /// would make an out-of-date input method invisible.
    #[test]
    fn the_running_message_reports_the_bundle_version() {
        assert_eq!(
            status_message(&Install::Idle, true, Some(true), Some("0.0.9")),
            Str::InputMethodRunning("0.0.9".into()),
        );
    }

    #[test]
    fn the_default_install_state_is_idle() {
        assert_eq!(Install::default(), Install::Idle);
    }
}
