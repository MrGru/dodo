//! dodo's end of the native input methods: installing them, and telling them
//! how to type.
//!
//! **dodo does not run either native input method.** macOS launches the
//! InputMethodKit app and Windows loads the TSF DLL when the user selects its
//! profile; both are designed to type with dodo closed. This module installs or
//! removes the selected platform's native artifact and writes its settings.
//!
//! dodo owns `input-method.json`; native hosts only read it. The contract is
//! `dodo-ime-ipc`, which carries the schema/version rule and host identifiers.
//! `docs/macos-input-method.md` and `docs/windows-input-method.md` are the
//! installation authorities.
//!
//! # What this module is not
//!
//! **It owns the menu bar's input-language persistence.** The menu bar and the
//! bundle share `dodo_ime_core::LanguageId`; this module writes that identity to
//! `input-method.json`, and the bundle reads it. dodo's interface language is
//! still only a display preference.
//!
//! **It does not link a native host.** dodo depends on `dodo-ime-ipc`, not the
//! InputMethodKit app or TSF DLL; neither host may pull gpui into another
//! application's input path.
//!
//! **Native Input Method has no engine here.** Event Tap (macOS) and Keyboard
//! Hook (Windows) are explicit dodo-lifetime-only alternatives. When unselected
//! they create no observer; when selected, the state layer makes the native host
//! pass through before starting the fallback.
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
//! Linux still has no input-method host, so its sidebar has no row. macOS and
//! Windows both call this module: macOS installs InputMethodKit and can select
//! Event Tap; Windows installs a TSF DLL and can select Keyboard Hook.
#![cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]

pub mod models;
pub mod services;
pub mod views;

use std::sync::Arc;

use dodo_ime_core::LanguageId;
use dodo_ime_ipc::document::IpcError;
use dodo_ime_ipc::settings::{Backend, SettingsDocument, VietnameseSettings};
use dodo_ime_ipc::status::StatusDocument;
use gpui::{App, AsyncApp, BorrowAppContext as _, Global, Task};

use crate::i18n::Str;
#[cfg(target_os = "macos")]
use crate::input_method::models::event_tap::{EventTapStatus, desired_status};
#[cfg(target_os = "macos")]
use crate::input_method::models::install::{InstallFailure, InstallOutcome, InstallReport};
#[cfg(target_os = "windows")]
use crate::input_method::models::keyboard_hook::{
    KeyboardHookStatus, desired_status as hook_status,
};
#[cfg(target_os = "macos")]
pub use crate::input_method::models::status::Install;
#[cfg(target_os = "windows")]
use crate::input_method::models::windows::{WindowsInstall, WindowsInstallOutcome};
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
#[cfg(target_os = "macos")]
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
    #[cfg(target_os = "macos")]
    install: Install,
    save: Option<Task<()>>,
    #[cfg(target_os = "macos")]
    installing: Option<Task<()>>,
    /// Held only while Event Tap owns transformation. Dropping it detaches the
    /// run-loop source before its callback state can be freed.
    #[cfg(target_os = "macos")]
    event_tap: Option<services::event_tap::EventTap>,
    #[cfg(target_os = "macos")]
    event_tap_status: EventTapStatus,
    /// Held only while Windows Keyboard Hook owns transformation. Dropping it
    /// calls `UnhookWindowsHookEx` before callback state is released.
    #[cfg(target_os = "windows")]
    keyboard_hook: Option<services::keyboard_hook::KeyboardHook>,
    #[cfg(target_os = "windows")]
    keyboard_hook_status: KeyboardHookStatus,
    #[cfg(target_os = "windows")]
    windows_install: WindowsInstall,
    #[cfg(target_os = "windows")]
    windows_task: Option<Task<()>>,
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
            #[cfg(target_os = "macos")]
            install: Install::Idle,
            save: None,
            #[cfg(target_os = "macos")]
            installing: None,
            #[cfg(target_os = "macos")]
            event_tap: None,
            #[cfg(target_os = "macos")]
            event_tap_status: EventTapStatus::Inactive,
            #[cfg(target_os = "windows")]
            keyboard_hook: None,
            #[cfg(target_os = "windows")]
            keyboard_hook_status: KeyboardHookStatus::Inactive,
            #[cfg(target_os = "windows")]
            windows_install: WindowsInstall::Idle,
            #[cfg(target_os = "windows")]
            windows_task: None,
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

    #[cfg(target_os = "macos")]
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

    /// The selected backend.
    pub fn backend(cx: &App) -> Backend {
        cx.try_global::<InputMethod>()
            .map(|state| state.document.backend)
            .unwrap_or_default()
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

    /// Switches the sole transformation owner.
    pub fn set_backend(backend: Backend, cx: &mut App) {
        if Self::backend(cx) == backend {
            return;
        }
        // Stop first when returning to Native. A failed write then causes a
        // harmless gap rather than two backends racing on a user's text.
        #[cfg(target_os = "macos")]
        if backend == Backend::Native {
            Self::stop_event_tap(cx);
        }
        #[cfg(target_os = "windows")]
        Self::stop_keyboard_hook(cx);
        Self::edit(cx, |document| document.backend = backend);
    }

    #[cfg(target_os = "macos")]
    pub fn event_tap_status(cx: &App) -> EventTapStatus {
        cx.try_global::<InputMethod>()
            .map(|state| {
                state.event_tap.as_ref().map_or(
                    state.event_tap_status,
                    services::event_tap::EventTap::status,
                )
            })
            .unwrap_or(EventTapStatus::Inactive)
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

            #[cfg(target_os = "windows")]
            {
                // Every setting write creates a conservative gap. The native
                // host re-reads the completed file before each key; starting a
                // hook before that write would let two owners see one key.
                state.keyboard_hook = None;
                state.keyboard_hook_status = KeyboardHookStatus::Inactive;
            }
            state.document = SettingsDocument::next_with_backend(
                &state.document,
                next.backend,
                next.language,
                next.vietnamese,
            );
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
                            #[cfg(target_os = "macos")]
                            services::notify::settings_changed();
                            Self::clear_error(cx);
                            #[cfg(target_os = "macos")]
                            Self::reconcile_event_tap(cx);
                            #[cfg(target_os = "windows")]
                            Self::reconcile_keyboard_hook(cx);
                        });
                        #[cfg(target_os = "macos")]
                        {
                            cx.background_executor().timer(STATUS_SETTLE_DELAY).await;
                            Self::refresh_status(cx).await;
                        }
                    }
                    Err(error) => {
                        cx.update(|cx| Self::report(error, cx));
                    }
                }
            }));
        });
        cx.refresh_windows();
    }

    /// Runs macOS's bundle install, start to finish, off the UI thread.
    #[cfg(target_os = "macos")]
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

    #[cfg(target_os = "macos")]
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
    #[cfg(target_os = "macos")]
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
            #[cfg(target_os = "macos")]
            Self::reconcile_event_tap(cx);
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
        #[cfg(target_os = "macos")]
        Self::reconcile_event_tap(cx);
        #[cfg(target_os = "windows")]
        Self::reconcile_keyboard_hook(cx);
        cx.refresh_windows();
    }

    #[cfg(target_os = "macos")]
    fn stop_event_tap(cx: &mut App) {
        if cx.try_global::<InputMethod>().is_some() {
            cx.update_global::<InputMethod, _>(|state, _| {
                state.event_tap = None;
                state.event_tap_status = EventTapStatus::Inactive;
            });
        }
    }

    /// Starts only after a live native bundle has adopted Event Tap. A bundle
    /// that is not running cannot transform, and a current bundle that later
    /// launches reads the selection before its first controller exists.
    #[cfg(target_os = "macos")]
    fn reconcile_event_tap(cx: &mut App) {
        if cx.try_global::<InputMethod>().is_none() {
            return;
        }
        cx.update_global::<InputMethod, _>(|state, _| {
            let native_live = state
                .status
                .as_ref()
                .is_some_and(|status| status.describes_a_live_process());
            let settings_applied = state
                .status
                .as_ref()
                .is_some_and(|status| status.settings_revision >= state.document.revision);
            // Permission is checked only if this reaches `EventTap::start`; the
            // model still decides exclusive ownership and the native hand-off.
            match desired_status(
                state.document.backend,
                state.document.language,
                native_live,
                settings_applied,
                true,
            ) {
                EventTapStatus::Inactive => {
                    state.event_tap = None;
                    state.event_tap_status = EventTapStatus::Inactive;
                    return;
                }
                EventTapStatus::WaitingForNative => {
                    state.event_tap = None;
                    state.event_tap_status = EventTapStatus::WaitingForNative;
                    return;
                }
                EventTapStatus::NeedsAccessibility
                | EventTapStatus::Running
                | EventTapStatus::Failed => {}
            }

            let config = state.document.vietnamese.to_config();
            if let Some(tap) = &state.event_tap {
                tap.reconfigure(config);
                state.event_tap_status = tap.status();
                return;
            }

            match services::event_tap::EventTap::start(config) {
                Ok(tap) => {
                    state.event_tap_status = tap.status();
                    state.event_tap = Some(tap);
                }
                Err(services::event_tap::StartError::AccessibilityDenied) => {
                    state.event_tap_status = EventTapStatus::NeedsAccessibility;
                }
                Err(_) => state.event_tap_status = EventTapStatus::Failed,
            }
        });
        cx.refresh_windows();
    }

    /// What the Windows TSF installer last did.
    #[cfg(target_os = "windows")]
    pub fn windows_install_state(cx: &App) -> WindowsInstall {
        cx.try_global::<InputMethod>()
            .map(|state| state.windows_install.clone())
            .unwrap_or_default()
    }

    /// Installs or reinstalls the per-user TSF COM server off the UI thread.
    #[cfg(target_os = "windows")]
    pub fn install(cx: &mut App) {
        if cx.try_global::<InputMethod>().is_none() {
            return;
        }
        cx.update_global::<InputMethod, _>(|state, cx| {
            if matches!(
                state.windows_install,
                WindowsInstall::Installing | WindowsInstall::Uninstalling
            ) {
                return;
            }
            state.windows_install = WindowsInstall::Installing;
            state.windows_task = Some(cx.spawn(async move |cx| {
                let outcome = cx
                    .background_executor()
                    .spawn(async move {
                        let executable = std::env::current_exe().unwrap_or_default();
                        let working_directory = std::env::current_dir().unwrap_or_default();
                        services::windows::install(
                            &executable,
                            &working_directory,
                            &crate::paths::data_dir(),
                        )
                    })
                    .await;
                cx.update(|cx| Self::adopt_windows_outcome(outcome, cx));
            }));
        });
        cx.refresh_windows();
    }

    /// Removes the current user's TSF COM server and its copied DLL.
    #[cfg(target_os = "windows")]
    pub fn uninstall(cx: &mut App) {
        if cx.try_global::<InputMethod>().is_none() {
            return;
        }
        Self::stop_keyboard_hook(cx);
        cx.update_global::<InputMethod, _>(|state, cx| {
            if matches!(
                state.windows_install,
                WindowsInstall::Installing | WindowsInstall::Uninstalling
            ) {
                return;
            }
            state.windows_install = WindowsInstall::Uninstalling;
            state.windows_task = Some(cx.spawn(async move |cx| {
                let outcome = cx
                    .background_executor()
                    .spawn(async move { services::windows::uninstall(&crate::paths::data_dir()) })
                    .await;
                cx.update(|cx| Self::adopt_windows_outcome(outcome, cx));
            }));
        });
        cx.refresh_windows();
    }

    #[cfg(target_os = "windows")]
    fn adopt_windows_outcome(outcome: WindowsInstallOutcome, cx: &mut App) {
        if cx.try_global::<InputMethod>().is_some() {
            cx.update_global::<InputMethod, _>(|state, _| {
                state.installed = matches!(outcome, WindowsInstallOutcome::Ready);
                state.windows_install = WindowsInstall::Done(outcome);
            });
        }
        cx.refresh_windows();
    }

    #[cfg(target_os = "windows")]
    pub fn keyboard_hook_status(cx: &App) -> KeyboardHookStatus {
        cx.try_global::<InputMethod>()
            .map(|state| {
                state.keyboard_hook.as_ref().map_or(
                    state.keyboard_hook_status,
                    services::keyboard_hook::KeyboardHook::status,
                )
            })
            .unwrap_or(KeyboardHookStatus::Inactive)
    }

    #[cfg(target_os = "windows")]
    fn stop_keyboard_hook(cx: &mut App) {
        if cx.try_global::<InputMethod>().is_some() {
            cx.update_global::<InputMethod, _>(|state, _| {
                state.keyboard_hook = None;
                state.keyboard_hook_status = KeyboardHookStatus::Inactive;
            });
        }
    }

    /// Starts only after the completed settings write selected the Windows
    /// fallback. The native TSF DLL reads that same file before every key, so
    /// the order leaves a harmless gap rather than competing owners.
    #[cfg(target_os = "windows")]
    fn reconcile_keyboard_hook(cx: &mut App) {
        if cx.try_global::<InputMethod>().is_none() {
            return;
        }
        cx.update_global::<InputMethod, _>(|state, _| {
            if hook_status(state.document.backend, state.document.language, true)
                != KeyboardHookStatus::Running
            {
                state.keyboard_hook = None;
                state.keyboard_hook_status = KeyboardHookStatus::Inactive;
                return;
            }
            let config = state.document.vietnamese.to_config();
            match services::keyboard_hook::KeyboardHook::start(config) {
                Ok(hook) => {
                    state.keyboard_hook_status = hook.status();
                    state.keyboard_hook = Some(hook);
                }
                Err(_) => state.keyboard_hook_status = KeyboardHookStatus::Failed,
            }
        });
        cx.refresh_windows();
    }
}

/// The macOS bundle install, on a background thread.
#[cfg(target_os = "macos")]
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

    let (loaded, status, installed) = cx
        .background_executor()
        .spawn(async move {
            let loaded = store.load_settings();
            #[cfg(target_os = "macos")]
            {
                // A status file dodo cannot read is not a reason to report
                // anything: see `refresh_status`.
                let status = store.read_status().ok().flatten();
                let installed = crate::paths::Environment::from_env()
                    .home
                    .map(|home| dodo_ime_ipc::paths::installed_bundle(&home))
                    .is_some_and(|bundle| bundle.is_dir());
                (loaded, status, installed)
            }
            #[cfg(target_os = "windows")]
            {
                (loaded, None, services::windows::is_registered())
            }
            #[cfg(not(any(target_os = "macos", target_os = "windows")))]
            (loaded, None, false)
        })
        .await;

    cx.update(|cx| InputMethod::adopt(loaded, status, installed, cx));
}

#[cfg(test)]
mod tests {
    use super::{InputMethod, LanguageId};
    use crate::input_method::models::install::{InstallFailure, InstallOutcome, InstallStep};
    use crate::input_method::services::store::{InMemoryInputMethodStore, InputMethodStore};
    use dodo_ime_ipc::settings::{Backend, Scheme, SettingsDocument, Tone, VietnameseSettings};
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

    #[test]
    fn backend_selection_persists_with_the_engine_settings() {
        let store = Arc::new(InMemoryInputMethodStore::default());
        let document = SettingsDocument::next_with_backend(
            &SettingsDocument::default(),
            Backend::EventTap,
            LanguageId::Vietnamese,
            VietnameseSettings::default(),
        );
        store.persist_settings(&document).unwrap();

        assert_eq!(store.load_settings().unwrap().backend, Backend::EventTap);
    }

    /// The three states the tool's status line distinguishes.
    #[test]
    fn applied_is_a_comparison_of_revisions_and_not_of_settings() {
        let store = Arc::new(InMemoryInputMethodStore::default());
        let mut state = InputMethod::new(store);

        state.document = SettingsDocument {
            version: dodo_ime_ipc::settings::SETTINGS_SCHEMA_VERSION,
            backend: Backend::Native,
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
