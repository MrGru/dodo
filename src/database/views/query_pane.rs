//! The right-hand side: the query editor above, the result below.
//!
//! The two are a `v_resizable` pair with the editor sized and the result taking
//! the rest, because the result is what grows.
//!
//! # The footer is the point of this file
//!
//! It states the row count, the elapsed time, the statement that produced them,
//! and — when the page budget stopped the read — says so in as many words. The
//! text comes from
//! [`Outcome::footer`](crate::database::state::query::Outcome::footer), which is
//! pure and tested, so what the footer claims is checked without a database.

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, Context, IntoElement, ParentElement as _, SharedString, Styled as _, div, px,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::Input;
use gpui_component::resizable::{resizable_panel, v_resizable};
use gpui_component::table::DataTable;
use gpui_component::{
    ActiveTheme as _, Disableable as _, Sizable as _, StyledExt as _, h_flex, v_flex,
};

use crate::app_icon::AppIcon;
use crate::database::components::notice::{Tone, notice};
use crate::database::components::states::{empty_state, error_state};
use crate::database::state::query::QueryState;
use crate::database::views::database::{DatabaseView, EDITOR_HEIGHT, EDITOR_MIN};
use crate::i18n::{Str, t};

impl DatabaseView {
    pub(super) fn render_workspace(&mut self, cx: &mut Context<Self>) -> AnyElement {
        // Nothing is selected but connections exist: the editor would have
        // nowhere to send a statement, so the pane says which choice is
        // missing rather than showing a dead Execute button.
        if self.connections.selected().is_none() && !self.connections.is_empty() {
            return empty_state(
                AppIcon::Database,
                t(Str::DbSelectConnection, cx),
                Some(t(Str::DbSelectConnectionHint, cx)),
                cx,
            )
            .into_any_element();
        }

        let editor = self.render_editor(cx);
        let result = self.render_result(cx);

        v_flex()
            .size_full()
            // `min_w_0` is load-bearing on a flex item beside a resizable
            // panel: without it the widest child — a wide result row — sets
            // this column's width and pushes the divider off the window.
            .min_w_0()
            .child(
                v_resizable("db-rows")
                    .with_state(&self.inner_split)
                    .child(
                        resizable_panel()
                            .size(EDITOR_HEIGHT)
                            .size_range(EDITOR_MIN..px(1200.))
                            .child(editor),
                    )
                    .child(resizable_panel().child(result)),
            )
            .into_any_element()
    }

    fn render_editor(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let connected = self.active_driver().is_some();
        let running = matches!(self.query, QueryState::Running);

        v_flex()
            .size_full()
            .min_w_0()
            .child(
                h_flex()
                    .w_full()
                    .px_2()
                    .py_1p5()
                    .gap_2()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_xs()
                            .font_medium()
                            .text_color(cx.theme().muted_foreground)
                            .child(t(Str::DbQuery, cx)),
                    )
                    .child(
                        h_flex()
                            .gap_1()
                            .child(
                                Button::new("db-format")
                                    .xsmall()
                                    .ghost()
                                    .label(t(Str::DbFormat, cx))
                                    .on_click(
                                        cx.listener(|this, _, window, cx| this.format(window, cx)),
                                    ),
                            )
                            .child(
                                Button::new("db-execute")
                                    .xsmall()
                                    .primary()
                                    .icon(AppIcon::Play)
                                    // Disabled with no connection: there is
                                    // nowhere to send a statement, and a button
                                    // that fails silently teaches nothing.
                                    .disabled(!connected || running)
                                    .label(if running {
                                        t(Str::DbRunning, cx)
                                    } else {
                                        t(Str::DbExecute, cx)
                                    })
                                    .on_click(cx.listener(|this, _, _, cx| this.execute(cx))),
                            ),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(
                        Input::new(&self.editor)
                            .font_family(cx.theme().mono_font_family.clone())
                            .text_size(cx.theme().mono_font_size)
                            .size_full(),
                    ),
            )
            .into_any_element()
    }

    fn render_result(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let body = match &self.query {
            QueryState::Idle => empty_state(
                AppIcon::Table,
                t(Str::DbNoResultYet, cx),
                Some(t(Str::DbNoResultYetHint, cx)),
                cx,
            )
            .into_any_element(),

            QueryState::Running => {
                empty_state(AppIcon::Table, t(Str::DbRunning, cx), None, cx).into_any_element()
            }

            QueryState::Failed(failure) => {
                let statement = failure
                    .statement()
                    .map(|text| SharedString::from(text.to_string()));
                error_state(t(Str::DbStatusError, cx), t(failure.message(), cx), cx)
                    .when_some(statement, |this, statement| {
                        this.child(
                            div()
                                .max_w(px(520.))
                                .text_xs()
                                .font_family(cx.theme().mono_font_family.clone())
                                .text_color(cx.theme().muted_foreground)
                                .child(statement),
                        )
                    })
                    .into_any_element()
            }

            QueryState::Done(outcome) if !outcome.has_grid() => {
                // A statement that changed rows rather than returning them. The
                // footer already says how many; this says there is no grid on
                // purpose, rather than showing an empty one.
                empty_state(AppIcon::Table, t(Str::DbNoRows, cx), None, cx).into_any_element()
            }

            QueryState::Done(_) => div()
                .size_full()
                .min_w_0()
                .child(DataTable::new(&self.table).stripe(true))
                .into_any_element(),
        };

        v_flex()
            .size_full()
            .min_w_0()
            .border_t_1()
            .border_color(cx.theme().border)
            .child(
                div()
                    .w_full()
                    .px_2()
                    .py_1p5()
                    .text_xs()
                    .font_medium()
                    .text_color(cx.theme().muted_foreground)
                    .child(t(Str::DbResult, cx)),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(body),
            )
            .children(self.render_footer(cx))
            .into_any_element()
    }

    /// The footer, or nothing when there is no result to describe.
    fn render_footer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let QueryState::Done(outcome) = &self.query else {
            return None;
        };

        let parts: Vec<SharedString> = outcome
            .footer()
            .into_iter()
            .map(|part| t(part, cx))
            .collect();
        let summary = SharedString::from(
            parts
                .iter()
                .map(|part| part.to_string())
                .collect::<Vec<_>>()
                .join(" · "),
        );

        Some(
            v_flex()
                .w_full()
                .flex_shrink_0()
                .gap_1()
                .px_2()
                .py_1p5()
                .border_t_1()
                .border_color(cx.theme().border)
                .child(
                    h_flex()
                        .w_full()
                        .gap_2()
                        .items_center()
                        .child(
                            div()
                                .flex_shrink_0()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(summary),
                        )
                        .when(outcome.truncated, |this| {
                            // The truncation notice is repeated in the danger
                            // tone rather than only in the run-on summary: it
                            // is the one thing in the footer a user must not
                            // skim past.
                            this.child(div().flex_1().min_w_0().child(notice(
                                Tone::Warning,
                                t(Str::DbFooterTruncated(outcome.rows.len()), cx),
                                cx,
                            )))
                        }),
                )
                // The statement the result came from, so a buffer of several
                // says which one produced these rows.
                .child(
                    h_flex()
                        .w_full()
                        .gap_2()
                        .items_baseline()
                        .child(
                            div()
                                .flex_shrink_0()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(t(Str::DbStatementLabel, cx)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_xs()
                                .truncate()
                                .font_family(cx.theme().mono_font_family.clone())
                                .child(SharedString::from(outcome.statement.clone())),
                        ),
                )
                .into_any_element(),
        )
    }
}
