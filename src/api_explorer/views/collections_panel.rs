//! The Collections panel: a searchable tree of collections, folders and saved
//! requests, with per-node create / rename / duplicate / delete.
//!
//! The tree data and every edit live in
//! [`CollectionTree`](crate::api_explorer::models::collection::CollectionTree)
//! and on [`ApiExplorer`]; this file is the drawing. Search filters by name and
//! reveals matches; rename is an inline bar rather than a per-row popover so it
//! works the same for every node.

use gpui::prelude::FluentBuilder as _;
use gpui::{
    Context, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, div, px,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::Input;
use gpui_component::popover::Popover;
use gpui_component::{ActiveTheme as _, Icon, Sizable as _, StyledExt as _, h_flex, v_flex};

use crate::api_explorer::components::empty_state::empty_state;
use crate::api_explorer::models::collection::{Node, NodeId, NodeKind};
use crate::api_explorer::views::explorer::ApiExplorer;
use crate::app_icon::AppIcon;
use crate::i18n::{Str, t};

impl ApiExplorer {
    pub(super) fn render_collections_panel(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let query = self.search_input.read(cx).value().trim().to_lowercase();
        let error = self.collections.error();

        v_flex()
            .size_full()
            .child(self.collections_header(cx))
            .child(self.collections_search(cx))
            .when_some(error, |this, error| {
                this.child(
                    div()
                        .w_full()
                        .px_3()
                        .py_1p5()
                        .text_xs()
                        .text_color(cx.theme().danger)
                        .bg(cx.theme().danger.opacity(0.08))
                        .child(t(error, cx)),
                )
            })
            .child(
                div()
                    .id("collections-body")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .when(self.rename_target.is_some(), |this| {
                        this.child(self.rename_bar(cx))
                    })
                    .child(if self.collections.is_empty() {
                        empty_state(
                            AppIcon::Folder,
                            t(Str::NoCollections, cx),
                            Some(t(Str::NoCollectionsHint, cx)),
                            cx,
                        )
                        .into_any_element()
                    } else {
                        self.collections_tree(&query, cx)
                    }),
            )
            .into_any_element()
    }

    /// The caption and the import / new-collection actions.
    fn collections_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .items_center()
            .justify_between()
            .h(px(38.))
            .px_3()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                div()
                    .text_xs()
                    .font_bold()
                    .text_color(cx.theme().muted_foreground)
                    .child(t(Str::Collections, cx)),
            )
            .child(
                h_flex()
                    .gap_0p5()
                    .child(
                        Button::new("import-collection")
                            .ghost()
                            .xsmall()
                            .icon(AppIcon::Import)
                            .tooltip(t(Str::ImportCollection, cx))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.import_collections(cx);
                            })),
                    )
                    .child(
                        Button::new("new-collection")
                            .ghost()
                            .xsmall()
                            .icon(AppIcon::Plus)
                            .tooltip(t(Str::NewCollection, cx))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.create_collection(window, cx);
                            })),
                    ),
            )
    }

    fn collections_search(&self, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .w_full()
            .px_2()
            .py_1p5()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(Input::new(&self.search_input).small().cleanable(true))
    }

    /// The inline rename bar, shown at the top of the tree while a node is being
    /// renamed. Enter confirms (the field is single-line), the check saves, the
    /// cross cancels.
    fn rename_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .w_full()
            .items_center()
            .gap_1()
            .px_2()
            .py_1p5()
            .bg(cx.theme().muted.opacity(0.4))
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(Input::new(&self.rename_input).small()),
            )
            .child(
                Button::new("rename-confirm")
                    .primary()
                    .xsmall()
                    .icon(AppIcon::Save)
                    .tooltip(t(Str::Rename, cx))
                    .on_click(cx.listener(|this, _, _, cx| this.confirm_rename(cx))),
            )
            .child(
                Button::new("rename-cancel")
                    .ghost()
                    .xsmall()
                    .icon(AppIcon::Close)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.rename_target = None;
                        cx.notify();
                    })),
            )
    }

    /// The whole tree, filtered by `query`.
    fn collections_tree(&self, query: &str, cx: &mut Context<Self>) -> gpui::AnyElement {
        // Collected eagerly: each row is built with `&mut cx`, so the rows
        // cannot be a lazy iterator that keeps `cx` borrowed.
        let mut rows: Vec<gpui::AnyElement> = Vec::new();
        for node in self.collections.tree().roots() {
            if let Some(element) = self.render_node(node, 0, query, cx) {
                rows.push(element);
            }
        }
        v_flex().w_full().py_1().children(rows).into_any_element()
    }

    /// One node and, when it is an expanded container, its children. Returns
    /// `None` when a search query is active and neither the node nor any
    /// descendant matches.
    fn render_node(
        &self,
        node: &Node,
        depth: usize,
        query: &str,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let filtering = !query.is_empty();
        if filtering && !node_matches(node, query) {
            return None;
        }

        let id = node.id;
        let is_container = node.is_container();
        // A search reveals matches by forcing every surviving container open.
        let expanded = node.expanded || filtering;

        let mut column = v_flex().w_full();
        column = column.child(self.node_row(node, depth, expanded, cx));

        if is_container && expanded {
            let mut children: Vec<gpui::AnyElement> = Vec::new();
            for child in &node.children {
                if let Some(element) = self.render_node(child, depth + 1, query, cx) {
                    children.push(element);
                }
            }
            column = column.children(children);
        }

        // Keep the whole subtree keyed so re-renders don't collide ids.
        Some(column.id(("node-subtree", id as usize)).into_any_element())
    }

    /// The single row for a node: the disclosure chevron, an icon, the name
    /// (clickable — a request opens, a container toggles), and the actions menu.
    fn node_row(
        &self,
        node: &Node,
        depth: usize,
        expanded: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let id = node.id;
        let is_container = node.is_container();
        let icon = node_icon(node, expanded);
        let name = node.name.clone();
        let indent = px(8. + depth as f32 * 14.);

        h_flex()
            .id(("node-row", id as usize))
            .w_full()
            .items_center()
            .gap_1()
            .pl(indent)
            .pr_1()
            .py(px(3.))
            .text_sm()
            .hover(|this| this.bg(cx.theme().accent.opacity(0.5)))
            .child(if is_container {
                Button::new(("node-toggle", id as usize))
                    .ghost()
                    .xsmall()
                    .icon(if expanded {
                        AppIcon::ChevronDown
                    } else {
                        AppIcon::ChevronRight
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.toggle_node(id, !expanded, cx);
                    }))
                    .into_any_element()
            } else {
                // Requests line up with a container's chevron column.
                div().w(px(20.)).flex_shrink_0().into_any_element()
            })
            .child(
                div()
                    .flex_shrink_0()
                    .text_color(cx.theme().muted_foreground)
                    .child(Icon::new(icon).size(px(14.))),
            )
            .child(
                div()
                    .id(("node-name", id as usize))
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .cursor_pointer()
                    .child(name)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if is_container {
                            this.toggle_node(id, !expanded, cx);
                        } else {
                            this.open_saved_request(id, window, cx);
                        }
                    })),
            )
            .child(self.node_actions(node, cx))
    }

    /// The per-row actions menu.
    fn node_actions(&self, node: &Node, cx: &mut Context<Self>) -> impl IntoElement {
        let id = node.id;
        let is_container = node.is_container();

        let action = |label: Str,
                      icon: AppIcon,
                      key: &'static str,
                      cx: &mut Context<Self>,
                      handler: fn(
            &mut ApiExplorer,
            NodeId,
            &mut gpui::Window,
            &mut Context<ApiExplorer>,
        )| {
            Button::new((key, id as usize))
                .ghost()
                .xsmall()
                .w_full()
                .justify_start()
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(Icon::new(icon).size(px(13.)))
                        .child(t(label, cx)),
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.node_menu_open = None;
                    handler(this, id, window, cx);
                }))
        };

        let mut menu = v_flex().gap_0p5().p_1();
        if is_container {
            menu = menu.child(action(
                Str::NewFolder,
                AppIcon::Folder,
                "node-new-folder",
                cx,
                |this, id, window, cx| this.create_folder(id, window, cx),
            ));
        } else {
            menu = menu.child(action(
                Str::Open,
                AppIcon::Send,
                "node-open",
                cx,
                |this, id, window, cx| this.open_saved_request(id, window, cx),
            ));
        }
        menu = menu
            .child(action(
                Str::Rename,
                AppIcon::SquareCode,
                "node-rename",
                cx,
                |this, id, window, cx| this.begin_rename(id, window, cx),
            ))
            .child(action(
                Str::Duplicate,
                AppIcon::Copy,
                "node-duplicate",
                cx,
                |this, id, _window, cx| this.duplicate_node(id, cx),
            ))
            .child(action(
                Str::Delete,
                AppIcon::Trash,
                "node-delete",
                cx,
                |this, id, _window, cx| this.delete_node(id, cx),
            ));

        Popover::new(("node-menu", id as usize))
            .open(self.node_menu_open == Some(id))
            .on_open_change(cx.listener(move |this, open, _, cx| {
                this.node_menu_open = if *open { Some(id) } else { None };
                cx.notify();
            }))
            .trigger(
                Button::new(("node-menu-trigger", id as usize))
                    .ghost()
                    .xsmall()
                    .icon(AppIcon::Ellipsis)
                    .tooltip(t(Str::MoreActions, cx)),
            )
            .w(px(180.))
            .child(menu)
    }
}

/// Whether a node or any descendant's name contains the (lowercased) query.
fn node_matches(node: &Node, query: &str) -> bool {
    node.name.to_lowercase().contains(query)
        || node.children.iter().any(|child| node_matches(child, query))
}

/// The icon for a node: an open/closed folder for containers, a file for a
/// saved request.
fn node_icon(node: &Node, expanded: bool) -> AppIcon {
    match node.kind {
        NodeKind::Collection | NodeKind::Folder => {
            if expanded {
                AppIcon::FolderOpen
            } else {
                AppIcon::Folder
            }
        }
        NodeKind::Request(_) => AppIcon::File,
    }
}
