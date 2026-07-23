//! The strip of open requests above the request editor.
//!
//! Each tab shows its method in the method's colour, the request's name, an
//! unsaved dot, and a close button; the `+` at the end opens another.

use gpui::prelude::FluentBuilder as _;
use gpui::{Context, IntoElement, ParentElement as _, Styled as _, div, px};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::tab::{Tab, TabBar};
use gpui_component::{ActiveTheme as _, Sizable as _, StyledExt as _, h_flex};

use crate::api_explorer::views::explorer::ApiExplorer;
use crate::app_icon::AppIcon;
use crate::i18n::{Str, t};

impl ApiExplorer {
    pub(super) fn render_tab_strip(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let active = self.ui.active_tab;

        let tabs: Vec<Tab> = self
            .tabs
            .iter()
            .enumerate()
            .map(|(index, tab)| {
                let state = tab.read(cx);
                let method = state.request.method;
                let dirty = state.request.dirty;
                let name = state.request.display_name(cx);

                Tab::new()
                    // The method label and close button otherwise sit flush
                    // against the tab's edges; a little horizontal padding gives
                    // each tab room to breathe.
                    .px_2()
                    .prefix(
                        div()
                            .text_xs()
                            .font_bold()
                            .text_color(method.color(cx))
                            .child(method.as_str()),
                    )
                    .label(name)
                    .suffix(
                        h_flex()
                            .items_center()
                            .gap_1()
                            .when(dirty, |this| {
                                // The unsaved dot. Drawn rather than an icon so
                                // it stays a dot at every font size.
                                this.child(
                                    div()
                                        .size(px(6.))
                                        .rounded_full()
                                        .bg(cx.theme().primary),
                                )
                            })
                            .child(
                                Button::new(("close-request-tab", index))
                                    .ghost()
                                    .xsmall()
                                    .icon(AppIcon::Close)
                                    .tooltip(t(Str::CloseRequest, cx))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.close_tab(index, cx);
                                    })),
                            ),
                    )
            })
            .collect();

        h_flex()
            .w_full()
            .min_w_0()
            .items_center()
            .overflow_hidden()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                TabBar::new("request-tabs")
                    .selected_index(active)
                    .children(tabs)
                    .suffix(
                        // Centered in a square slot the height of the tab strip,
                        // so the `+` sits in the middle of its cell rather than
                        // hard against the last tab.
                        h_flex()
                            .size(px(28.))
                            .items_center()
                            .justify_center()
                            .child(
                                Button::new("new-request-tab")
                                    .ghost()
                                    .xsmall()
                                    .icon(AppIcon::Plus)
                                    .tooltip(t(Str::NewRequest, cx))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.open_tab(window, cx);
                                    })),
                            ),
                    )
                    .on_click(cx.listener(|this, index: &usize, _, cx| {
                        this.ui.active_tab = *index;
                        cx.notify();
                    })),
            )
            .into_any_element()
    }
}
