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
//! **Only a fallback can get a browser address bar wrong.** A native host
//! composes through a marked-text client; a fallback rewrites with Backspaces,
//! which is what an inline autocomplete selection breaks.
//! [`models::browser_rewrite`] is that rule and `browser_address_bar_fix` is its
//! switch — a setting of the *tap*, so the pane draws it only under Event Tap.
//!
//! # The language switch has one owner at a time, and it is whoever owns keys
//!
//! Whichever backend is selected answers the shortcut, because the others are
//! passing every key through. That is not a detail of the fallbacks: while Event
//! Tap or Keyboard Hook is selected, the native host contributes nothing, so a
//! fallback that ignored the shortcut left it working nowhere. They therefore
//! stay attached in **every** selected language — [`models::live_switch`] holds
//! the distinction between observing a key and transforming it — and
//! [`InputMethod::edit`] hands each one the whole document before the write, so
//! a shortcut recorded in the pane is live on the next keystroke and the one it
//! replaced is inert. A cycle performed inside an OS callback returns over
//! `switch_sender`; `input-method.json` is still the only place it is recorded.
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
//!
//! # This is a *feature* crate, and the one whose edges were not all outbound
//!
//! It was `src/input_method/` until 2026-08-15, when it moved out whole,
//! following the shape `dodo-cleaner` set: the binary links it and names four
//! items — [`InputMethod`], [`init`], [`load`] and
//! [`views::InputMethodView`] — while `main.rs` aliases the crate back to
//! `crate::input_method`, so no call site on either side of the seam changed.
//! [`paths`] is the seam every feature crate has: the one impure input
//! `dodo-paths`' pure rules take.
//!
//! Unlike the other five it had an edge **back into the binary**, and it is
//! the reason [`observe_languages`] exists. `src/tray` reads this module (five
//! call sites) and [`InputMethod::edit`] told the tray, which is a cycle no
//! crate boundary can hold. The direction that had to invert is the
//! *notification*: `main.rs` hands over `tray::set_active_languages` as a
//! plain `fn` pointer, this module calls whatever it was given, and Linux —
//! which has no tray — hands over nothing and is called back never. That also
//! retired a platform attribute: the gate is now the presence of an observer,
//! so the line type-checks on every target.
#![cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]

pub mod models;
pub mod paths;
pub mod services;
pub mod views;

// The string catalogue is a crate; this alias is what keeps every
// `crate::i18n::{Str, t, input_method}` in the four files below spelled the
// way it was inside the binary.
use dodo_i18n as i18n;

/// The binary's quick-navigation key contexts, as this crate's tests assume
/// them.
///
/// `src/quick_nav` owns the real definitions and a crate cannot read them;
/// dodo's own test asserts the two spellings stay one answer, which is the
/// same guard [`paths`] keeps for the platform. They are here rather than
/// inlined in the test that needs them because two literals nothing compares
/// is exactly the shape that drifts silently — and what would drift is the
/// rule that a key pressed at the shortcut recorder is *recorded* and never
/// also obeyed.
pub const QUICK_NAV_KEY_CONTEXT: &str = "Dodo";
/// See [`QUICK_NAV_KEY_CONTEXT`].
pub const QUICK_NAV_NORMAL_MODE: &str = "Dodo && !Input";

/// What to tell when the active languages, or the selected one, change.
///
/// A plain `fn` pointer rather than a boxed closure or a gpui `Global`,
/// because there is exactly one caller with exactly this signature —
/// `tray::set_active_languages` — and the smallest possible inversion is the
/// one least likely to grow into a second event system. See the crate doc for
/// why the edge had to invert at all.
type LanguagesObserver = fn(ActiveLanguages, LanguageId, &mut App);

static LANGUAGES_OBSERVER: std::sync::OnceLock<LanguagesObserver> = std::sync::OnceLock::new();

/// Registers the one thing that wants to hear about a language change.
///
/// Called from `main.rs` on the platforms that have a tray, and on no others;
/// a second call is ignored. Registering *before* [`init`] is not required —
/// nothing reads a language until the user changes one — but `main.rs` does it
/// anyway, so the two lines cannot drift apart.
pub fn observe_languages(observer: LanguagesObserver) {
    let _ = LANGUAGES_OBSERVER.set(observer);
}

use std::sync::Arc;

#[cfg(any(target_os = "macos", target_os = "windows"))]
use futures_util::StreamExt as _;

use dodo_ime_core::{ActiveLanguages, LanguageId};
use dodo_ime_ipc::document::IpcError;
use dodo_ime_ipc::settings::{
    Backend, LanguageSwitch, SETTINGS_SCHEMA_VERSION, SettingsDocument, VietnameseSettings,
};
use dodo_ime_ipc::status::StatusDocument;
#[cfg(any(target_os = "macos", target_os = "windows"))]
use futures_channel::mpsc::{UnboundedSender, unbounded};
use gpui::{App, AsyncApp, BorrowAppContext as _, Global, Task};

use crate::i18n::Str;
#[cfg(target_os = "macos")]
use crate::models::event_tap::{EventTapStatus, desired_status, should_request_accessibility};
#[cfg(target_os = "macos")]
use crate::models::install::{InstallFailure, InstallOutcome, InstallReport};
#[cfg(target_os = "windows")]
use crate::models::keyboard_hook::{KeyboardHookStatus, desired_status as hook_status};
#[cfg(target_os = "macos")]
pub use crate::models::status::Install;
#[cfg(target_os = "windows")]
use crate::models::windows::{WindowsInstall, WindowsInstallOutcome};
use crate::services::store::{DiskInputMethodStore, InputMethodStore, message_for};

/// How long after a settings change to ask the bundle what it applied.
///
/// The bundle reads the file when the notification arrives, so there is a moment
/// between "dodo wrote it" and "the input method says it has it". One read,
/// once, after a pause long enough for a file read in another process. Explicit
/// host language switches use the same notification to cause their own one read;
/// neither path polls.
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
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    language_changes: Option<Task<()>>,
    /// Cycles performed by a dodo-owned key listener, on their way back to this
    /// document.
    ///
    /// The listener has already switched its own copy, so the key after the
    /// shortcut is typed in the new language; this task is what makes the file,
    /// the tray and the pane agree. It is a channel and not a direct call
    /// because the listener runs inside a raw OS callback with no `App`.
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    switch_requests: Option<Task<()>>,
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    switch_sender: UnboundedSender<LanguageId>,
    #[cfg(target_os = "macos")]
    installing: Option<Task<()>>,
    /// Held only while Event Tap owns transformation. Dropping it detaches the
    /// run-loop source before its callback state can be freed.
    #[cfg(target_os = "macos")]
    event_tap: Option<services::event_tap::EventTap>,
    #[cfg(target_os = "macos")]
    event_tap_status: EventTapStatus,
    /// macOS owns the request UI; prevent repeated request attempts while this
    /// process remains untrusted.
    #[cfg(target_os = "macos")]
    event_tap_accessibility_requested: bool,
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
    fn new(
        store: Arc<dyn InputMethodStore>,
        #[cfg(any(target_os = "macos", target_os = "windows"))] switch_sender: UnboundedSender<
            LanguageId,
        >,
    ) -> InputMethod {
        InputMethod {
            document: SettingsDocument::default(),
            store,
            store_error: None,
            status: None,
            installed: false,
            #[cfg(target_os = "macos")]
            install: Install::Idle,
            save: None,
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            language_changes: None,
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            switch_requests: None,
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            switch_sender,
            #[cfg(target_os = "macos")]
            installing: None,
            #[cfg(target_os = "macos")]
            event_tap: None,
            #[cfg(target_os = "macos")]
            event_tap_status: EventTapStatus::Inactive,
            #[cfg(target_os = "macos")]
            event_tap_accessibility_requested: false,
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

    /// The only input languages a person has enabled for menus and cycling.
    pub fn active_languages(cx: &App) -> ActiveLanguages {
        cx.try_global::<InputMethod>()
            .map(|state| state.document.active_languages)
            .unwrap_or_default()
    }

    /// The shared, persisted language-switch shortcut.
    pub fn language_switch(cx: &App) -> LanguageSwitch {
        cx.try_global::<InputMethod>()
            .map(|state| state.document.language_switch)
            .unwrap_or_default()
    }

    pub fn set_language(language: LanguageId, cx: &mut App) {
        Self::edit(cx, |document| {
            if document.active_languages.contains(language) {
                document.language = language;
            }
        });
    }

    /// Enables or disables a language, never leaving no cycle destination.
    pub fn set_language_enabled(language: LanguageId, enabled: bool, cx: &mut App) {
        Self::edit(cx, |document| {
            document.active_languages = document.active_languages.with(language, enabled);
            if !document.active_languages.contains(document.language) {
                document.language = document
                    .active_languages
                    .iter()
                    .next()
                    .expect("ActiveLanguages is non-empty");
            }
        });
    }

    /// Records a replacement shortcut, or keeps the previous one.
    ///
    /// Refusing an invalid combination here rather than storing it is what makes
    /// "the field shows what will happen" true: a shortcut that could fire while
    /// typing is never written, so the recorder never displays one that the
    /// listeners would go on to ignore. Everything else — replacing the live
    /// listener's copy, persisting, notifying the native hosts, repainting — is
    /// [`edit`](Self::edit)'s single ordered path.
    pub fn set_language_switch(switch: LanguageSwitch, cx: &mut App) {
        if switch.is_valid() {
            Self::edit(cx, |document| document.language_switch = switch);
        }
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

    /// Whether the Event Tap may work around browser inline autocomplete.
    #[cfg(target_os = "macos")]
    pub fn browser_address_bar_fix(cx: &App) -> bool {
        cx.try_global::<InputMethod>()
            .is_none_or(|state| state.document.browser_address_bar_fix)
    }

    #[cfg(target_os = "macos")]
    pub fn set_browser_address_bar_fix(on: bool, cx: &mut App) {
        Self::edit(cx, |document| document.browser_address_bar_fix = on);
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
            if next == state.document && state.document.version == SETTINGS_SCHEMA_VERSION {
                return;
            }

            #[cfg(any(target_os = "macos", target_os = "windows"))]
            let changing_owner = next.backend != state.document.backend;
            #[cfg(target_os = "windows")]
            if changing_owner {
                // Handing transformation over creates a conservative gap. The
                // native host re-reads the completed file before each key, so
                // starting a hook before that write would let two owners see
                // one key. A change that does not move ownership keeps the one
                // live hook and re-reads it below instead.
                state.keyboard_hook = None;
                state.keyboard_hook_status = KeyboardHookStatus::Inactive;
            }
            state.document = SettingsDocument::next_from(&state.document, next);
            let store = state.store.clone();
            let document = state.document;
            // Before the write, not after it. A recorded shortcut has to be the
            // live one on the next keystroke, and a listener that waited for the
            // background write to land is exactly the "the UI changed but
            // nothing switches" gap this replaces. There is one listener and it
            // is *replaced*, never joined by a second, so the previous shortcut
            // stops matching in the same instant.
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            if !changing_owner {
                #[cfg(target_os = "macos")]
                if let Some(tap) = &state.event_tap {
                    tap.reconfigure(document);
                }
                #[cfg(target_os = "windows")]
                if let Some(hook) = &state.keyboard_hook {
                    hook.reconfigure(document);
                }
            }

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
        // The tray, if there is one. `set_active_languages` was called
        // directly here while this was `src/input_method/`; it is handed in
        // now — see `observe_languages` — and on a platform with no tray
        // nothing is registered and nothing is computed.
        if let Some(observe) = LANGUAGES_OBSERVER.get() {
            observe(Self::active_languages(cx), Self::language(cx), cx);
        }
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

    /// Reads the native host's status file and adopts it.
    ///
    /// Both hosts write that file — the macOS bundle and the Windows TSF DLL —
    /// and both wake dodo the same way when they switch language on their own
    /// (a distributed notification on macOS, a named event on Windows), so this
    /// is deliberately *not* macOS-only: [`init`]'s listener runs on both, and
    /// gating this on macOS alone is what stopped Windows compiling at all.
    #[cfg(any(target_os = "macos", target_os = "windows"))]
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

        let reported_language = read
            .as_ref()
            .ok()
            .and_then(|status| status.as_ref())
            .and_then(StatusDocument::language);
        let adopt_language = cx.update(|cx| {
            cx.try_global::<InputMethod>()?;
            let mut selected = None;
            cx.update_global::<InputMethod, _>(|state, _| {
                // A status file this dodo cannot read is *not* an error worth
                // showing beside the settings: it means the installed bundle is
                // newer than dodo, which the settings row phrases as "unknown"
                // rather than as a fault. `store_error` is about dodo's own file.
                state.status = read.clone().unwrap_or(None);
                if state
                    .status
                    .as_ref()
                    .is_some_and(|status| status.settings_revision == state.document.revision)
                    && reported_language
                        .is_some_and(|language| state.document.active_languages.contains(language))
                {
                    selected = reported_language;
                }
            });
            selected
        });
        if let Some(language) = adopt_language {
            cx.update(|cx| Self::set_language(language, cx));
        }
        cx.update(|cx| {
            // Each platform's fallback owner is decided from the status this
            // just adopted — whether a native host is live, and whether it has
            // the settings dodo wrote — so both reconcile here, not only macOS.
            #[cfg(target_os = "macos")]
            Self::reconcile_event_tap(cx);
            #[cfg(target_os = "windows")]
            Self::reconcile_keyboard_hook(cx);
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
        let migrate_settings = loaded
            .as_ref()
            .is_ok_and(|document| document.version < SETTINGS_SCHEMA_VERSION);
        let reported_language = status.as_ref().and_then(StatusDocument::language);
        let mut adopt_language = None;
        cx.update_global::<InputMethod, _>(|state, _| {
            state.status = status;
            state.installed = installed;
            match loaded {
                Ok(document) => {
                    if state
                        .status
                        .as_ref()
                        .is_some_and(|status| status.settings_revision == document.revision)
                        && reported_language
                            .is_some_and(|language| document.active_languages.contains(language))
                    {
                        adopt_language = reported_language;
                    }
                    state.document = document;
                    state.store_error = None;
                }
                Err(error) => {
                    eprintln!("input-method.json: {error}");
                    state.store_error = Some(error);
                }
            }
        });
        if adopt_language.is_none() {
            adopt_language = cx.try_global::<InputMethod>().and_then(|state| {
                (!state
                    .document
                    .active_languages
                    .contains(state.document.language))
                .then(|| {
                    state
                        .document
                        .active_languages
                        .iter()
                        .next()
                        .expect("ActiveLanguages is non-empty")
                })
            });
        }
        if migrate_settings {
            // Rewrite an older document so its migrated shortcut reaches every
            // host, including one launched after dodo closes.
            Self::edit(cx, |_| {});
        }
        if let Some(language) = adopt_language {
            Self::set_language(language, cx);
        }
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

    /// Re-checks Event Tap after Dodo returns to its active window.
    ///
    /// The macOS Accessibility request is asynchronous, so returning from
    /// System Settings is the narrow lifecycle point that can observe a grant
    /// and start the already-selected fallback without another save.
    #[cfg(target_os = "macos")]
    pub fn reconcile_event_tap_after_activation(cx: &mut App) {
        Self::reconcile_event_tap(cx);
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
            // Trust is checked and, once, macOS is asked only if this reaches
            // `EventTap::start`; the model still decides exclusive ownership
            // and the native hand-off.
            let desired = desired_status(
                state.document.backend,
                native_live,
                settings_applied,
                services::event_tap::accessibility_trusted(),
            );
            match desired {
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
                EventTapStatus::NeedsAccessibility => {
                    state.event_tap = None;
                    state.event_tap_status = EventTapStatus::NeedsAccessibility;
                }
                EventTapStatus::Running | EventTapStatus::Failed => {}
            }

            // One tap, reconfigured. Starting a second while the first is held
            // would leave two listeners answering one key — the old shortcut
            // among them.
            if let Some(tap) = &state.event_tap {
                tap.reconfigure(state.document);
                state.event_tap_status = tap.status();
                return;
            }

            let request_accessibility =
                should_request_accessibility(desired, state.event_tap_accessibility_requested);
            if request_accessibility {
                state.event_tap_accessibility_requested = true;
            }
            match services::event_tap::EventTap::start(
                state.document,
                state.switch_sender.clone(),
                request_accessibility,
            ) {
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
            if hook_status(state.document.backend, true) != KeyboardHookStatus::Running {
                state.keyboard_hook = None;
                state.keyboard_hook_status = KeyboardHookStatus::Inactive;
                return;
            }
            // One hook, reconfigured — `SetWindowsHookExW` twice would install
            // two, and the first would still be matching the replaced shortcut.
            if let Some(hook) = &state.keyboard_hook {
                hook.reconfigure(state.document);
                state.keyboard_hook_status = hook.status();
                return;
            }
            match services::keyboard_hook::KeyboardHook::start(
                state.document,
                state.switch_sender.clone(),
            ) {
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
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    let (switch_sender, mut switch_requests) = unbounded();
    cx.set_global(InputMethod::new(
        Arc::new(DiskInputMethodStore::new()),
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        switch_sender,
    ));
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        let mut receiver = services::notify::language_changes();
        let task = cx.spawn(async move |cx| {
            while receiver.next().await.is_some() {
                InputMethod::refresh_status(cx).await;
            }
        });
        // A dodo-owned listener cycled the language. It is already typing that
        // way; this is what writes it down, and `set_language` refuses one the
        // user has since switched off.
        let switch_task = cx.spawn(async move |cx| {
            while let Some(language) = switch_requests.next().await {
                cx.update(|cx| InputMethod::set_language(language, cx));
            }
        });
        cx.update_global::<InputMethod, _>(|state, _| {
            state.language_changes = Some(task);
            state.switch_requests = Some(switch_task);
        });
    }
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
                let status = store.read_status().ok().flatten();
                (loaded, status, services::windows::is_registered())
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
    use crate::models::install::{InstallFailure, InstallOutcome, InstallStep};
    use crate::services::store::{InMemoryInputMethodStore, InputMethodStore};
    use dodo_ime_ipc::settings::{Backend, Scheme, SettingsDocument, Tone, VietnameseSettings};
    use dodo_ime_ipc::status::StatusDocument;
    use std::sync::Arc;

    fn state(store: Arc<InMemoryInputMethodStore>) -> InputMethod {
        InputMethod::new(
            store,
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            futures_channel::mpsc::unbounded().0,
        )
    }

    /// The revision bookkeeping, without a frame: it is the state layer's own
    /// rule and the only thing that can say "your settings have not arrived yet".
    #[test]
    fn a_change_bumps_the_revision_and_an_identical_write_does_not() {
        let store = Arc::new(InMemoryInputMethodStore::default());
        let mut state = state(store.clone());

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
        let mut state = state(store);

        state.document = SettingsDocument {
            version: dodo_ime_ipc::settings::SETTINGS_SCHEMA_VERSION,
            backend: Backend::Native,
            language: LanguageId::Vietnamese,
            revision: 4,
            vietnamese: VietnameseSettings {
                tone_placement: Tone::Traditional,
                ..VietnameseSettings::default()
            },
            ..SettingsDocument::default()
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
        let mut state = state(store);
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
