//! The result grid: one `TableDelegate`, and the footer under it.
//!
//! # There is no sort affordance, and that is the design
//!
//! `gpui_component`'s table supports sorting, and this delegate deliberately
//! does not use it. Sorting the rows that happened to arrive is not sorting the
//! result, and a control that is present but disabled invites the question
//! every time. Round 3 server-pages table data but did not accept sorting into
//! scope, so no half-client-side version was added.
//!
//! # The header carries the column's type under its name
//!
//! Which is the whole reason `render_th` is overridden. The type is the
//! server's own name for it — `int4`, `character varying(255)` — so a value
//! this build could not decode is still labelled with what it really is.
//!
//! # `NULL` is drawn, not written
//!
//! [`Value::display`](crate::models::value::Value::display) renders
//! `NULL` as an empty string on purpose, and this file draws the word in the
//! muted colour instead. Otherwise a `text` column containing the four
//! characters `NULL` would be indistinguishable from an absent value, which in
//! a database client is a real way to misread a result.
//!
//! # The grid is measured in text sizes, not in pixels
//!
//! Round 1 drew a clipped header: the column name and the type under it were
//! both cut off. The mechanism is worth stating, because it is not obvious and
//! it constrains everything below.
//!
//! `DataTable` has **one** height knob. `Size::table_row_height()` sets the
//! header row *and* the body rows, and each cell is `overflow_hidden` with the
//! size's own vertical padding taken out of it — so the header's two lines had
//! `32px − 8px = 24px` to live in, while two `text_xs` lines with gpui's default
//! `phi` line height need about `40px`. Nothing about that is fixable by
//! styling the header container: the inner row's height is set by the widget.
//!
//! So the row height is bought back from the padding — every [`Column`] carries
//! its own `paddings` with **no vertical padding**, which `render_cell` uses in
//! place of the size's — and both the row height and the two header lines are
//! expressed as multiples of the base text size. That is what makes
//! [`fits`](tests) checkable arithmetic rather than a thing somebody eyeballed
//! once at one font size: dodo's Settings dialog offers 14, 16 and 18 px, and
//! the invariant has to hold at all three.
//!
//! Long values are **truncated and the grid scrolls**; a column never grows to
//! fit its widest cell. That is the `COLUMN_WIDTH` below plus the widget's own
//! horizontal scrolling, and it is why one full-width UUID cannot push the rest
//! of the result off the right of the window.

use gpui::{
    App, ClipboardItem, Context, Div, Edges, InteractiveElement as _, IntoElement,
    ParentElement as _, Pixels, SharedString, Stateful, Styled as _, Window, div, px, relative,
    rems,
};
use gpui_component::menu::{ContextMenuExt as _, PopupMenuItem};
use gpui_component::table::{Column, TableDelegate, TableState};
use gpui_component::{ActiveTheme as _, Size, StyledExt as _, h_flex, v_flex};

use crate::app_icon::AppIcon;
use crate::i18n::{db_query, t};
use crate::models::value::{ColumnMeta, Row, Value};

/// A column's default width. Wide enough for a timestamp, which is the widest
/// thing most result sets have; every column is resizable from there.
const COLUMN_WIDTH: Pixels = px(160.);
const COLUMN_MIN_WIDTH: Pixels = px(60.);

/// A row's height, and the header's, as a multiple of the base text size —
/// `DataTable` has one knob for both (see this module's doc).
const ROW_HEIGHT_REMS: f32 = 1.875;

/// The header's two lines: the column's name, then the server's type name
/// under it. Both as multiples of the base text size.
///
/// `NAME_REMS` is deliberately the same `0.75` that `text_xs` is, so the header
/// name and the body text match; the type is a step smaller because it is
/// secondary, exactly as the reference tool draws it.
const NAME_REMS: f32 = 0.75;
const TYPE_REMS: f32 = 0.625;
/// How tall each header line is, relative to its own font size. Tighter than
/// gpui's default `phi` (1.618), because two stacked lines have to fit a row
/// that is also the body's.
const HEADER_LINE_RATIO: f32 = 1.15;
/// gpui's default line height (`phi`), which the single-line body cells keep.
/// Not set anywhere — it is what the body already gets — so it exists to be
/// checked against the row height below.
#[cfg(test)]
const BODY_LINE_RATIO: f32 = 1.618_034;

/// Cell padding. **No vertical padding at all**, and that is the whole reason
/// the header fits: `render_cell` uses a column's own `paddings` in place of the
/// size's, whose 4px top and bottom would take a third of the row away.
/// Vertical centring is done by the cell contents instead.
const CELL_PADDING: Edges<Pixels> = Edges {
    top: px(0.),
    bottom: px(0.),
    left: px(8.),
    right: px(8.),
};

/// The height of one row, header included, for a given base text size.
///
/// Split out from [`row_height`] so the tests can check the arithmetic at every
/// size the Settings dialog offers without a `Window`.
fn row_height_for(font_size: f32) -> f32 {
    font_size * ROW_HEIGHT_REMS
}

/// The height of one row, header included, at the current base text size.
pub(super) fn row_height(cx: &App) -> Pixels {
    px(row_height_for(cx.theme().font_size.into()))
}

/// The size to give [`DataTable`](gpui_component::table::DataTable).
///
/// `Size::Size` rather than one of the named sizes: the named ones are fixed
/// pixel heights that ignore the font-size setting, and the arithmetic this
/// module rests on is in text sizes.
pub(super) fn table_size(cx: &App) -> Size {
    Size::Size(row_height(cx))
}

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

    pub(super) fn copy_cell_text(&self, row_ix: usize, col_ix: usize) -> Option<String> {
        self.cell(row_ix, col_ix).map(Value::display)
    }

    /// Tab-separated so a copied row pastes into a spreadsheet as one row.
    pub(super) fn copy_row_text(&self, row_ix: usize) -> Option<String> {
        self.rows.get(row_ix).map(|row| {
            row.iter()
                .map(Value::display)
                .collect::<Vec<_>>()
                .join("\t")
        })
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
            .paddings(CELL_PADDING)
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

        // No `gap`: the two lines carry their own tight line heights, and a gap
        // on top of them is exactly the kind of extra height that does not show
        // up in the arithmetic the tests check.
        v_flex()
            .size_full()
            .justify_center()
            .child(
                div()
                    .text_size(rems(NAME_REMS))
                    .line_height(relative(HEADER_LINE_RATIO))
                    .font_semibold()
                    .truncate()
                    .child(SharedString::from(column.name.clone())),
            )
            .child(
                div()
                    .text_size(rems(TYPE_REMS))
                    .line_height(relative(HEADER_LINE_RATIO))
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
        let copy_cell = self.copy_cell_text(row_ix, col_ix).unwrap_or_default();
        let copy_row = self.copy_row_text(row_ix).unwrap_or_default();

        // `h_flex().size_full()` rather than a bare div: the cell has no
        // vertical padding left to centre the text for it (see this module's
        // doc), so the content centres itself.
        let cell = h_flex()
            .size_full()
            .min_w_0()
            .text_size(rems(NAME_REMS))
            .context_menu(move |menu, _, cx| {
                let cell = copy_cell.clone();
                let row = copy_row.clone();
                menu.item(
                    PopupMenuItem::new(t(db_query::Text::CopyCell, cx))
                        .icon(AppIcon::Copy)
                        .on_click(move |_, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(cell.clone()));
                        }),
                )
                .item(
                    PopupMenuItem::new(t(db_query::Text::CopyRow, cx))
                        .icon(AppIcon::Copy)
                        .on_click(move |_, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(row.clone()));
                        }),
                )
            });
        match self.cell(row_ix, col_ix) {
            // `NULL` is drawn in the muted colour rather than written into the
            // value, so text that spells NULL cannot be mistaken for one.
            Some(Value::Null) | None => cell
                .text_color(cx.theme().muted_foreground)
                .child(t(db_query::Text::ColumnNull, cx))
                .into_any_element(),
            // Truncated, never wrapped and never widening: a full-width UUID
            // scrolls with the grid rather than pushing the columns beside it
            // off the window.
            Some(value) => cell
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .child(SharedString::from(value.display())),
                )
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
    use super::{
        BODY_LINE_RATIO, HEADER_LINE_RATIO, NAME_REMS, ROW_HEIGHT_REMS, ResultDelegate, TYPE_REMS,
        row_height_for,
    };
    use crate::models::value::{ColumnMeta, Value};

    /// Every base text size the Settings dialog offers, and the default. The
    /// header has to fit at all of them, not just at the default.
    const FONT_SIZES: [f32; 3] = [14., 16., 18.];
    const DEFAULT_FONT_SIZE: f32 = 16.;

    /// How tall the header's two lines are together, in the same units as
    /// [`ROW_HEIGHT_REMS`].
    fn header_content_rems() -> f32 {
        NAME_REMS * HEADER_LINE_RATIO + TYPE_REMS * HEADER_LINE_RATIO
    }

    /// The defect: the header row was 32px with 8px of padding taken out of it,
    /// and two lines of `text_xs` at gpui's default `phi` line height need
    /// about 40px, so both lines were cut off.
    ///
    /// Nothing here can prove the glyphs are drawn — that needs a rendered
    /// frame — but the clipping was arithmetic, and the arithmetic is checkable.
    #[test]
    fn the_headers_two_lines_fit_the_row_the_widget_gives_them() {
        assert!(
            header_content_rems() <= ROW_HEIGHT_REMS,
            "the header needs {} rems and the row is {ROW_HEIGHT_REMS}",
            header_content_rems()
        );
    }

    /// The same in pixels, at every font size the user can choose. A ratio that
    /// holds in the abstract but rounds badly at 14px is still a clipped header.
    #[test]
    fn the_header_fits_at_every_font_size_the_settings_dialog_offers() {
        for font_size in FONT_SIZES {
            let row = font_size * ROW_HEIGHT_REMS;
            let header = font_size * header_content_rems();
            assert!(
                header <= row,
                "at {font_size}px the header wants {header}px in a {row}px row"
            );
        }
    }

    /// The body is one line, and it keeps gpui's default line height, so it has
    /// to fit the same row.
    #[test]
    fn a_body_line_fits_the_row_at_every_font_size() {
        for font_size in FONT_SIZES {
            let row = font_size * ROW_HEIGHT_REMS;
            let line = font_size * NAME_REMS * BODY_LINE_RATIO;
            assert!(
                line <= row,
                "at {font_size}px a body line wants {line}px in a {row}px row"
            );
        }
    }

    /// Round 1 took `Size::Medium`: a **fixed** 32px row carrying body text at
    /// the full base size. Two things changed and both are checked here.
    ///
    /// The grid is more compact at the default font size — which is what the
    /// captain asked for — and the row height now *tracks* the setting, where
    /// round 1's did not. At 18px the row is taller than 32px, and that is the
    /// point rather than a regression: round 1 clipped its body text there too.
    #[test]
    fn the_grid_is_more_compact_at_the_default_size_and_tracks_the_setting() {
        /// What `Size::Medium` — the widget's default, and round 1's — gave
        /// every row at every font size.
        const ROUND_1_ROW_HEIGHT: f32 = 32.;

        let row = row_height_for(DEFAULT_FONT_SIZE);
        assert!(
            row < ROUND_1_ROW_HEIGHT,
            "a {row}px row is not shorter than round 1's {ROUND_1_ROW_HEIGHT}px"
        );
        for font_size in FONT_SIZES {
            let text = font_size * NAME_REMS;
            assert!(
                text < font_size,
                "at {font_size}px cell text of {text}px is not smaller than the base size"
            );
        }

        let heights: Vec<f32> = FONT_SIZES.iter().copied().map(row_height_for).collect();
        assert!(
            heights.windows(2).all(|pair| pair[0] < pair[1]),
            "a bigger font must buy a taller row, unlike round 1's fixed {ROUND_1_ROW_HEIGHT}px: {heights:?}"
        );
    }

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
        assert_eq!(delegate.copy_cell_text(0, 1).as_deref(), Some("x"));
        assert_eq!(delegate.copy_row_text(0).as_deref(), Some("1\tx"));

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
