//! The Collections panel: header, actions, and (in this phase) an empty state.
//!
//! The contents are static here. The panel already reads
//! [`CollectionState`](crate::api_explorer::state::collection::CollectionState),
//! so phase 3 fills that in and this file changes only to render a tree in the
//! branch that currently renders nothing.

use gpui::{Context, IntoElement, ParentElement as _, Styled as _, div, px};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::{ActiveTheme as _, Disableable as _, Sizable as _, StyledExt as _, h_flex, v_flex};

use crate::api_explorer::components::empty_state::empty_state;
use crate::api_explorer::views::explorer::ApiExplorer;
use crate::app_icon::AppIcon;
use crate::i18n::{Str, t};

impl ApiExplorer {
    pub(super) fn render_collections_panel(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        v_flex()
            .size_full()
            .border_r_1()
            .border_color(cx.theme().border)
            .child(self.collections_header(cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .child(if self.collections.is_empty() {
                        empty_state(
                            AppIcon::Folder,
                            t(Str::NoCollections, cx),
                            Some(t(Str::NoCollectionsHint, cx)),
                            cx,
                        )
                        .into_any_element()
                    } else {
                        // Phase 3 renders the tree here; today the state can
                        // never be non-empty, so this branch stays a container.
                        v_flex()
                            .size_full()
                            .children(self.collections.all().iter().map(|collection| {
                                div()
                                    .px_3()
                                    .py_1p5()
                                    .text_sm()
                                    .child(collection.name.clone())
                            }))
                            .into_any_element()
                    }),
            )
            .into_any_element()
    }

    /// The caption and the two action buttons, matching the reference's
    /// import / new-collection pair.
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
                            // Never a dead control: the button is visibly
                            // disabled and says when it starts working.
                            .disabled(true)
                            .tooltip(t(Str::ImportCollectionLater, cx)),
                    )
                    .child(
                        Button::new("new-collection")
                            .ghost()
                            .xsmall()
                            .icon(AppIcon::Plus)
                            .disabled(true)
                            .tooltip(t(Str::NewCollectionLater, cx)),
                    )
                    .child(
                        Button::new("collapse-collections")
                            .ghost()
                            .xsmall()
                            .icon(AppIcon::PanelLeftClose)
                            .tooltip(t(Str::HideCollections, cx))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.ui.collections_collapsed = true;
                                cx.notify();
                            })),
                    ),
            )
    }
}
