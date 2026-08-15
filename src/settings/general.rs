use std::{cell::Cell, rc::Rc};

use gpui::*;
use gpui_component::setting::SettingField;
use gpui_component::switch::Switch;

use crate::api_explorer::ScriptPolicy;
use crate::api_explorer::models::script_consent::ConsentPolicy;
use crate::i18n::{Language, LanguageExt, shell, t};
use crate::session::Session;

/// The one OS-backed setting's last trustworthy answer.
///
/// It begins loading rather than pretending that a false switch reflects the
/// OS. A failed write is unknown too: the OS may have changed despite returning
/// an error, so the old value is no longer an honest answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum StartupStatus {
    Loading,
    Known(bool),
    Unknown,
}

impl StartupStatus {
    pub(super) fn read_once(read: impl FnOnce() -> bool) -> Self {
        Self::Known(read())
    }

    pub(super) fn after_successful_set(enabled: bool) -> Self {
        Self::Known(enabled)
    }

    pub(super) fn after_failed_set() -> Self {
        Self::Unknown
    }
}

pub(super) fn language_field() -> SettingField<SharedString> {
    let options = Language::ALL
        .map(|language| (language.code().into(), language.label().into()))
        .to_vec();

    SettingField::dropdown(
        options,
        |cx: &App| Language::current(cx).code().into(),
        |value: SharedString, cx: &mut App| {
            let language = Language::from_code(&value);
            language.set(cx);
            // Persisted here rather than inside `Language::set`: `i18n` is the
            // mechanism and has no business knowing dodo writes files.
            Session::set_language(language.code(), cx);
        },
    )
    .default_value(Language::default().code())
}

/// Whether the API Explorer runs a request's scripts.
///
/// **The one setting on this page that is not persisted**, now that
/// `session.json` keeps the rest: a fresh launch always asks about imported
/// scripts rather than running them. A security default that silently stopped
/// resetting is exactly the kind of change nobody notices until it matters, so
/// this stays deliberate rather than convenient. The *approvals* the prompt
/// collects are persisted separately, per script — see
/// `api_explorer::services::consent_store`.
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(super) fn start_with_os_field(status: Rc<Cell<StartupStatus>>) -> SettingField<SharedString> {
    SettingField::render(move |_, _, cx| match status.get() {
        StartupStatus::Loading => div()
            .child(t(shell::Text::StartWithOsChecking, cx))
            .into_any_element(),
        StartupStatus::Unknown => div()
            .child(t(shell::Text::StartWithOsStatusUnknown, cx))
            .into_any_element(),
        StartupStatus::Known(enabled) => {
            let status = status.clone();
            Switch::new("start-with-os")
                .checked(enabled)
                .on_click(move |enabled: &bool, _, cx: &mut App| {
                    match crate::tray::startup::set_enabled(*enabled) {
                        Ok(()) => status.set(StartupStatus::after_successful_set(*enabled)),
                        Err(error) => {
                            status.set(StartupStatus::after_failed_set());
                            eprintln!("start with OS: {error}");
                        }
                    }
                    cx.refresh_windows();
                })
                .into_any_element()
        }
    })
}

pub(super) fn run_scripts_field(cx: &App) -> SettingField<SharedString> {
    let options = ConsentPolicy::ALL
        .map(|policy| (policy.code().into(), t(policy.label(), cx)))
        .to_vec();

    SettingField::dropdown(
        options,
        |cx: &App| ScriptPolicy::current(cx).code().into(),
        |value: SharedString, cx: &mut App| ScriptPolicy::set(ConsentPolicy::from_code(&value), cx),
    )
    .default_value(ConsentPolicy::default().code())
}
