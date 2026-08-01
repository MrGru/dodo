//! The left panel: the connection list above, the object tree below.
//!
//! Both halves belong to [`DatabaseView`]; this file is the drawing half of it,
//! split out the way the API Explorer splits its regions, so `database.rs`
//! stays about behaviour.

use std::collections::HashMap;
use std::rc::Rc;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, InteractiveElement as _, IntoElement, ParentElement as _, SharedString,
    StatefulInteractiveElement as _, Styled as _, div, px,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::list::ListItem;
use gpui_component::tree::tree;
use gpui_component::{
    ActiveTheme as _, Disableable as _, Icon, Sizable as _, StyledExt as _, h_flex, v_flex,
};

use crate::app_icon::AppIcon;
use crate::database::components::notice::{Tone, notice};
use crate::database::components::states::empty_state;
use crate::database::models::engine::Engine;
use crate::database::state::connections::Status;
use crate::database::views::database::{
    DatabaseView, RowLook, TREE_INDENT, TREE_PADDING, row_looks,
};
use crate::i18n::{Str, t};

impl DatabaseView {
    /// The whole left panel.
    pub(super) fn render_panel(&mut self, cx: &mut gpui::Context<Self>) -> AnyElement {
        v_flex()
            .size_full()
            .min_w_0()
            .child(self.render_connections(cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(self.render_objects(cx)),
            )
            .into_any_element()
    }

    // ---- connections -----------------------------------------------------

    fn render_connections(&mut self, cx: &mut gpui::Context<Self>) -> AnyElement {
        let store_error = self.store_error.clone();

        v_flex()
            .w_full()
            .max_h(px(280.))
            .child(section_header(
                t(Str::DbConnections, cx),
                Button::new("db-new-connection")
                    .ghost()
                    .xsmall()
                    .icon(AppIcon::Plus)
                    .tooltip(t(Str::DbNewConnection, cx))
                    .on_click(cx.listener(|this, _, window, cx| {
                        let draft = this.connections.draft(Engine::PostgreSql);
                        this.open_form(draft, false, window, cx);
                    }))
                    .into_any_element(),
                cx,
            ))
            .when_some(store_error, |this, error| {
                this.child(
                    div()
                        .px_2()
                        .pb_1()
                        .child(notice(Tone::Danger, t(error, cx), cx)),
                )
            })
            .child(if self.connections.is_empty() {
                self.render_no_connections(cx)
            } else {
                self.render_connection_rows(cx)
            })
            .into_any_element()
    }

    fn render_no_connections(&self, cx: &mut gpui::Context<Self>) -> AnyElement {
        // Nothing at all until the file has been read: a "no connections yet"
        // panel that flashes on every launch reads as data loss.
        if !self.connections.loaded() {
            return div().h(px(80.)).into_any_element();
        }

        empty_state(
            AppIcon::Database,
            t(Str::DbNoConnections, cx),
            Some(t(Str::DbNoConnectionsHint, cx)),
            cx,
        )
        .h(px(160.))
        .child(
            Button::new("db-new-connection-empty")
                .small()
                .primary()
                .label(t(Str::DbNewConnection, cx))
                .on_click(cx.listener(|this, _, window, cx| {
                    let draft = this.connections.draft(Engine::PostgreSql);
                    this.open_form(draft, false, window, cx);
                })),
        )
        .into_any_element()
    }

    fn render_connection_rows(&self, cx: &mut gpui::Context<Self>) -> AnyElement {
        let selected = self.connections.selected_id();
        let rows: Vec<AnyElement> = self
            .connections
            .profiles()
            .iter()
            .map(|profile| {
                let id = profile.id;
                let status = self.connections.status(id).clone();
                self.render_connection_row(
                    id,
                    profile.display_name(),
                    profile.target(),
                    profile.engine,
                    status,
                    selected == Some(id),
                    cx,
                )
            })
            .collect();

        v_flex()
            .id("db-connection-list")
            .w_full()
            .min_h_0()
            .p_1()
            .gap_0p5()
            .overflow_y_scroll()
            .children(rows)
            .into_any_element()
    }

    #[allow(clippy::too_many_arguments)]
    fn render_connection_row(
        &self,
        id: u64,
        name: String,
        target: String,
        engine: Engine,
        status: Status,
        selected: bool,
        cx: &mut gpui::Context<Self>,
    ) -> AnyElement {
        let connected = status.is_connected();
        let busy = status.is_busy();
        let dot = match status {
            Status::Connected => cx.theme().success,
            Status::Connecting => cx.theme().warning,
            Status::Error(_) => cx.theme().danger,
            Status::Disconnected => cx.theme().muted_foreground,
        };
        let error = match &status {
            Status::Error(error) => Some(t(error.message(), cx)),
            _ => None,
        };
        let status_label = status.label();

        v_flex()
            .id(("db-connection", id as usize))
            .w_full()
            .gap_1()
            .p_2()
            .rounded(cx.theme().radius)
            .cursor_pointer()
            .when(selected, |this| this.bg(cx.theme().accent))
            .when(!selected, |this| {
                this.hover(|this| this.bg(cx.theme().accent.opacity(0.5)))
            })
            .on_click(cx.listener(move |this, _, _, cx| this.select(id, cx)))
            .child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .items_center()
                    .child(div().size(px(7.)).rounded_full().bg(dot).flex_shrink_0())
                    .child(Icon::new(engine_icon(engine)).small().flex_shrink_0())
                    // `min_w_0` on the name: a long connection name would
                    // otherwise set the panel's width and push the buttons out.
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_sm()
                            .truncate()
                            .child(SharedString::from(name)),
                    ),
            )
            .child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .items_baseline()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_xs()
                            .truncate()
                            .text_color(cx.theme().muted_foreground)
                            .child(SharedString::from(target)),
                    )
                    // The status in words as well as in the dot: a colour alone
                    // is not a label, and two of the four states differ only by
                    // hue.
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_xs()
                            .text_color(dot)
                            .child(t(status_label, cx)),
                    ),
            )
            .when_some(error, |this, error| {
                this.child(
                    div()
                        .w_full()
                        .text_xs()
                        .text_color(cx.theme().danger)
                        .child(error),
                )
            })
            .when(selected, |this| {
                this.child(
                    h_flex()
                        .w_full()
                        .gap_1()
                        .flex_wrap()
                        .child(
                            Button::new(("db-connect", id as usize))
                                .xsmall()
                                .outline()
                                .disabled(busy)
                                .label(if connected {
                                    t(Str::DbDisconnect, cx)
                                } else if matches!(status, Status::Error(_)) {
                                    t(Str::DbReconnect, cx)
                                } else if busy {
                                    t(Str::DbStatusConnecting, cx)
                                } else {
                                    t(Str::DbConnect, cx)
                                })
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if connected {
                                        this.disconnect(id, cx);
                                    } else {
                                        this.connect(id, cx);
                                    }
                                })),
                        )
                        .child(
                            Button::new(("db-edit", id as usize))
                                .xsmall()
                                .ghost()
                                .label(t(Str::DbEditConnection, cx))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    if let Some(profile) = this.connections.find(id).cloned() {
                                        this.open_form(profile, true, window, cx);
                                    }
                                })),
                        )
                        .child(
                            Button::new(("db-duplicate", id as usize))
                                .xsmall()
                                .ghost()
                                .label(t(Str::DbDuplicateConnection, cx))
                                .on_click(
                                    cx.listener(move |this, _, _, cx| this.duplicate(id, cx)),
                                ),
                        )
                        .child(
                            Button::new(("db-delete", id as usize))
                                .xsmall()
                                .ghost()
                                .label(t(Str::DbDeleteConnection, cx))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.delete(id, window, cx)
                                })),
                        ),
                )
            })
            .into_any_element()
    }

    // ---- the object tree -------------------------------------------------

    fn render_objects(&mut self, cx: &mut gpui::Context<Self>) -> AnyElement {
        let header = section_header(
            t(Str::DbObjects, cx),
            Button::new("db-refresh-tree")
                .ghost()
                .xsmall()
                .icon(AppIcon::Refresh)
                .tooltip(t(Str::DbRefreshTree, cx))
                .disabled(self.active_driver().is_none())
                .on_click(cx.listener(|this, _, _, cx| this.refresh_tree(cx)))
                .into_any_element(),
            cx,
        );

        let body = if self.active_driver().is_some() {
            self.render_tree(cx)
        } else {
            empty_state(
                AppIcon::Database,
                t(Str::DbTreeNotConnected, cx),
                Some(t(Str::DbTreeNotConnectedHint, cx)),
                cx,
            )
            .into_any_element()
        };

        v_flex()
            .size_full()
            .min_h_0()
            .child(header)
            .child(div().flex_1().min_h_0().child(body))
            .into_any_element()
    }

    fn render_tree(&mut self, cx: &mut gpui::Context<Self>) -> AnyElement {
        // `TreeItem` has room for one string, so the icon and the dimmed detail
        // are looked up by element id as each row is drawn. The map is rebuilt
        // per frame from the same outline the items came from, so the two
        // cannot disagree.
        let mut looks: HashMap<SharedString, RowLook> = HashMap::new();
        row_looks(&self.tree.outline(), &mut looks);
        let looks = Rc::new(looks);

        let muted = cx.theme().muted_foreground;

        tree(
            &self.tree_state,
            move |ix, entry, _selected, _window, _cx| {
                let item = entry.item();
                let look = looks.get(&item.id).cloned();
                let (icon, detail, is_muted) = match look {
                    Some(look) => (look.icon, look.detail, look.muted),
                    None => (AppIcon::File, None, false),
                };

                ListItem::new(ix)
                    .w_full()
                    .pl(TREE_INDENT * entry.depth() + TREE_PADDING)
                    .pr(TREE_PADDING)
                    .child(
                        h_flex()
                            .w_full()
                            .gap_1p5()
                            .items_center()
                            .child(Icon::new(icon).xsmall().flex_shrink_0())
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .text_xs()
                                    .when(is_muted, |this| this.text_color(muted))
                                    .child(item.label.clone()),
                            )
                            // The dimmed trailing text — a column's type, an
                            // index's uniqueness. `min_w_0` so it truncates instead
                            // of widening the panel.
                            .when_some(detail, |this, detail| {
                                this.child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .text_xs()
                                        .truncate()
                                        .text_color(muted)
                                        .child(detail),
                                )
                            }),
                    )
            },
        )
        .size_full()
        .into_any_element()
    }
}

/// A panel section's header: a label and one action, on one row.
fn section_header(label: SharedString, action: AnyElement, cx: &gpui::App) -> AnyElement {
    h_flex()
        .w_full()
        .px_2()
        .py_1p5()
        .gap_2()
        .items_center()
        .justify_between()
        .child(
            div()
                .text_xs()
                .font_medium()
                .text_color(cx.theme().muted_foreground)
                .child(label),
        )
        .child(action)
        .into_any_element()
}

/// The glyph beside a connection's name.
fn engine_icon(engine: Engine) -> AppIcon {
    match engine {
        Engine::PostgreSql | Engine::Sqlite => AppIcon::Database,
    }
}
