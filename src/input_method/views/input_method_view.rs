//! The Input method tool: install dodo's input method, and tell it how to type.
//!
//! This is the whole surface. It was a Settings page until the captain asked on
//! 2026-08-09 for it to be a feature instead, and **nothing was left behind** —
//! the install button and the four engine settings are here and nowhere else, so
//! there is no control a user can reach from two places and no second answer to
//! "where do I set my input scheme".
//!
//! # It holds no state, on purpose
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
//! # Nothing here starts, stops or talks to the input method
//!
//! dodo cannot: macOS launches the bundle out of `~/Library/Input Methods` and
//! it keeps typing with dodo closed. Every control writes `input-method.json` and
//! posts one notification; [`crate::input_method`] is where that happens and why
//! the order is what it is. The status line is
//! [`models::status::status_message`](crate::input_method::models::status), which
//! is pure and tested — this file only assembles its four arguments.

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::radio::RadioGroup;
use gpui_component::switch::Switch;
use gpui_component::{ActiveTheme, Disableable as _, StyledExt as _, h_flex, v_flex};

use dodo_ime_ipc::settings::{Scheme, Tone};

use crate::i18n::{Str, t};
use crate::input_method::models::status::status_message;
use crate::input_method::{InputMethod, Install};

/// The Input method pane. A unit struct rather than one with fields — see the
/// module docs for why holding anything here would be a defect rather than an
/// optimisation.
pub struct InputMethodView;

impl InputMethodView {
    /// Takes `window` it does not use, because every tool's constructor has this
    /// signature and `Layout::new` calls them all the same way.
    pub fn new(_window: &mut Window, _cx: &mut Context<Self>) -> Self {
        Self
    }

    /// The one sentence about the input method's standing state.
    ///
    /// Assembles the four arguments [`status_message`] decides from and does
    /// nothing else. `describes_a_live_process` is the one call that cannot move
    /// into the model: it is `kill(pid, 0)`, a syscall, and the model is pure.
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

impl Render for InputMethodView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .gap_4()
            // The one thing worth saying about the whole tool, said once at the
            // top: what an input method *is* here, and that it outlives dodo.
            // Every row below is then about one decision rather than repeating
            // the premise.
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(t(Str::InputMethodDescription, cx)),
            )
            .child(Self::status_card(cx))
            .when_some(InputMethod::store_error(cx), |this, problem| {
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
