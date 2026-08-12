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
//! selected. Every control writes `input-method.json`; [`crate::input_method`]
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

use crate::i18n::{Str, t};
use crate::input_method::InputMethod;
#[cfg(target_os = "macos")]
use crate::input_method::Install;
#[cfg(target_os = "macos")]
use crate::input_method::models::event_tap::EventTapStatus;
#[cfg(target_os = "windows")]
use crate::input_method::models::keyboard_hook::KeyboardHookStatus;
#[cfg(target_os = "macos")]
use crate::input_method::models::status::status_message;
#[cfg(target_os = "windows")]
use crate::input_method::models::windows::{
    WindowsInstall, WindowsInstallFailure, WindowsInstallOutcome,
};

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

    #[cfg(target_os = "macos")]
    const BACKENDS: [Backend; 2] = [Backend::Native, Backend::EventTap];
    #[cfg(target_os = "windows")]
    const BACKENDS: [Backend; 2] = [Backend::Native, Backend::KeyboardHook];

    /// The one sentence about the input method's standing state.
    ///
    /// Assembles the four arguments [`status_message`] decides from and does
    /// nothing else. `describes_a_live_process` is the one call that cannot move
    /// into the model: it is `kill(pid, 0)`, a syscall, and the model is pure.
    #[cfg(target_os = "macos")]
    fn status_line(cx: &App) -> Str {
        let status = InputMethod::status(cx);
        let running = status
            .as_ref()
            .filter(|status| status.describes_a_live_process())
            .map(|status| status.bundle_version.clone());

        status_message(
            &InputMethod::install_state(cx),
            InputMethod::is_installed(cx),
            InputMethod::settings_applied(cx),
            running.as_deref(),
        )
    }

    /// Install, or reinstall, or nothing while one is running.
    #[cfg(target_os = "macos")]
    fn install_button(cx: &App) -> Button {
        let running = InputMethod::install_state(cx) == Install::Running;
        let label = if InputMethod::is_installed(cx) {
            Str::InputMethodReinstall
        } else {
            Str::InputMethodInstall
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
                    .child(div().font_bold().child(t(Str::InputMethodStatus, cx)))
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(t(Self::status_line(cx), cx)),
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
                    .child(t(Str::InputMethodStorageProblem, cx)),
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

    #[cfg(target_os = "macos")]
    fn description() -> Str {
        Str::InputMethodDescription
    }

    #[cfg(target_os = "windows")]
    fn description() -> Str {
        Str::InputMethodWindowsDescription
    }

    fn backend_label(backend: Backend) -> Str {
        match backend {
            Backend::Native => {
                #[cfg(target_os = "macos")]
                {
                    Str::InputMethodNative
                }
                #[cfg(target_os = "windows")]
                {
                    Str::InputMethodNativeTsf
                }
            }
            Backend::EventTap => Str::InputMethodEventTap,
            Backend::KeyboardHook => Str::InputMethodKeyboardHook,
        }
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

    #[cfg(target_os = "macos")]
    fn event_tap_status_line(cx: &App) -> Str {
        match InputMethod::event_tap_status(cx) {
            EventTapStatus::Inactive => Str::InputMethodEventTapInactive,
            EventTapStatus::WaitingForNative => Str::InputMethodEventTapWaitingForNative,
            EventTapStatus::NeedsAccessibility => Str::InputMethodEventTapNeedsAccessibility,
            EventTapStatus::Running => Str::InputMethodEventTapRunning,
            EventTapStatus::Failed => Str::InputMethodEventTapFailed,
        }
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
                            .child(t(Str::InputMethodEventTapStatus, cx)),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(t(Self::event_tap_status_line(cx), cx)),
                    ),
            )
    }

    #[cfg(target_os = "windows")]
    fn windows_tsf_status_line(cx: &App) -> Str {
        match InputMethod::windows_install_state(cx) {
            WindowsInstall::Installing => Str::InputMethodInstalling,
            WindowsInstall::Uninstalling => Str::InputMethodUninstalling,
            WindowsInstall::Done(WindowsInstallOutcome::Ready) => {
                Str::InputMethodWindowsTsfInstalled
            }
            WindowsInstall::Done(WindowsInstallOutcome::Removed) => {
                Str::InputMethodWindowsTsfRemoved
            }
            WindowsInstall::Done(WindowsInstallOutcome::Failed(
                WindowsInstallFailure::NoSourceDll,
            )) => Str::InputMethodWindowsTsfNoDll,
            WindowsInstall::Done(WindowsInstallOutcome::Failed(WindowsInstallFailure::Copy {
                detail,
            })) => Str::InputMethodCopyFailed(detail),
            WindowsInstall::Done(WindowsInstallOutcome::Failed(
                WindowsInstallFailure::Register { detail },
            )) => Str::InputMethodWindowsTsfRegisterFailed(detail),
            WindowsInstall::Done(WindowsInstallOutcome::Failed(
                WindowsInstallFailure::Unregister { detail },
            )) => Str::InputMethodWindowsTsfUnregisterFailed(detail),
            WindowsInstall::Idle if InputMethod::is_installed(cx) => {
                Str::InputMethodWindowsTsfInstalled
            }
            WindowsInstall::Idle => Str::InputMethodWindowsTsfNotInstalled,
        }
    }

    #[cfg(target_os = "windows")]
    fn windows_tsf_status_card(cx: &App) -> impl IntoElement {
        let running = matches!(
            InputMethod::windows_install_state(cx),
            WindowsInstall::Installing | WindowsInstall::Uninstalling
        );
        let install_label = if InputMethod::is_installed(cx) {
            Str::InputMethodReinstall
        } else {
            Str::InputMethodInstall
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
                            .child(t(Str::InputMethodWindowsTsfStatus, cx)),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(t(Self::windows_tsf_status_line(cx), cx)),
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
                            .label(t(Str::InputMethodUninstall, cx))
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
                    .child(div().child(t(Str::InputMethodLanguageSwitch, cx)))
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(t(Str::InputMethodLanguageSwitchDescription, cx)),
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
            t(Str::InputMethodShortcutRecording, cx)
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
                            .child(div().text_sm().child(t(Str::InputMethodShortcutBeep, cx))),
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
            return Some(Str::InputMethodShortcutUnsupportedKey);
        }
        #[cfg(target_os = "macos")]
        if InputMethod::backend(cx) == Backend::Native
            && InputMethod::language_switch(cx).shortcut.key == ShortcutKey::Modifiers
        {
            return Some(Str::InputMethodShortcutNeedsEventTap);
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

    #[cfg(target_os = "windows")]
    fn keyboard_hook_status_line(cx: &App) -> Str {
        match InputMethod::keyboard_hook_status(cx) {
            KeyboardHookStatus::Inactive => Str::InputMethodKeyboardHookInactive,
            KeyboardHookStatus::Running => Str::InputMethodKeyboardHookRunning,
            KeyboardHookStatus::Failed => Str::InputMethodKeyboardHookFailed,
        }
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
                            .child(t(Str::InputMethodKeyboardHookStatus, cx)),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(t(Self::keyboard_hook_status_line(cx), cx)),
                    ),
            )
    }

    /// Telex or VNI. The labels are proper nouns and identical in every
    /// language, so `Str::InputMethodTelex` and `…Vni` both answer `_`.
    fn scheme_choice(cx: &App) -> impl IntoElement {
        let selected = Scheme::ALL
            .iter()
            .position(|scheme| *scheme == InputMethod::settings(cx).scheme);

        RadioGroup::horizontal("input-method-scheme")
            .children(Scheme::ALL.map(|scheme| {
                t(
                    match scheme {
                        Scheme::Telex => Str::InputMethodTelex,
                        Scheme::Vni => Str::InputMethodVni,
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
                        Tone::Modern => Str::InputMethodToneModern,
                        Tone::Traditional => Str::InputMethodToneTraditional,
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

/// macOS prints the four modifiers as glyphs on the keys themselves.
#[cfg(target_os = "macos")]
const MODIFIER_CONTROL: &str = "⌃";
#[cfg(target_os = "macos")]
const MODIFIER_ALT: &str = "⌥";
#[cfg(target_os = "macos")]
const MODIFIER_SHIFT: &str = "⇧";
#[cfg(target_os = "macos")]
const MODIFIER_META: &str = "⌘";
#[cfg(not(target_os = "macos"))]
const MODIFIER_CONTROL: &str = "Ctrl";
#[cfg(not(target_os = "macos"))]
const MODIFIER_ALT: &str = "Alt";
#[cfg(not(target_os = "macos"))]
const MODIFIER_SHIFT: &str = "Shift";
#[cfg(not(target_os = "macos"))]
const MODIFIER_META: &str = "Win";

/// `None` for the modifier-only shortcut, whose modifiers are the whole label.
fn shortcut_key_label(key: ShortcutKey) -> Option<Str> {
    Some(match key {
        ShortcutKey::Modifiers => return None,
        ShortcutKey::Space => Str::InputMethodShortcutSpace,
        ShortcutKey::Enter => Str::InputMethodShortcutEnter,
        ShortcutKey::Tab => Str::InputMethodShortcutTab,
        ShortcutKey::Escape => Str::InputMethodShortcutEscape,
        ShortcutKey::Backspace => Str::InputMethodShortcutBackspace,
        ShortcutKey::Delete => Str::InputMethodShortcutDelete,
        ShortcutKey::Home => Str::InputMethodShortcutHome,
        ShortcutKey::End => Str::InputMethodShortcutEnd,
        ShortcutKey::PageUp => Str::InputMethodShortcutPageUp,
        ShortcutKey::PageDown => Str::InputMethodShortcutPageDown,
        ShortcutKey::ArrowLeft => Str::InputMethodShortcutArrowLeft,
        ShortcutKey::ArrowRight => Str::InputMethodShortcutArrowRight,
        ShortcutKey::ArrowUp => Str::InputMethodShortcutArrowUp,
        ShortcutKey::ArrowDown => Str::InputMethodShortcutArrowDown,
    })
}

impl Render for InputMethodView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let root = v_flex()
            .size_full()
            .gap_4()
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(t(Self::description(), cx)),
            )
            .child(Self::row(
                Str::InputMethodBackend,
                Str::InputMethodBackendDescription,
                Self::backend_choice(cx),
                cx,
            ));
        #[cfg(target_os = "macos")]
        let root = root
            .when(InputMethod::backend(cx) == Backend::Native, |this| {
                this.child(Self::status_card(cx))
            })
            .when(InputMethod::backend(cx) == Backend::EventTap, |this| {
                this.child(Self::event_tap_status_card(cx))
            });
        let root = root
            .child(Self::row(
                Str::InputMethodActiveLanguages,
                Str::InputMethodActiveLanguagesDescription,
                Self::active_languages_choice(cx),
                cx,
            ))
            .child(Self::row(
                Str::TrayKeyboardInput,
                Str::InputMethodLanguageDescription,
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
                    Str::InputMethodScheme,
                    Str::InputMethodSchemeDescription,
                    Self::scheme_choice(cx),
                    cx,
                ))
                .child(Self::row(
                    Str::InputMethodTonePlacement,
                    Str::InputMethodTonePlacementDescription,
                    Self::tone_choice(cx),
                    cx,
                ))
                .child(Self::row(
                    Str::InputMethodSpellCheck,
                    Str::InputMethodSpellCheckDescription,
                    Self::spell_check_switch(cx),
                    cx,
                ))
                .child(Self::row(
                    Str::InputMethodBracketShortcuts,
                    Str::InputMethodBracketShortcutsDescription,
                    Self::bracket_shortcuts_switch(cx),
                    cx,
                )),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{RECORDER_CONTEXT, recordable_key, recorded_modifiers, shortcut_key_label};
    use dodo_ime_ipc::settings::{Shortcut, ShortcutKey, ShortcutModifiers};
    use gpui::{KeyBindingContextPredicate, KeyContext, Modifiers};

    /// A key pressed at the recorder must be *recorded* and never also *obeyed*.
    /// `quick_nav::NORMAL_MODE` is the binding set that would otherwise take
    /// `⌘V`, `p` and `Esc` out from under it.
    #[test]
    fn a_recording_field_suppresses_every_normal_mode_binding() {
        let path = |contexts: &[&str]| -> Vec<KeyContext> {
            contexts
                .iter()
                .map(|name| KeyContext::parse(name).expect("a bare identifier parses"))
                .collect()
        };
        let normal_mode = KeyBindingContextPredicate::parse(crate::quick_nav::NORMAL_MODE)
            .expect("the predicate has to parse, or `KeyBinding::new` panics at startup");

        let idle = path(&["Root", crate::quick_nav::KEY_CONTEXT, "InputMethod"]);
        assert!(
            normal_mode.depth_of(&idle).is_some(),
            "an idle pane is still normal mode"
        );

        let recording = path(&[
            "Root",
            crate::quick_nav::KEY_CONTEXT,
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
