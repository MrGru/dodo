//! dodo's Input method feature: persisted language settings plus the platform
//! listener that applies them while dodo is running.
//!
//! macOS uses an Accessibility-gated Event Tap and Windows uses a low-level
//! Keyboard Hook. Each listener stays attached across language changes so the
//! configured shortcut can always switch back to Vietnamese; languages with no
//! engine pass ordinary keys through unchanged.
//!
//! [`InputMethod`] is a `Global`. The tool pane and tray read it directly, and
//! every edit writes `input-method.json` on the background executor before
//! refreshing the windows. Listener callbacks report language changes over a
//! channel because they have no GPUI [`App`].
#![cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]

pub mod models;
pub mod paths;
pub mod services;
pub mod views;

use dodo_i18n as i18n;

/// The binary's quick-navigation key contexts, mirrored for the shortcut
/// recorder's tests. `src/quick_nav` asserts these stay equal.
pub const QUICK_NAV_KEY_CONTEXT: &str = "Dodo";
pub const QUICK_NAV_NORMAL_MODE: &str = "Dodo && !Input";

type LanguagesObserver = fn(ActiveLanguages, LanguageId, &mut App);

static LANGUAGES_OBSERVER: std::sync::OnceLock<LanguagesObserver> = std::sync::OnceLock::new();

/// Registers the tray's one language-change observer.
pub fn observe_languages(observer: LanguagesObserver) {
    let _ = LANGUAGES_OBSERVER.set(observer);
}

use std::sync::Arc;

use dodo_ime_core::{ActiveLanguages, LanguageId};
#[cfg(any(target_os = "macos", target_os = "windows"))]
use futures_channel::mpsc::{UnboundedSender, unbounded};
#[cfg(any(target_os = "macos", target_os = "windows"))]
use futures_util::StreamExt as _;
use gpui::{App, AsyncApp, BorrowAppContext as _, Global, Task};

use crate::i18n::Str;
#[cfg(target_os = "macos")]
use crate::models::event_tap::{EventTapStatus, desired_status, should_request_accessibility};
#[cfg(target_os = "windows")]
use crate::models::keyboard_hook::KeyboardHookStatus;
use crate::models::settings::{
    LanguageSwitch, SETTINGS_SCHEMA_VERSION, Scheme, SettingsDocument, Tone, VietnameseSettings,
};
use crate::services::document::SettingsError;
use crate::services::store::{DiskInputMethodStore, InputMethodStore, message_for};

/// The input method's persisted state and live platform listener.
pub struct InputMethod {
    document: SettingsDocument,
    store: Arc<dyn InputMethodStore>,
    store_error: Option<SettingsError>,
    save: Option<Task<()>>,
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    switch_requests: Option<Task<()>>,
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    switch_sender: UnboundedSender<LanguageId>,
    #[cfg(target_os = "macos")]
    event_tap: Option<services::event_tap::EventTap>,
    #[cfg(target_os = "macos")]
    event_tap_status: EventTapStatus,
    #[cfg(target_os = "macos")]
    event_tap_accessibility_requested: bool,
    #[cfg(target_os = "windows")]
    keyboard_hook: Option<services::keyboard_hook::KeyboardHook>,
    #[cfg(target_os = "windows")]
    keyboard_hook_status: KeyboardHookStatus,
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
            save: None,
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            switch_requests: None,
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            switch_sender,
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
        }
    }

    pub fn settings(cx: &App) -> VietnameseSettings {
        cx.try_global::<InputMethod>()
            .map(|state| state.document.vietnamese)
            .unwrap_or_default()
    }

    pub fn store_error(cx: &App) -> Option<Str> {
        cx.try_global::<InputMethod>()
            .and_then(|state| state.store_error.as_ref().map(message_for))
    }

    pub fn language(cx: &App) -> LanguageId {
        cx.try_global::<InputMethod>()
            .map(|state| state.document.language)
            .unwrap_or_default()
    }

    pub fn active_languages(cx: &App) -> ActiveLanguages {
        cx.try_global::<InputMethod>()
            .map(|state| state.document.active_languages)
            .unwrap_or_default()
    }

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

    pub fn set_language_switch(switch: LanguageSwitch, cx: &mut App) {
        if switch.is_valid() {
            Self::edit(cx, |document| document.language_switch = switch);
        }
    }

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

    pub fn set_scheme(scheme: Scheme, cx: &mut App) {
        Self::edit(cx, |document| document.vietnamese.scheme = scheme);
    }

    pub fn set_tone_placement(tone: Tone, cx: &mut App) {
        Self::edit(cx, |document| document.vietnamese.tone_placement = tone);
    }

    pub fn set_spell_check(on: bool, cx: &mut App) {
        Self::edit(cx, |document| document.vietnamese.spell_check = on);
    }

    pub fn set_bracket_shortcuts(on: bool, cx: &mut App) {
        Self::edit(cx, |document| document.vietnamese.bracket_shortcuts = on);
    }

    /// Applies one change, reconfigures the live listener, and persists it.
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

            state.document = SettingsDocument::next_from(next);
            let document = state.document;
            #[cfg(target_os = "macos")]
            if let Some(tap) = &state.event_tap {
                tap.reconfigure(document);
            }
            #[cfg(target_os = "windows")]
            if let Some(hook) = &state.keyboard_hook {
                hook.reconfigure(document);
            }

            let store = state.store.clone();
            state.save = Some(cx.spawn(async move |cx| {
                let written = cx
                    .background_executor()
                    .spawn(async move { store.persist_settings(&document) })
                    .await;
                cx.update(|cx| match written {
                    Ok(()) => {
                        Self::clear_error(cx);
                        #[cfg(target_os = "macos")]
                        Self::reconcile_event_tap(cx);
                        #[cfg(target_os = "windows")]
                        Self::reconcile_keyboard_hook(cx);
                    }
                    Err(error) => Self::report(error, cx),
                });
            }));
        });

        if let Some(observe) = LANGUAGES_OBSERVER.get() {
            observe(Self::active_languages(cx), Self::language(cx), cx);
        }
        cx.refresh_windows();
    }

    fn clear_error(cx: &mut App) {
        if cx.try_global::<InputMethod>().is_some() {
            cx.update_global::<InputMethod, _>(|state, _| state.store_error = None);
        }
    }

    fn report(error: SettingsError, cx: &mut App) {
        eprintln!("input-method.json: {error}");
        if cx.try_global::<InputMethod>().is_some() {
            cx.update_global::<InputMethod, _>(|state, _| state.store_error = Some(error));
        }
        cx.refresh_windows();
    }

    fn adopt(loaded: Result<SettingsDocument, SettingsError>, cx: &mut App) {
        if cx.try_global::<InputMethod>().is_none() {
            return;
        }
        let migrate = loaded
            .as_ref()
            .is_ok_and(|document| document.version < SETTINGS_SCHEMA_VERSION);
        cx.update_global::<InputMethod, _>(|state, _| match loaded {
            Ok(document) => {
                state.document = document;
                state.store_error = None;
            }
            Err(error) => {
                eprintln!("input-method.json: {error}");
                state.store_error = Some(error);
            }
        });

        let replacement = cx.try_global::<InputMethod>().and_then(|state| {
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
        if migrate {
            Self::edit(cx, |_| {});
        }
        if let Some(language) = replacement {
            Self::set_language(language, cx);
        }
        #[cfg(target_os = "macos")]
        Self::reconcile_event_tap(cx);
        #[cfg(target_os = "windows")]
        Self::reconcile_keyboard_hook(cx);
        cx.refresh_windows();
    }

    /// Re-checks Event Tap after Dodo returns from macOS Accessibility settings.
    #[cfg(target_os = "macos")]
    pub fn reconcile_event_tap_after_activation(cx: &mut App) {
        Self::reconcile_event_tap(cx);
    }

    #[cfg(target_os = "macos")]
    fn reconcile_event_tap(cx: &mut App) {
        if cx.try_global::<InputMethod>().is_none() {
            return;
        }
        cx.update_global::<InputMethod, _>(|state, _| {
            let desired = desired_status(services::event_tap::accessibility_trusted());
            if desired == EventTapStatus::NeedsAccessibility {
                state.event_tap = None;
                state.event_tap_status = desired;
            }

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

    #[cfg(target_os = "windows")]
    fn reconcile_keyboard_hook(cx: &mut App) {
        if cx.try_global::<InputMethod>().is_none() {
            return;
        }
        cx.update_global::<InputMethod, _>(|state, _| {
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

/// Registers the global and channel carrying listener-driven language changes.
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
        let task = cx.spawn(async move |cx| {
            while let Some(language) = switch_requests.next().await {
                cx.update(|cx| InputMethod::set_language(language, cx));
            }
        });
        cx.update_global::<InputMethod, _>(|state, _| state.switch_requests = Some(task));
    }
}

/// Loads settings on the background executor, then starts this platform's listener.
pub async fn load(cx: &mut AsyncApp) {
    let Some(store) = cx.update(|cx| {
        cx.try_global::<InputMethod>()
            .map(|state| state.store.clone())
    }) else {
        return;
    };

    let loaded = cx
        .background_executor()
        .spawn(async move { store.load_settings() })
        .await;
    cx.update(|cx| InputMethod::adopt(loaded, cx));
}
