//! The left panel: **one tree**, whose roots are the saved connections.
//!
//! Round 1 stacked a connection list with inline action buttons on top of a
//! separate object tree. This is the one tree that replaced them, and three
//! things about it are worth stating because none is obvious from the widget
//! API:
//!
//! - **The disclosure arrow is ours.** `gpui_component`'s tree draws no
//!   triangle at all — `render_item` returns a `ListItem` and the widget wraps
//!   it, adding nothing — so [`disclosure`] draws the chevron from
//!   `TreeEntry::is_folder` / `is_expanded`, and a leaf gets a spacer of the
//!   same width so labels stay in one column. What the widget *does* own is the
//!   rule that makes a folder a folder: `is_folder` is `children.len() > 0`,
//!   which is why `state::tree` gives every expandable node a placeholder child.
//! - **The per-connection actions are a right-click menu**, built with the
//!   tree's own `context_menu` builder rather than hand-rolled. An action that
//!   does not apply to the current status is *disabled* rather than shown and
//!   silently doing nothing — Connect is off while a connection is in flight,
//!   Disconnect is off unless there is a session to end.
//! - **The hover card never carries the password**, in any form, not even
//!   masked. Its rows come from
//!   [`ConnectionProfile::details`](crate::database::models::connection::ConnectionProfile::details),
//!   which has no way to produce one and a test that says so.

use std::collections::HashMap;
use std::rc::Rc;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, InteractiveElement as _, IntoElement, ParentElement as _, SharedString,
    StatefulInteractiveElement as _, Styled as _, div, px,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::list::ListItem;
use gpui_component::menu::{PopupMenu, PopupMenuItem};
use gpui_component::tooltip::Tooltip;
use gpui_component::tree::{TreeEntry, tree};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Icon, Sizable as _, StyledExt as _, h_flex, v_flex,
};

use crate::app_icon::AppIcon;
use crate::database::components::notice::{Tone, notice};
use crate::database::components::states::empty_state;
use crate::database::models::engine::Engine;
use crate::database::state::tree::RowRef;
use crate::database::views::database::{
    ConnectionLook, DatabaseView, RowLook, TREE_INDENT, TREE_PADDING, row_looks,
};
use crate::i18n::{Str, t};

/// The width of the disclosure column. Wide enough for the chevron, and every
/// row reserves it so a leaf's label lines up with a folder's.
const ARROW_WIDTH: gpui::Pixels = px(14.);

impl DatabaseView {
    /// The whole left panel: a header with the two actions that are not
    /// per-connection, and the tree.
    pub(super) fn render_panel(&mut self, cx: &mut gpui::Context<Self>) -> AnyElement {
        let store_error = self.store_error.clone();

        v_flex()
            .size_full()
            .min_w_0()
            .child(self.render_header(cx))
            .when_some(store_error, |this, error| {
                this.child(
                    div()
                        .px_2()
                        .pb_1()
                        .child(notice(Tone::Danger, t(error, cx), cx)),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(if self.connections.is_empty() {
                        self.render_no_connections(cx)
                    } else {
                        self.render_tree(cx)
                    }),
            )
            .into_any_element()
    }

    fn render_header(&self, cx: &mut gpui::Context<Self>) -> AnyElement {
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
                    .child(t(Str::DbConnections, cx)),
            )
            .child(
                h_flex()
                    .gap_1()
                    .child(
                        Button::new("db-refresh-tree")
                            .ghost()
                            .xsmall()
                            .icon(AppIcon::Refresh)
                            .tooltip(t(Str::DbRefreshTree, cx))
                            // Refresh re-reads the *selected* connection, which
                            // is the only one the word can mean once the tree
                            // holds several.
                            .disabled(self.active_driver().is_none())
                            .on_click(cx.listener(|this, _, _, cx| this.refresh_tree(cx))),
                    )
                    .child(
                        Button::new("db-new-connection")
                            .ghost()
                            .xsmall()
                            .icon(AppIcon::Plus)
                            .tooltip(t(Str::DbNewConnection, cx))
                            .on_click(cx.listener(|this, _, window, cx| {
                                let draft = this.connections.draft(Engine::PostgreSql);
                                this.open_form(draft, false, window, cx);
                            })),
                    ),
            )
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

    fn render_tree(&mut self, cx: &mut gpui::Context<Self>) -> AnyElement {
        // `TreeItem` has room for one string, so everything else about a row is
        // looked up by element id as it is drawn. The map is rebuilt per frame
        // from the same outline the items came from, so the two cannot disagree.
        let mut looks: HashMap<SharedString, RowLook> = HashMap::new();
        row_looks(&self.outline(), &self.connections, &mut looks, cx);
        let looks = Rc::new(looks);

        let muted = cx.theme().muted_foreground;
        // The row the Execute button will run against. With several roots in
        // one tree this is the only thing on screen that says which, so it is
        // marked rather than merely bolded.
        let accent = cx.theme().accent;
        let radius = cx.theme().radius;
        let selected_connection = self.connections.selected_id();
        let view = cx.entity();

        tree(&self.tree_state, {
            let looks = looks.clone();
            let view = view.clone();
            move |ix, entry, _selected, _window, _cx| {
                let item = entry.item();
                let row = ListItem::new(ix)
                    .w_full()
                    .pl(TREE_INDENT * entry.depth() + TREE_PADDING)
                    .pr(TREE_PADDING);

                match looks.get(&item.id) {
                    Some(RowLook::Connection(look)) => {
                        let selected = selected_connection == Some(look.id);
                        let id = look.id;
                        let view = view.clone();
                        row.on_click(move |_, _, cx| {
                            view.update(cx, |this, cx| this.select(id, cx));
                        })
                        .child(connection_row(
                            entry,
                            look,
                            item.label.clone(),
                            selected.then_some((accent, radius)),
                        ))
                    }
                    Some(RowLook::Object {
                        icon,
                        detail,
                        muted: is_muted,
                        open,
                    }) => {
                        let row = row.child(object_row(
                            entry,
                            *icon,
                            item.label.clone(),
                            detail.clone(),
                            *is_muted,
                            muted,
                        ));
                        match (open.clone(), RowRef::parse(&item.id)) {
                            (Some(target), Some(reference)) => {
                                let view = view.clone();
                                row.on_click(move |event, _, cx| {
                                    if event.click_count() == 2 {
                                        let target = target.clone();
                                        view.update(cx, |this, cx| {
                                            this.open_detail(reference.connection, target, cx)
                                        });
                                    }
                                })
                            }
                            _ => row,
                        }
                    }
                    // Only reachable if the outline and the look map disagree,
                    // which they cannot — but a panic here would be a crash on
                    // a redraw.
                    None => row.child(object_row(
                        entry,
                        AppIcon::File,
                        item.label.clone(),
                        None,
                        false,
                        muted,
                    )),
                }
            }
        })
        .context_menu(move |_ix, entry, menu, _window, cx| {
            let Some(RowLook::Connection(look)) = looks.get(&entry.item().id) else {
                // Object rows have no actions of their own in this round, and an
                // empty menu is not shown at all.
                return menu;
            };
            connection_menu(menu, look, &view, cx)
        })
        .size_full()
        .into_any_element()
    }
}

/// A connection's root row: the disclosure arrow, the engine's mark, the status
/// dot, the name, and the status in words.
fn connection_row(
    entry: &TreeEntry,
    look: &ConnectionLook,
    label: SharedString,
    selected: Option<(gpui::Hsla, gpui::Pixels)>,
) -> AnyElement {
    let details = look.details.clone();
    let is_selected = selected.is_some();

    h_flex()
        .id(("db-connection", look.id as usize))
        .w_full()
        .gap_1p5()
        .items_center()
        .when_some(selected, |this, (accent, radius)| {
            this.px_1().bg(accent).rounded(radius)
        })
        .child(disclosure(entry))
        .child(Icon::new(look.icon).xsmall().flex_shrink_0())
        // The dot and the word both: a colour alone is not a label, and two of
        // the four states differ only by hue.
        .child(
            div()
                .size(px(6.))
                .rounded_full()
                .bg(look.dot)
                .flex_shrink_0(),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_xs()
                .truncate()
                .when(is_selected, |this| this.font_medium())
                .child(label),
        )
        .child(
            div()
                .flex_shrink_0()
                .text_xs()
                .text_color(look.dot)
                .child(look.status.clone()),
        )
        .tooltip(move |window, cx| {
            let details = details.clone();
            Tooltip::element(move |_, cx| detail_card(&details, cx)).build(window, cx)
        })
        .into_any_element()
}

/// The hover card: a plain label/value list, values in the monospace face
/// because most of them are addresses. **No password row exists.**
fn detail_card(details: &[(SharedString, SharedString)], cx: &gpui::App) -> AnyElement {
    v_flex()
        .gap_0p5()
        .children(details.iter().map(|(label, value)| {
            h_flex()
                .gap_3()
                .items_baseline()
                .justify_between()
                .child(
                    div()
                        .flex_shrink_0()
                        .whitespace_nowrap()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(label.clone()),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .whitespace_nowrap()
                        .text_xs()
                        .font_family(cx.theme().mono_font_family.clone())
                        .child(value.clone()),
                )
        }))
        .into_any_element()
}

/// An object row — a database, a schema, a table, a column, or a placeholder.
fn object_row(
    entry: &TreeEntry,
    icon: AppIcon,
    label: SharedString,
    detail: Option<SharedString>,
    is_muted: bool,
    muted: gpui::Hsla,
) -> AnyElement {
    h_flex()
        .w_full()
        .gap_1p5()
        .items_center()
        .child(disclosure(entry))
        .child(Icon::new(icon).xsmall().flex_shrink_0())
        .child(
            // Short, fixed-length text: `flex_shrink_0` and no wrapping, so the
            // dimmed detail beside it is what gives way.
            div()
                .flex_shrink_0()
                .whitespace_nowrap()
                .text_xs()
                .when(is_muted, |this| this.text_color(muted))
                .child(label),
        )
        // The dimmed trailing text — a column's type, an index's uniqueness.
        // `min_w_0` so it truncates instead of widening the panel.
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
        })
        .into_any_element()
}

/// The chevron, or a spacer the same width for a leaf.
///
/// The tree widget draws no disclosure indicator of its own; this is the whole
/// of it. `is_folder` is the widget's own `children.len() > 0`, which is why a
/// node whose children have not loaded yet still needs a placeholder child.
fn disclosure(entry: &TreeEntry) -> AnyElement {
    if !entry.is_folder() {
        return div().w(ARROW_WIDTH).flex_shrink_0().into_any_element();
    }

    let icon = if entry.is_expanded() {
        AppIcon::ChevronDown
    } else {
        AppIcon::ChevronRight
    };
    div()
        .w(ARROW_WIDTH)
        .flex_shrink_0()
        .child(Icon::new(icon).xsmall())
        .into_any_element()
}

/// The right-click menu on a connection root.
///
/// Every item that does not apply to the current status is **disabled** rather
/// than absent: a menu whose shape changes under the cursor is harder to learn
/// than one whose items grey out, and an action that is present and silently
/// does nothing is worse than both.
fn connection_menu(
    menu: PopupMenu,
    look: &ConnectionLook,
    view: &gpui::Entity<DatabaseView>,
    cx: &mut gpui::Context<gpui_component::tree::TreeState>,
) -> PopupMenu {
    let id = look.id;
    let connect_label = if look.connected {
        Str::DbDisconnect
    } else if look.failed {
        Str::DbReconnect
    } else {
        Str::DbConnect
    };

    menu.item(
        PopupMenuItem::new(t(connect_label, cx))
            .disabled(look.busy)
            .on_click({
                let view = view.clone();
                let connected = look.connected;
                move |_, _, cx| {
                    view.update(cx, |this, cx| {
                        if connected {
                            this.disconnect(id, cx);
                        } else {
                            this.connect(id, cx);
                        }
                    });
                }
            }),
    )
    .item(PopupMenuItem::separator())
    .item(
        PopupMenuItem::new(t(Str::DbEditConnection, cx))
            .icon(AppIcon::Settings)
            .on_click({
                let view = view.clone();
                move |_, window, cx| {
                    view.update(cx, |this, cx| {
                        if let Some(profile) = this.connections.find(id).cloned() {
                            this.open_form(profile, true, window, cx);
                        }
                    });
                }
            }),
    )
    .item(
        PopupMenuItem::new(t(Str::DbDuplicateConnection, cx))
            .icon(AppIcon::Copy)
            .on_click({
                let view = view.clone();
                move |_, _, cx| {
                    view.update(cx, |this, cx| this.duplicate(id, cx));
                }
            }),
    )
    .item(
        PopupMenuItem::new(t(Str::DbDeleteConnection, cx))
            .icon(AppIcon::Trash)
            .on_click({
                let view = view.clone();
                move |_, window, cx| {
                    view.update(cx, |this, cx| this.delete(id, window, cx));
                }
            }),
    )
}
