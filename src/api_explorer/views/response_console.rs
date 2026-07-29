//! The Console tab: what this tab's scripts printed, and what dodo said about
//! running them.
//!
//! Two things it does that a plainer log view would not:
//!
//! - It is reachable **before** any response arrives. A pre-request script that
//!   failed produced no exchange at all, and its output is the only explanation
//!   of why; sending the user to "no response yet" would hide the answer.
//! - It says what it dropped. The buffer is capped (`models::console`), and the
//!   footer states the count rather than letting the top of the log quietly
//!   vanish — the rule `Str::BodyTruncated` already follows.
//!
//! Level chips are the same `Button::selected` idiom the body-view modes use,
//! and they select a *minimum*: picking Warn shows warnings and errors.

use gpui::prelude::FluentBuilder as _;
use gpui::{
    ClipboardItem, Context, Entity, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, div, px,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::{ActiveTheme as _, Selectable as _, Sizable as _, h_flex, v_flex};

use crate::api_explorer::components::empty_state::empty_state;
use crate::api_explorer::models::console::{ConsoleEntry, ConsoleLevel, ConsoleSource};
use crate::api_explorer::state::tab::RequestTabState;
use crate::api_explorer::views::explorer::ApiExplorer;
use crate::app_icon::AppIcon;
use crate::i18n::{Str, t};

impl ApiExplorer {
    pub(super) fn console_pane(
        &self,
        tab: &Entity<RequestTabState>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let state = tab.read(cx);
        let level = state.response.console_level;
        let dropped = state.response.console.dropped();
        // Copied out so `cx` is free to be borrowed mutably while rendering.
        let entries: Vec<ConsoleEntry> = state.response.console.visible(level).cloned().collect();
        let empty = state.response.console.is_empty();

        v_flex()
            .size_full()
            .min_h_0()
            .min_w_0()
            .child(self.console_toolbar(tab, level, cx))
            .child(if empty {
                div()
                    .flex_1()
                    .min_h_0()
                    .child(empty_state(
                        AppIcon::SquareCode,
                        t(Str::ConsoleEmpty, cx),
                        Some(t(Str::ConsoleEmptyHint, cx)),
                        cx,
                    ))
                    .into_any_element()
            } else {
                v_flex()
                    .id("console-lines")
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .overflow_y_scroll()
                    .px_3()
                    .py_2()
                    .text_size(cx.theme().mono_font_size)
                    .font_family(cx.theme().mono_font_family.clone())
                    .children(entries.iter().map(|entry| self.console_line(entry, cx)))
                    .into_any_element()
            })
            .when(dropped > 0, |this| {
                this.child(
                    div()
                        .w_full()
                        .flex_shrink_0()
                        .px_3()
                        .py_1()
                        .border_t_1()
                        .border_color(cx.theme().border)
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(t(Str::ConsoleDropped(dropped), cx)),
                )
            })
            .into_any_element()
    }

    /// The level chips, Copy and Clear.
    fn console_toolbar(
        &self,
        tab: &Entity<RequestTabState>,
        level: ConsoleLevel,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let clear_tab = tab.clone();
        let copy_tab = tab.clone();

        h_flex()
            .w_full()
            .flex_shrink_0()
            .items_center()
            .justify_between()
            .gap_2()
            .px_2()
            .py_1()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                h_flex()
                    .min_w_0()
                    .gap_1()
                    .children(ConsoleLevel::ALL.map(|candidate| {
                        let tab = tab.clone();
                        Button::new(("console-level", candidate as usize))
                            .ghost()
                            .xsmall()
                            .selected(level == candidate)
                            .label(t(candidate.label(), cx))
                            .on_click(cx.listener(move |_, _, _, cx| {
                                tab.update(cx, |state, cx| {
                                    state.response.console_level = candidate;
                                    cx.notify();
                                });
                                cx.notify();
                            }))
                    })),
            )
            .child(
                h_flex()
                    .flex_shrink_0()
                    .gap_1()
                    .child(
                        Button::new("console-copy")
                            .ghost()
                            .xsmall()
                            .icon(AppIcon::Copy)
                            .tooltip(t(Str::Copy, cx))
                            .on_click(cx.listener(move |_, _, _, cx| {
                                let state = copy_tab.read(cx);
                                let text = state
                                    .response
                                    .console
                                    .copy_text(state.response.console_level, |entry| {
                                        line_text(entry, cx)
                                    });
                                if !text.is_empty() {
                                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                                }
                            })),
                    )
                    .child(
                        Button::new("console-clear")
                            .ghost()
                            .xsmall()
                            .label(t(Str::ConsoleClear, cx))
                            .on_click(cx.listener(move |_, _, _, cx| {
                                clear_tab.update(cx, |state, cx| {
                                    state.response.console.clear();
                                    cx.notify();
                                });
                                cx.notify();
                            })),
                    ),
            )
    }

    /// One line: a run rule, or a level-coloured message.
    fn console_line(&self, entry: &ConsoleEntry, cx: &mut Context<Self>) -> gpui::AnyElement {
        let text = line_text(entry, cx);

        if entry.separator {
            return h_flex()
                .w_full()
                .items_center()
                .gap_2()
                .pt_2()
                .pb_1()
                .child(
                    div()
                        .flex_shrink_0()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(text),
                )
                .child(div().flex_1().h(px(1.)).bg(cx.theme().border))
                .into_any_element();
        }

        let colour = match entry.level {
            ConsoleLevel::Error => cx.theme().danger,
            ConsoleLevel::Warn => cx.theme().warning,
            ConsoleLevel::Debug => cx.theme().muted_foreground,
            ConsoleLevel::Log => cx.theme().foreground,
        };

        div()
            .w_full()
            .min_w_0()
            .py_0p5()
            .text_color(colour)
            // A rule down the left of dodo's own lines, so the app's voice is
            // never mistaken for the script's output.
            .when(entry.source == ConsoleSource::Runtime, |this| {
                this.border_l_2().border_color(cx.theme().border).pl_2()
            })
            .child(text)
            .into_any_element()
    }
}

/// One entry as the user reads it.
///
/// dodo's own lines are held as a [`Str`] and rendered here, so a console
/// already on screen re-translates when the language changes; a script's own
/// output is verbatim and has no translation.
fn line_text(entry: &ConsoleEntry, cx: &gpui::App) -> String {
    match &entry.localized {
        Some(str) => t(str.clone(), cx).to_string(),
        None => entry.message.clone(),
    }
}
