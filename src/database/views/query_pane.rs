//! The right-hand side: the tab strip and query editor above, the result below.
//!
//! The two are a `v_resizable` pair with the editor sized and the result taking
//! the rest, because the result is what grows.
//!
//! # The tab strip
//!
//! One row per open query, drawn from
//! [`QueryTabs`](crate::database::state::tabs::QueryTabs). A tab's label is
//! short, fixed-length text, so it is `flex_shrink_0().whitespace_nowrap()`: a
//! `flex_1().min_w_0()` label wraps the moment anything competes for width, and
//! a strip of tabs is exactly that.
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
use gpui_component::tab::{Tab, TabBar};
use gpui_component::table::DataTable;
use gpui_component::{
    ActiveTheme as _, Disableable as _, Sizable as _, StyledExt as _, h_flex, v_flex,
};

use crate::app_icon::AppIcon;
use crate::database::components::notice::{Tone, notice};
use crate::database::components::states::{empty_state, error_state};
use crate::database::services::export::ExportFormat;
use crate::database::state::query::QueryState;
use crate::database::views::database::{DatabaseView, EDITOR_HEIGHT, EDITOR_MIN};
use crate::database::views::result_grid;
use crate::i18n::{Str, t};

impl DatabaseView {
    pub(super) fn render_workspace(&mut self, cx: &mut Context<Self>) -> AnyElement {
        if self.detail.is_some() {
            return self.render_object_detail(cx);
        }

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

    /// The strip of open queries above the editor.
    fn render_tab_strip(&self, cx: &mut Context<Self>) -> AnyElement {
        let active = self.tabs.active_index();
        let closable = self.tabs.len() > 1;

        let tabs: Vec<Tab> = self
            .tabs
            .tabs()
            .iter()
            .enumerate()
            .map(|(index, tab)| {
                let running = tab.is_running();
                Tab::new()
                    .px_2()
                    .label(t(Str::DbQueryTabTitle(tab.number), cx))
                    .suffix(
                        h_flex()
                            .items_center()
                            .gap_1()
                            .when(running, |this| {
                                // A run in flight in a tab nobody is looking at
                                // is otherwise invisible.
                                this.child(div().size(px(6.)).rounded_full().bg(cx.theme().primary))
                            })
                            // The only tab has no close button rather than a
                            // dead one: closing it cannot remove it, and a
                            // control that does not do what it says is worse
                            // than an absent one.
                            .when(closable, |this| {
                                this.child(
                                    Button::new(("db-close-tab", index))
                                        .ghost()
                                        .xsmall()
                                        .icon(AppIcon::Close)
                                        .tooltip(t(Str::DbCloseQueryTab, cx))
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.close_tab(index, window, cx);
                                        })),
                                )
                            }),
                    )
            })
            .collect();

        h_flex()
            .w_full()
            .min_w_0()
            .items_center()
            .overflow_hidden()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                TabBar::new("db-query-tabs")
                    .selected_index(active)
                    .children(tabs)
                    .suffix(
                        h_flex()
                            .size(px(28.))
                            .items_center()
                            .justify_center()
                            .child(
                                Button::new("db-new-tab")
                                    .ghost()
                                    .xsmall()
                                    .icon(AppIcon::Plus)
                                    .tooltip(t(Str::DbNewQueryTab, cx))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.open_tab(window, cx);
                                    })),
                            ),
                    )
                    .on_click(cx.listener(|this, index: &usize, _, cx| {
                        this.select_tab(*index, cx);
                    })),
            )
            .into_any_element()
    }

    fn render_editor(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let connected = self.active_driver().is_some();
        let running = self.tabs.active().is_some_and(|tab| tab.is_running());
        let can_explain = self
            .active_driver()
            .is_some_and(|driver| driver.capabilities().explain)
            && !running;
        let can_format = self
            .active_driver()
            .map(|driver| driver.capabilities().editor_language == "sql")
            .or_else(|| {
                self.connections
                    .selected()
                    .map(|profile| profile.engine.editor_language() == "sql")
            })
            .unwrap_or(true);
        let can_cancel = self.tabs.active().is_some_and(|tab| tab.can_cancel());
        let tab_notice = self
            .tabs
            .active()
            .and_then(|tab| tab.notice.clone().map(|text| (text, tab.notice_success)));
        let editor = self.tabs.active().map(|tab| tab.editor.clone());
        let strip = self.render_tab_strip(cx);

        v_flex()
            .size_full()
            .min_w_0()
            .child(strip)
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
                            .flex_shrink_0()
                            .whitespace_nowrap()
                            .text_xs()
                            .font_medium()
                            .text_color(cx.theme().muted_foreground)
                            .child(t(Str::DbQuery, cx)),
                    )
                    .child(
                        h_flex()
                            .gap_1()
                            .child(
                                Button::new("db-history")
                                    .xsmall()
                                    .ghost()
                                    .icon(AppIcon::Clock)
                                    .label(t(Str::DbHistory, cx))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.open_history(window, cx)
                                    })),
                            )
                            .when(can_format, |this| {
                                this.child(
                                    Button::new("db-format")
                                        .xsmall()
                                        .ghost()
                                        .label(t(Str::DbFormat, cx))
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.format(window, cx)
                                        })),
                                )
                            })
                            // Only while there is something to stop, and only
                            // when the backend can really stop it: a Cancel
                            // that dropped the wait and left the server working
                            // would be a lie the user cannot see through.
                            .when(can_cancel, |this| {
                                this.child(
                                    Button::new("db-cancel")
                                        .xsmall()
                                        .danger()
                                        .icon(AppIcon::Stop)
                                        .label(t(Str::DbCancelQuery, cx))
                                        .on_click(cx.listener(|this, _, _, cx| this.cancel(cx))),
                                )
                            })
                            // PostgreSQL only. SQLite reports no useful plan
                            // surface, so absence is more honest than a button
                            // that can never produce the promised result.
                            .when(can_explain, |this| {
                                this.child(
                                    Button::new("db-explain")
                                        .xsmall()
                                        .ghost()
                                        .label(t(Str::DbExplain, cx))
                                        .on_click(cx.listener(|this, _, _, cx| this.explain(cx))),
                                )
                            })
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
            // A message about the tab that is not its result: dodo could not
            // deliver a cancel request, or what an export did.
            .children(tab_notice.map(|(text, success)| {
                div().w_full().px_2().pb_1p5().child(notice(
                    if success {
                        Tone::Success
                    } else {
                        Tone::Warning
                    },
                    t(text, cx),
                    cx,
                ))
            }))
            .children(editor.map(|editor| {
                div()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(
                        Input::new(&editor)
                            .font_family(cx.theme().mono_font_family.clone())
                            .text_size(cx.theme().mono_font_size)
                            .size_full(),
                    )
            }))
            .into_any_element()
    }

    fn render_result(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let idle = QueryState::Idle;
        let query = self.tabs.active().map_or(&idle, |tab| &tab.query);
        let body = match query {
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
                // A cancellation is not a fault. It gets the neutral empty
                // state and the Stop glyph rather than the danger tone and a
                // red triangle, because the user is the one who did it — and
                // it is still a *distinct* outcome, never a silent empty grid.
                let body = if failure.is_cancelled() {
                    empty_state(
                        AppIcon::Stop,
                        t(Str::DbCancelledTitle, cx),
                        Some(t(Str::DbCancelledHint, cx)),
                        cx,
                    )
                } else {
                    error_state(t(Str::DbStatusError, cx), t(failure.message(), cx), cx)
                };
                body.when_some(statement, |this, statement| {
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

            // `with_size` is what stops the header clipping its two lines, and
            // `scrollbar_visible(_, true)` is what keeps a wide value inside
            // the grid instead of pushing the columns beside it off the window.
            // `result_grid`'s module doc has the arithmetic behind both.
            QueryState::Done(_) => div()
                .size_full()
                .min_w_0()
                .child(
                    DataTable::new(&self.table)
                        .stripe(true)
                        .scrollbar_visible(true, true)
                        .with_size(result_grid::table_size(cx)),
                )
                .into_any_element(),
        };

        let show_edit_toolbar = matches!(query, QueryState::Done(outcome) if outcome.has_grid());
        let read_only_notice = show_edit_toolbar
            .then(|| {
                self.active_grid()
                    .and_then(|(_, grid)| grid.editability().reason())
                    .map(|reason| reason.message())
            })
            .flatten();
        let edit_notice = self.edit_notice.clone();
        v_flex()
            .size_full()
            .min_w_0()
            .border_t_1()
            .border_color(cx.theme().border)
            .child(
                h_flex()
                    .w_full()
                    .min_w_0()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .px_2()
                    .py_1p5()
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_xs()
                            .font_medium()
                            .text_color(cx.theme().muted_foreground)
                            .child(t(Str::DbResult, cx)),
                    )
                    .children(show_edit_toolbar.then(|| self.render_edit_toolbar(cx))),
            )
            .children(read_only_notice.map(|message| {
                div()
                    .w_full()
                    .px_2()
                    .pb_1p5()
                    .child(notice(Tone::Info, t(message, cx), cx))
            }))
            .children(edit_notice.map(|(message, success)| {
                div().w_full().px_2().pb_1p5().child(notice(
                    if success {
                        Tone::Success
                    } else {
                        Tone::Warning
                    },
                    t(message, cx),
                    cx,
                ))
            }))
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

    pub(super) fn render_edit_toolbar(&self, cx: &mut Context<Self>) -> AnyElement {
        let selected_cell = self.selected_cell(cx);
        let selected_row = self.selected_row(cx);
        let committing = self.is_committing();
        let (read_only, cell_error, row_error, duplicate_error, pending) = self
            .active_grid()
            .map(|(_, grid)| {
                (
                    grid.editability().reason().cloned(),
                    selected_cell.and_then(|(row, column)| grid.cell_error(row, column)),
                    selected_row.and_then(|row| grid.row_error(row)),
                    selected_row.and_then(|row| grid.duplicate_error(row)),
                    grid.pending_rows(),
                )
            })
            .unwrap_or((
                Some(crate::database::models::identity::ReadOnlyReason::NoColumns),
                None,
                None,
                None,
                0,
            ));
        let read_only = read_only.map(|reason| reason.message());
        let busy = committing.then_some(Str::DbCommitRunning);
        let cell_reason = busy.clone().or_else(|| read_only.clone()).or_else(|| {
            selected_cell
                .is_none()
                .then_some(Str::DbEditSelectRow)
                .or_else(|| cell_error.map(Self::edit_error_text))
        });
        let row_reason = busy.clone().or_else(|| read_only.clone()).or_else(|| {
            selected_row
                .is_none()
                .then_some(Str::DbEditSelectRow)
                .or_else(|| row_error.map(Self::edit_error_text))
        });
        let duplicate_reason = busy.clone().or_else(|| read_only.clone()).or_else(|| {
            selected_row
                .is_none()
                .then_some(Str::DbEditSelectRow)
                .or_else(|| duplicate_error.map(Self::edit_error_text))
        });
        let add_reason = busy.clone().or_else(|| read_only.clone());
        let pending_reason = busy
            .or(read_only)
            .or_else(|| (pending == 0).then_some(Str::DbEditNoPending));

        h_flex()
            .min_w_0()
            .items_center()
            .gap_1()
            .children((pending > 0).then(|| {
                div()
                    .mr_1()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(t(Str::DbPendingChanges(pending), cx))
            }))
            .child(
                Button::new("db-edit-cell")
                    .ghost()
                    .xsmall()
                    .disabled(cell_reason.is_some())
                    .label(t(Str::DbEditCell, cx))
                    .when_some(cell_reason, |button, reason| button.tooltip(t(reason, cx)))
                    .on_click(cx.listener(|this, _, window, cx| {
                        if let Some((row, column)) = this.selected_cell(cx) {
                            this.open_cell_editor(row, column, window, cx);
                        }
                    })),
            )
            .child(
                Button::new("db-add-row")
                    .ghost()
                    .xsmall()
                    .icon(AppIcon::Plus)
                    .disabled(add_reason.is_some())
                    .label(t(Str::DbAddRow, cx))
                    .when_some(add_reason, |button, reason| button.tooltip(t(reason, cx)))
                    .on_click(cx.listener(|this, _, window, cx| this.open_add_row(window, cx))),
            )
            .child(
                Button::new("db-duplicate-row")
                    .ghost()
                    .xsmall()
                    .disabled(duplicate_reason.is_some())
                    .label(t(Str::DbDuplicateRow, cx))
                    .when_some(duplicate_reason, |button, reason| {
                        button.tooltip(t(reason, cx))
                    })
                    .on_click(
                        cx.listener(|this, _, window, cx| this.open_duplicate_row(window, cx)),
                    ),
            )
            .child(
                Button::new("db-delete-row")
                    .ghost()
                    .danger()
                    .xsmall()
                    .icon(AppIcon::Trash)
                    .disabled(row_reason.is_some())
                    .label(t(Str::DbDeleteRow, cx))
                    .when_some(row_reason, |button, reason| button.tooltip(t(reason, cx)))
                    .on_click(cx.listener(|this, _, _, cx| this.delete_selected_row(cx))),
            )
            .child(
                Button::new("db-rollback")
                    .ghost()
                    .xsmall()
                    .disabled(pending_reason.is_some())
                    .label(t(Str::DbRollback, cx))
                    .when_some(pending_reason.clone(), |button, reason| {
                        button.tooltip(t(reason, cx))
                    })
                    .on_click(cx.listener(|this, _, _, cx| this.rollback_edits(cx))),
            )
            .child(
                Button::new("db-commit")
                    .primary()
                    .xsmall()
                    .disabled(pending_reason.is_some())
                    .label(t(Str::DbCommit, cx))
                    .when_some(pending_reason, |button, reason| {
                        button.tooltip(t(reason, cx))
                    })
                    .on_click(cx.listener(|this, _, window, cx| this.open_commit(window, cx))),
            )
            .into_any_element()
    }

    /// The footer, or nothing when there is no result to describe.
    fn render_footer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let QueryState::Done(outcome) = &self.tabs.active()?.query else {
            return None;
        };

        let can_export = self.can_export();
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
                                t(Str::DbFooterTruncated(outcome.grid.rows().len()), cx),
                                cx,
                            )))
                        })
                        .when(outcome.has_grid(), |this| {
                            this.child(
                                h_flex()
                                    .flex_shrink_0()
                                    .gap_1()
                                    .child(
                                        Button::new("db-export-csv")
                                            .xsmall()
                                            .ghost()
                                            .icon(AppIcon::Download)
                                            .disabled(!can_export)
                                            .label(t(Str::DbExportCsv, cx))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.export(ExportFormat::Csv, cx)
                                            })),
                                    )
                                    .child(
                                        Button::new("db-export-json")
                                            .xsmall()
                                            .ghost()
                                            .icon(AppIcon::Download)
                                            .disabled(!can_export)
                                            .label(t(Str::DbExportJson, cx))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.export(ExportFormat::Json, cx)
                                            })),
                                    ),
                            )
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
