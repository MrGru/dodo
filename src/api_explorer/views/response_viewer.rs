//! The response half: status metadata, response tabs, and the body.

use std::time::Duration;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    Context, Entity, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, div, px,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::Input;
use gpui_component::tab::{Tab, TabBar};
use gpui_component::tag::Tag;
use gpui_component::{
    ActiveTheme as _, Selectable as _, Sizable as _, StyledExt as _, h_flex, v_flex,
};

use crate::api_explorer::components::empty_state::empty_state;
use crate::api_explorer::components::later_step::later_step;
use crate::api_explorer::components::status_tag::{metric, status_tag};
use crate::api_explorer::models::exchange::{StatusClass, format_duration, format_size};
use crate::api_explorer::state::response::{BodyView, Outcome, ResponseTab};
use crate::api_explorer::state::tab::RequestTabState;
use crate::api_explorer::views::explorer::ApiExplorer;
use crate::app_icon::AppIcon;
use crate::i18n::{Str, t};

impl ApiExplorer {
    pub(super) fn render_response_viewer(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let Some(tab) = self.active_tab().cloned() else {
            return div().size_full().into_any_element();
        };

        let state = tab.read(cx);
        let collapsed = state.response.collapsed;

        v_flex()
            .size_full()
            .border_t_1()
            .border_color(cx.theme().border)
            .child(self.response_meta_row(&tab, cx))
            .when(!collapsed, |this| {
                this.child(self.response_tab_bar(&tab, cx)).child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .child(self.response_pane(&tab, cx)),
                )
            })
            .into_any_element()
    }

    /// Status, class caption, timing, size, and the collapse control.
    fn response_meta_row(
        &self,
        tab: &Entity<RequestTabState>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let state = tab.read(cx);
        let collapsed = state.response.collapsed;
        let tab = tab.clone();

        let summary = match &state.response.outcome {
            Outcome::Idle => h_flex()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(t(Str::NoResponseYet, cx))
                .into_any_element(),
            Outcome::InFlight => h_flex()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(t(Str::Sending, cx))
                .into_any_element(),
            Outcome::Failed(_) => h_flex()
                .items_center()
                .gap_2()
                .child(
                    Tag::custom(
                        cx.theme().danger.opacity(0.15),
                        cx.theme().danger,
                        cx.theme().danger.opacity(0.4),
                    )
                    .small()
                    .rounded(cx.theme().radius)
                    .child(div().font_bold().child(t(Str::RequestFailed, cx))),
                )
                .into_any_element(),
            // Copied out by value: `exchange` borrows from `state`, which
            // borrows `cx`, and the summary needs `cx` mutably.
            Outcome::Received(exchange) => {
                let (status, class, elapsed, size) = (
                    exchange.status,
                    exchange.status_class(),
                    exchange.elapsed,
                    exchange.size_bytes,
                );
                Self::received_summary(status, class, elapsed, size, cx)
            }
        };

        h_flex()
            .w_full()
            .items_center()
            .justify_between()
            .h(px(38.))
            .px_3()
            .gap_2()
            .child(summary)
            .child(
                Button::new("toggle-response")
                    .ghost()
                    .xsmall()
                    .icon(AppIcon::PanelBottom)
                    .tooltip(t(
                        if collapsed {
                            Str::ExpandResponse
                        } else {
                            Str::CollapseResponse
                        },
                        cx,
                    ))
                    .on_click(cx.listener(move |_, _, _, cx| {
                        tab.update(cx, |state, cx| {
                            state.response.collapsed = !state.response.collapsed;
                            cx.notify();
                        });
                        cx.notify();
                    })),
            )
    }

    /// The status badge, class caption, duration and size of a real response.
    fn received_summary(
        status: u16,
        class: StatusClass,
        elapsed: Duration,
        size_bytes: usize,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        h_flex()
            .items_center()
            .gap_3()
            .child(status_tag(status, class, cx))
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(t(class.label(), cx)),
            )
            .child(metric(AppIcon::Clock, format_duration(elapsed).into(), cx))
            .child(metric(AppIcon::HardDrive, format_size(size_bytes).into(), cx))
            .into_any_element()
    }

    fn response_tab_bar(
        &self,
        tab: &Entity<RequestTabState>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let state = tab.read(cx);
        let active = state.response.active_tab;
        let header_count = state.response.header_count();
        let body_view = state.response.body_view;
        let has_body = state.response.exchange().is_some();
        let selected = ResponseTab::ALL
            .iter()
            .position(|candidate| *candidate == active)
            .unwrap_or(0);

        let switch_tab = tab.clone();

        // The tab strip gets the leftover width and scrolls its own tabs when
        // there is not enough of it; the body controls are pinned so that a
        // narrow window can never push Copy off the edge.
        h_flex()
            .w_full()
            .min_w_0()
            .items_center()
            .gap_2()
            .px_2()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                // `TabBar` sizes itself to its tabs and has no width of its
                // own, so it needs a slot that both bounds it (`flex_1` +
                // `min_w_0`) and clips it. Without `overflow_hidden` it spills
                // out of the slot and paints over the controls beside it.
                div().flex_1().min_w_0().overflow_hidden().child(
                // Denser than the request strip: this row also carries the
                // Pretty/Raw/Copy controls, and both have to fit dodo's
                // 900px-wide default window without either being clipped.
                TabBar::new("response-panes")
                    .xsmall()
                    .selected_index(selected)
                    .children(ResponseTab::ALL.map(|pane| {
                        let tab = Tab::new().label(t(pane.label(), cx));
                        // The count badge the reference shows beside Headers.
                        if pane == ResponseTab::Headers && header_count > 0 {
                            tab.suffix(
                                Tag::secondary()
                                    .small()
                                    .rounded_full()
                                    .child(format!("{header_count}")),
                            )
                        } else {
                            tab
                        }
                    }))
                    .on_click(cx.listener(move |_, index: &usize, _, cx| {
                        let Some(pane) = ResponseTab::ALL.get(*index).copied() else {
                            return;
                        };
                        switch_tab.update(cx, |state, cx| {
                            state.response.active_tab = pane;
                            cx.notify();
                        });
                        cx.notify();
                    })),
                ),
            )
            .when(active == ResponseTab::Body && has_body, |this| {
                this.child(
                    h_flex()
                        .flex_shrink_0()
                        .child(self.body_controls(tab, body_view, cx)),
                )
            })
    }

    /// Pretty / Raw and Copy.
    fn body_controls(
        &self,
        tab: &Entity<RequestTabState>,
        body_view: BodyView,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let pretty_tab = tab.clone();
        let raw_tab = tab.clone();
        let copy_tab = tab.clone();

        h_flex()
            .items_center()
            .gap_1()
            .child(
                Button::new("body-pretty")
                    .ghost()
                    .xsmall()
                    .selected(body_view == BodyView::Pretty)
                    .label(t(Str::BodyPretty, cx))
                    .on_click(cx.listener(move |_, _, window, cx| {
                        pretty_tab.update(cx, |state, cx| {
                            state.response.body_view = BodyView::Pretty;
                            state.refresh_body(window, cx);
                            cx.notify();
                        });
                        cx.notify();
                    })),
            )
            .child(
                Button::new("body-raw")
                    .ghost()
                    .xsmall()
                    .selected(body_view == BodyView::Raw)
                    .label(t(Str::BodyRaw, cx))
                    .on_click(cx.listener(move |_, _, window, cx| {
                        raw_tab.update(cx, |state, cx| {
                            state.response.body_view = BodyView::Raw;
                            state.refresh_body(window, cx);
                            cx.notify();
                        });
                        cx.notify();
                    })),
            )
            .child(
                Button::new("body-copy")
                    .ghost()
                    .xsmall()
                    .icon(AppIcon::Copy)
                    .tooltip(t(Str::Copy, cx))
                    .on_click(cx.listener(move |_, _, _, cx| {
                        // Copies the whole body, not just the windowed part —
                        // the window is a rendering limit, not a data limit.
                        let body = copy_tab
                            .read(cx)
                            .response
                            .exchange()
                            .map(|exchange| exchange.body.clone());
                        if let Some(body) = body {
                            cx.write_to_clipboard(gpui::ClipboardItem::new_string(body));
                        }
                    })),
            )
    }

    fn response_pane(
        &self,
        tab: &Entity<RequestTabState>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let state = tab.read(cx);

        // A failure replaces the whole pane: there are no headers or body to
        // show, and the reason is the only useful thing on screen.
        if let Outcome::Failed(error) = &state.response.outcome {
            return self.error_banner(error.clone(), cx).into_any_element();
        }

        if state.response.exchange().is_none() {
            return empty_state(
                AppIcon::Send,
                t(Str::NoResponseYet, cx),
                Some(t(Str::NoResponseHint, cx)),
                cx,
            )
            .into_any_element();
        }

        let pane = state.response.active_tab;
        if !pane.is_implemented() {
            return later_step(
                AppIcon::SquareCode,
                t(pane.label(), cx),
                t(Str::ArrivesLater, cx),
                cx,
            )
            .into_any_element();
        }

        match pane {
            ResponseTab::Headers => self.headers_pane(tab, cx).into_any_element(),
            // Body is the default pane, and the only other implemented one.
            _ => self.body_pane(tab, cx).into_any_element(),
        }
    }

    /// The same calm banner the other tools use for errors.
    fn error_banner(&self, error: Str, cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().p_3().child(
            div()
                .rounded(cx.theme().radius)
                .border_1()
                .border_color(cx.theme().danger)
                .bg(cx.theme().danger.opacity(0.1))
                .text_color(cx.theme().danger)
                .text_sm()
                .px_3()
                .py_2()
                .child(t(error, cx)),
        )
    }

    fn body_pane(
        &self,
        tab: &Entity<RequestTabState>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let state = tab.read(cx);
        let body = state.response.body.clone();
        let shown = state.response.visible_lines.min(state.response.total_lines);
        let total = state.response.total_lines;
        let has_more = state.response.has_more_lines();
        let truncated = state
            .response
            .exchange()
            .is_some_and(|exchange| exchange.truncated);
        let more_tab = tab.clone();

        v_flex()
            .size_full()
            .child(
                div().flex_1().min_h_0().child(
                    Input::new(&body)
                        .font_family(cx.theme().mono_font_family.clone())
                        .text_size(cx.theme().mono_font_size)
                        .size_full(),
                ),
            )
            .child(
                // The footer states exactly what is on screen and what is not,
                // so a capped render is never a silent one.
                h_flex()
                    .w_full()
                    .items_center()
                    .justify_between()
                    .px_3()
                    .py_1()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(div().child(t(Str::LineRange { shown, total }, cx)))
                    .child(
                        h_flex()
                            .items_center()
                            .gap_2()
                            .when(truncated, |this| {
                                this.child(div().child(t(Str::BodyTruncated, cx)))
                            })
                            .when(has_more, |this| {
                                this.child(
                                    Button::new("load-more-lines")
                                        .ghost()
                                        .xsmall()
                                        .label(t(Str::LoadMoreLines, cx))
                                        .on_click(cx.listener(move |_, _, window, cx| {
                                            more_tab.update(cx, |state, cx| {
                                                state.response.show_more_lines();
                                                state.refresh_body(window, cx);
                                                cx.notify();
                                            });
                                            cx.notify();
                                        })),
                                )
                            }),
                    ),
            )
    }

    fn headers_pane(
        &self,
        tab: &Entity<RequestTabState>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let state = tab.read(cx);
        let headers = state
            .response
            .exchange()
            .map(|exchange| exchange.headers.clone())
            .unwrap_or_default();

        div()
            .id("response-headers")
            .size_full()
            .overflow_y_scroll()
            .child(
                v_flex()
                    .w_full()
                    .children(headers.into_iter().enumerate().map(|(index, (name, value))| {
                        h_flex()
                            .w_full()
                            .items_start()
                            .gap_3()
                            .px_3()
                            .py_1p5()
                            .border_b_1()
                            .border_color(cx.theme().border.opacity(0.5))
                            .text_xs()
                            .font_family(cx.theme().mono_font_family.clone())
                            .when(index % 2 == 1, |this| this.bg(cx.theme().list_even))
                            .child(
                                div()
                                    .w(px(220.))
                                    .flex_shrink_0()
                                    .font_bold()
                                    .child(name),
                            )
                            .child(div().flex_1().min_w_0().child(value))
                    })),
            )
    }
}
