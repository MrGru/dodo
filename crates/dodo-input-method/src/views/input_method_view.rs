//! The Input method tool: install dodo's input method, and tell it how to type.
//!
//! This is the whole surface. It was a Settings page until the captain asked on
//! 2026-08-09 for it to be a feature instead, and **nothing was left behind** —
//! the install button and the four engine settings are here and nowhere else, so
//! there is no control a user can reach from two places and no second answer to
//! "where do I set my input scheme".
//!
//! # It holds no *setting*, on purpose
//!
//! Every control reads [`InputMethod`] in `render` and writes it on click. There
//! is no `SelectState`, no cached settings and nothing to keep in step, which
//! matters more here than in the other tools for one reason: **this view is
//! built at startup and the settings are loaded after it.** `Layout::new` runs
//! before `input_method::load` has finished reading `input-method.json`, so a
//! control that captured its value in `new` would show the defaults until the
//! user clicked something. A control that reads the global every frame shows
//! whatever has arrived.
//!
//! That is also why the two either/or settings are [`RadioGroup`]s rather than
//! the dropdowns the settings page used: a dropdown owns an `Entity<SelectState>`
//! whose selected row is a second copy of the setting, and the copy is what would
//! drift.
//!
//! The shortcut recorder is the one thing here with fields, and it is the
//! exception that proves the rule: what it holds is *what the user is doing right
//! now* — a focus handle, whether it is capturing, the modifiers held so far —
//! and none of that is a setting. The shortcut itself is still read from
//! [`InputMethod`] every frame, and the recorded combination goes straight to
//! [`InputMethod::set_language_switch`] without being kept here first.
//!
//! # Backend ownership stays outside this view
//!
//! Native Input Method is still launched by macOS and keeps typing after dodo
//! closes. Event Tap is dodo-owned, Accessibility-gated, and active only while
//! selected. Every control writes `input-method.json`; [`crate`]
//! is where exclusive ownership and lifecycle are decided. This file only reads
//! that global and assembles the native status sentence.

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::radio::RadioGroup;
use gpui_component::switch::Switch;
use gpui_component::{
    ActiveTheme, Disableable as _, Selectable as _, StyledExt as _, h_flex, v_flex,
};

use dodo_ime_core::LanguageId;
use dodo_ime_ipc::settings::{Backend, Scheme, Shortcut, ShortcutKey, ShortcutModifiers, Tone};

use crate::InputMethod;
#[cfg(target_os = "macos")]
use crate::Install;
use crate::i18n::{Str, input_method, t, tray};
use crate::models::event_tap::EventTapStatus;
use crate::models::keyboard_hook::KeyboardHookStatus;
#[cfg(target_os = "macos")]
use crate::models::status::status_message;
use crate::models::windows::{WindowsInstall, WindowsInstallFailure, WindowsInstallOutcome};

/// The key context the recorder claims **while it is capturing**, and gpui
/// components' own name for a focused text field.
///
/// Claiming it is what stops a recorded keystroke also *doing* something.
/// `quick_nav::NORMAL_MODE` is `Dodo && !Input` and gpui evaluates `!` against
/// the whole dispatch path, so while this sits on the path every quick-navigation
/// binding declines — `⌘V` records as an unsupported key instead of pasting the
/// clipboard into a tool, and `Esc` reaches this view's own handler rather than
/// `LeaveInsertMode`. It is dropped the moment recording ends, so the field is
/// never mistaken for a text input the rest of the time.
const RECORDER_CONTEXT: &str = "Input";

/// The Input method pane.
///
/// Every *setting* is still read from [`InputMethod`] in `render` — see the
/// module docs. The three fields here are not settings: they are what the
/// recorder is doing right now, which nothing outside this view can observe and
/// nothing needs to survive a restart.
pub struct InputMethodView {
    /// Where key and modifier events arrive while recording.
    recorder: FocusHandle,
    recording: bool,
    /// The largest modifier set seen since recording began, which is what a
    /// modifier-only shortcut is committed from when they are all released.
    held: ShortcutModifiers,
    /// Whether the last attempt pressed something no host could match.
    unsupported_key: bool,
    /// Cancels recording when the field loses focus. Held because a
    /// [`Subscription`] stops the moment it is dropped, and a field left saying
    /// "press a combination…" while the keyboard has gone somewhere else is a
    /// lie the user has no way to correct.
    _blur: Subscription,
}

impl InputMethodView {
    /// Takes `window` it does not use, because every tool's constructor has this
    /// signature and `Layout::new` calls them all the same way.
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let recorder = cx.focus_handle();
        let blur = cx.on_focus_out(&recorder, window, |this, _, _, cx| {
            if this.recording {
                this.recording = false;
                this.held = ShortcutModifiers::NONE;
                cx.notify();
            }
        });
        Self {
            recorder,
            recording: false,
            held: ShortcutModifiers::NONE,
            unsupported_key: false,
            _blur: blur,
        }
    }

    /// The radio group's two rows: Native, plus whichever no-install fallback
    /// this platform has. A `cfg!` rather than two `#[cfg]` definitions, so
    /// **both** arms typecheck wherever this file is compiled — a mistake in
    /// the Windows arm is otherwise invisible from a Mac, which is exactly how
    /// the platform-gated labels broke a build the day before this moved.
    const BACKENDS: [Backend; 2] = if cfg!(target_os = "windows") {
        [Backend::Native, Backend::KeyboardHook]
    } else {
        [Backend::Native, Backend::EventTap]
    };

    /// Install, or reinstall, or nothing while one is running.
    #[cfg(target_os = "macos")]
    fn install_button(cx: &App) -> Button {
        let running = InputMethod::install_state(cx) == Install::Running;
        let label = if InputMethod::is_installed(cx) {
            input_method::Text::Reinstall
        } else {
            input_method::Text::Install
        };

        Button::new("install-input-method")
            .primary()
            .label(t(label, cx))
            .disabled(running)
            // Everything the press does is asynchronous — the copy, the four Text
            // Input Sources calls and the `pkill` all run on the background
            // executor — so this closure only starts it. See
            // `InputMethod::install`.
            .on_click(|_, _, cx| InputMethod::install(cx))
    }

    /// The status line and the button that changes it, in one card.
    ///
    /// **One card, not a "Status" row and an "Install" row.** There is one thing
    /// to say and one thing to do about it, and splitting them would be two rows
    /// that are each half an answer.
    #[cfg(target_os = "macos")]
    fn status_card(cx: &App) -> impl IntoElement {
        // `describes_a_live_process` is the one part that cannot move into the
        // pure status model: it is `kill(pid, 0)`, a syscall.
        let status = InputMethod::status(cx);
        let running = status
            .as_ref()
            .filter(|status| status.describes_a_live_process())
            .map(|status| status.bundle_version.clone());
        let status_line = status_message(
            &InputMethod::install_state(cx),
            InputMethod::is_installed(cx),
            InputMethod::settings_applied(cx),
            running.as_deref(),
        );

        h_flex()
            .items_center()
            .gap_3()
            .p_3()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(cx.theme().border)
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_1()
                    .child(div().font_bold().child(t(input_method::Text::Status, cx)))
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(t(status_line, cx)),
                    ),
            )
            .child(div().flex_shrink_0().child(Self::install_button(cx)))
    }

    /// Shown only when there is something wrong with `input-method.json` — the
    /// same treatment quick navigation's storage row and the session's get. A
    /// refused settings file here usually means the installed bundle and this
    /// dodo are different versions, which is why the message says to reinstall
    /// rather than "update".
    fn storage_problem(problem: Str, cx: &App) -> impl IntoElement {
        v_flex()
            .gap_1()
            .p_3()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(cx.theme().danger)
            .bg(cx.theme().danger.opacity(0.1))
            .text_color(cx.theme().danger)
            .child(
                div()
                    .font_bold()
                    .child(t(input_method::Text::StorageProblem, cx)),
            )
            .child(div().text_sm().child(t(problem, cx)))
    }

    /// One setting: what it is and what it does on the left, the control on the
    /// right. `min_w_0` on the text column is what lets a long description wrap
    /// instead of pushing the control off the pane.
    fn row(title: Str, description: Str, control: impl IntoElement, cx: &App) -> impl IntoElement {
        h_flex()
            .items_start()
            .gap_4()
            .py_2()
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_1()
                    .child(div().child(t(title, cx)))
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(t(description, cx)),
                    ),
            )
            .child(div().flex_shrink_0().child(control))
    }

    fn description() -> Str {
        let description = if cfg!(target_os = "windows") {
            input_method::Text::WindowsDescription
        } else {
            input_method::Text::Description
        };
        description.into()
    }

    fn backend_label(backend: Backend) -> Str {
        let label = match backend {
            Backend::Native => {
                if cfg!(target_os = "windows") {
                    input_method::Text::NativeTsf
                } else {
                    input_method::Text::Native
                }
            }
            Backend::EventTap => input_method::Text::EventTap,
            Backend::KeyboardHook => input_method::Text::KeyboardHook,
        };
        label.into()
    }

    /// The two real transformation hosts. The radio reads the global on every
    /// render, so asynchronous settings loading cannot leave it stale.
    fn backend_choice(cx: &App) -> impl IntoElement {
        let selected = Self::BACKENDS
            .iter()
            .position(|backend| *backend == InputMethod::backend(cx));

        RadioGroup::horizontal("input-method-backend")
            .children(Self::BACKENDS.map(|backend| t(Self::backend_label(backend), cx)))
            .selected_index(selected)
            .on_click(|ix: &usize, _, cx| {
                if let Some(backend) = Self::BACKENDS.get(*ix).copied() {
                    InputMethod::set_backend(backend, cx);
                }
            })
    }

    #[allow(
        dead_code,
        reason = "kept portable so every target type-checks each platform's label conversion"
    )]
    fn event_tap_status_line(status: EventTapStatus) -> Str {
        let label = match status {
            EventTapStatus::Inactive => input_method::Text::EventTapInactive,
            EventTapStatus::WaitingForNative => input_method::Text::EventTapWaitingForNative,
            EventTapStatus::NeedsAccessibility => input_method::Text::EventTapNeedsAccessibility,
            EventTapStatus::Running => input_method::Text::EventTapRunning,
            EventTapStatus::Failed => input_method::Text::EventTapFailed,
        };
        label.into()
    }

    /// Event Tap has no install action: macOS owns both the request and the
    /// Accessibility grant, which dodo reports but never changes.
    #[cfg(target_os = "macos")]
    fn event_tap_status_card(cx: &App) -> impl IntoElement {
        h_flex()
            .items_center()
            .gap_3()
            .p_3()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(cx.theme().border)
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_1()
                    .child(
                        div()
                            .font_bold()
                            .child(t(input_method::Text::EventTapStatus, cx)),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(t(
                                Self::event_tap_status_line(InputMethod::event_tap_status(cx)),
                                cx,
                            )),
                    ),
            )
    }

    #[allow(
        dead_code,
        reason = "kept portable so every target type-checks each platform's label conversion"
    )]
    fn windows_tsf_status_line(state: WindowsInstall, installed: bool) -> Str {
        let label = match state {
            WindowsInstall::Installing => input_method::Text::Installing,
            WindowsInstall::Uninstalling => input_method::Text::Uninstalling,
            WindowsInstall::Done(WindowsInstallOutcome::Ready) => {
                input_method::Text::WindowsTsfInstalled
            }
            WindowsInstall::Done(WindowsInstallOutcome::Removed) => {
                input_method::Text::WindowsTsfRemoved
            }
            WindowsInstall::Done(WindowsInstallOutcome::Failed(
                WindowsInstallFailure::NoSourceDll,
            )) => input_method::Text::WindowsTsfNoDll,
            WindowsInstall::Done(WindowsInstallOutcome::Failed(WindowsInstallFailure::Copy {
                detail,
            })) => input_method::Text::CopyFailed(detail),
            WindowsInstall::Done(WindowsInstallOutcome::Failed(
                WindowsInstallFailure::Register { detail },
            )) => input_method::Text::WindowsTsfRegisterFailed(detail),
            WindowsInstall::Done(WindowsInstallOutcome::Failed(
                WindowsInstallFailure::Unregister { detail },
            )) => input_method::Text::WindowsTsfUnregisterFailed(detail),
            WindowsInstall::Idle if installed => input_method::Text::WindowsTsfInstalled,
            WindowsInstall::Idle => input_method::Text::WindowsTsfNotInstalled,
        };
        label.into()
    }

    #[cfg(target_os = "windows")]
    fn windows_tsf_status_card(cx: &App) -> impl IntoElement {
        let running = matches!(
            InputMethod::windows_install_state(cx),
            WindowsInstall::Installing | WindowsInstall::Uninstalling
        );
        let install_label = if InputMethod::is_installed(cx) {
            input_method::Text::Reinstall
        } else {
            input_method::Text::Install
        };
        h_flex()
            .items_center()
            .gap_3()
            .p_3()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(cx.theme().border)
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_1()
                    .child(
                        div()
                            .font_bold()
                            .child(t(input_method::Text::WindowsTsfStatus, cx)),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(t(
                                Self::windows_tsf_status_line(
                                    InputMethod::windows_install_state(cx),
                                    InputMethod::is_installed(cx),
                                ),
                                cx,
                            )),
                    ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .flex_shrink_0()
                    .child(
                        Button::new("install-windows-tsf")
                            .primary()
                            .label(t(install_label, cx))
                            .disabled(running)
                            .on_click(|_, _, cx| InputMethod::install(cx)),
                    )
                    .child(
                        Button::new("uninstall-windows-tsf")
                            .ghost()
                            .label(t(input_method::Text::Uninstall, cx))
                            .disabled(running || !InputMethod::is_installed(cx))
                            .on_click(|_, _, cx| InputMethod::uninstall(cx)),
                    ),
            )
    }

    fn active_languages_choice(cx: &App) -> impl IntoElement {
        let active = InputMethod::active_languages(cx);
        let count = active.iter().count();
        h_flex().gap_3().children(LanguageId::ALL.map(|language| {
            let enabled = active.contains(language);
            h_flex()
                .items_center()
                .gap_1()
                .child(
                    Switch::new(format!("input-method-active-language-{}", language.code()))
                        .checked(enabled)
                        .disabled(enabled && count == 1)
                        .on_click(move |checked: &bool, _, cx| {
                            InputMethod::set_language_enabled(language, *checked, cx)
                        }),
                )
                // Keyboard language names are endonyms, so they deliberately
                // stay recognizable regardless of dodo's display language.
                .child(div().text_sm().child(language_label(language)))
        }))
    }

    fn language_choice(cx: &App) -> impl IntoElement {
        let active = InputMethod::active_languages(cx);
        let selected = active
            .iter()
            .position(|language| language == InputMethod::language(cx));
        RadioGroup::horizontal("input-method-language")
            .children(active.iter().map(language_label))
            .selected_index(selected)
            .on_click(move |ix: &usize, _, cx| {
                if let Some(language) = active.iter().nth(*ix) {
                    InputMethod::set_language(language, cx);
                }
            })
    }

    /// Keeps the label at the left while there is room, then wraps its bounded
    /// control group below it instead of letting either column overlap.
    fn language_switch_row(&self, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .items_start()
            .flex_wrap()
            .gap_4()
            .py_2()
            .child(
                v_flex()
                    .flex_1()
                    .min_w(px(180.))
                    .gap_1()
                    .child(div().child(t(input_method::Text::LanguageSwitch, cx)))
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(t(input_method::Text::LanguageSwitchDescription, cx)),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(260.))
                    .child(self.language_switch_choice(cx)),
            )
    }

    /// The recorder field, the beep switch, and whatever the last recording
    /// attempt has to say.
    fn language_switch_choice(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let switch = InputMethod::language_switch(cx);
        let label: SharedString = if self.recording {
            t(input_method::Text::ShortcutRecording, cx)
        } else {
            shortcut_display(switch.shortcut, cx).into()
        };
        v_flex()
            .w_full()
            .gap_2()
            .child(
                h_flex()
                    .w_full()
                    .flex_wrap()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            // The focus handle is what makes this a recorder
                            // rather than a button: gpui delivers key and
                            // modifier events to the focused element, and with
                            // nothing focused the dispatch path is the window
                            // root, which carries none of them here.
                            .track_focus(&self.recorder)
                            .when(self.recording, |this| this.key_context(RECORDER_CONTEXT))
                            .on_key_down(cx.listener(Self::key_recorded))
                            .on_modifiers_changed(cx.listener(Self::modifiers_recorded))
                            .child(
                                Button::new("input-method-language-switch-recorder")
                                    .outline()
                                    .selected(self.recording)
                                    .label(label)
                                    .on_click(cx.listener(Self::start_recording)),
                            ),
                    )
                    .child(
                        h_flex()
                            .items_center()
                            .gap_1()
                            .child(
                                Switch::new("input-method-language-switch-beep")
                                    .checked(switch.beep)
                                    .on_click(|checked: &bool, _, cx| {
                                        let mut switch = InputMethod::language_switch(cx);
                                        switch.beep = *checked;
                                        InputMethod::set_language_switch(switch, cx);
                                    }),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .child(t(input_method::Text::ShortcutBeep, cx)),
                            ),
                    ),
            )
            .when_some(self.recorder_hint(cx), |this, hint| {
                this.child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(t(hint, cx)),
                )
            })
    }

    /// What the row has to say beneath the field, if anything.
    ///
    /// The macOS caveat is not a nicety. `recognizedEvents:` in the
    /// InputMethodKit bundle is `NSEventMaskKeyDown` alone, so a bare modifier
    /// press never reaches it and a modifier-only shortcut is simply never
    /// delivered — the recorder would otherwise show a setting that does
    /// nothing, which is the exact failure this round exists to end.
    fn recorder_hint(&self, cx: &App) -> Option<Str> {
        if self.unsupported_key {
            return Some(input_method::Text::ShortcutUnsupportedKey.into());
        }
        #[cfg(target_os = "macos")]
        if InputMethod::backend(cx) == Backend::Native
            && InputMethod::language_switch(cx).shortcut.key == ShortcutKey::Modifiers
        {
            return Some(input_method::Text::ShortcutNeedsEventTap.into());
        }
        #[cfg(not(target_os = "macos"))]
        let _ = cx;
        None
    }

    fn start_recording(&mut self, _: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.recording = true;
        self.unsupported_key = false;
        self.held = ShortcutModifiers::NONE;
        self.recorder.focus(window, cx);
        cx.notify();
    }

    fn stop_recording(&mut self, cx: &mut Context<Self>) {
        self.recording = false;
        self.held = ShortcutModifiers::NONE;
        cx.notify();
    }

    /// One key press while recording.
    ///
    /// Escape with nothing held cancels, because a recorder with no way out
    /// would trap the keyboard; Escape *with* modifiers is an ordinary
    /// recordable combination and is recorded.
    fn key_recorded(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if !self.recording {
            return;
        }
        cx.stop_propagation();
        if event.is_held {
            return;
        }
        let modifiers = recorded_modifiers(&event.keystroke.modifiers);
        if event.keystroke.key == "escape" && modifiers == ShortcutModifiers::NONE {
            self.unsupported_key = false;
            self.stop_recording(cx);
            return;
        }
        // A key arrived, so the modifiers were a prefix and not the shortcut.
        self.held = ShortcutModifiers::NONE;
        match recordable_key(&event.keystroke.key).map(|key| Shortcut { modifiers, key }) {
            Some(shortcut) if shortcut.is_valid() => {
                self.unsupported_key = false;
                self.record(shortcut, cx);
            }
            _ => {
                self.unsupported_key = true;
                cx.notify();
            }
        }
    }

    /// Modifier presses while recording, which are how a modifier-only shortcut
    /// is captured.
    ///
    /// gpui reports the whole modifier set on every change, so the high-water
    /// mark is what the user held: `⌃` then `⌃⇧` then `⌃` then nothing records
    /// `⌃⇧`. Committing on the way *down* instead would make `⌃⌥⇧` impossible
    /// to record, since `⌃⌥` would already have been taken.
    fn modifiers_recorded(
        &mut self,
        event: &ModifiersChangedEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.recording {
            return;
        }
        let modifiers = recorded_modifiers(&event.modifiers);
        if modifiers.count() >= self.held.count() {
            self.held = modifiers;
            cx.notify();
            return;
        }
        if modifiers != ShortcutModifiers::NONE {
            return;
        }
        let shortcut = Shortcut {
            modifiers: self.held,
            key: ShortcutKey::Modifiers,
        };
        self.held = ShortcutModifiers::NONE;
        if shortcut.is_valid() {
            self.unsupported_key = false;
            self.record(shortcut, cx);
        } else {
            // One modifier tapped on its own. Keep recording rather than
            // storing something that would fire on every `⌘C`.
            self.unsupported_key = true;
            cx.notify();
        }
    }

    /// The one place a recorded combination becomes the live shortcut.
    fn record(&mut self, shortcut: Shortcut, cx: &mut Context<Self>) {
        let mut switch = InputMethod::language_switch(cx);
        switch.shortcut = shortcut;
        InputMethod::set_language_switch(switch, cx);
        self.stop_recording(cx);
    }

    #[allow(
        dead_code,
        reason = "kept portable so every target type-checks each platform's label conversion"
    )]
    fn keyboard_hook_status_line(status: KeyboardHookStatus) -> Str {
        let label = match status {
            KeyboardHookStatus::Inactive => input_method::Text::KeyboardHookInactive,
            KeyboardHookStatus::Running => input_method::Text::KeyboardHookRunning,
            KeyboardHookStatus::Failed => input_method::Text::KeyboardHookFailed,
        };
        label.into()
    }

    #[cfg(target_os = "windows")]
    fn keyboard_hook_status_card(cx: &App) -> impl IntoElement {
        h_flex()
            .items_center()
            .gap_3()
            .p_3()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(cx.theme().border)
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_1()
                    .child(
                        div()
                            .font_bold()
                            .child(t(input_method::Text::KeyboardHookStatus, cx)),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(t(
                                Self::keyboard_hook_status_line(InputMethod::keyboard_hook_status(
                                    cx,
                                )),
                                cx,
                            )),
                    ),
            )
    }

    /// Telex or VNI. The labels are proper nouns and identical in every
    /// language, so `input_method::Text::Telex` and `…Vni` both answer `_`.
    fn scheme_choice(cx: &App) -> impl IntoElement {
        let selected = Scheme::ALL
            .iter()
            .position(|scheme| *scheme == InputMethod::settings(cx).scheme);

        RadioGroup::horizontal("input-method-scheme")
            .children(Scheme::ALL.map(|scheme| {
                t(
                    match scheme {
                        Scheme::Telex => input_method::Text::Telex,
                        Scheme::Vni => input_method::Text::Vni,
                    },
                    cx,
                )
            }))
            .selected_index(selected)
            // The index is into `Scheme::ALL`, which is what built the labels, so
            // the two cannot disagree about which row is which scheme. An index
            // past the end is dropped rather than defaulting to Telex: a stray
            // one should change nothing, not silently reset the user's scheme.
            .on_click(|ix: &usize, _, cx| {
                if let Some(scheme) = Scheme::ALL.get(*ix).copied() {
                    InputMethod::set_scheme(scheme, cx);
                }
            })
    }

    /// Where the tone mark sits in a syllable that could take it in two places.
    fn tone_choice(cx: &App) -> impl IntoElement {
        let selected = Tone::ALL
            .iter()
            .position(|tone| *tone == InputMethod::settings(cx).tone_placement);

        RadioGroup::horizontal("input-method-tone")
            .children(Tone::ALL.map(|tone| {
                t(
                    match tone {
                        Tone::Modern => input_method::Text::ToneModern,
                        Tone::Traditional => input_method::Text::ToneTraditional,
                    },
                    cx,
                )
            }))
            .selected_index(selected)
            .on_click(|ix: &usize, _, cx| {
                if let Some(tone) = Tone::ALL.get(*ix).copied() {
                    InputMethod::set_tone_placement(tone, cx);
                }
            })
    }

    /// The Event Tap's browser workaround, and the one row that appears only
    /// under a fallback backend.
    ///
    /// It is drawn beside the Event Tap status card rather than with the four
    /// engine settings below, because it is not one: Native composes through a
    /// marked-text client and has no Backspace rewrite for a browser selection
    /// to land in the middle of, so a switch offered there would control
    /// nothing.
    #[cfg(target_os = "macos")]
    fn browser_fix_switch(cx: &App) -> Switch {
        Switch::new("input-method-browser-address-bar-fix")
            .checked(InputMethod::browser_address_bar_fix(cx))
            .on_click(|checked: &bool, _, cx| {
                InputMethod::set_browser_address_bar_fix(*checked, cx)
            })
    }

    fn spell_check_switch(cx: &App) -> Switch {
        Switch::new("input-method-spell-check")
            .checked(InputMethod::settings(cx).spell_check)
            .on_click(|checked: &bool, _, cx| InputMethod::set_spell_check(*checked, cx))
    }

    fn bracket_shortcuts_switch(cx: &App) -> Switch {
        Switch::new("input-method-bracket-shortcuts")
            .checked(InputMethod::settings(cx).bracket_shortcuts)
            .on_click(|checked: &bool, _, cx| InputMethod::set_bracket_shortcuts(*checked, cx))
    }
}

/// Keyboard input languages are endonyms, as in the macOS tray menu; translating
/// them would make the identifier less recognizable to the person selecting it.
fn language_label(language: LanguageId) -> &'static str {
    match language {
        LanguageId::English => "English",
        LanguageId::Vietnamese => "Tiếng Việt",
        LanguageId::Japanese => "日本語",
    }
}

/// gpui's modifier set in the shared vocabulary.
///
/// `platform` is the one field worth naming: gpui calls Command and the Windows
/// key by that name, `dodo_ime_core::Modifiers` calls the same physical key
/// `meta`, and every host normalizes into the latter. Mapping it here is what
/// makes a shortcut recorded on macOS mean the same hand shape on Windows.
fn recorded_modifiers(modifiers: &Modifiers) -> ShortcutModifiers {
    ShortcutModifiers {
        control: modifiers.control,
        alt: modifiers.alt,
        shift: modifiers.shift,
        meta: modifiers.platform,
    }
}

/// The shortcut key a gpui keystroke names, or `None` for one no input-method
/// host could recognise again.
///
/// A printing key is deliberately absent; `dodo_ime_ipc::settings::ShortcutKey`
/// carries the reason. Everything here is a name gpui produces for a key that
/// types nothing, so the refusal is about the host contract and not about this
/// table being short.
fn recordable_key(key: &str) -> Option<ShortcutKey> {
    Some(match key {
        "space" => ShortcutKey::Space,
        "enter" => ShortcutKey::Enter,
        "tab" => ShortcutKey::Tab,
        "escape" => ShortcutKey::Escape,
        "backspace" => ShortcutKey::Backspace,
        "delete" => ShortcutKey::Delete,
        "home" => ShortcutKey::Home,
        "end" => ShortcutKey::End,
        "pageup" => ShortcutKey::PageUp,
        "pagedown" => ShortcutKey::PageDown,
        "left" => ShortcutKey::ArrowLeft,
        "right" => ShortcutKey::ArrowRight,
        "up" => ShortcutKey::ArrowUp,
        "down" => ShortcutKey::ArrowDown,
        _ => return None,
    })
}

/// The shortcut as a keyboard shows it.
///
/// The modifier part is untranslated on purpose, for the reason
/// [`language_label`] gives about endonyms: `⌃⌥⇧⌘` and `Ctrl`/`Alt`/`Shift`/
/// `Win` are what is printed on the key the person is being asked to press, and
/// a translated name would be a worse identifier than the keycap. The base key
/// keeps its `Str`, which is where a language does have something to say.
fn shortcut_display(shortcut: Shortcut, cx: &App) -> String {
    let mut parts: Vec<String> = Vec::with_capacity(5);
    for (held, symbol) in [
        (shortcut.modifiers.control, MODIFIER_CONTROL),
        (shortcut.modifiers.alt, MODIFIER_ALT),
        (shortcut.modifiers.shift, MODIFIER_SHIFT),
        (shortcut.modifiers.meta, MODIFIER_META),
    ] {
        if held {
            parts.push(symbol.to_owned());
        }
    }
    if let Some(key) = shortcut_key_label(shortcut.key) {
        parts.push(t(key, cx).to_string());
    }
    parts.join(" ")
}

/// macOS prints the four modifiers as glyphs on the keys themselves; every
/// other platform spells them.
///
/// `cfg!` rather than eight `#[cfg]` definitions, so both spellings are
/// compiled — and asserted — from whichever platform you are on. These are not
/// `Str`: a modifier glyph is the same in every interface language, which is
/// why `i18n_lint`'s scan sees no finding here.
const MODIFIER_CONTROL: &str = if cfg!(target_os = "macos") {
    "⌃"
} else {
    "Ctrl"
};
const MODIFIER_ALT: &str = if cfg!(target_os = "macos") {
    "⌥"
} else {
    "Alt"
};
const MODIFIER_SHIFT: &str = if cfg!(target_os = "macos") {
    "⇧"
} else {
    "Shift"
};
const MODIFIER_META: &str = if cfg!(target_os = "macos") {
    "⌘"
} else {
    "Win"
};

/// `None` for the modifier-only shortcut, whose modifiers are the whole label.
fn shortcut_key_label(key: ShortcutKey) -> Option<Str> {
    Some(match key {
        ShortcutKey::Modifiers => return None,
        ShortcutKey::Space => input_method::Text::ShortcutSpace.into(),
        ShortcutKey::Enter => input_method::Text::ShortcutEnter.into(),
        ShortcutKey::Tab => input_method::Text::ShortcutTab.into(),
        ShortcutKey::Escape => input_method::Text::ShortcutEscape.into(),
        ShortcutKey::Backspace => input_method::Text::ShortcutBackspace.into(),
        ShortcutKey::Delete => input_method::Text::ShortcutDelete.into(),
        ShortcutKey::Home => input_method::Text::ShortcutHome.into(),
        ShortcutKey::End => input_method::Text::ShortcutEnd.into(),
        ShortcutKey::PageUp => input_method::Text::ShortcutPageUp.into(),
        ShortcutKey::PageDown => input_method::Text::ShortcutPageDown.into(),
        ShortcutKey::ArrowLeft => input_method::Text::ShortcutArrowLeft.into(),
        ShortcutKey::ArrowRight => input_method::Text::ShortcutArrowRight.into(),
        ShortcutKey::ArrowUp => input_method::Text::ShortcutArrowUp.into(),
        ShortcutKey::ArrowDown => input_method::Text::ShortcutArrowDown.into(),
    })
}

/// The page's root, empty. This is the longest tool page dodo has — every row
/// below is conditional on the platform, the selected backend or a storage
/// problem, and the tallest combination does not fit the smallest window — so
/// it is the one page whose height has to be its **own**, not the pane's.
///
/// It is `w_full` and deliberately not `size_full`: a height of 100% is a
/// definite height, and a page pinned to the pane cannot report the overflow
/// the main pane's scroll container exists to reveal — the rows past the
/// bottom edge are just clipped. `layout::tool_box` is the other half of this;
/// neither half is any use alone.
fn page_root() -> Div {
    v_flex().w_full()
}

impl Render for InputMethodView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let root = page_root()
            .gap_4()
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(t(Self::description(), cx)),
            )
            .child(Self::row(
                input_method::Text::Backend.into(),
                input_method::Text::BackendDescription.into(),
                Self::backend_choice(cx),
                cx,
            ));
        #[cfg(target_os = "macos")]
        let root = root
            .when(InputMethod::backend(cx) == Backend::Native, |this| {
                this.child(Self::status_card(cx))
            })
            .when(InputMethod::backend(cx) == Backend::EventTap, |this| {
                this.child(Self::event_tap_status_card(cx)).child(Self::row(
                    input_method::Text::BrowserFix.into(),
                    input_method::Text::BrowserFixDescription.into(),
                    Self::browser_fix_switch(cx),
                    cx,
                ))
            });
        let root = root
            .child(Self::row(
                input_method::Text::ActiveLanguages.into(),
                input_method::Text::ActiveLanguagesDescription.into(),
                Self::active_languages_choice(cx),
                cx,
            ))
            .child(Self::row(
                tray::Text::KeyboardInput.into(),
                input_method::Text::LanguageDescription.into(),
                Self::language_choice(cx),
                cx,
            ))
            .child(self.language_switch_row(cx));
        #[cfg(target_os = "windows")]
        let root = root
            .when(InputMethod::backend(cx) == Backend::Native, |this| {
                this.child(Self::windows_tsf_status_card(cx))
            })
            .when(InputMethod::backend(cx) == Backend::KeyboardHook, |this| {
                this.child(Self::keyboard_hook_status_card(cx))
            });
        root.when_some(InputMethod::store_error(cx), |this, problem| {
            this.child(Self::storage_problem(problem, cx))
        })
        .child(
            v_flex()
                .gap_1()
                .child(Self::row(
                    input_method::Text::Scheme.into(),
                    input_method::Text::SchemeDescription.into(),
                    Self::scheme_choice(cx),
                    cx,
                ))
                .child(Self::row(
                    input_method::Text::TonePlacement.into(),
                    input_method::Text::TonePlacementDescription.into(),
                    Self::tone_choice(cx),
                    cx,
                ))
                .child(Self::row(
                    input_method::Text::SpellCheck.into(),
                    input_method::Text::SpellCheckDescription.into(),
                    Self::spell_check_switch(cx),
                    cx,
                ))
                .child(Self::row(
                    input_method::Text::BracketShortcuts.into(),
                    input_method::Text::BracketShortcutsDescription.into(),
                    Self::bracket_shortcuts_switch(cx),
                    cx,
                )),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{
        RECORDER_CONTEXT, page_root, recordable_key, recorded_modifiers, shortcut_key_label,
    };
    use dodo_ime_ipc::settings::{Shortcut, ShortcutKey, ShortcutModifiers};
    use gpui::{KeyBindingContextPredicate, KeyContext, Length, Modifiers, Styled as _, relative};

    /// The captain's report: at a small window the last settings on this page
    /// could not be reached. The cause was not a missing scroll container —
    /// `layout::main_pane` has always been one — but this page reporting the
    /// pane's height as its own, which leaves nothing to scroll and clips the
    /// rows past the bottom edge instead. `layout::tool_box` is the other half.
    ///
    /// Asserted on the style because a gpui layout needs a window to compute.
    #[test]
    fn the_page_reports_its_own_height_so_a_small_window_can_scroll_it() {
        let mut root = page_root();
        let style = root.style();

        assert_eq!(
            style.size.height, None,
            "`size_full` here is a height of 100% of the pane, and a page that \
             is exactly the pane can never overflow it",
        );
        assert_eq!(
            style.size.width,
            Some(Length::from(relative(1.))),
            "the width is the half that must not regress: rows and rules still \
             reach the pane's edge",
        );
    }

    /// A key pressed at the recorder must be *recorded* and never also *obeyed*.
    /// `quick_nav::NORMAL_MODE` is the binding set that would otherwise take
    /// `⌘V`, `p` and `Esc` out from under it. It lives in the binary, which a
    /// crate cannot read, so this uses [`crate::QUICK_NAV_NORMAL_MODE`] — the
    /// mirror dodo's own test keeps honest.
    #[test]
    fn a_recording_field_suppresses_every_normal_mode_binding() {
        let path = |contexts: &[&str]| -> Vec<KeyContext> {
            contexts
                .iter()
                .map(|name| KeyContext::parse(name).expect("a bare identifier parses"))
                .collect()
        };
        let normal_mode = KeyBindingContextPredicate::parse(crate::QUICK_NAV_NORMAL_MODE)
            .expect("the predicate has to parse, or `KeyBinding::new` panics at startup");

        let idle = path(&["Root", crate::QUICK_NAV_KEY_CONTEXT, "InputMethod"]);
        assert!(
            normal_mode.depth_of(&idle).is_some(),
            "an idle pane is still normal mode"
        );

        let recording = path(&[
            "Root",
            crate::QUICK_NAV_KEY_CONTEXT,
            "InputMethod",
            RECORDER_CONTEXT,
        ]);
        assert!(
            normal_mode.depth_of(&recording).is_none(),
            "a recording field must take every key for itself"
        );
    }

    /// gpui's `platform` is Command on macOS and the Windows key on Windows, and
    /// both must land in `meta` — the field every host normalizes into. A
    /// shortcut recorded on one platform is then the same document on the other.
    #[test]
    fn command_and_the_windows_key_are_both_recorded_as_meta() {
        assert_eq!(
            recorded_modifiers(&Modifiers {
                platform: true,
                ..Modifiers::none()
            }),
            ShortcutModifiers {
                meta: true,
                ..ShortcutModifiers::NONE
            }
        );
        assert_eq!(
            recorded_modifiers(&Modifiers {
                alt: true,
                ..Modifiers::none()
            }),
            ShortcutModifiers {
                alt: true,
                ..ShortcutModifiers::NONE
            },
            "Option and Alt are one field, and it is not meta"
        );
        assert_eq!(
            recorded_modifiers(&Modifiers {
                control: true,
                shift: true,
                ..Modifiers::none()
            }),
            ShortcutModifiers {
                control: true,
                shift: true,
                ..ShortcutModifiers::NONE
            }
        );
        // Function is not a command modifier and must not become one.
        assert_eq!(
            recorded_modifiers(&Modifiers {
                function: true,
                ..Modifiers::none()
            }),
            ShortcutModifiers::NONE
        );
    }

    /// Every key the recorder accepts must be one a host can name again, and
    /// every key a shortcut can hold must be recordable. The two directions
    /// together are what stop a stored shortcut that never fires.
    #[test]
    fn the_recorder_and_the_shortcut_vocabulary_agree() {
        let mut recorded = Vec::new();
        for name in [
            "space",
            "enter",
            "tab",
            "escape",
            "backspace",
            "delete",
            "home",
            "end",
            "pageup",
            "pagedown",
            "left",
            "right",
            "up",
            "down",
        ] {
            let key = recordable_key(name).unwrap_or_else(|| panic!("{name} is not recordable"));
            assert!(shortcut_key_label(key).is_some(), "{name} has no label");
            recorded.push(key);
        }
        for key in ShortcutKey::ALL {
            // The modifier-only shortcut is recorded from a modifier release,
            // not from a key name, and its label is its modifiers.
            if key == ShortcutKey::Modifiers {
                assert_eq!(shortcut_key_label(key), None);
                continue;
            }
            assert!(recorded.contains(&key), "{key:?} cannot be recorded");
        }
    }

    /// A printing key is refused rather than stored, because the host is handed
    /// what the key *types* and `⌥Z` types `Ω`. The row says so out loud instead
    /// of saving a shortcut that would never fire.
    ///
    /// The four modifier names are refused here too, and for a different
    /// reason: gpui *synthesizes* a key-down for a single modifier tapped on its
    /// own, so `⇧` alone arrives at the recorder twice — once as that keystroke
    /// and once as the release. Neither may store it, because a shortcut of one
    /// modifier fires on the way into every `⌘C`. `modifiers_recorded` is where
    /// a two-modifier combination is captured instead.
    #[test]
    fn a_printing_key_is_never_recorded() {
        for name in [
            "a", "z", "1", "f5", "ç", "", "shift", "control", "alt", "platform", "function",
        ] {
            assert_eq!(recordable_key(name), None, "{name:?}");
        }
    }

    /// The recorder never stores a combination that could fire while typing,
    /// which is the same rule the file's parser applies to a stored one.
    #[test]
    fn a_recorded_combination_must_be_a_valid_shortcut() {
        let space = recordable_key("space").unwrap();
        assert!(
            !Shortcut {
                modifiers: ShortcutModifiers::NONE,
                key: space,
            }
            .is_valid()
        );
        assert!(
            !Shortcut {
                modifiers: ShortcutModifiers {
                    shift: true,
                    ..ShortcutModifiers::NONE
                },
                key: space,
            }
            .is_valid()
        );
        assert!(
            Shortcut {
                modifiers: recorded_modifiers(&Modifiers {
                    platform: true,
                    ..Modifiers::none()
                }),
                key: space,
            }
            .is_valid()
        );
    }
}
