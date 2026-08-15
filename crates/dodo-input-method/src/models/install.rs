//! What installing dodo's input method *is*, as data.
//!
//! Everything on this page is pure. The five steps, the order they must happen
//! in, which identifier each one takes, where the bundle is copied from and to,
//! how many times registration may be retried, and what each failure means —
//! all of it decided here and asserted by unit tests, so that
//! [`services::installer`](crate::services::installer) is a driver
//! with no judgement in it and the UI callback is three lines.
//!
//! `docs/macos-input-method.md` §2 is the authority the tests here encode. Three
//! things in it are the whole reason this file exists rather than a function in
//! the view:
//!
//! - **`TISRegisterInputSource` returning `0` does not mean the source exists.**
//!   On a fresh install it can take a few seconds to appear in
//!   `TISCreateInputSourceList`, and after a remove-and-reinstall at the same
//!   identifier it did not appear until the call was *repeated*. So registration
//!   is a loop that ends when the source is visible, not a call whose return
//!   value is believed.
//! - **Enable and select the mode, never the parent.** The parent's
//!   `kTISPropertyInputSourceIsSelectCapable` is false and selecting it fails
//!   with `-50`. The two identifiers differ by one suffix, which is exactly the
//!   kind of mistake that produces a plausible-looking failure.
//! - **Replacing the bundle on disk does not restart the running process.** The
//!   old binary keeps serving until it exits, so an upgrade has to end by killing
//!   it and letting macOS relaunch it.

use std::path::{Path, PathBuf};

use dodo_ime_ipc::bundle::{BUNDLE_IDENTIFIER, BUNDLE_NAME, INPUT_SOURCE_ID};

/// The steps of an install, in the order they must happen.
///
/// The order is not a matter of taste and each adjacent pair has a reason:
/// the signature must be verified before the bundle is copied into the system
/// directory; nothing can be registered before it is on disk; nothing can be enabled before
/// the system can see it; selecting an input source that is not enabled fails;
/// and killing the old process must come last, because everything above it
/// operates on the *bundle* and this one operates on the process serving from it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstallStep {
    /// Verify the source bundle with `codesign --verify --deep --strict`.
    VerifySignature,
    /// `ditto` the verified bundle into `~/Library/Input Methods`.
    Copy,
    /// `TISRegisterInputSource`, until the source is visible.
    Register,
    /// `TISEnableInputSource`, on the mode.
    Enable,
    /// `TISSelectInputSource`, on the mode.
    Select,
    /// Kill any `DodoVietnamese` still serving the previous bundle.
    Restart,
}

impl InstallStep {
    /// Every step, in order.
    ///
    /// Read by tests rather than by the driver, deliberately: the driver records
    /// each step as it performs it and the test compares the two lists, which a
    /// driver iterating this array could not disagree with. `#[allow]` rather than
    /// `#[cfg(test)]` because it is documentation of the sequence and belongs in
    /// the shipped module — the condition for removing it is a caller outside
    /// tests, which would probably be a mistake.
    #[allow(dead_code)]
    pub const ORDER: [InstallStep; 6] = [
        InstallStep::VerifySignature,
        InstallStep::Copy,
        InstallStep::Register,
        InstallStep::Enable,
        InstallStep::Select,
        InstallStep::Restart,
    ];
}

/// How many times registration is attempted before giving up.
///
/// §2's "once is not always enough" measured *twice* being enough after a
/// reinstall. Five is that with room, and the loop exits as soon as the source is
/// visible, so the number only bounds the failing case.
pub const REGISTER_ATTEMPTS: u32 = 5;

/// How long to wait between registration attempts.
///
/// §2: the source "can take a few seconds to appear". Waiting 700ms four times
/// spends at most 2.8 seconds on a failure and usually nothing at all, which is
/// the right shape for something a user is watching a button for.
pub const REGISTER_RETRY_DELAY: std::time::Duration = std::time::Duration::from_millis(700);

/// The identifier `TISEnableInputSource` and `TISSelectInputSource` take.
///
/// **The mode, never the parent.** This is a function rather than a re-export so
/// that the choice has a name, a doc comment and a test —
/// [`tests::the_selectable_source_is_the_mode_and_not_the_parent`] is what fails
/// if someone "simplifies" it to [`BUNDLE_IDENTIFIER`], which is the shorter
/// string, the one the bundle's `Info.plist` calls its identifier, and wrong.
pub fn selectable_source() -> &'static str {
    INPUT_SOURCE_ID
}

/// The parent input method, which is what must *not* be enabled or selected.
///
/// Named so that the distinction is visible at the call site and assertable in a
/// test. Nothing in the install sequence passes this to TIS — which is why it is
/// `#[allow(dead_code)]`: its only callers are the two tests that prove the
/// parent never reaches `TISEnableInputSource` or `TISSelectInputSource`, and a
/// non-test caller would be the bug it exists to catch.
#[allow(dead_code)]
pub fn parent_input_method() -> &'static str {
    BUNDLE_IDENTIFIER
}

/// Where the bundle is copied from and to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstallPlan {
    /// The `Dodo Vietnamese.app` to install, which exists.
    pub source: PathBuf,
    /// `~/Library/Input Methods/Dodo Vietnamese.app`.
    pub destination: PathBuf,
}

/// Places the bundle to install might be, best first.
///
/// Two, and both are real:
///
/// - **`<dodo.app>/Contents/Helpers/Dodo Vietnamese.app`** — where
///   `scripts/macos-app-bundle.sh --input-method` nests it. That is the shipping
///   case, and the location is fixed by `docs/macos-signing.md` §7.2 rather than
///   chosen here.
/// - **`<cwd>/dist/Dodo Vietnamese.app`** — where
///   `scripts/macos-input-method-bundle.sh` writes it. This is the *development*
///   case: a `cargo run` binary is not in a bundle at all and has no `Helpers`
///   directory to look in, and without this candidate the install button could
///   not be exercised without packaging first.
///
/// The `Helpers` candidate is only offered when the executable really is inside a
/// bundle — `…/Contents/MacOS/dodo`. A bare binary at `/usr/local/bin/dodo`
/// would otherwise produce `/usr/local/Helpers/…`, which is not wrong so much as
/// meaningless, and a candidate list is easier to reason about when every entry
/// is somewhere a bundle is actually put.
pub fn source_candidates(executable: &Path, working_directory: &Path) -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    // `…/Contents/MacOS/dodo` → `…/Contents`.
    if let Some(contents) = executable.parent().and_then(Path::parent)
        && contents.file_name().is_some_and(|name| name == "Contents")
    {
        candidates.push(contents.join("Helpers").join(BUNDLE_NAME));
    }

    candidates.push(working_directory.join("dist").join(BUNDLE_NAME));
    candidates
}

/// Why an install did not finish.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InstallFailure {
    /// There is no bundle to install. This dodo was built or packaged without
    /// one — `scripts/package.sh` does not pass `--input-method` yet, so a
    /// released `dodo.app` is exactly this case until the release round lands.
    NoSourceBundle,
    /// `ditto` refused. The detail is `ditto`'s own message, and is third-party
    /// English kept verbatim inside a translated frame.
    Copy { detail: String },
    /// The bundle's code signature failed verification with
    /// `codesign --verify --deep --strict`. The detail is `codesign`'s own
    /// message. An invalid signature cannot produce a misleading successful
    /// install result.
    InvalidSignature { detail: String },
    /// Registration was accepted and the source never appeared in the Text Input
    /// Sources database. Distinct from every other failure because it is the one
    /// where `TISRegisterInputSource` *returned success* — see the module docs.
    NeverAppeared { attempts: u32 },
}

/// What happened, in the terms the Input method tool reports.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InstallOutcome {
    /// Copied, registered, visible, enabled and selected. The user is typing
    /// Vietnamese now.
    Ready,
    /// Installed and visible, and the system declined to enable or switch to it.
    ///
    /// **This is not necessarily a defect in dodo**, and the code must not
    /// present it as one. On the machine this round was written on,
    /// `TISSelectInputSource` returns `-50` for *every* input source including
    /// Apple's own `VietnameseTelex` — `docs/macos-input-method.md` §5 has the
    /// measurement and the control. The user can still turn the input method on
    /// in System Settings, which is what the message says.
    Installed {
        refused: InstallStep,
        status: i32,
    },
    Failed(InstallFailure),
}

impl InstallOutcome {
    /// Whether the bundle is now in `~/Library/Input Methods` and known to the
    /// system, whatever happened afterwards.
    pub fn is_installed(&self) -> bool {
        matches!(
            self,
            InstallOutcome::Ready | InstallOutcome::Installed { .. }
        )
    }
}

/// What an install did, for the UI and for the tests.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstallReport {
    pub outcome: InstallOutcome,
    /// The steps that ran, in the order they ran. Asserted against
    /// [`InstallStep::ORDER`], which is how §2's sequence is held in place.
    pub steps: Vec<InstallStep>,
    /// How many `TISRegisterInputSource` calls it took before the source was
    /// visible. `0` when the copy failed and registration never ran.
    pub register_attempts: u32,
}

#[cfg(test)]
mod tests {
    use super::{
        BUNDLE_NAME, InstallOutcome, InstallStep, REGISTER_ATTEMPTS, parent_input_method,
        selectable_source, source_candidates,
    };
    use std::path::{Path, PathBuf};

    /// The mistake this file exists to prevent. §2 is emphatic: the parent has
    /// `kTISPropertyInputSourceIsSelectCapable = false` and selecting it fails
    /// with `-50`, so a "simplification" to the bundle identifier produces an
    /// install that looks like it worked and cannot be switched to.
    #[test]
    fn the_selectable_source_is_the_mode_and_not_the_parent() {
        assert_ne!(
            selectable_source(),
            parent_input_method(),
            "TISEnableInputSource and TISSelectInputSource take the mode"
        );
        assert!(
            selectable_source().starts_with(parent_input_method()),
            "the mode is a child of the input method"
        );
        assert_eq!(
            selectable_source(),
            format!("{}.Vietnamese", parent_input_method())
        );
    }

    #[test]
    fn the_steps_are_in_the_order_section_two_requires() {
        assert_eq!(
            InstallStep::ORDER,
            [
                InstallStep::VerifySignature,
                InstallStep::Copy,
                InstallStep::Register,
                InstallStep::Enable,
                InstallStep::Select,
                InstallStep::Restart
            ]
        );
        assert_eq!(
            *InstallStep::ORDER.last().unwrap(),
            InstallStep::Restart,
            "killing the old process is last, so an upgrade takes effect"
        );
    }

    #[test]
    fn registration_is_retried_more_than_once() {
        // Read through a binding: an `assert!` on a `const` is a compile-time
        // tautology and clippy rejects it, the same way `DEFAULT_CONFIG`'s tests
        // in `dodo-ime-macos` do.
        let attempts = std::hint::black_box(REGISTER_ATTEMPTS);
        assert!(
            attempts > 1,
            "§2: `TISRegisterInputSource` returning 0 once is not always enough"
        );
    }

    #[test]
    fn a_packaged_dodo_finds_the_bundle_it_carries() {
        let candidates = source_candidates(
            Path::new("/Applications/dodo.app/Contents/MacOS/dodo"),
            Path::new("/Users/someone"),
        );
        assert_eq!(
            candidates.first(),
            Some(&PathBuf::from(format!(
                "/Applications/dodo.app/Contents/Helpers/{BUNDLE_NAME}"
            ))),
            "the nested bundle is the first place to look"
        );
    }

    /// The development case: `cargo run` produces a bare binary in `target/`,
    /// and `scripts/macos-input-method-bundle.sh` writes to `dist/`.
    #[test]
    fn a_bare_binary_falls_back_to_the_build_directory() {
        let candidates =
            source_candidates(Path::new("/repo/target/debug/dodo"), Path::new("/repo"));
        assert_eq!(
            candidates,
            vec![PathBuf::from(format!("/repo/dist/{BUNDLE_NAME}"))],
            "no Contents directory means no Helpers candidate"
        );
    }

    #[test]
    fn a_binary_somewhere_unrelated_offers_nothing_meaningless() {
        let candidates =
            source_candidates(Path::new("/usr/local/bin/dodo"), Path::new("/tmp/wherever"));
        assert!(
            !candidates
                .iter()
                .any(|path| path.starts_with("/usr/local/Helpers")),
            "{candidates:?}"
        );
    }

    /// A refused `TISSelectInputSource` still leaves an installed input method,
    /// and the tool's status line depends on this distinction.
    #[test]
    fn a_refusal_to_switch_is_still_an_install() {
        assert!(
            InstallOutcome::Installed {
                refused: InstallStep::Select,
                status: -50,
            }
            .is_installed()
        );
        assert!(InstallOutcome::Ready.is_installed());
        assert!(!InstallOutcome::Failed(super::InstallFailure::NoSourceBundle).is_installed());
    }
}
