//! Read-only confirmation for the exact generated mutation batch.
//!
//! Nothing reaches a driver until the Commit button in this dialog is pressed.
//! SQL and bound values are shown separately because execution keeps the values
//! bound; the diagnostic rendering is never interpolated back into SQL.

use std::sync::Arc;

use gpui::{
    App, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, SharedString, StatefulInteractiveElement as _, Styled as _, Window,
    div, px,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::{
    ActiveTheme as _, Sizable as _, StyledExt as _, WindowExt as _, h_flex, v_flex,
};

use crate::database::components::notice::{Tone, notice};
use crate::database::models::statement::{GeneratedBatch, display_parameter, placeholder};
use crate::database::services::Driver;
use crate::database::views::database::{DatabaseView, MutationTarget};
use crate::i18n::{Str, t};

const WIDTH: gpui::Pixels = px(760.);
const PADDING: gpui::Pixels = px(32.);

pub(super) fn open(
    page: Entity<DatabaseView>,
    driver: Arc<dyn Driver>,
    batch: GeneratedBatch,
    target: MutationTarget,
    window: &mut Window,
    cx: &mut App,
) {
    let view = cx.new(|_| CommitDialog {
        page,
        driver,
        batch,
        target,
    });
    let body = view.clone();
    window.open_dialog(cx, move |dialog, _, cx| {
        dialog.title(t(Str::DbCommitTitle, cx)).w(WIDTH).content({
            let body = body.clone();
            move |content, _, _| content.child(div().w(WIDTH - PADDING).child(body.clone()))
        })
    });
}

struct CommitDialog {
    page: Entity<DatabaseView>,
    driver: Arc<dyn Driver>,
    batch: GeneratedBatch,
    target: MutationTarget,
}

impl Render for CommitDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let expected = self.batch.expected_rows();
        let page = self.page.clone();
        let driver = self.driver.clone();
        let batch = self.batch.clone();
        let target = self.target.clone();

        v_flex()
            .w_full()
            .gap_3()
            .child(notice(
                Tone::Warning,
                t(Str::DbCommitLostUpdateNotice, cx),
                cx,
            ))
            .child(div().text_sm().child(t(Str::DbCommitSummary(expected), cx)))
            .child(
                div()
                    .text_xs()
                    .font_bold()
                    .text_color(cx.theme().muted_foreground)
                    .child(t(Str::DbCommitExactStatements, cx)),
            )
            .child(
                v_flex()
                    .id("db-commit-statements")
                    .w_full()
                    .max_h(px(360.))
                    .overflow_y_scroll()
                    .rounded(cx.theme().radius)
                    .border_1()
                    .border_color(cx.theme().border)
                    .children(self.batch.statements.iter().enumerate().map(
                        |(index, statement)| {
                            v_flex()
                                .w_full()
                                .gap_1()
                                .p_3()
                                .border_b_1()
                                .border_color(cx.theme().border)
                                .child(
                                    h_flex()
                                        .w_full()
                                        .justify_between()
                                        .child(
                                            div().font_semibold().child(t(
                                                Str::DbCommitStatementLabel(index + 1),
                                                cx,
                                            )),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(cx.theme().muted_foreground)
                                                .child(t(Str::DbExpectedOneRow, cx)),
                                        ),
                                )
                                .child(
                                    div()
                                        .w_full()
                                        .font_family(cx.theme().mono_font_family.clone())
                                        .text_sm()
                                        .child(SharedString::from(statement.sql.clone())),
                                )
                                .children((!statement.params.is_empty()).then(|| {
                                    v_flex()
                                        .w_full()
                                        .gap_1()
                                        .pt_1()
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(cx.theme().muted_foreground)
                                                .child(t(Str::DbCommitParameters, cx)),
                                        )
                                        .children(statement.params.iter().enumerate().map(
                                            |(parameter, value)| {
                                                div()
                                                    .font_family(
                                                        cx.theme().mono_font_family.clone(),
                                                    )
                                                    .text_xs()
                                                    .child(SharedString::from(format!(
                                                        "{} = {}",
                                                        placeholder(
                                                            parameter + 1,
                                                            self.batch.dialect,
                                                        ),
                                                        display_parameter(value)
                                                    )))
                                            },
                                        ))
                                }))
                        },
                    )),
            )
            .child(
                h_flex()
                    .w_full()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("db-commit-cancel")
                            .ghost()
                            .small()
                            .label(t(Str::DbCancel, cx))
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("db-commit-confirm")
                            .danger()
                            .small()
                            .label(t(Str::DbCommit, cx))
                            .on_click(move |_, window, cx| {
                                window.close_dialog(cx);
                                page.update(cx, |page, cx| {
                                    page.start_commit(
                                        driver.clone(),
                                        batch.clone(),
                                        target.clone(),
                                        cx,
                                    );
                                });
                            }),
                    ),
            )
    }
}
