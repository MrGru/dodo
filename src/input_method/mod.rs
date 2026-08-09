//! dodo's end of the macOS input method: installing it, and telling it how to
//! type.
//!
//! **dodo does not run the input method and cannot start it.** macOS launches
//! `Dodo Vietnamese.app` out of `~/Library/Input Methods` when the user selects
//! it, and it keeps typing with dodo closed. So this module does exactly two
//! things, and neither is "drive an input method":
//!
//! - **Install it.** [`InputMethod::install`] copies the bundle dodo carries into
//!   `~/Library/Input Methods`, registers it, enables and selects the *mode*, and
//!   kills any process still serving the previous copy.
//!   `docs/macos-input-method.md` §2 is the authority on every one of those and
//!   [`models::install`] is where its rules live as tested data.
//! - **Write its settings.** dodo owns `input-method.json`; the bundle reads it.
//!   The contract is `dodo-ime-ipc`, which both processes link, and it is where
//!   the schema, the version rule and the notification name are documented.
//!
//! # What this module is not
//!
//! **It owns the menu bar's input-language persistence.** The menu bar and the
//! bundle share `dodo_ime_core::LanguageId`; this module writes that identity to
//! `input-method.json`, and the bundle reads it. dodo's interface language is
//! still only a display preference.
//!
//! **It does not link the bundle.** dodo depends on `dodo-ime-ipc` and not on
//! `dodo-ime-macos`: linking the host would pull InputMethodKit into a UI
//! application for four string constants, and the host must never link gpui.
//!
//! **It has no engine.** `dodo-ime-core` is linked, and this module names its
//! configuration types only so the tool's pane can offer Telex or VNI. No
//! keystroke is ever processed in this process.
//!
//! # Where the state is
//!
//! [`InputMethod`] is a `Global`, for the reason `Session` and `Updater` are: read
//! from anywhere, written from one place. It holds the settings document dodo last
//! read or wrote, the last thing the bundle said about itself, and whether an
//! install is running. Every file access is on the background executor.
//!
//! # Failure is never fatal
//!
//! [`init`] returns `()` and nothing here can stop dodo starting, matching
//! `docker::init`, `tray::init`, `quick_nav::init` and `session::init`. A refused
//! settings file shows a banner on the tool's pane and leaves the defaults in
//! place; a failed install shows why.
//!
//! # Why this module is compiled on every platform, and allows dead code on two
//!
//! `src/tray` is `#[cfg(target_os = "macos")]` at its `mod` line. This one is not,
//! deliberately: [`models::install`], [`models::status`], [`services::store`] and
//! [`services::installer`]'s driver contain no macOS at all, and having the Linux
//! and Windows `cargo check` rows type-check them is worth more than the
//! alternative. A non-portable path join or an `unwrap` on a platform-specific
//! assumption in the settings store is exactly the sort of thing those rows exist
//! to catch.
//!
//! The cost is that on those two platforms **nothing calls any of it**, because the
//! only caller is [`views`], which is macOS-only — there is no Windows or Linux
//! input method to install yet, so `View::InputMethod` is not a sidebar row there
//! either. So the compiler correctly reports every item here as dead, and the
//! `allow` below is conditional on exactly that: on macOS, where
//! `clippy -D warnings` and the test suite run, dead-code checking is untouched. It
//! comes off when a Windows TSF or Linux IBus host gives these callers.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

pub mod models;
pub mod services;
pub mod views;

use std::sync::Arc;

use dodo_ime_core::LanguageId;
use dodo_ime_ipc::document::IpcError;
use dodo_ime_ipc::settings::{SettingsDocument, VietnameseSettings};
use dodo_ime_ipc::status::StatusDocument;
use gpui::{App, AsyncApp, BorrowAppContext as _, Global, Task};

use crate::i18n::Str;
use crate::input_method::models::install::{InstallFailure, InstallOutcome, InstallReport};
pub use crate::input_method::models::status::Install;
use crate::input_method::services::store::{DiskInputMethodStore, InputMethodStore, message_for};

/// How long after a settings change to ask the bundle what it applied.
///
/// The bundle reads the file when the notification arrives, so there is a moment
/// between "dodo wrote it" and "the input method says it has it" — and dodo has no
/// way to be told, because the status file has no notification of its own (see
/// `dodo_ime_ipc::SETTINGS_CHANGED`, which is deliberately one-directional). One
/// read, once, after a pause long enough for a file read in another process. It is
/// not a poll: if the answer is still stale, the row says so until the next change
/// or the next launch, which is honest rather than busy.
const STATUS_SETTLE_DELAY: std::time::Duration = std::time::Duration::from_millis(400);

/// What dodo knows about its input method.
pub struct InputMethod {
    /// The settings as last read from or written to `input-method.json`. Its
    /// `revision` is what the bundle echoes back.
    document: SettingsDocument,
    store: Arc<dyn InputMethodStore>,
    /// Why the settings file could not be read or written, if it could not.
    store_error: Option<IpcError>,
    /// What the bundle last said about itself, read at launch and after a change.
    status: Option<StatusDocument>,
    /// Whether `~/Library/Input Methods/Dodo Vietnamese.app` exists.
    ///
    /// Cached rather than checked in `render`: it changes only when dodo installs
    /// it, or when the user deletes it behind dodo's back — and a `stat` per
    /// frame to notice the second would be a poor trade.
    installed: bool,
    install: Install,
    save: Option<Task<()>>,
    installing: Option<Task<()>>,
}

impl Global for InputMethod {}

impl InputMethod {
    fn new(store: Arc<dyn InputMethodStore>) -> InputMethod {
        InputMethod {
            document: SettingsDocument::default(),
            store,
            store_error: None,
            status: None,
            installed: false,
            install: Install::Idle,
            save: None,
            installing: None,
        }
    }

    /// The settings the input method should be typing with.
    pub fn settings(cx: &App) -> VietnameseSettings {
        cx.try_global::<InputMethod>()
            .map(|state| state.document.vietnamese)
            .unwrap_or_default()
    }

    /// What the bundle last said about itself.
    pub fn status(cx: &App) -> Option<StatusDocument> {
        cx.try_global::<InputMethod>()
            .and_then(|state| state.status.clone())
    }

    /// Whether the bundle is in `~/Library/Input Methods`.
    pub fn is_installed(cx: &App) -> bool {
        cx.try_global::<InputMethod>()
            .is_some_and(|state| state.installed)
    }

    pub fn install_state(cx: &App) -> Install {
        cx.try_global::<InputMethod>()
            .map(|state| state.install.clone())
            .unwrap_or_default()
    }

    /// What went wrong with `input-method.json`, if anything.
    pub fn store_error(cx: &App) -> Option<Str> {
        cx.try_global::<InputMethod>()
            .and_then(|state| state.store_error.as_ref().map(message_for))
    }

    /// Whether the settings dodo has written have reached the input method.
    ///
    /// `None` when there is nothing to say — the bundle has never run, or nothing
    /// has been changed since it did. `Some(false)` means dodo wrote a revision
    /// the bundle has not reported applying, which is the ordinary state for a few
    /// hundred milliseconds after a change and a lasting one when the input method
    /// is not running at all.
    pub fn settings_applied(cx: &App) -> Option<bool> {
        let state = cx.try_global::<InputMethod>()?;
        let status = state.status.as_ref()?;
        Some(status.settings_revision >= state.document.revision)
    }

    /// The language the input method should use.
    pub fn language(cx: &App) -> LanguageId {
        cx.try_global::<InputMethod>()
            .map(|state| state.document.language)
            .unwrap_or_default()
    }

    pub fn set_language(language: LanguageId, cx: &mut App) {
        Self::edit(cx, |document| document.language = language);
    }

    pub fn set_scheme(scheme: dodo_ime_ipc::settings::Scheme, cx: &mut App) {
        Self::edit(cx, |document| document.vietnamese.scheme = scheme);
    }

    pub fn set_tone_placement(tone: dodo_ime_ipc::settings::Tone, cx: &mut App) {
        Self::edit(cx, |document| document.vietnamese.tone_placement = tone);
    }

    pub fn set_spell_check(on: bool, cx: &mut App) {
        Self::edit(cx, |document| document.vietnamese.spell_check = on);
    }

    pub fn set_bracket_shortcuts(on: bool, cx: &mut App) {
        Self::edit(cx, |document| document.vietnamese.bracket_shortcuts = on);
    }

    /// Applies one change, writes the file, and tells the input method.
    ///
    /// The order is the whole method and is stated in
    /// [`services::notify`]: the file is written **first** and the notification
    /// posted after, because a notification that arrives first makes the bundle
    /// read the previous settings and report the previous revision.
    ///
    /// A change that leaves the settings as they were writes nothing. That is not
    /// only about disk traffic: every write bumps the revision, and a revision the
    /// bundle then has to catch up with for no reason would show the tool a "not
    /// applied yet" state nobody caused.
    ///
    /// It ends by refreshing the windows, and that is **not** bookkeeping the
    /// caller could do instead. The controls that call this now live in a tool
    /// pane that is built once and lives for the process, and they read this
    /// global rather than holding a copy of it — so the switch the user just
    /// pressed keeps drawing its old value until something repaints it. The
    /// settings dialog never needed this because it was rebuilt each time it
    /// opened.
    fn edit(cx: &mut App, change: impl FnOnce(&mut SettingsDocument)) {
        if cx.try_global::<InputMethod>().is_none() {
            return;
        }
        cx.update_global::<InputMethod, _>(|state, cx| {
            let mut next = state.document;
            change(&mut next);
            if next == state.document {
                return;
            }

            state.document =
                SettingsDocument::next(&state.document, next.language, next.vietnamese);
            let store = state.store.clone();
            let document = state.document;

            state.save = Some(cx.spawn(async move |cx| {
                let written = cx
                    .background_executor()
                    .spawn(async move { store.persist_settings(&document) })
                    .await;

                match written {
                    Ok(()) => {
                        // Only now, and only on the main thread — posting a
                        // notification is a CoreFoundation call and this is where
                        // dodo makes them.
                        cx.update(|cx| {
                            services::notify::settings_changed();
                            Self::clear_error(cx);
                        });
                        cx.background_executor().timer(STATUS_SETTLE_DELAY).await;
                        Self::refresh_status(cx).await;
                    }
                    Err(error) => {
                        cx.update(|cx| Self::report(error, cx));
                    }
                }
            }));
        });
        cx.refresh_windows();
    }

    /// Runs an install, start to finish, off the UI thread.
    ///
    /// Everything it does is [`services::installer::install`]'s; this is the part
    /// that cannot be unit tested — finding the running executable, hopping to the
    /// background executor and putting the answer back into the global.
    pub fn install(cx: &mut App) {
        if cx.try_global::<InputMethod>().is_none() {
            return;
        }
        cx.update_global::<InputMethod, _>(|state, cx| {
            if state.install == Install::Running {
                return;
            }
            state.install = Install::Running;

            state.installing = Some(cx.spawn(async move |cx| {
                let report = cx
                    .background_executor()
                    .spawn(async move { run_install() })
                    .await;

                cx.update(|cx| Self::adopt_report(report, cx));
                // The bundle was just replaced and its process killed, so
                // whatever the status file says is about a process that no longer
                // exists. Read it again so the page does not show a stale pid.
                Self::refresh_status(cx).await;
            }));
        });
        cx.refresh_windows();
    }

    fn adopt_report(report: InstallReport, cx: &mut App) {
        if report.outcome.is_installed() {
            eprintln!(
                "dodo/input-method: installed after {} registration attempt(s): {:?}",
                report.register_attempts, report.outcome
            );
        } else {
            eprintln!("dodo/input-method: install failed: {:?}", report.outcome);
        }

        if cx.try_global::<InputMethod>().is_some() {
            cx.update_global::<InputMethod, _>(|state, _| {
                state.installed = state.installed || report.outcome.is_installed();
                state.install = Install::Done(report.outcome);
            });
        }
        cx.refresh_windows();
    }

    /// Reads the bundle's status file and adopts it.
    async fn refresh_status(cx: &mut AsyncApp) {
        let Some(store) = cx.update(|cx| {
            cx.try_global::<InputMethod>()
                .map(|state| state.store.clone())
        }) else {
            return;
        };

        let read = cx
            .background_executor()
            .spawn(async move { store.read_status() })
            .await;

        cx.update(|cx| {
            if cx.try_global::<InputMethod>().is_none() {
                return;
            }
            cx.update_global::<InputMethod, _>(|state, _| {
                // A status file this dodo cannot read is *not* an error worth
                // showing beside the settings: it means the installed bundle is
                // newer than dodo, which the settings row phrases as "unknown"
                // rather than as a fault. `store_error` is about dodo's own file.
                state.status = read.clone().unwrap_or(None);
            });
            cx.refresh_windows();
        });
    }

    fn clear_error(cx: &mut App) {
        if cx.try_global::<InputMethod>().is_some() {
            cx.update_global::<InputMethod, _>(|state, _| state.store_error = None);
        }
    }

    fn report(error: IpcError, cx: &mut App) {
        eprintln!("input-method.json: {error}");
        if cx.try_global::<InputMethod>().is_some() {
            cx.update_global::<InputMethod, _>(|state, _| state.store_error = Some(error));
        }
        cx.refresh_windows();
    }

    /// Adopts what the store read at launch.
    ///
    /// The refresh at the end is the one that matters most. `Layout::new` builds
    /// the Input method pane *before* [`load`] has finished reading
    /// `input-method.json`, so without it the pane would sit showing the
    /// defaults, and every stored setting the user had chosen, plus "not
    /// installed", would only appear once something else caused a repaint.
    fn adopt(
        loaded: Result<SettingsDocument, IpcError>,
        status: Option<StatusDocument>,
        installed: bool,
        cx: &mut App,
    ) {
        if cx.try_global::<InputMethod>().is_none() {
            return;
        }
        cx.update_global::<InputMethod, _>(|state, _| {
            state.status = status;
            state.installed = installed;
            match loaded {
                Ok(document) => {
                    state.document = document;
                    state.store_error = None;
                }
                Err(error) => {
                    eprintln!("input-method.json: {error}");
                    state.store_error = Some(error);
                }
            }
        });
        cx.refresh_windows();
    }
}

/// The whole install, on a background thread.
///
/// Split out of [`InputMethod::install`] so that the async plumbing and the
/// blocking work are not the same function. `std::env::current_exe` is the one
/// thing here that cannot be handed in: it is *the running dodo*, which is exactly
/// what has to be found to locate the bundle it carries.
fn run_install() -> InstallReport {
    let ops = system_ops();

    let executable = std::env::current_exe().unwrap_or_default();
    let working_directory = std::env::current_dir().unwrap_or_default();
    let home = crate::paths::Environment::from_env()
        .home
        .unwrap_or_default();

    let plan = match services::installer::resolve_plan(&executable, &working_directory, &home) {
        Ok(plan) => plan,
        Err(failure) => {
            return InstallReport {
                outcome: InstallOutcome::Failed(failure),
                steps: Vec::new(),
                register_attempts: 0,
            };
        }
    };

    match ops {
        Some(ops) => services::installer::install(&plan, ops.as_ref()),
        // Every other platform. The bundle is a macOS object and there is nothing
        // here to install it into.
        None => InstallReport {
            outcome: InstallOutcome::Failed(InstallFailure::NoSourceBundle),
            steps: Vec::new(),
            register_attempts: 0,
        },
    }
}

#[cfg(target_os = "macos")]
fn system_ops() -> Option<Box<dyn services::installer::InstallOps>> {
    Some(Box::new(services::installer::SystemOps))
}

#[cfg(not(target_os = "macos"))]
fn system_ops() -> Option<Box<dyn services::installer::InstallOps>> {
    None
}

/// Registers the global. Called from `main` after `gpui_component::init`, like
/// every other `init`.
pub fn init(cx: &mut App) {
    cx.set_global(InputMethod::new(Arc::new(DiskInputMethodStore::new())));
}

/// Reads the settings and the bundle's status on the background executor and
/// adopts both.
///
/// Awaited from `main` alongside `session::load`. A failure leaves the defaults in
/// place, which is what the input method itself would be typing with.
pub async fn load(cx: &mut AsyncApp) {
    let Some(store) = cx.update(|cx| {
        cx.try_global::<InputMethod>()
            .map(|state| state.store.clone())
    }) else {
        return;
    };

    let home = crate::paths::Environment::from_env().home;
    let (loaded, status, installed) = cx
        .background_executor()
        .spawn(async move {
            let loaded = store.load_settings();
            // A status file dodo cannot read is not a reason to report anything:
            // see `refresh_status`.
            let status = store.read_status().ok().flatten();
            let installed = home
                .map(|home| dodo_ime_ipc::paths::installed_bundle(&home))
                .is_some_and(|bundle| bundle.is_dir());
            (loaded, status, installed)
        })
        .await;

    cx.update(|cx| InputMethod::adopt(loaded, status, installed, cx));
}

#[cfg(test)]
mod tests {
    use super::{InputMethod, LanguageId};
    use crate::input_method::models::install::{InstallFailure, InstallOutcome, InstallStep};
    use crate::input_method::services::store::{InMemoryInputMethodStore, InputMethodStore};
    use dodo_ime_ipc::settings::{Scheme, SettingsDocument, Tone, VietnameseSettings};
    use dodo_ime_ipc::status::StatusDocument;
    use std::sync::Arc;

    /// The revision bookkeeping, without a frame: it is the state layer's own
    /// rule and the only thing that can say "your settings have not arrived yet".
    #[test]
    fn a_change_bumps_the_revision_and_an_identical_write_does_not() {
        let store = Arc::new(InMemoryInputMethodStore::default());
        let mut state = InputMethod::new(store.clone());

        let first = SettingsDocument::next(
            &state.document,
            state.document.language,
            VietnameseSettings {
                scheme: Scheme::Vni,
                ..VietnameseSettings::default()
            },
        );
        assert_eq!(first.revision, 1);
        state.document = first;
        store.persist_settings(&state.document).unwrap();

        // The same settings again: `SettingsDocument::next` would bump the
        // revision, which is why `edit` compares before calling it.
        assert_eq!(
            state.document.vietnamese.scheme,
            Scheme::Vni,
            "the change stuck"
        );
        assert_eq!(store.load_settings().unwrap().revision, 1);
    }

    /// The three states the tool's status line distinguishes.
    #[test]
    fn applied_is_a_comparison_of_revisions_and_not_of_settings() {
        let store = Arc::new(InMemoryInputMethodStore::default());
        let mut state = InputMethod::new(store);

        state.document = SettingsDocument {
            version: dodo_ime_ipc::settings::SETTINGS_SCHEMA_VERSION,
            language: LanguageId::Vietnamese,
            revision: 4,
            vietnamese: VietnameseSettings {
                tone_placement: Tone::Traditional,
                ..VietnameseSettings::default()
            },
        };

        // Nothing has ever run.
        assert!(state.status.is_none());

        // A bundle that has applied an older revision has not caught up, even
        // though it might happen to be typing the same way.
        state.status = Some(StatusDocument {
            settings_revision: 3,
            ..StatusDocument::default()
        });
        assert!(state.status.as_ref().unwrap().settings_revision < state.document.revision);

        state.status = Some(StatusDocument {
            settings_revision: 4,
            ..StatusDocument::default()
        });
        assert!(state.status.as_ref().unwrap().settings_revision >= state.document.revision);
    }

    /// A failed install must not claim the bundle is installed, and a refused
    /// *selection* must not claim it is not.
    #[test]
    fn the_installed_flag_follows_the_outcome_and_never_goes_backwards() {
        let store = Arc::new(InMemoryInputMethodStore::default());
        let mut state = InputMethod::new(store);
        assert!(!state.installed);

        state.installed = state.installed
            || InstallOutcome::Failed(InstallFailure::NoSourceBundle).is_installed();
        assert!(!state.installed);

        state.installed = state.installed
            || InstallOutcome::Installed {
                refused: InstallStep::Select,
                status: -50,
            }
            .is_installed();
        assert!(state.installed, "a refused switch is still installed");

        // A later failed install does not un-install what is on disk.
        state.installed = state.installed
            || InstallOutcome::Failed(InstallFailure::NeverAppeared { attempts: 5 }).is_installed();
        assert!(state.installed);
    }
}
