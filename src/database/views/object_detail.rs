//! The table/view detail surface.
//!
//! It reuses the query result's `DataTable` and delegate. Only one grid is on
//! screen, so a second table implementation would add state without adding a
//! capability. Views omit Indexes and Constraints because those objects cannot
//! have either; unavailable backend answers remain an explicit state.

use gpui::{
    AnyElement, ClipboardItem, Context, InteractiveElement as _, IntoElement, ParentElement as _,
    SharedString, StatefulInteractiveElement as _, Styled as _, div,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::tab::{Tab, TabBar};
use gpui_component::table::DataTable;
use gpui_component::{
    ActiveTheme as _, Disableable as _, Sizable as _, StyledExt as _, h_flex, v_flex,
};

use crate::app_icon::AppIcon;
use crate::database::components::notice::{Tone, notice};
use crate::database::components::states::{empty_state, error_state};
use crate::database::models::catalog::NodeKind;
use crate::database::models::detail::{DdlSource, DetailNotice, DetailTab};
use crate::database::state::detail::DetailLoad;
use crate::database::views::database::DatabaseView;
use crate::database::views::result_grid;
use crate::i18n::{Str, t};

impl DatabaseView {
    pub(super) fn render_object_detail(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let Some(detail) = self.detail.as_ref() else {
            return div().into_any_element();
        };
        let name = SharedString::from(detail.target.name.clone());
        let kind = detail.target.kind;
        let active = detail.tab;
        let load = detail.load.clone();
        let ddl_source = detail.ddl_source;
        let tabs: Vec<DetailTab> = detail.visible_tabs().collect();
        let active_index = tabs.iter().position(|tab| *tab == active).unwrap_or(0);

        let tab_elements = tabs
            .iter()
            .map(|tab| Tab::new().px_2().label(t(tab.label(), cx)))
            .collect::<Vec<_>>();
        let body = self.render_detail_body(active, &load, ddl_source, cx);

        v_flex()
            .size_full()
            .min_w_0()
            .child(
                h_flex()
                    .w_full()
                    .min_w_0()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .py_1p5()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        gpui_component::Icon::new(match kind {
                            NodeKind::View => AppIcon::Eye,
                            _ => AppIcon::Table,
                        })
                        .xsmall(),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .font_semibold()
                            .truncate()
                            .child(name),
                    )
                    .child(
                        Button::new("db-close-detail")
                            .ghost()
                            .xsmall()
                            .icon(AppIcon::Close)
                            .tooltip(t(Str::DbDetailClose, cx))
                            .on_click(cx.listener(|this, _, _, cx| this.close_detail(cx))),
                    ),
            )
            .child(
                TabBar::new("db-detail-tabs")
                    .selected_index(active_index)
                    .children(tab_elements)
                    .on_click(
                        cx.listener(|this, index: &usize, _, cx| {
                            this.select_detail_tab(*index, cx)
                        }),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(body),
            )
            .children(self.render_detail_pager(active, &load, cx))
            .into_any_element()
    }

    fn render_detail_body(
        &self,
        tab: DetailTab,
        load: &DetailLoad,
        ddl_source: DdlSource,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match load {
            DetailLoad::Idle | DetailLoad::Loading => {
                empty_state(AppIcon::Ellipsis, t(Str::DbTreeLoading, cx), None, cx)
                    .into_any_element()
            }
            DetailLoad::Unavailable => empty_state(
                AppIcon::AlertTriangle,
                t(Str::DbDetailUnavailable, cx),
                None,
                cx,
            )
            .into_any_element(),
            DetailLoad::Failed(error) => {
                error_state(t(Str::DbStatusError, cx), t(error.message(), cx), cx)
                    .into_any_element()
            }
            DetailLoad::Empty(detail_notice) => v_flex()
                .size_full()
                .children(detail_notice.map(|notice_kind| {
                    let text = match notice_kind {
                        DetailNotice::SqliteConstraintsExcludeChecks => {
                            Str::DbDetailConstraintsPartial
                        }
                    };
                    div()
                        .w_full()
                        .px_2()
                        .pt_1p5()
                        .child(notice(Tone::Warning, t(text, cx), cx))
                }))
                .child(empty_state(
                    AppIcon::Table,
                    t(
                        if tab == DetailTab::Data {
                            Str::DbDetailNoRows
                        } else {
                            Str::DbDetailNoMetadata
                        },
                        cx,
                    ),
                    None,
                    cx,
                ))
                .into_any_element(),
            DetailLoad::Grid(grid) => v_flex()
                .size_full()
                .min_w_0()
                .children(grid.notice.map(|notice_kind| {
                    let text = match notice_kind {
                        DetailNotice::SqliteConstraintsExcludeChecks => {
                            Str::DbDetailConstraintsPartial
                        }
                    };
                    div()
                        .w_full()
                        .px_2()
                        .pt_1p5()
                        .child(notice(Tone::Warning, t(text, cx), cx))
                }))
                .children((grid.capped_cells > 0).then(|| {
                    div().w_full().px_2().pt_1p5().child(notice(
                        Tone::Warning,
                        t(Str::DbFooterCapped(grid.capped_cells), cx),
                        cx,
                    ))
                }))
                .children((tab != DetailTab::Data && grid.has_more).then(|| {
                    div().w_full().px_2().pt_1p5().child(notice(
                        Tone::Warning,
                        t(Str::DbDetailMetadataTruncated(grid.rows.len()), cx),
                        cx,
                    ))
                }))
                .child(
                    div().flex_1().min_h_0().min_w_0().child(
                        DataTable::new(&self.table)
                            .stripe(true)
                            .scrollbar_visible(true, true)
                            .with_size(result_grid::table_size(cx)),
                    ),
                )
                .into_any_element(),
            DetailLoad::Ddl(sql) => {
                let copied = sql.clone();
                v_flex()
                    .size_full()
                    .min_w_0()
                    .children((ddl_source == DdlSource::Reconstructed).then(|| {
                        div().w_full().px_2().pt_1p5().child(notice(
                            Tone::Warning,
                            t(Str::DbDetailDdlReconstructed, cx),
                            cx,
                        ))
                    }))
                    .child(
                        h_flex().w_full().justify_end().px_2().py_1p5().child(
                            Button::new("db-copy-ddl")
                                .ghost()
                                .xsmall()
                                .icon(AppIcon::Copy)
                                .label(t(Str::DbDetailCopyDdl, cx))
                                .on_click(move |_, _, cx| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(
                                        copied.clone(),
                                    ));
                                }),
                        ),
                    )
                    .child(
                        div()
                            .id("db-detail-ddl")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .border_t_1()
                            .border_color(cx.theme().border)
                            .p_3()
                            .font_family(cx.theme().mono_font_family.clone())
                            .text_sm()
                            .child(SharedString::from(sql.clone())),
                    )
                    .into_any_element()
            }
        }
    }

    fn render_detail_pager(
        &self,
        tab: DetailTab,
        load: &DetailLoad,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if tab != DetailTab::Data || matches!(load, DetailLoad::Idle | DetailLoad::Loading) {
            return None;
        }
        let detail = self.detail.as_ref()?;
        let first = detail.first_row_number();
        let rows = match load {
            DetailLoad::Grid(grid) => grid.rows.len() as u64,
            _ => 0,
        };
        let range = (rows > 0).then(|| {
            t(
                Str::DbDetailRowsRange {
                    first,
                    last: first + rows - 1,
                },
                cx,
            )
        });

        Some(
            h_flex()
                .w_full()
                .items_center()
                .justify_between()
                .gap_2()
                .px_2()
                .py_1p5()
                .border_t_1()
                .border_color(cx.theme().border)
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .children(range),
                )
                .child(
                    h_flex()
                        .items_center()
                        .gap_1()
                        .child(
                            Button::new("db-detail-previous")
                                .ghost()
                                .xsmall()
                                .disabled(!detail.can_previous())
                                .label(t(Str::DbDetailPrevious, cx))
                                .on_click(cx.listener(|this, _, _, cx| this.detail_previous(cx))),
                        )
                        .child(
                            div()
                                .px_2()
                                .text_xs()
                                .child(t(Str::DbDetailPage(detail.page_number()), cx)),
                        )
                        .child(
                            Button::new("db-detail-next")
                                .ghost()
                                .xsmall()
                                .disabled(!detail.can_next())
                                .label(t(Str::DbDetailNext, cx))
                                .on_click(cx.listener(|this, _, _, cx| this.detail_next(cx))),
                        ),
                )
                .into_any_element(),
        )
    }
}
