//! The editable key/value table behind the Params, Headers and form-body tabs.
//!
//! # Why this is not the library's `Table`
//!
//! `gpui_component::table` is a virtualized, delegate-driven table for
//! displaying rows. Every cell here is a live `InputState` that has to keep its
//! own cursor, selection and undo history, which fights the delegate's
//! render-on-demand model. Built from `Checkbox`, `Input` and `Button` instead,
//! this reuses three library widgets and stays about a screenful of code.
//!
//! # One table, three uses
//!
//! Params, Headers and the form body differ only in which rows they own and
//! what their empty cells say. [`RowTable`] carries that difference and every
//! row operation lives once, in `state::request`; this module is the drawing
//! and the words around it.

use gpui::{
    App, Entity, InteractiveElement as _, IntoElement, ParentElement as _, Pixels, SharedString,
    StatefulInteractiveElement as _, Styled as _,
};
use gpui::{div, px};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::checkbox::Checkbox;
use gpui_component::input::Input;
use gpui_component::{ActiveTheme as _, Disableable as _, Sizable as _, h_flex, v_flex};

use crate::api_explorer::state::request::{KeyValueRow, MoveRow, RowTable};
use crate::api_explorer::state::tab::RequestTabState;
use crate::app_icon::AppIcon;
use crate::i18n::{Str, t};

/// Width of the enable checkbox column, and of the header cell above it.
const ENABLE_COLUMN: Pixels = px(24.);

/// Width of the trailing column holding move, duplicate and delete.
///
/// Fixed rather than content-sized so the three text columns line up between
/// rows even in a narrow window, and so the controls can never be squeezed to
/// nothing.
const ACTIONS_COLUMN: Pixels = px(104.);

/// The wording that differs between the three tables.
///
/// [`RowTable`] owns the behaviour and the cell placeholders; this owns the
/// words around the table and the element-id prefix, both of which are drawing.
struct Labels {
    /// The "N active" line above the table, given the count.
    summary: fn(usize) -> Str,
    /// The trailing "add another" row.
    add_row_label: Str,
    /// Element ids have to be unique across every table on screen.
    id_prefix: &'static str,
}

fn labels(table: RowTable) -> Labels {
    match table {
        RowTable::Params => Labels {
            summary: |count| match count {
                0 => Str::NoActiveParams,
                n => Str::ActiveParams(n),
            },
            add_row_label: Str::AddParameter,
            id_prefix: "param",
        },
        RowTable::Headers => Labels {
            summary: |count| match count {
                0 => Str::NoActiveHeaders,
                n => Str::ActiveHeaders(n),
            },
            add_row_label: Str::AddHeader,
            id_prefix: "header",
        },
        RowTable::BodyFields => Labels {
            summary: |count| match count {
                0 => Str::NoActiveFields,
                n => Str::ActiveFields(n),
            },
            add_row_label: Str::AddField,
            id_prefix: "field",
        },
    }
}

/// Renders one table, wired to `tab` for every edit.
pub fn key_value_table(
    table: RowTable,
    tab: &Entity<RequestTabState>,
    cx: &App,
) -> impl IntoElement {
    let labels = labels(table);
    let rows = tab.read(cx).request.rows(table);
    let active = rows
        .iter()
        .filter(|row| row.enabled && !row.key.read(cx).value().trim().is_empty())
        .count();
    let last = rows.len().saturating_sub(1);

    v_flex()
        .size_full()
        .child(summary_row(table, &labels, active, tab, cx))
        .child(column_header(cx))
        .child(
            div()
                .id(SharedString::from(format!("{}-rows", labels.id_prefix)))
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .children(rows.iter().enumerate().map(|(index, row)| {
                    render_row(table, &labels, row, index == 0, index == last, tab, cx)
                        .into_any_element()
                }))
                .child(add_row(table, &labels, tab, cx)),
        )
}

/// "No active params" on the left, "+ Add" on the right.
fn summary_row(
    table: RowTable,
    labels: &Labels,
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
                .child(t((labels.summary)(active), cx)),
        )
        .child(
            Button::new(format!("{}-add-top", labels.id_prefix))
                .ghost()
                .xsmall()
                .icon(AppIcon::Plus)
                .label(t(Str::Add, cx))
                .on_click(move |_, window, cx| {
                    tab.update(cx, |state, cx| {
                        state.request.add_row(table, window, cx);
                        state.request.dirty = true;
                        cx.notify();
                    });
                }),
        )
}

/// The `KEY` / `VALUE` / `DESCRIPTION` rule.
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
        .child(div().w(ENABLE_COLUMN).flex_shrink_0())
        .child(div().flex_1().min_w_0().child(t(Str::ColumnKey, cx)))
        .child(div().flex_1().min_w_0().child(t(Str::ColumnValue, cx)))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .child(t(Str::ColumnDescription, cx)),
        )
        .child(div().w(ACTIONS_COLUMN).flex_shrink_0())
}

fn render_row(
    table: RowTable,
    labels: &Labels,
    row: &KeyValueRow,
    is_first: bool,
    is_last: bool,
    tab: &Entity<RequestTabState>,
    cx: &App,
) -> impl IntoElement {
    let id = row.id;
    let prefix = labels.id_prefix;
    let enable_tab = tab.clone();

    h_flex()
        .items_center()
        .px_3()
        .py_1()
        .gap_2()
        .border_b_1()
        .border_color(cx.theme().border.opacity(0.5))
        .child(
            div().w(ENABLE_COLUMN).flex_shrink_0().child(
                Checkbox::new(format!("{prefix}-enabled-{id}"))
                    .checked(row.enabled)
                    .on_click(move |checked, _, cx| {
                        let checked = *checked;
                        enable_tab.update(cx, |state, cx| {
                            state.request.set_row_enabled(table, id, checked);
                            state.request.dirty = true;
                            cx.notify();
                        });
                    }),
            ),
        )
        // Placeholders live on the `InputState` the row owns, set when the row
        // is created; see `RequestState::sync_placeholders`.
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
        // The description is a note, not payload: it is kept with the row
        // through duplicate and reorder and never sent. See `KeyValueRow`.
        .child(
            div()
                .flex_1()
                .min_w_0()
                .child(Input::new(&row.description).appearance(false).small()),
        )
        .child(row_actions(table, prefix, id, is_first, is_last, tab, cx))
}

/// Move up, move down, duplicate, delete.
///
/// The move buttons are disabled at the ends of the table rather than removed,
/// so every row's controls stay in the same place as the eye travels down the
/// column.
fn row_actions(
    table: RowTable,
    prefix: &'static str,
    id: usize,
    is_first: bool,
    is_last: bool,
    tab: &Entity<RequestTabState>,
    cx: &App,
) -> impl IntoElement {
    let move_button =
        |name: &'static str, icon: AppIcon, tooltip: Str, direction: MoveRow, disabled: bool| {
            let tab = tab.clone();
            Button::new(format!("{prefix}-{name}-{id}"))
                .ghost()
                .xsmall()
                .icon(icon)
                .tooltip(t(tooltip, cx))
                .disabled(disabled)
                .on_click(move |_, _, cx| {
                    tab.update(cx, |state, cx| {
                        state.request.move_row(table, id, direction);
                        state.request.dirty = true;
                        cx.notify();
                    });
                })
        };

    let duplicate_tab = tab.clone();
    let delete_tab = tab.clone();

    h_flex()
        .w(ACTIONS_COLUMN)
        .flex_shrink_0()
        .items_center()
        .justify_end()
        .child(move_button(
            "move-up",
            AppIcon::ArrowUp,
            Str::MoveRowUp,
            MoveRow::Up,
            is_first,
        ))
        .child(move_button(
            "move-down",
            AppIcon::ArrowDown,
            Str::MoveRowDown,
            MoveRow::Down,
            is_last,
        ))
        .child(
            Button::new(format!("{prefix}-duplicate-{id}"))
                .ghost()
                .xsmall()
                .icon(AppIcon::Copy)
                .tooltip(t(Str::DuplicateRow, cx))
                .on_click(move |_, window, cx| {
                    duplicate_tab.update(cx, |state, cx| {
                        state.request.duplicate_row(table, id, window, cx);
                        state.request.dirty = true;
                        cx.notify();
                    });
                }),
        )
        .child(
            Button::new(format!("{prefix}-delete-{id}"))
                .ghost()
                .xsmall()
                .icon(AppIcon::Close)
                .tooltip(t(Str::DeleteRow, cx))
                .on_click(move |_, _, cx| {
                    delete_tab.update(cx, |state, cx| {
                        state.request.remove_row(table, id);
                        state.request.dirty = true;
                        cx.notify();
                    });
                }),
        )
}

/// The trailing "+ Add parameter" row.
fn add_row(
    table: RowTable,
    labels: &Labels,
    tab: &Entity<RequestTabState>,
    cx: &App,
) -> impl IntoElement {
    let tab = tab.clone();
    h_flex().px_3().py_1p5().child(
        Button::new(format!("{}-add-row", labels.id_prefix))
            .ghost()
            .xsmall()
            .icon(AppIcon::Plus)
            .label(t(labels.add_row_label.clone(), cx))
            .on_click(move |_, window, cx| {
                tab.update(cx, |state, cx| {
                    state.request.add_row(table, window, cx);
                    state.request.dirty = true;
                    cx.notify();
                });
            }),
    )
}
