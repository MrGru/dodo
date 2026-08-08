//! Running the install, and the seam that lets it be tested without a machine.
//!
//! [`install`] is the driver: it walks
//! [`InstallStep::ORDER`](crate::input_method::models::install::InstallStep::ORDER),
//! retries registration until the source is visible, and decides nothing else.
//! Every judgement is in [`models::install`](crate::input_method::models::install)
//! and every effect is behind [`InstallOps`], so the sequence — including the two
//! parts that would otherwise only be provable by installing an input method on
//! a real Mac and watching — is a unit test.
//!
//! # What the seam is for
//!
//! Not mocking for its own sake. Three defects in this sequence are silent on the
//! machine and obvious to a fake:
//!
//! - handing the **parent** identifier to enable or select instead of the mode,
//! - registering **once** and believing the `0` it returned,
//! - killing the running bundle **before** enabling, so the relaunched process
//!   serves the new binary and the enable applies to nothing.
//!
//! [`tests`] catches all three by recording the calls.
//!
//! # Threads, and the crash that decided them
//!
//! This is **blocking by contract**, like `Transport::execute` and every store in
//! `src/api_explorer/services/`: it copies a directory tree, shells out twice and
//! sleeps between registration attempts. Callers run it on GPUI's background
//! executor, so none of that touches the UI thread.
//!
//! The four Text Input Sources calls are the exception, and they are the exception
//! because of a measurement rather than a preference: **two concurrent
//! `TISCreateInputSourceList` calls abort the process** — `SIGABRT` inside
//! HIToolbox, found by running `super::tis`'s own tests in parallel. Calling TIS
//! from a background thread is fine; calling it while something else is also
//! calling it is not, and *AppKit calls TIS on the main thread whenever it likes*
//! — on focus changes, on key events, whenever a text input context is set up. A
//! lock of ours cannot serialise against that.
//!
//! So [`SystemOps`] performs each TIS call on the **main queue**, through
//! [`on_main`], and the driver above stays exactly as it was. That is what the
//! [`InstallOps`] seam was for; it simply got used in this round rather than a
//! later one. The hop costs microseconds per call and none of the waiting — the
//! sleeps stay on the background thread, where they belong.

use std::path::Path;

use crate::input_method::models::install::{
    InstallFailure, InstallOutcome, InstallPlan, InstallReport, InstallStep, REGISTER_ATTEMPTS,
    REGISTER_RETRY_DELAY, selectable_source,
};

/// Which bundle to install and where to put it, or why there is nothing to do.
///
/// The one part of planning that cannot be pure: it asks the filesystem which of
/// [`source_candidates`](crate::input_method::models::install::source_candidates)'
/// candidates is really there. Everything about *what* the candidates are is in
/// `models::install` and tested without a disk.
pub fn resolve_plan(
    executable: &Path,
    working_directory: &Path,
    home: &Path,
) -> Result<InstallPlan, InstallFailure> {
    let source =
        crate::input_method::models::install::source_candidates(executable, working_directory)
            .into_iter()
            // `is_dir` rather than `exists`: an `.app` is a directory, and a stray file
            // of that name is not a bundle.
            .find(|candidate| candidate.is_dir())
            .ok_or(InstallFailure::NoSourceBundle)?;

    Ok(InstallPlan {
        source,
        destination: dodo_ime_ipc::paths::installed_bundle(home),
    })
}

/// Every effect an install has, so the driver can be run against a fake.
///
/// The two TIS methods answer `None` for "no such source" and `Some(status)` for
/// "the system had an opinion", which is the distinction the driver needs: the
/// first means registration has not taken effect, the second that it has.
pub trait InstallOps {
    /// Copy the bundle, replacing whatever is at the destination.
    fn copy_bundle(&self, source: &Path, destination: &Path) -> Result<(), String>;

    /// `TISRegisterInputSource`. The returned status is deliberately unused by
    /// the driver — see [`InstallOps::is_visible`].
    fn register(&self, bundle: &Path);

    /// Whether the system can see the source yet. **This**, not `register`'s
    /// return value, is the answer.
    fn is_visible(&self, source_id: &str) -> bool;

    fn enable(&self, source_id: &str) -> Option<i32>;

    fn select(&self, source_id: &str) -> Option<i32>;

    /// Kill any input-method process still serving the old bundle.
    fn restart(&self);

    /// Wait before the next registration attempt. **How long** is the driver's
    /// decision, not the implementation's — see
    /// [`REGISTER_RETRY_DELAY`](crate::input_method::models::install::REGISTER_RETRY_DELAY).
    fn wait(&self, delay: std::time::Duration);
}

/// Copy, register until visible, enable the mode, select the mode, kill the old
/// process.
///
/// The order is `InstallStep::ORDER` and the report says which steps ran, so the
/// order is asserted rather than described.
pub fn install(plan: &InstallPlan, ops: &dyn InstallOps) -> InstallReport {
    let mut steps = Vec::new();
    let source_id = selectable_source();

    steps.push(InstallStep::Copy);
    if let Err(detail) = ops.copy_bundle(&plan.source, &plan.destination) {
        return InstallReport {
            outcome: InstallOutcome::Failed(InstallFailure::Copy { detail }),
            steps,
            register_attempts: 0,
        };
    }

    // §2: registration returns `0` whether or not the source appears, and after a
    // reinstall at the same identifier it did not appear until the call was
    // repeated. So the loop's exit condition is the *database*, and the return
    // value is not consulted at all.
    steps.push(InstallStep::Register);
    let mut register_attempts = 0;
    let visible = loop {
        ops.register(&plan.destination);
        register_attempts += 1;

        if ops.is_visible(source_id) {
            break true;
        }
        if register_attempts >= REGISTER_ATTEMPTS {
            break false;
        }
        ops.wait(REGISTER_RETRY_DELAY);
    };

    if !visible {
        return InstallReport {
            outcome: InstallOutcome::Failed(InstallFailure::NeverAppeared {
                attempts: register_attempts,
            }),
            steps,
            register_attempts,
        };
    }

    // The mode, never the parent. `selectable_source` is the only place that
    // choice is made, and `models::install` is where it is tested.
    steps.push(InstallStep::Enable);
    let enable = ops.enable(source_id);

    steps.push(InstallStep::Select);
    let select = ops.select(source_id);

    // Last, and unconditionally: replacing the bundle on disk does not restart
    // the process serving from it, so an upgrade only takes effect once the old
    // one exits. Doing it after enable/select rather than before is what makes
    // the relaunched process pick up both the new binary and the new state.
    steps.push(InstallStep::Restart);
    ops.restart();

    // A refusal is reported, not diagnosed. `-50` here is what this project's own
    // test machine returns for every input source including Apple's, so the code
    // may not call it a defect — see `InstallOutcome::Installed`.
    let outcome = match (enable, select) {
        (Some(0), Some(0)) => InstallOutcome::Ready,
        (Some(status), _) if status != 0 => InstallOutcome::Installed {
            refused: InstallStep::Enable,
            status,
        },
        // Enabling worked and the source is not in the *selectable* list, which
        // means the enable has not taken effect. Reported as a refusal of the
        // select step, because that is the step the user is denied.
        (_, None) => InstallOutcome::Installed {
            refused: InstallStep::Select,
            status: 0,
        },
        (_, Some(status)) => InstallOutcome::Installed {
            refused: InstallStep::Select,
            status,
        },
    };

    InstallReport {
        outcome,
        steps,
        register_attempts,
    }
}

/// The real thing: `ditto`, Text Input Sources, and `pkill`.
///
/// Only exists on macOS, because none of the three does. Everything above this
/// point compiles and is tested on every platform.
#[cfg(target_os = "macos")]
pub struct SystemOps;

/// Runs `work` on the main queue and waits for its answer.
///
/// Why: see the module docs. Every Text Input Sources call goes through this, so
/// that dodo's calls are serialised against AppKit's own by the main run loop
/// rather than by a lock that cannot see them.
///
/// Two things about it that matter:
///
/// - **`dispatch_sync` to the main queue deadlocks if you are already on the main
///   thread.** The install always runs on the background executor, so that cannot
///   happen today; the `MainThreadMarker` check makes it impossible rather than
///   merely unlikely, because a future caller would otherwise hang dodo with no
///   clue why.
/// - **It needs a main thread that drains its queue.** dodo's does — that is what
///   `[NSApp run]` is — but a `cargo test` process does not, which is why nothing
///   in the test suite constructs [`SystemOps`]. The driver is tested against a
///   fake and `super::tis` is tested directly, both of which stay on the calling
///   thread.
#[cfg(target_os = "macos")]
fn on_main<T: Send>(work: impl Send + FnOnce() -> T) -> T {
    if objc2::MainThreadMarker::new().is_some() {
        return work();
    }

    let mut answer = None;
    dispatch2::DispatchQueue::main().exec_sync(|| answer = Some(work()));
    // `exec_sync` returns only after the block has run, so this is always `Some`.
    answer.expect("dispatch_sync ran the block")
}

#[cfg(target_os = "macos")]
impl InstallOps for SystemOps {
    /// `ditto <source> <destination>`.
    ///
    /// **The destination names the bundle**, and that is not a detail:
    /// `ditto "X.app" some/dir/` copies X.app's *contents* into `some/dir`, so
    /// the documented-by-hand form in §2 leaves `Contents/` sitting directly in
    /// `~/Library/Input Methods`. Measured while writing this round; §2 now says
    /// so.
    ///
    /// `ditto` rather than a Rust recursive copy because it is what Apple's own
    /// documentation prescribes for bundles: it preserves symlinks, resource
    /// forks and extended attributes, and a hand-rolled `copy_dir_all` that
    /// silently dropped one of those would produce a bundle that installs and
    /// does not launch.
    fn copy_bundle(&self, source: &Path, destination: &Path) -> Result<(), String> {
        // `ditto` replaces files but does not remove ones that are no longer in
        // the source, so an upgrade that dropped a file would leave it behind.
        // Removing first makes the destination exactly the source.
        if destination.exists() {
            std::fs::remove_dir_all(destination)
                .map_err(|err| format!("{}: {err}", destination.display()))?;
        }

        let output = std::process::Command::new("/usr/bin/ditto")
            .arg(source)
            .arg(destination)
            .output()
            .map_err(|err| format!("ditto: {err}"))?;

        if output.status.success() {
            return Ok(());
        }
        // `ditto` writes its complaint to stderr. Third-party English, kept
        // verbatim inside a translated frame, which is `i18n.rs`'s convention.
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        Err(if detail.is_empty() {
            format!("ditto: {}", output.status)
        } else {
            detail
        })
    }

    fn register(&self, bundle: &Path) {
        // The status is discarded on purpose: it is `0` for a bundle that never
        // appears. `is_visible` is the answer.
        on_main(|| {
            let _ = super::tis::register(bundle);
        });
    }

    fn is_visible(&self, source_id: &str) -> bool {
        on_main(|| super::tis::is_visible(source_id))
    }

    fn enable(&self, source_id: &str) -> Option<i32> {
        on_main(|| super::tis::enable(source_id))
    }

    fn select(&self, source_id: &str) -> Option<i32> {
        on_main(|| super::tis::select(source_id))
    }

    /// `pkill -x DodoVietnamese`.
    ///
    /// `-x` matches the process *name* exactly, not the command line: `pkill -f
    /// DodoVietnamese` would match any process whose arguments happen to contain
    /// the string, which on a developer's machine includes the editor that has
    /// this file open. Without root, `pkill` can only signal this user's own
    /// processes, and the only process with this name is the one dodo installed.
    ///
    /// Exit status 1 means "nothing matched", which is the ordinary first-install
    /// case and not a failure. Nothing is reported either way: macOS relaunches
    /// the input method on the next input session, so a kill that did not happen
    /// costs an upgrade one login, not correctness.
    fn restart(&self) {
        let _ = std::process::Command::new("/usr/bin/pkill")
            .arg("-x")
            .arg("DodoVietnamese")
            .output();
    }

    fn wait(&self, delay: std::time::Duration) {
        // On the background executor, which is what keeps the seconds §2 warns
        // about off the UI thread.
        std::thread::sleep(delay);
    }
}

#[cfg(test)]
mod tests {
    use super::{InstallOps, install};
    use crate::input_method::models::install::{
        InstallFailure, InstallOutcome, InstallPlan, InstallStep, REGISTER_ATTEMPTS,
        REGISTER_RETRY_DELAY, parent_input_method, selectable_source,
    };
    use std::cell::RefCell;
    use std::path::{Path, PathBuf};

    /// Every call the driver made, in order.
    #[derive(Debug, PartialEq, Eq)]
    enum Call {
        Copy {
            source: PathBuf,
            destination: PathBuf,
        },
        Register(PathBuf),
        Visible(String),
        Enable(String),
        Select(String),
        Restart,
        Wait(std::time::Duration),
    }

    /// A machine that does whatever the test says.
    struct Fake {
        calls: RefCell<Vec<Call>>,
        copy: Result<(), String>,
        /// How many registration attempts happen before the source appears.
        /// `None` means never.
        visible_after: Option<u32>,
        registers: RefCell<u32>,
        enable: Option<i32>,
        select: Option<i32>,
    }

    impl Fake {
        fn ready() -> Fake {
            Fake {
                calls: RefCell::new(Vec::new()),
                copy: Ok(()),
                visible_after: Some(1),
                registers: RefCell::new(0),
                enable: Some(0),
                select: Some(0),
            }
        }

        fn calls(&self) -> std::cell::Ref<'_, Vec<Call>> {
            self.calls.borrow()
        }
    }

    impl InstallOps for Fake {
        fn copy_bundle(&self, source: &Path, destination: &Path) -> Result<(), String> {
            self.calls.borrow_mut().push(Call::Copy {
                source: source.to_owned(),
                destination: destination.to_owned(),
            });
            self.copy.clone()
        }

        fn register(&self, bundle: &Path) {
            *self.registers.borrow_mut() += 1;
            self.calls
                .borrow_mut()
                .push(Call::Register(bundle.to_owned()));
        }

        fn is_visible(&self, source_id: &str) -> bool {
            self.calls
                .borrow_mut()
                .push(Call::Visible(source_id.to_owned()));
            self.visible_after
                .is_some_and(|after| *self.registers.borrow() >= after)
        }

        fn enable(&self, source_id: &str) -> Option<i32> {
            self.calls
                .borrow_mut()
                .push(Call::Enable(source_id.to_owned()));
            self.enable
        }

        fn select(&self, source_id: &str) -> Option<i32> {
            self.calls
                .borrow_mut()
                .push(Call::Select(source_id.to_owned()));
            self.select
        }

        fn restart(&self) {
            self.calls.borrow_mut().push(Call::Restart);
        }

        fn wait(&self, delay: std::time::Duration) {
            self.calls.borrow_mut().push(Call::Wait(delay));
        }
    }

    fn plan() -> InstallPlan {
        InstallPlan {
            source: PathBuf::from("/dodo.app/Contents/Helpers/Dodo Vietnamese.app"),
            destination: PathBuf::from("/home/Library/Input Methods/Dodo Vietnamese.app"),
        }
    }

    #[test]
    fn the_happy_path_runs_every_step_once_in_order() {
        let fake = Fake::ready();
        let report = install(&plan(), &fake);

        assert_eq!(report.outcome, InstallOutcome::Ready);
        assert_eq!(report.steps, InstallStep::ORDER.to_vec());
        assert_eq!(report.register_attempts, 1);
    }

    /// The defect a fake catches and a Mac does not: the identifier handed to
    /// enable and select is the **mode**, and the parent never reaches TIS.
    #[test]
    fn enable_and_select_take_the_mode_and_never_the_parent() {
        let fake = Fake::ready();
        install(&plan(), &fake);

        let calls = fake.calls();
        assert!(calls.contains(&Call::Enable(selectable_source().to_owned())));
        assert!(calls.contains(&Call::Select(selectable_source().to_owned())));

        let parent = parent_input_method().to_owned();
        assert!(
            !calls.iter().any(|call| matches!(
                call,
                Call::Enable(id) | Call::Select(id) | Call::Visible(id) if *id == parent
            )),
            "the parent input method must never be enabled or selected: {calls:?}"
        );
    }

    /// §2's "once is not always enough": registration is repeated until the
    /// source is visible, with a wait in between.
    #[test]
    fn registration_repeats_until_the_source_appears() {
        let fake = Fake {
            visible_after: Some(3),
            ..Fake::ready()
        };
        let report = install(&plan(), &fake);

        assert_eq!(report.outcome, InstallOutcome::Ready);
        assert_eq!(report.register_attempts, 3);

        let calls = fake.calls();
        let waits: Vec<_> = calls
            .iter()
            .filter(|call| matches!(call, Call::Wait(_)))
            .collect();
        assert_eq!(waits.len(), 2, "one wait between each pair of attempts");
        assert!(
            waits
                .iter()
                .all(|call| **call == Call::Wait(REGISTER_RETRY_DELAY)),
            "the delay is the driver's policy, not the implementation's: {waits:?}"
        );
        assert_eq!(
            calls.first(),
            Some(&Call::Copy {
                source: plan().source,
                destination: plan().destination
            })
        );
    }

    #[test]
    fn a_source_that_never_appears_fails_rather_than_enabling_nothing() {
        let fake = Fake {
            visible_after: None,
            ..Fake::ready()
        };
        let report = install(&plan(), &fake);

        assert_eq!(
            report.outcome,
            InstallOutcome::Failed(InstallFailure::NeverAppeared {
                attempts: REGISTER_ATTEMPTS
            })
        );
        assert_eq!(report.register_attempts, REGISTER_ATTEMPTS);
        assert!(
            !fake
                .calls()
                .iter()
                .any(|call| matches!(call, Call::Enable(_) | Call::Select(_))),
            "nothing is enabled when nothing is there"
        );
        assert_eq!(report.steps, vec![InstallStep::Copy, InstallStep::Register]);
    }

    #[test]
    fn a_failed_copy_stops_before_touching_the_system() {
        let fake = Fake {
            copy: Err("ditto: No space left on device".to_owned()),
            ..Fake::ready()
        };
        let report = install(&plan(), &fake);

        assert_eq!(
            report.outcome,
            InstallOutcome::Failed(InstallFailure::Copy {
                detail: "ditto: No space left on device".to_owned()
            })
        );
        assert_eq!(
            *fake.calls(),
            vec![Call::Copy {
                source: plan().source,
                destination: plan().destination
            }]
        );
        assert_eq!(report.register_attempts, 0);
    }

    /// The measured environment behaviour, as the outcome the UI shows. It is an
    /// *install*, not a failure — §5's control is that Apple's own input methods
    /// fail identically on the same machine.
    #[test]
    fn a_refused_selection_is_an_install_that_is_not_active() {
        let fake = Fake {
            select: Some(-50),
            ..Fake::ready()
        };
        let report = install(&plan(), &fake);

        assert_eq!(
            report.outcome,
            InstallOutcome::Installed {
                refused: InstallStep::Select,
                status: -50
            }
        );
        assert!(report.outcome.is_installed());
        assert_eq!(report.steps, InstallStep::ORDER.to_vec());
    }

    #[test]
    fn a_refused_enable_is_reported_as_the_enable_step() {
        let fake = Fake {
            enable: Some(-25_211),
            ..Fake::ready()
        };
        let report = install(&plan(), &fake);

        assert_eq!(
            report.outcome,
            InstallOutcome::Installed {
                refused: InstallStep::Enable,
                status: -25_211
            }
        );
    }

    /// Enabling reported success and the source is still not in the selectable
    /// list. Reported against the step the user is actually denied.
    #[test]
    fn an_enable_that_did_not_take_is_reported_against_select() {
        let fake = Fake {
            select: None,
            ..Fake::ready()
        };
        let report = install(&plan(), &fake);

        assert_eq!(
            report.outcome,
            InstallOutcome::Installed {
                refused: InstallStep::Select,
                status: 0
            }
        );
    }

    /// The upgrade case, and the ordering §2 is emphatic about: the old process is
    /// killed **after** the bundle is in place and registered, never before.
    #[test]
    fn the_old_process_is_killed_last() {
        let fake = Fake::ready();
        install(&plan(), &fake);

        let calls = fake.calls();
        let restart = calls
            .iter()
            .position(|call| *call == Call::Restart)
            .expect("the running bundle is killed");
        assert_eq!(
            restart,
            calls.len() - 1,
            "kill is the last thing that happens: {calls:?}"
        );
        assert!(
            calls
                .iter()
                .position(|call| matches!(call, Call::Copy { .. }))
                .unwrap()
                < restart
        );
    }

    /// Even a refused selection ends with the kill, because the *bundle* was
    /// replaced whatever the system thought of switching to it.
    #[test]
    fn an_upgrade_that_cannot_be_selected_still_restarts_the_old_process() {
        let fake = Fake {
            select: Some(-50),
            ..Fake::ready()
        };
        install(&plan(), &fake);
        assert_eq!(fake.calls().last(), Some(&Call::Restart));
    }

    /// A dodo with no input method to install says so, rather than shelling out
    /// to `ditto` with a path that is not there.
    #[test]
    fn a_build_that_carries_no_bundle_has_nothing_to_install() {
        let dir = std::env::temp_dir().join(format!("dodo-im-plan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        assert_eq!(
            super::resolve_plan(&dir.join("target/debug/dodo"), &dir, &dir),
            Err(crate::input_method::models::install::InstallFailure::NoSourceBundle)
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_development_bundle_is_found_and_the_destination_is_the_system_directory() {
        let dir = std::env::temp_dir().join(format!("dodo-im-plan-dist-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let source = dir.join("dist").join("Dodo Vietnamese.app");
        std::fs::create_dir_all(&source).unwrap();

        let plan = super::resolve_plan(&dir.join("target/debug/dodo"), &dir, &dir).unwrap();
        assert_eq!(plan.source, source);
        assert_eq!(
            plan.destination,
            dir.join("Library")
                .join("Input Methods")
                .join("Dodo Vietnamese.app")
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A file where the bundle should be is not a bundle.
    #[test]
    fn a_plain_file_of_the_right_name_is_not_a_bundle() {
        let dir = std::env::temp_dir().join(format!("dodo-im-plan-file-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("dist")).unwrap();
        std::fs::write(
            dir.join("dist").join("Dodo Vietnamese.app"),
            b"not a bundle",
        )
        .unwrap();

        assert_eq!(
            super::resolve_plan(&dir.join("dodo"), &dir, &dir),
            Err(crate::input_method::models::install::InstallFailure::NoSourceBundle)
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
