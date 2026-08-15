//! One bounded background catalog crawl followed by an in-memory list filter.
//! Typing never performs remote work.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, AppContext as _, Context, Entity, IntoElement, ParentElement as _, Pixels, Render,
    SharedString, Styled as _, Task, Window, div, px,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::list::{List, ListDelegate, ListItem, ListState};
use gpui_component::{ActiveTheme as _, IndexPath, Sizable as _, WindowExt as _, h_flex, v_flex};

use crate::components::notice::{Tone, notice};
use crate::i18n::{Str, db_catalog, db_connection, t};
use crate::models::catalog::{NodeKind, NodeLabel};
use crate::state::catalog_search::{CatalogIndex, CatalogSource, crawl_catalogs};
use crate::views::database::{DatabaseView, node_icon};

const PANEL_W: Pixels = px(720.);
const PANEL_H: Pixels = px(500.);
const PANEL_MARGIN: Pixels = px(24.);
const DIALOG_PADDING_X: Pixels = px(32.);

pub fn open(
    sources: Vec<CatalogSource>,
    database: Entity<DatabaseView>,
    window: &mut Window,
    cx: &mut App,
) {
    let view = cx.new(|cx| CatalogSearchView::new(database, window, cx));
    view.update(cx, |view, cx| view.start(sources, cx));
    window.open_dialog(cx, move |dialog, window, cx| {
        let view = view.clone();
        let viewport = window.viewport_size();
        let card_w = PANEL_W.min(viewport.width - PANEL_MARGIN * 2.);
        let body_h = PANEL_H.min(viewport.height - PANEL_MARGIN * 4.);
        dialog
            .w(card_w)
            .title(t(db_catalog::Text::CatalogSearch, cx))
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

struct CatalogSearchView {
    list: Entity<ListState<CatalogDelegate>>,
    cancel: Arc<AtomicBool>,
    task: Option<Task<()>>,
    loading: bool,
    index: Option<Arc<CatalogIndex>>,
}

impl CatalogSearchView {
    fn new(database: Entity<DatabaseView>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let list = cx
            .new(|cx| ListState::new(CatalogDelegate::new(database), window, cx).searchable(true));
        list.update(cx, |state, cx| state.focus(window, cx));
        Self {
            list,
            cancel: Arc::new(AtomicBool::new(false)),
            task: None,
            loading: true,
            index: None,
        }
    }

    fn start(&mut self, sources: Vec<CatalogSource>, cx: &mut Context<Self>) {
        let cancel = self.cancel.clone();
        self.task = Some(cx.spawn(async move |this, cx| {
            let index = cx
                .background_executor()
                .spawn(async move { crawl_catalogs(sources, cancel) })
                .await;
            let _ = this.update(cx, |this, cx| {
                let index = Arc::new(index);
                this.list.update(cx, |state, cx| {
                    state.delegate_mut().adopt(index.clone(), cx);
                });
                this.index = Some(index);
                this.loading = false;
                this.task = None;
                cx.notify();
            });
        }));
    }

    fn cancel(&mut self, cx: &mut Context<Self>) {
        self.cancel.store(true, Ordering::Relaxed);
        cx.notify();
    }
}

impl Drop for CatalogSearchView {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl Render for CatalogSearchView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let truncated = self.index.as_ref().is_some_and(|index| index.truncated);
        let cancelled = self.index.as_ref().is_some_and(|index| index.cancelled);
        let failures = self.index.as_ref().map_or(0, |index| index.failures);
        let nodes = self.index.as_ref().map_or(0, |index| index.nodes);

        v_flex()
            .size_full()
            .gap_2()
            .child(notice(
                Tone::Info,
                t(db_catalog::Text::CatalogSearchConnectedOnly, cx),
                cx,
            ))
            .when(self.loading, |this| {
                this.child(
                    h_flex()
                        .w_full()
                        .justify_between()
                        .child(t(db_catalog::Text::CatalogSearchLoading, cx))
                        .child(
                            Button::new("db-cancel-catalog-search")
                                .ghost()
                                .small()
                                .label(t(db_connection::Text::Cancel, cx))
                                .on_click(cx.listener(|this, _, _, cx| this.cancel(cx))),
                        ),
                )
            })
            .when(truncated, |this| {
                this.child(notice(
                    Tone::Warning,
                    t(db_catalog::Text::CatalogSearchTruncated(nodes), cx),
                    cx,
                ))
            })
            .when(failures > 0, |this| {
                this.child(notice(
                    Tone::Warning,
                    t(db_catalog::Text::CatalogSearchPartial(failures), cx),
                    cx,
                ))
            })
            .when(cancelled, |this| {
                this.child(notice(
                    Tone::Info,
                    t(db_catalog::Text::CancelledMessage, cx),
                    cx,
                ))
            })
            .child(
                div().flex_1().min_h_0().child(
                    List::new(&self.list)
                        .search_placeholder(t(db_catalog::Text::CatalogSearchPlaceholder, cx))
                        .size_full(),
                ),
            )
    }
}

struct CatalogDelegate {
    index: Option<Arc<CatalogIndex>>,
    filtered: Vec<usize>,
    selected: Option<IndexPath>,
    query: String,
    database: Entity<DatabaseView>,
}

impl CatalogDelegate {
    fn new(database: Entity<DatabaseView>) -> Self {
        Self {
            index: None,
            filtered: Vec::new(),
            selected: None,
            query: String::new(),
            database,
        }
    }

    fn adopt(&mut self, index: Arc<CatalogIndex>, cx: &mut Context<ListState<Self>>) {
        self.index = Some(index);
        self.filter(cx);
    }

    fn filter(&mut self, cx: &mut Context<ListState<Self>>) {
        self.filtered = self
            .index
            .as_ref()
            .map(|index| {
                index
                    .entries
                    .iter()
                    .enumerate()
                    .filter(|(_, entry)| {
                        let kind = t(kind_label(entry.node.kind), cx);
                        entry.matches(&self.query, &kind)
                    })
                    .map(|(index, _)| index)
                    .collect()
            })
            .unwrap_or_default();
        self.selected = None;
        cx.notify();
    }
}

impl ListDelegate for CatalogDelegate {
    type Item = ListItem;

    fn perform_search(
        &mut self,
        query: &str,
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Task<()> {
        self.query = query.into();
        self.filter(cx);
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
        let index = self.index.as_ref()?;
        let entry = index.entries.get(*self.filtered.get(ix.row)?)?;
        let name = match &entry.node.label {
            NodeLabel::Name(name) => name.clone(),
            NodeLabel::Group(_) => return None,
        };
        Some(
            ListItem::new(("db-catalog-search-row", ix.row)).child(
                h_flex()
                    .w_full()
                    .min_w_0()
                    .gap_2()
                    .child(gpui_component::Icon::new(node_icon(entry.node.kind)).xsmall())
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_1()
                            .child(
                                h_flex()
                                    .w_full()
                                    .gap_2()
                                    .child(div().truncate().child(SharedString::from(name)))
                                    .child(
                                        div()
                                            .flex_shrink_0()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(t(kind_label(entry.node.kind), cx)),
                                    ),
                            )
                            .child(
                                div()
                                    .w_full()
                                    .truncate()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(SharedString::from(format!(
                                        "{} · {}",
                                        entry.scope.connection_name,
                                        entry.path_names().join(" › ")
                                    ))),
                            ),
                    ),
            ),
        )
    }

    fn render_empty(
        &mut self,
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> impl IntoElement {
        let text = match &self.index {
            None => db_catalog::Text::CatalogSearchLoading,
            Some(index) if index.entries.is_empty() => db_catalog::Text::CatalogSearchEmpty,
            Some(_) => db_catalog::Text::CatalogSearchNoMatches,
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
        let Some(index) = self.index.clone() else {
            return;
        };
        let Some(entry) = self
            .selected
            .and_then(|ix| self.filtered.get(ix.row).copied())
            .and_then(|entry| index.entries.get(entry))
            .cloned()
        else {
            return;
        };
        self.database.update(cx, |database, cx| {
            database.navigate_catalog_result(entry, index, cx)
        });
        window.close_dialog(cx);
    }
}

fn kind_label(kind: NodeKind) -> Str {
    match kind {
        NodeKind::Database => db_catalog::Text::CatalogKindDatabase.into(),
        NodeKind::Schema => db_catalog::Text::CatalogKindSchema.into(),
        NodeKind::Table => db_catalog::Text::CatalogKindTable.into(),
        NodeKind::View => db_catalog::Text::CatalogKindView.into(),
        NodeKind::Column => db_catalog::Text::CatalogKindColumn.into(),
        NodeKind::Index => db_catalog::Text::CatalogKindIndex.into(),
        NodeKind::Constraint => db_catalog::Text::CatalogKindConstraint.into(),
        NodeKind::Namespace => db_catalog::Text::CatalogKindNamespace.into(),
        NodeKind::Key => db_catalog::Text::CatalogKindKey.into(),
        NodeKind::Folder | NodeKind::Other => db_catalog::Text::CatalogKindObject.into(),
    }
}
