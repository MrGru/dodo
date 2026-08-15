//! Persisted, searchable query history.
//!
//! The dialog receives an owned snapshot and clicking a row opens its text in a
//! new tab. The saved connection scope is checked before the page changes its
//! selection, so a repointed profile can never silently receive an old query.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use gpui::{
    App, AppContext as _, Context, Entity, IntoElement, ParentElement as _, Pixels, Render,
    SharedString, Styled as _, Task, Window, div, px,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::list::{List, ListDelegate, ListItem, ListState};
use gpui_component::{
    ActiveTheme as _, Disableable as _, IndexPath, Sizable as _, WindowExt as _, h_flex, v_flex,
};

use crate::app_icon::AppIcon;
use crate::database::models::library::{HistoryEntry, HistoryOutcome};
use crate::database::state::query::format_elapsed;
use crate::database::views::database::DatabaseView;
use crate::i18n::{Str, db_catalog, db_query, t};

const PANEL_W: Pixels = px(680.);
const PANEL_H: Pixels = px(440.);
const PANEL_MARGIN: Pixels = px(24.);
const DIALOG_PADDING_X: Pixels = px(32.);

pub fn open(
    entries: Vec<HistoryEntry>,
    writable: bool,
    database: Entity<DatabaseView>,
    window: &mut Window,
    cx: &mut App,
) {
    let view = cx.new(|cx| HistoryView::new(entries, writable, database, window, cx));
    window.open_dialog(cx, move |dialog, window, cx| {
        let view = view.clone();
        let viewport = window.viewport_size();
        let card_w = PANEL_W.min(viewport.width - PANEL_MARGIN * 2.);
        let body_h = PANEL_H.min(viewport.height - PANEL_MARGIN * 4.);
        dialog
            .w(card_w)
            .title(t(db_query::Text::History, cx))
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
    database: Entity<DatabaseView>,
    can_clear: bool,
}

impl HistoryView {
    fn new(
        entries: Vec<HistoryEntry>,
        writable: bool,
        database: Entity<DatabaseView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let can_clear = writable && !entries.is_empty();
        let list = cx.new(|cx| {
            ListState::new(HistoryDelegate::new(entries, database.clone()), window, cx)
                .searchable(true)
        });
        list.update(cx, |state, cx| state.focus(window, cx));
        Self {
            list,
            database,
            can_clear,
        }
    }
}

impl Render for HistoryView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let database = self.database.clone();
        v_flex()
            .size_full()
            .gap_2()
            .child(
                h_flex().w_full().justify_end().child(
                    Button::new("db-clear-history")
                        .ghost()
                        .small()
                        .icon(AppIcon::Trash)
                        .label(t(db_query::Text::HistoryClear, cx))
                        .disabled(!self.can_clear)
                        .on_click(move |_, window, cx| {
                            window.close_dialog(cx);
                            database.update(cx, |database, cx| {
                                database.confirm_clear_history(window, cx)
                            });
                        }),
                ),
            )
            .child(
                div().flex_1().min_h_0().child(
                    List::new(&self.list)
                        .search_placeholder(t(db_query::Text::HistorySearch, cx))
                        .size_full(),
                ),
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
        let outcome = match entry.outcome {
            HistoryOutcome::Succeeded => Str::from(db_query::Text::HistorySucceeded),
            HistoryOutcome::Failed => Str::from(db_query::Text::HistoryFailed),
            HistoryOutcome::Cancelled => Str::from(db_catalog::Text::CancelledMessage),
        };
        let separator = || {
            div()
                .text_color(cx.theme().muted_foreground)
                .child(SharedString::new_static("·"))
        };
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
                        h_flex()
                            .gap_1()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(SharedString::from(entry.scope.connection_name.clone()))
                            .child(separator())
                            .child(t(outcome, cx))
                            .children(entry.duration_ms.map(|millis| {
                                h_flex().gap_1().child(separator()).child(t(
                                    db_query::Text::FooterElapsed(format_elapsed(
                                        Duration::from_millis(millis),
                                    )),
                                    cx,
                                ))
                            }))
                            .child(separator())
                            .child(t(relative_age(entry.recorded_at, now()), cx)),
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
            db_query::Text::HistoryEmpty
        } else {
            db_query::Text::HistoryNoMatches
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
            .and_then(|ix| self.filtered.get(ix.row).copied())
            .and_then(|index| self.entries.get(index))
            .cloned()
        else {
            return;
        };
        self.database.update(cx, |database, cx| {
            database.open_scoped_statement(entry.scope, entry.statement, window, cx)
        });
        window.close_dialog(cx);
    }
}

fn statement_summary(statement: &str) -> String {
    statement.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn relative_age(recorded_at: u64, current: u64) -> Str {
    let seconds = current.saturating_sub(recorded_at);
    match seconds {
        0..=59 => db_query::Text::HistoryJustNow.into(),
        60..=3_599 => db_query::Text::HistoryMinutesAgo(seconds / 60).into(),
        3_600..=86_399 => db_query::Text::HistoryHoursAgo(seconds / 3_600).into(),
        _ => db_query::Text::HistoryDaysAgo(seconds / 86_400).into(),
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::{relative_age, statement_summary};
    use crate::i18n::{Str, db_query};

    #[test]
    fn a_multiline_statement_is_one_search_result_row() {
        assert_eq!(
            statement_summary("SELECT *\n  FROM users\nWHERE id = 1"),
            "SELECT * FROM users WHERE id = 1"
        );
    }

    #[test]
    fn persisted_timestamps_render_as_bounded_relative_units() {
        assert!(matches!(
            relative_age(100, 120),
            Str::DbQuery(db_query::Text::HistoryJustNow)
        ));
        assert!(matches!(
            relative_age(100, 220),
            Str::DbQuery(db_query::Text::HistoryMinutesAgo(2))
        ));
        assert!(matches!(
            relative_age(100, 7_300),
            Str::DbQuery(db_query::Text::HistoryHoursAgo(2))
        ));
        assert!(matches!(
            relative_age(100, 172_900),
            Str::DbQuery(db_query::Text::HistoryDaysAgo(2))
        ));
    }
}
