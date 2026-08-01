//! The result grid: one `TableDelegate`, and the footer under it.
//!
//! # There is no sort affordance, and that is the design
//!
//! `gpui_component`'s table supports sorting, and this delegate deliberately
//! does not use it. Sorting the thousand rows that happened to arrive is not
//! sorting the result, and a control that is present but disabled invites the
//! question every time. Server-side `ORDER BY` on a table-data view is the
//! honest version and it is a later round; nothing here makes it hard, and
//! nothing here is built for it either.
//!
//! # The header carries the column's type under its name
//!
//! Which is the whole reason `render_th` is overridden. The type is the
//! server's own name for it — `int4`, `character varying(255)` — so a value
//! this build could not decode is still labelled with what it really is.
//!
//! # `NULL` is drawn, not written
//!
//! [`Value::display`](crate::database::models::value::Value::display) renders
//! `NULL` as an empty string on purpose, and this file draws the word in the
//! muted colour instead. Otherwise a `text` column containing the four
//! characters `NULL` would be indistinguishable from an absent value, which in
//! a database client is a real way to misread a result.

use gpui::{
    App, Context, Div, InteractiveElement as _, IntoElement, ParentElement as _, Pixels,
    SharedString, Stateful, Styled as _, Window, div, px,
};
use gpui_component::table::{Column, TableDelegate, TableState};
use gpui_component::{ActiveTheme as _, StyledExt as _, v_flex};

use crate::database::models::value::{ColumnMeta, Row, Value};
use crate::i18n::{Str, t};

/// A column's default width. Wide enough for a timestamp, which is the widest
/// thing most result sets have; every column is resizable from there.
const COLUMN_WIDTH: Pixels = px(160.);
const COLUMN_MIN_WIDTH: Pixels = px(60.);

/// The rows and columns currently on screen.
#[derive(Default)]
pub struct ResultDelegate {
    columns: Vec<ColumnMeta>,
    rows: Vec<Row>,
}

impl ResultDelegate {
    /// Replaces what the grid is showing. The caller follows this with
    /// `TableState::refresh`, which is what re-measures the header.
    pub fn set(&mut self, columns: Vec<ColumnMeta>, rows: Vec<Row>) {
        self.columns = columns;
        self.rows = rows;
    }

    pub fn clear(&mut self) {
        self.columns.clear();
        self.rows.clear();
    }

    /// Whether there is anything to draw. The pane asks the query state
    /// instead, which knows *why* there is nothing; this is for assertions.
    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.columns.is_empty()
    }

    fn cell(&self, row_ix: usize, col_ix: usize) -> Option<&Value> {
        self.rows.get(row_ix).and_then(|row| row.get(col_ix))
    }
}

impl TableDelegate for ResultDelegate {
    fn columns_count(&self, _: &App) -> usize {
        self.columns.len()
    }

    fn rows_count(&self, _: &App) -> usize {
        self.rows.len()
    }

    fn column(&self, col_ix: usize, _: &App) -> Column {
        let name = self
            .columns
            .get(col_ix)
            .map(|column| column.name.clone())
            .unwrap_or_default();
        // The key is the column's index, not its name: two result columns may
        // legally share a name (`SELECT a.id, b.id`), and a duplicate key would
        // make the table's own bookkeeping ambiguous.
        Column::new(col_ix.to_string(), name)
            .width(COLUMN_WIDTH)
            .min_width(COLUMN_MIN_WIDTH)
            .resizable(true)
    }

    fn render_th(
        &mut self,
        col_ix: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let Some(column) = self.columns.get(col_ix) else {
            return div().into_any_element();
        };
        let type_name = column.type_name.clone();

        v_flex()
            .size_full()
            .justify_center()
            .gap_0p5()
            .child(
                div()
                    .text_xs()
                    .font_medium()
                    .truncate()
                    .child(SharedString::from(column.name.clone())),
            )
            .child(
                div()
                    .text_xs()
                    .truncate()
                    .text_color(cx.theme().muted_foreground)
                    .child(SharedString::from(type_name)),
            )
            .into_any_element()
    }

    fn render_td(
        &mut self,
        row_ix: usize,
        col_ix: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        match self.cell(row_ix, col_ix) {
            // `NULL` is drawn in the muted colour rather than written into the
            // value, so text that spells NULL cannot be mistaken for one.
            Some(Value::Null) | None => div()
                .text_color(cx.theme().muted_foreground)
                .child(t(Str::DbColumnNull, cx))
                .into_any_element(),
            Some(value) => div()
                .truncate()
                .child(SharedString::from(value.display()))
                .into_any_element(),
        }
    }

    fn render_tr(
        &mut self,
        row_ix: usize,
        _: &mut Window,
        _: &mut Context<TableState<Self>>,
    ) -> Stateful<Div> {
        div().id(("db-result-row", row_ix))
    }
}

#[cfg(test)]
mod tests {
    use super::ResultDelegate;
    use crate::database::models::value::{ColumnMeta, Value};

    #[test]
    fn a_fresh_delegate_has_nothing_to_draw() {
        let delegate = ResultDelegate::default();
        assert!(delegate.is_empty());
        assert_eq!(delegate.cell(0, 0), None);
    }

    #[test]
    fn setting_a_result_replaces_the_previous_one_entirely() {
        let mut delegate = ResultDelegate::default();
        delegate.set(
            vec![ColumnMeta::new("a", "int4"), ColumnMeta::new("b", "text")],
            vec![vec![Value::Int(1), Value::Text("x".into())]],
        );
        assert!(!delegate.is_empty());
        assert_eq!(delegate.cell(0, 1), Some(&Value::Text("x".into())));

        delegate.set(vec![ColumnMeta::new("only", "text")], Vec::new());
        assert_eq!(delegate.columns.len(), 1);
        assert!(delegate.rows.is_empty());
        assert_eq!(delegate.cell(0, 0), None, "the old rows must not survive");
    }

    #[test]
    fn clearing_leaves_nothing_to_draw() {
        let mut delegate = ResultDelegate::default();
        delegate.set(
            vec![ColumnMeta::new("a", "int4")],
            vec![vec![Value::Int(1)]],
        );
        delegate.clear();
        assert!(delegate.is_empty());
    }

    /// A ragged row — fewer values than columns — must not panic the grid. It
    /// cannot happen through a driver, and a delegate that indexes blindly is
    /// one bug away from it happening anyway.
    #[test]
    fn a_row_shorter_than_the_header_reads_as_empty_rather_than_panicking() {
        let mut delegate = ResultDelegate::default();
        delegate.set(
            vec![ColumnMeta::new("a", "int4"), ColumnMeta::new("b", "text")],
            vec![vec![Value::Int(1)]],
        );
        assert_eq!(delegate.cell(0, 0), Some(&Value::Int(1)));
        assert_eq!(delegate.cell(0, 1), None);
        assert_eq!(delegate.cell(9, 0), None);
    }
}
