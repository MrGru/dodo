//! The History panel: every request sent this session, newest first, with
//! Reopen, Resend, Duplicate and Delete per entry and Clear All in the header.
//!
//! The list is [`History`](crate::api_explorer::state::history::History) — an
//! in-memory record fed by each tab completing. This file only draws it.

use std::time::SystemTime;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    Context, InteractiveElement as _, IntoElement, ParentElement as _, SharedString,
    StatefulInteractiveElement as _, Styled as _, div, px,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::tag::Tag;
use gpui_component::{ActiveTheme as _, Sizable as _, StyledExt as _, h_flex, v_flex};

use crate::api_explorer::components::empty_state::empty_state;
use crate::api_explorer::components::status_tag::status_tag;
use crate::api_explorer::models::exchange::format_duration;
use crate::api_explorer::state::history::HistoryEntry;
use crate::api_explorer::views::explorer::ApiExplorer;
use crate::app_icon::AppIcon;
use crate::i18n::{Str, t};

impl ApiExplorer {
    pub(super) fn render_history_panel(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        v_flex()
            .size_full()
            .child(self.history_header(cx))
            .child(
                div()
                    .id("history-body")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(if self.history.is_empty() {
                        empty_state(
                            AppIcon::Clock,
                            t(Str::NoHistory, cx),
                            Some(t(Str::NoHistoryHint, cx)),
                            cx,
                        )
                        .into_any_element()
                    } else {
                        let mut rows: Vec<gpui::AnyElement> = Vec::new();
                        for entry in self.history.entries() {
                            rows.push(self.history_row(entry, cx).into_any_element());
                        }
                        v_flex().w_full().py_1().children(rows).into_any_element()
                    }),
            )
            .into_any_element()
    }

    fn history_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let has_entries = !self.history.is_empty();
        h_flex()
            .items_center()
            .justify_between()
            .h(px(38.))
            .px_3()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                div()
                    .text_xs()
                    .font_bold()
                    .text_color(cx.theme().muted_foreground)
                    .child(t(Str::History, cx)),
            )
            .when(has_entries, |this| {
                this.child(
                    Button::new("history-clear-all")
                        .ghost()
                        .xsmall()
                        .label(t(Str::HistoryClearAll, cx))
                        .on_click(cx.listener(|this, _, _, cx| this.clear_history(cx))),
                )
            })
    }

    fn history_row(&self, entry: &HistoryEntry, cx: &mut Context<Self>) -> impl IntoElement {
        let id = entry.id;
        let method = entry.method;
        let url = if entry.url.trim().is_empty() {
            SharedString::new_static("/")
        } else {
            SharedString::from(entry.url.clone())
        };

        // Status badge, or a small "failed" tag when nothing came back.
        let status = match (entry.status, entry.status_class()) {
            (Some(code), Some(class)) => status_tag(code, class, cx).into_any_element(),
            _ => Tag::custom(
                cx.theme().danger.opacity(0.15),
                cx.theme().danger,
                cx.theme().danger.opacity(0.4),
            )
            .small()
            .rounded(cx.theme().radius)
            .child(div().font_bold().child(t(Str::RequestFailed, cx)))
            .into_any_element(),
        };

        v_flex()
            .id(("history-row", id as usize))
            .w_full()
            .gap_1()
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(cx.theme().border.opacity(0.5))
            .hover(|this| this.bg(cx.theme().accent.opacity(0.4)))
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_xs()
                            .font_bold()
                            .text_color(method.color(cx))
                            .child(method.as_str()),
                    )
                    .child(status)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(age_label(entry.at, cx)),
                    ),
            )
            .child(
                div()
                    .w_full()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_sm()
                    .child(url),
            )
            .child(self.history_actions(entry, cx))
    }

    /// The per-entry actions row. Reopen and Resend need a window (they open a
    /// tab); Duplicate and Delete do not.
    fn history_actions(&self, entry: &HistoryEntry, cx: &mut Context<Self>) -> impl IntoElement {
        let id = entry.id;
        let elapsed = entry
            .elapsed
            .map(|elapsed| SharedString::from(format_duration(elapsed)));

        h_flex()
            .w_full()
            .items_center()
            .gap_1()
            .when_some(elapsed, |this, elapsed| {
                this.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(elapsed),
                )
            })
            .child(div().flex_1().min_w_0())
            .child(
                Button::new(("history-reopen", id as usize))
                    .ghost()
                    .xsmall()
                    .icon(AppIcon::SquareCode)
                    .tooltip(t(Str::HistoryReopen, cx))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.reopen_history(id, window, cx);
                    })),
            )
            .child(
                Button::new(("history-resend", id as usize))
                    .ghost()
                    .xsmall()
                    .icon(AppIcon::Send)
                    .tooltip(t(Str::HistoryResend, cx))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.resend_history(id, window, cx);
                    })),
            )
            .child(
                Button::new(("history-duplicate", id as usize))
                    .ghost()
                    .xsmall()
                    .icon(AppIcon::Copy)
                    .tooltip(t(Str::Duplicate, cx))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.duplicate_history(id, cx);
                    })),
            )
            .child(
                Button::new(("history-delete", id as usize))
                    .ghost()
                    .xsmall()
                    .icon(AppIcon::Trash)
                    .tooltip(t(Str::Delete, cx))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.delete_history(id, cx);
                    })),
            )
    }
}

/// How long ago an entry ran, bucketed to the coarsest unit that reads well.
fn age_label(at: SystemTime, cx: &mut Context<ApiExplorer>) -> SharedString {
    let seconds = SystemTime::now()
        .duration_since(at)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0);

    let str = if seconds < 60 {
        Str::HistoryJustNow
    } else if seconds < 3600 {
        Str::HistoryMinutesAgo(seconds / 60)
    } else if seconds < 86400 {
        Str::HistoryHoursAgo(seconds / 3600)
    } else {
        Str::HistoryDaysAgo(seconds / 86400)
    };
    t(str, cx)
}
