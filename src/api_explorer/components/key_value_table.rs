//! The editable key/value table behind the Params and Headers tabs.
//!
//! # Why this is not the library's `Table`
//!
//! `gpui_component::table` is a virtualized, delegate-driven table for
//! displaying rows. Every cell here is a live `InputState` that has to keep its
//! own cursor, selection and undo history, which fights the delegate's
//! render-on-demand model. Built from `Checkbox`, `Input` and `Button` instead,
//! this reuses three library widgets and stays about a screenful of code.

use gpui::{
    App, Entity, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _,
};
use gpui::{Window, div, px};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::checkbox::Checkbox;
use gpui_component::input::Input;
use gpui_component::{ActiveTheme as _, Sizable as _, h_flex, v_flex};

use crate::api_explorer::state::request::KeyValueRow;
use crate::api_explorer::state::tab::RequestTabState;
use crate::app_icon::AppIcon;
use crate::i18n::{Str, t};

/// Which of the two tables is being drawn. The rows live in different fields
/// and the copy differs, but every other behaviour is shared.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Table {
    Params,
    Headers,
}

impl Table {
    fn rows(self, state: &RequestTabState) -> &[KeyValueRow] {
        match self {
            Table::Params => &state.request.params,
            Table::Headers => &state.request.headers,
        }
    }

    /// The "N active" line above the table.
    fn summary(self, count: usize) -> Str {
        match (self, count) {
            (Table::Params, 0) => Str::NoActiveParams,
            (Table::Params, n) => Str::ActiveParams(n),
            (Table::Headers, 0) => Str::NoActiveHeaders,
            (Table::Headers, n) => Str::ActiveHeaders(n),
        }
    }

    /// The trailing "add another" row.
    fn add_row_label(self) -> Str {
        match self {
            Table::Params => Str::AddParameter,
            Table::Headers => Str::AddHeader,
        }
    }

    /// Element ids have to be unique across both tables on screen.
    fn id_prefix(self) -> &'static str {
        match self {
            Table::Params => "param",
            Table::Headers => "header",
        }
    }

    fn add(self, state: &mut RequestTabState, window: &mut Window, cx: &mut App) {
        match self {
            Table::Params => state.request.add_param(window, cx),
            Table::Headers => state.request.add_header(window, cx),
        }
    }

    fn remove(self, state: &mut RequestTabState, id: usize) {
        match self {
            Table::Params => state.request.remove_param(id),
            Table::Headers => state.request.remove_header(id),
        }
    }

    fn set_enabled(self, state: &mut RequestTabState, id: usize, enabled: bool) {
        let rows = match self {
            Table::Params => &mut state.request.params,
            Table::Headers => &mut state.request.headers,
        };
        if let Some(row) = rows.iter_mut().find(|row| row.id == id) {
            row.enabled = enabled;
        }
    }
}

/// Renders one table, wired to `tab` for every edit.
pub fn key_value_table(table: Table, tab: &Entity<RequestTabState>, cx: &App) -> impl IntoElement {
    let state = tab.read(cx);
    let rows = table.rows(state);
    let active = rows
        .iter()
        .filter(|row| row.enabled && !row.key.read(cx).value().trim().is_empty())
        .count();

    v_flex()
        .size_full()
        .child(summary_row(table, active, tab, cx))
        .child(column_header(cx))
        .child(
            div()
                .id(match table {
                    Table::Params => "param-rows",
                    Table::Headers => "header-rows",
                })
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .children(
                    rows.iter()
                        .map(|row| render_row(table, row, tab, cx).into_any_element()),
                )
                .child(add_row(table, tab, cx)),
        )
}

/// "No active params" on the left, "+ Add" on the right.
fn summary_row(
    table: Table,
    active: usize,
    tab: &Entity<RequestTabState>,
    cx: &App,
) -> impl IntoElement {
    let tab = tab.clone();
    h_flex()
        .items_center()
        .justify_between()
        .px_3()
        .py_2()
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(t(table.summary(active), cx)),
        )
        .child(
            Button::new(format!("{}-add-top", table.id_prefix()))
                .ghost()
                .xsmall()
                .icon(AppIcon::Plus)
                .label(t(Str::Add, cx))
                .on_click(move |_, window, cx| {
                    tab.update(cx, |state, cx| {
                        table.add(state, window, cx);
                        cx.notify();
                    });
                }),
        )
}

/// The `KEY` / `VALUE` rule.
fn column_header(cx: &App) -> impl IntoElement {
    h_flex()
        .items_center()
        .px_3()
        .py_1p5()
        .gap_2()
        .border_b_1()
        .border_color(cx.theme().border)
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        // Aligns with the checkbox column of the rows below.
        .child(div().w(px(24.)))
        .child(div().flex_1().child(t(Str::ColumnKey, cx)))
        .child(div().flex_1().child(t(Str::ColumnValue, cx)))
        .child(div().w(px(28.)))
}

fn render_row(
    table: Table,
    row: &KeyValueRow,
    tab: &Entity<RequestTabState>,
    cx: &App,
) -> impl IntoElement {
    let id = row.id;
    let prefix = table.id_prefix();
    let enable_tab = tab.clone();
    let delete_tab = tab.clone();

    h_flex()
        .items_center()
        .px_3()
        .py_1()
        .gap_2()
        .border_b_1()
        .border_color(cx.theme().border.opacity(0.5))
        .child(
            div().w(px(24.)).child(
                Checkbox::new(format!("{prefix}-enabled-{id}"))
                    .checked(row.enabled)
                    .on_click(move |checked, _, cx| {
                        let checked = *checked;
                        enable_tab.update(cx, |state, cx| {
                            table.set_enabled(state, id, checked);
                            cx.notify();
                        });
                    }),
            ),
        )
        // Placeholders live on the `InputState` the row owns, set when the row
        // is created; see `RequestState::sync_row_placeholders`.
        .child(
            div()
                .flex_1()
                .min_w_0()
                .child(Input::new(&row.key).appearance(false).small()),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .child(Input::new(&row.value).appearance(false).small()),
        )
        .child(
            div().w(px(28.)).child(
                Button::new(format!("{prefix}-delete-{id}"))
                    .ghost()
                    .xsmall()
                    .icon(AppIcon::Close)
                    .tooltip(t(Str::DeleteRow, cx))
                    .on_click(move |_, _, cx| {
                        delete_tab.update(cx, |state, cx| {
                            table.remove(state, id);
                            cx.notify();
                        });
                    }),
            ),
        )
}

/// The trailing "+ Add parameter" row.
fn add_row(table: Table, tab: &Entity<RequestTabState>, cx: &App) -> impl IntoElement {
    let tab = tab.clone();
    h_flex().px_3().py_1p5().child(
        Button::new(format!("{}-add-row", table.id_prefix()))
            .ghost()
            .xsmall()
            .icon(AppIcon::Plus)
            .label(t(table.add_row_label(), cx))
            .on_click(move |_, window, cx| {
                tab.update(cx, |state, cx| {
                    table.add(state, window, cx);
                    cx.notify();
                });
            }),
    )
}
