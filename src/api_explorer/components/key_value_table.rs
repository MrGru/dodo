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
//!
//! # The TYPE column
//!
//! One table *does* differ structurally: a multipart form row may send a file
//! instead of text. That is the `typed` flag — passed in by the caller rather
//! than read off the body type here, so this module keeps knowing nothing about
//! what a body is. A typed table grows a TYPE cell and, on a `File` row, swaps
//! the value input for the picker.

use std::path::PathBuf;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, Entity, InteractiveElement as _, IntoElement, ParentElement as _, Pixels, SharedString,
    StatefulInteractiveElement as _, Styled as _,
};
use gpui::{div, px};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::checkbox::Checkbox;
use gpui_component::input::Input;
use gpui_component::popover::Popover;
use gpui_component::{
    ActiveTheme as _, Disableable as _, Icon, Selectable as _, Sizable as _, h_flex, v_flex,
};

use crate::api_explorer::models::key_value::FieldKind;
use crate::api_explorer::services::file_picker;
use crate::api_explorer::state::request::{KeyValueRow, MoveRow, RowTable};
use crate::api_explorer::state::tab::RequestTabState;
use crate::app_icon::AppIcon;
use crate::i18n::{Str, api_explorer, api_variables, t};

/// Width of the enable checkbox column, and of the header cell above it.
const ENABLE_COLUMN: Pixels = px(24.);

/// Width of the TYPE column, on the tables that have one.
///
/// Fixed, like the two columns either side of it, so the rows line up; kept
/// tight because it is width taken from the three text columns, and a narrow
/// window has none to spare.
const TYPE_COLUMN: Pixels = px(76.);

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
                0 => api_explorer::Text::NoActiveParams.into(),
                n => api_explorer::Text::ActiveParams(n).into(),
            },
            add_row_label: api_explorer::Text::AddParameter.into(),
            id_prefix: "param",
        },
        RowTable::Headers => Labels {
            summary: |count| match count {
                0 => api_explorer::Text::NoActiveHeaders.into(),
                n => api_explorer::Text::ActiveHeaders(n).into(),
            },
            add_row_label: api_explorer::Text::AddHeader.into(),
            id_prefix: "header",
        },
        RowTable::BodyFields => Labels {
            summary: |count| match count {
                0 => api_explorer::Text::NoActiveFields.into(),
                n => api_explorer::Text::ActiveFields(n).into(),
            },
            add_row_label: api_explorer::Text::AddField.into(),
            id_prefix: "field",
        },
    }
}

/// Renders one table, wired to `tab` for every edit.
///
/// The table has two views the toolbar switches between: the row editor (Table)
/// and a `Key: Value` text area (Bulk Edit). Both write back to the same request
/// state, so switching never loses data.
///
/// `typed` adds the TYPE column; only the multipart form body sets it. Bulk
/// Edit is `Key: Value` text and cannot express a file, so a typed table's file
/// rows survive the round trip only because `set_edit_mode` reuses the existing
/// row entities positionally — the same way a row's description survives.
pub fn key_value_table(
    table: RowTable,
    typed: bool,
    tab: &Entity<RequestTabState>,
    cx: &App,
) -> impl IntoElement {
    let labels = labels(table);
    let bulk = tab.read(cx).request.is_bulk_edit(table);
    let rows = tab.read(cx).request.rows(table);
    let active = rows
        .iter()
        .filter(|row| row.enabled && !row.key.read(cx).value().trim().is_empty())
        .count();
    let incomplete = if typed {
        rows.iter().filter(|row| row.is_incomplete_file(cx)).count()
    } else {
        0
    };
    let all_enabled = tab.read(cx).request.all_rows_enabled(table);
    let last = rows.len().saturating_sub(1);

    let body = if bulk {
        bulk_pane(table, tab, cx).into_any_element()
    } else {
        table_pane(table, &labels, typed, all_enabled, last, tab, cx).into_any_element()
    };

    v_flex()
        .size_full()
        .child(toolbar_row(table, &labels, bulk, active, tab, cx))
        // Stated above the rows rather than only inside them: a row whose file
        // was never chosen is skipped at send time, and a payload that silently
        // vanishes is the one failure a user cannot debug.
        .when(incomplete > 0 && !bulk, |this| {
            this.child(
                h_flex()
                    .w_full()
                    .items_center()
                    .gap_1p5()
                    .px_3()
                    .py_1p5()
                    .text_xs()
                    .text_color(cx.theme().warning)
                    .bg(cx.theme().warning.opacity(0.1))
                    .child(Icon::new(AppIcon::AlertTriangle).size(px(12.)))
                    .child(t(api_explorer::Text::IncompleteFileFields(incomplete), cx)),
            )
        })
        .child(body)
}

/// The Table view: the column header (with the toggle-all control) over the
/// scrolling list of rows.
fn table_pane(
    table: RowTable,
    labels: &Labels,
    typed: bool,
    all_enabled: bool,
    last: usize,
    tab: &Entity<RequestTabState>,
    cx: &App,
) -> impl IntoElement {
    let rows = tab.read(cx).request.rows(table);
    v_flex()
        .flex_1()
        .min_h_0()
        .child(column_header(
            table,
            labels.id_prefix,
            typed,
            all_enabled,
            tab,
            cx,
        ))
        .child(
            div()
                .id(SharedString::from(format!("{}-rows", labels.id_prefix)))
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .children(rows.iter().enumerate().map(|(index, row)| {
                    render_row(
                        table,
                        labels,
                        typed,
                        row,
                        index == 0,
                        index == last,
                        tab,
                        cx,
                    )
                    .into_any_element()
                }))
                .child(add_row(table, labels, tab, cx)),
        )
}

/// The Bulk Edit view: one multiline text area of `Key: Value` lines.
fn bulk_pane(table: RowTable, tab: &Entity<RequestTabState>, cx: &App) -> impl IntoElement {
    let editor = tab.read(cx).request.bulk_editor(table).clone();
    div().flex_1().min_h_0().p_2().child(
        div()
            .size_full()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(cx.theme().border)
            .child(
                Input::new(&editor)
                    .font_family(cx.theme().mono_font_family.clone())
                    .text_size(cx.theme().mono_font_size)
                    .size_full(),
            ),
    )
}

/// The active-row count on the left; the Table / Bulk Edit switch and, in Table
/// view, the "+ Add" button on the right.
fn toolbar_row(
    table: RowTable,
    labels: &Labels,
    bulk: bool,
    active: usize,
    tab: &Entity<RequestTabState>,
    cx: &App,
) -> impl IntoElement {
    h_flex()
        .items_center()
        .justify_between()
        .gap_2()
        .px_3()
        .py_2()
        .child(
            // The count reads off the rows, which are stale mid-bulk-edit, so it
            // is shown only in Table view.
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .when(!bulk, |this| this.child(t((labels.summary)(active), cx))),
        )
        .child(
            h_flex()
                .items_center()
                .gap_2()
                .child(mode_switch(table, labels, bulk, tab, cx))
                .when(!bulk, |this| {
                    this.child(add_top_button(table, labels, tab, cx))
                }),
        )
}

/// The Table / Bulk Edit segmented switch.
fn mode_switch(
    table: RowTable,
    labels: &Labels,
    bulk: bool,
    tab: &Entity<RequestTabState>,
    cx: &App,
) -> impl IntoElement {
    h_flex()
        .items_center()
        .gap_1()
        .child(mode_button(
            table,
            labels,
            "table",
            api_explorer::Text::EditModeTable.into(),
            !bulk,
            false,
            tab,
            cx,
        ))
        .child(mode_button(
            table,
            labels,
            "bulk",
            api_explorer::Text::EditModeBulk.into(),
            bulk,
            true,
            tab,
            cx,
        ))
}

/// One button of the mode switch. `bulk` is the mode it selects.
#[allow(clippy::too_many_arguments)]
fn mode_button(
    table: RowTable,
    labels: &Labels,
    suffix: &'static str,
    label: Str,
    selected: bool,
    bulk: bool,
    tab: &Entity<RequestTabState>,
    cx: &App,
) -> impl IntoElement {
    let tab = tab.clone();
    Button::new(format!("{}-mode-{suffix}", labels.id_prefix))
        .ghost()
        .xsmall()
        .selected(selected)
        .label(t(label, cx))
        .on_click(move |_, window, cx| {
            tab.update(cx, |state, cx| {
                state.request.set_edit_mode(table, bulk, window, cx);
                cx.notify();
            });
        })
}

/// The "+ Add" button in the toolbar.
fn add_top_button(
    table: RowTable,
    labels: &Labels,
    tab: &Entity<RequestTabState>,
    cx: &App,
) -> impl IntoElement {
    let tab = tab.clone();
    Button::new(format!("{}-add-top", labels.id_prefix))
        .ghost()
        .xsmall()
        .icon(AppIcon::Plus)
        .label(t(api_explorer::Text::Add, cx))
        .on_click(move |_, window, cx| {
            tab.update(cx, |state, cx| {
                state.request.add_row(table, window, cx);
                state.request.dirty = true;
                cx.notify();
            });
        })
}

/// The VALUE column's cell, sized for the table it is in.
///
/// On a typed table it takes a double share of the leftover width: the cell
/// holds a file name, a size and a clear button rather than a text input, so
/// an equal split leaves the name with nothing. A hard `min_w` was tried here
/// and is wrong — it makes the row overflow instead of shrink, and the KEY and
/// DESCRIPTION headers collapse into one-character-wide stacks. Used for the
/// header cell and the row cell alike, so the two cannot drift out of
/// alignment.
fn value_column(typed: bool) -> gpui::Div {
    let cell = div().min_w_0();
    if typed {
        cell.flex_grow(2.).flex_shrink(1.).flex_basis(px(0.))
    } else {
        cell.flex_1()
    }
}

/// The `KEY` / `VALUE` / `DESCRIPTION` rule, led by the toggle-all checkbox that
/// enables or disables every row at once.
fn column_header(
    table: RowTable,
    prefix: &'static str,
    typed: bool,
    all_enabled: bool,
    tab: &Entity<RequestTabState>,
    cx: &App,
) -> impl IntoElement {
    let tab = tab.clone();
    h_flex()
        .items_center()
        .px_3()
        .py_1p5()
        .gap_2()
        .border_b_1()
        .border_color(cx.theme().border)
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        // Aligns with, and toggles, the checkbox column of the rows below.
        .child(
            div().w(ENABLE_COLUMN).flex_shrink_0().child(
                Checkbox::new(format!("{prefix}-toggle-all"))
                    .checked(all_enabled)
                    .tooltip(t(api_explorer::Text::ToggleAllRows, cx))
                    .on_click(move |checked, _, cx| {
                        let checked = *checked;
                        tab.update(cx, |state, cx| {
                            state.request.set_all_rows_enabled(table, checked);
                            state.request.dirty = true;
                            cx.notify();
                        });
                    }),
            ),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .child(t(api_variables::Text::ColumnKey, cx)),
        )
        .when(typed, |this| {
            this.child(
                div()
                    .w(TYPE_COLUMN)
                    .flex_shrink_0()
                    .child(t(api_explorer::Text::ColumnType, cx)),
            )
        })
        .child(value_column(typed).child(t(api_variables::Text::ColumnValue, cx)))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .child(t(api_explorer::Text::ColumnDescription, cx)),
        )
        .child(div().w(ACTIONS_COLUMN).flex_shrink_0())
}

/// Drawing one row needs the whole context it sits in: which table, its words,
/// whether it is typed, and where it is in the list. Bundling those into a
/// struct would only move the same seven values one indirection away.
#[allow(clippy::too_many_arguments)]
fn render_row(
    table: RowTable,
    labels: &Labels,
    typed: bool,
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
        .when(typed, |this| {
            this.child(
                div()
                    .w(TYPE_COLUMN)
                    .flex_shrink_0()
                    .child(type_picker(table, prefix, row, tab, cx)),
            )
        })
        .child(
            value_column(typed)
                .when(typed && row.kind == FieldKind::File, |this| {
                    this.child(file_cell(table, prefix, row, tab, cx))
                })
                .when(!(typed && row.kind == FieldKind::File), |this| {
                    this.child(Input::new(&row.value).appearance(false).small())
                }),
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

/// The Text / File switch for one row of a typed table.
///
/// A two-item `Popover` rather than a segmented pair of buttons: the cell is
/// narrow, and the closed state has to say which type is in force without
/// costing the value column any width.
fn type_picker(
    table: RowTable,
    prefix: &'static str,
    row: &KeyValueRow,
    tab: &Entity<RequestTabState>,
    cx: &App,
) -> impl IntoElement {
    let id = row.id;
    let current = row.kind;

    let options = FieldKind::ALL.map(|candidate| {
        let tab = tab.clone();
        Button::new(format!("{prefix}-kind-{id}-{}", candidate as usize))
            .ghost()
            .small()
            .w_full()
            .justify_start()
            .selected(candidate == current)
            .label(t(candidate.label(), cx))
            .on_click(move |_, _, cx| {
                tab.update(cx, |state, cx| {
                    state.request.set_row_kind(table, id, candidate);
                    state.request.dirty = true;
                    cx.notify();
                });
            })
    });

    Popover::new(SharedString::from(format!("{prefix}-kind-{id}")))
        .trigger(
            Button::new(format!("{prefix}-kind-trigger-{id}"))
                .ghost()
                .xsmall()
                .child(
                    h_flex()
                        .items_center()
                        .gap_1()
                        .text_xs()
                        .child(t(current.label(), cx))
                        .child(Icon::new(AppIcon::ChevronDown).size(px(10.))),
                ),
        )
        .p_1()
        .w(px(120.))
        .children(options)
}

/// The value cell of a `File` row: the picker, the chosen file, and the way
/// back out of it.
///
/// The full path lives in the tooltip rather than in the label — a cell this
/// narrow can only show a file name, and the name is what identifies the file
/// to the person who picked it. An unchosen file is drawn in the warning colour
/// so the row reads as unfinished at a glance.
fn file_cell(
    table: RowTable,
    prefix: &'static str,
    row: &KeyValueRow,
    tab: &Entity<RequestTabState>,
    cx: &App,
) -> impl IntoElement {
    let id = row.id;
    let path = row.file_path.clone();
    let chosen = !path.trim().is_empty();
    let name = file_picker::ChosenFile {
        path: PathBuf::from(&path),
        size: row.file_size,
    }
    .display_name();
    let size = row.file_size.map(file_picker::format_size);

    let pick_tab = tab.clone();
    let clear_tab = tab.clone();

    let label = if chosen {
        SharedString::from(name)
    } else {
        t(api_explorer::Text::ChooseFile, cx)
    };

    h_flex()
        .w_full()
        .min_w_0()
        .items_center()
        .gap_1()
        // The name is the only part that may give way. Without the `min_w_0`
        // here *and* the `truncate` on the label itself, a long file name grows
        // the button past its cell and lands on top of the description column
        // at a narrow window — a `flex_1` parent alone does not stop a child
        // from overflowing.
        .child(
            div().flex_1().min_w_0().overflow_hidden().child(
                Button::new(format!("{prefix}-file-{id}"))
                    .ghost()
                    .xsmall()
                    .w_full()
                    .justify_start()
                    .child(
                        h_flex()
                            .w_full()
                            .min_w_0()
                            .items_center()
                            .gap_1()
                            .text_xs()
                            .child(Icon::new(AppIcon::File).size(px(12.)).flex_shrink_0())
                            .child(div().flex_1().min_w_0().truncate().child(label)),
                    )
                    .when(chosen, |this| this.tooltip(SharedString::from(path)))
                    .when(!chosen, |this| {
                        this.tooltip(t(api_explorer::Text::NoFileSelected, cx))
                    })
                    .on_click(move |_, _, cx| {
                        file_picker::choose_file(pick_tab.clone(), cx, move |state, chosen, cx| {
                            state.request.set_row_file(
                                table,
                                id,
                                chosen.path.display().to_string(),
                                chosen.size,
                            );
                            state.request.dirty = true;
                            cx.notify();
                        });
                    }),
            ),
        )
        .when_some(size.filter(|_| chosen), |this, size| {
            this.child(
                div()
                    .flex_shrink_0()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(size),
            )
        })
        .when(chosen, |this| {
            this.child(
                Button::new(format!("{prefix}-file-clear-{id}"))
                    .ghost()
                    .xsmall()
                    .flex_shrink_0()
                    .icon(AppIcon::Close)
                    .tooltip(t(api_explorer::Text::ClearFile, cx))
                    .on_click(move |_, _, cx| {
                        clear_tab.update(cx, |state, cx| {
                            state.request.set_row_file(table, id, String::new(), None);
                            state.request.dirty = true;
                            cx.notify();
                        });
                    }),
            )
        })
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
            api_explorer::Text::MoveRowUp.into(),
            MoveRow::Up,
            is_first,
        ))
        .child(move_button(
            "move-down",
            AppIcon::ArrowDown,
            api_explorer::Text::MoveRowDown.into(),
            MoveRow::Down,
            is_last,
        ))
        .child(
            Button::new(format!("{prefix}-duplicate-{id}"))
                .ghost()
                .xsmall()
                .icon(AppIcon::Copy)
                .tooltip(t(api_explorer::Text::DuplicateRow, cx))
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
                .tooltip(t(api_variables::Text::DeleteRow, cx))
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
