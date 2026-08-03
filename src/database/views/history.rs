//! Searchable query history for this session.
//!
//! The dialog is an entity because dialog layers repaint independently from
//! the Database page. It receives an owned snapshot, never reads the page while
//! the click handler has it leased, and clicking a row opens a new query tab.

use gpui::{
    App, AppContext as _, Context, Entity, IntoElement, ParentElement as _, Pixels, Render,
    SharedString, Styled as _, Task, Window, div, px,
};
use gpui_component::list::{List, ListDelegate, ListItem, ListState};
use gpui_component::{ActiveTheme as _, IndexPath, WindowExt as _, v_flex};

use crate::database::state::history::HistoryEntry;
use crate::database::views::database::DatabaseView;
use crate::i18n::{Str, t};

const PANEL_W: Pixels = px(640.);
const PANEL_H: Pixels = px(440.);
const PANEL_MARGIN: Pixels = px(24.);
const DIALOG_PADDING_X: Pixels = px(32.);

pub fn open(
    entries: Vec<HistoryEntry>,
    database: Entity<DatabaseView>,
    window: &mut Window,
    cx: &mut App,
) {
    let view = cx.new(|cx| HistoryView::new(entries, database, window, cx));
    window.open_dialog(cx, move |dialog, window, cx| {
        let view = view.clone();
        let viewport = window.viewport_size();
        let card_w = PANEL_W.min(viewport.width - PANEL_MARGIN * 2.);
        let body_h = PANEL_H.min(viewport.height - PANEL_MARGIN * 4.);
        dialog
            .w(card_w)
            .title(t(Str::DbHistory, cx))
            .content(move |content, _, _| {
                content.child(
                    div()
                        .w(card_w - DIALOG_PADDING_X)
                        .h(body_h)
                        .child(view.clone()),
                )
            })
    });
}

struct HistoryView {
    list: Entity<ListState<HistoryDelegate>>,
}

impl HistoryView {
    fn new(
        entries: Vec<HistoryEntry>,
        database: Entity<DatabaseView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let list = cx.new(|cx| {
            ListState::new(HistoryDelegate::new(entries, database), window, cx).searchable(true)
        });
        list.update(cx, |state, cx| state.focus(window, cx));
        Self { list }
    }
}

impl Render for HistoryView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(
            List::new(&self.list)
                .search_placeholder(t(Str::DbHistorySearch, cx))
                .size_full(),
        )
    }
}

struct HistoryDelegate {
    entries: Vec<HistoryEntry>,
    filtered: Vec<usize>,
    selected: Option<IndexPath>,
    database: Entity<DatabaseView>,
}

impl HistoryDelegate {
    fn new(entries: Vec<HistoryEntry>, database: Entity<DatabaseView>) -> Self {
        let filtered = (0..entries.len()).collect();
        Self {
            entries,
            filtered,
            selected: None,
            database,
        }
    }
}

impl ListDelegate for HistoryDelegate {
    type Item = ListItem;

    fn perform_search(
        &mut self,
        query: &str,
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Task<()> {
        self.filtered = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.matches(query))
            .map(|(index, _)| index)
            .collect();
        self.selected = None;
        cx.notify();
        Task::ready(())
    }

    fn items_count(&self, _: usize, _: &App) -> usize {
        self.filtered.len()
    }

    fn render_item(
        &mut self,
        ix: IndexPath,
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<Self::Item> {
        let entry = self.entries.get(*self.filtered.get(ix.row)?)?;
        Some(
            ListItem::new(("db-history-row", ix.row)).child(
                v_flex()
                    .min_w_0()
                    .gap_1()
                    .child(
                        div()
                            .w_full()
                            .truncate()
                            .font_family(cx.theme().mono_font_family.clone())
                            .child(SharedString::from(statement_summary(&entry.statement))),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(SharedString::from(entry.connection.clone())),
                    ),
            ),
        )
    }

    fn render_empty(
        &mut self,
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> impl IntoElement {
        let text = if self.entries.is_empty() {
            Str::DbHistoryEmpty
        } else {
            Str::DbHistoryNoMatches
        };
        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .text_color(cx.theme().muted_foreground)
            .child(t(text, cx))
            .into_any_element()
    }

    fn set_selected_index(
        &mut self,
        ix: Option<IndexPath>,
        _: &mut Window,
        _: &mut Context<ListState<Self>>,
    ) {
        self.selected = ix;
    }

    fn confirm(&mut self, _: bool, window: &mut Window, cx: &mut Context<ListState<Self>>) {
        let Some(entry) = self
            .selected
            .and_then(|ix| self.filtered.get(ix.row))
            .and_then(|index| self.entries.get(*index))
            .cloned()
        else {
            return;
        };
        self.database.update(cx, |database, cx| {
            database.open_tab_with_statement(entry.statement, window, cx)
        });
        window.close_dialog(cx);
    }
}

fn statement_summary(statement: &str) -> String {
    statement.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::statement_summary;

    #[test]
    fn a_multiline_statement_is_one_search_result_row() {
        assert_eq!(
            statement_summary("SELECT *\n  FROM users\nWHERE id = 1"),
            "SELECT * FROM users WHERE id = 1"
        );
    }
}
