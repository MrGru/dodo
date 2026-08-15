//! Searchable saved queries. Opening copies text into a new query tab; the
//! saved connection scope is checked by [`DatabaseView`] before selection.

use gpui::{
    App, AppContext as _, Context, Entity, IntoElement, ParentElement as _, Pixels, Render,
    SharedString, Styled as _, Task, Window, div, px,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::list::{List, ListDelegate, ListItem, ListState};
use gpui_component::{
    ActiveTheme as _, IndexPath, Sizable as _, StyledExt as _, WindowExt as _, h_flex, v_flex,
};

use crate::app_icon::AppIcon;
use crate::i18n::{db_query, t};
use crate::models::library::SavedQuery;
use crate::views::database::DatabaseView;

const PANEL_W: Pixels = px(680.);
const PANEL_H: Pixels = px(440.);
const PANEL_MARGIN: Pixels = px(24.);
const DIALOG_PADDING_X: Pixels = px(32.);

pub fn open(
    entries: Vec<SavedQuery>,
    database: Entity<DatabaseView>,
    window: &mut Window,
    cx: &mut App,
) {
    let view = cx.new(|cx| SavedQueriesView::new(entries, database, window, cx));
    window.open_dialog(cx, move |dialog, window, cx| {
        let view = view.clone();
        let viewport = window.viewport_size();
        let card_w = PANEL_W.min(viewport.width - PANEL_MARGIN * 2.);
        let body_h = PANEL_H.min(viewport.height - PANEL_MARGIN * 4.);
        dialog
            .w(card_w)
            .title(t(db_query::Text::SavedQueries, cx))
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

struct SavedQueriesView {
    list: Entity<ListState<SavedQueriesDelegate>>,
}

impl SavedQueriesView {
    fn new(
        entries: Vec<SavedQuery>,
        database: Entity<DatabaseView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let list = cx.new(|cx| {
            ListState::new(SavedQueriesDelegate::new(entries, database), window, cx)
                .searchable(true)
        });
        list.update(cx, |state, cx| state.focus(window, cx));
        Self { list }
    }
}

impl Render for SavedQueriesView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        List::new(&self.list)
            .search_placeholder(t(db_query::Text::SavedQuerySearch, cx))
            .size_full()
    }
}

struct SavedQueriesDelegate {
    entries: Vec<SavedQuery>,
    filtered: Vec<usize>,
    selected: Option<IndexPath>,
    database: Entity<DatabaseView>,
}

impl SavedQueriesDelegate {
    fn new(entries: Vec<SavedQuery>, database: Entity<DatabaseView>) -> Self {
        let filtered = (0..entries.len()).collect();
        Self {
            entries,
            filtered,
            selected: None,
            database,
        }
    }
}

impl ListDelegate for SavedQueriesDelegate {
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
        let query = self.entries.get(*self.filtered.get(ix.row)?)?.clone();
        let edit_query = query.clone();
        let delete_query = query.clone();
        let edit_page = self.database.clone();
        let delete_page = self.database.clone();
        Some(
            ListItem::new(("db-saved-query-row", ix.row))
                .child(
                    v_flex()
                        .min_w_0()
                        .gap_1()
                        .child(
                            div()
                                .w_full()
                                .truncate()
                                .font_medium()
                                .child(SharedString::from(query.name)),
                        )
                        .child(
                            div()
                                .w_full()
                                .truncate()
                                .font_family(cx.theme().mono_font_family.clone())
                                .child(SharedString::from(statement_summary(&query.statement))),
                        )
                        .child(
                            h_flex()
                                .gap_1()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(SharedString::from(query.scope.connection_name))
                                .child(SharedString::new_static("·"))
                                .child(SharedString::from(query.scope.target)),
                        ),
                )
                .suffix(move |_, cx| {
                    h_flex()
                        .gap_1()
                        .child(
                            Button::new(("db-edit-saved-query", edit_query.id as usize))
                                .ghost()
                                .xsmall()
                                .icon(AppIcon::Settings)
                                .tooltip(t(db_query::Text::SavedQueryEdit, cx))
                                .on_click({
                                    let page = edit_page.clone();
                                    let query = edit_query.clone();
                                    move |_, window, cx| {
                                        window.close_dialog(cx);
                                        page.update(cx, |page, cx| {
                                            page.edit_saved_query(query.clone(), window, cx)
                                        });
                                    }
                                }),
                        )
                        .child(
                            Button::new(("db-delete-saved-query", delete_query.id as usize))
                                .ghost()
                                .xsmall()
                                .icon(AppIcon::Trash)
                                .tooltip(t(db_query::Text::SavedQueryDelete, cx))
                                .on_click({
                                    let page = delete_page.clone();
                                    let query = delete_query.clone();
                                    move |_, window, cx| {
                                        window.close_dialog(cx);
                                        page.update(cx, |page, cx| {
                                            page.delete_saved_query(query.clone(), window, cx)
                                        });
                                    }
                                }),
                        )
                }),
        )
    }

    fn render_empty(
        &mut self,
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> impl IntoElement {
        let text = if self.entries.is_empty() {
            db_query::Text::SavedQueryEmpty
        } else {
            db_query::Text::SavedQueryNoMatches
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
        let Some(query) = self
            .selected
            .and_then(|ix| self.filtered.get(ix.row).copied())
            .and_then(|index| self.entries.get(index))
            .cloned()
        else {
            return;
        };
        self.database.update(cx, |database, cx| {
            database.open_scoped_statement(query.scope, query.statement, window, cx)
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
    fn multiline_saved_text_has_one_compact_search_row() {
        assert_eq!(
            statement_summary("SELECT *\nFROM users"),
            "SELECT * FROM users"
        );
    }
}
