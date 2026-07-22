//! The request bar and the Params / Headers / Body / Auth / Scripts tabs.

use gpui::{Context, Entity, IntoElement, ParentElement as _, Styled as _, Window, div, px};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::Input;
use gpui_component::popover::Popover;
use gpui_component::tab::{Tab, TabBar};
use gpui_component::{ActiveTheme as _, Disableable as _, Icon, Sizable as _, StyledExt as _, h_flex, v_flex};

use crate::api_explorer::components::key_value_table::{Table, key_value_table};
use crate::api_explorer::components::later_step::later_step;
use crate::api_explorer::models::method::HttpMethod;
use crate::api_explorer::state::request::RequestTab;
use crate::api_explorer::state::tab::RequestTabState;
use crate::api_explorer::views::explorer::ApiExplorer;
use crate::app_icon::AppIcon;
use crate::i18n::{Str, t};

impl ApiExplorer {
    pub(super) fn render_request_editor(
        &self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let Some(tab) = self.active_tab().cloned() else {
            return div().size_full().into_any_element();
        };

        v_flex()
            .size_full()
            .child(self.request_bar(&tab, cx))
            .child(self.request_tab_bar(&tab, cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .child(self.request_pane(&tab, cx)),
            )
            .into_any_element()
    }

    /// Method · URL · code generation · save · Send.
    fn request_bar(
        &self,
        tab: &Entity<RequestTabState>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let state = tab.read(cx);
        let in_flight = state.response.is_in_flight();
        let url = state.request.url.clone();

        h_flex()
            .w_full()
            .items_center()
            .gap_2()
            .p_2()
            .child(div().flex_shrink_0().child(self.method_picker(tab, cx)))
            // The URL is the only element that grows; `min_w_0` lets it give
            // way rather than pushing the buttons off the row.
            .child(
                div().flex_1().min_w_0().child(
                    Input::new(&url)
                        .font_family(cx.theme().mono_font_family.clone())
                        .w_full(),
                ),
            )
            .child(
                Button::new("generate-code")
                    .flex_shrink_0()
                    .ghost()
                    .icon(AppIcon::SquareCode)
                    // Visibly disabled with a tooltip that names the step it
                    // arrives in, rather than a control that does nothing.
                    .disabled(true)
                    .tooltip(t(Str::GenerateCodeLater, cx)),
            )
            .child(div().flex_shrink_0().child(self.save_button(tab, cx)))
            .child(
                Button::new("send-request")
                    .flex_shrink_0()
                    .primary()
                    .icon(AppIcon::Send)
                    .label(t(Str::Send, cx))
                    .loading(in_flight)
                    .disabled(in_flight)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.send_active(window, cx);
                    })),
            )
    }

    /// The colour-coded method dropdown.
    ///
    /// A `Popover` rather than a `Select`, because `Select` renders its trigger
    /// from a plain string and cannot colour it, and a `PopupMenu` would need
    /// nine separate actions to carry the choice.
    fn method_picker(
        &self,
        tab: &Entity<RequestTabState>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let method = tab.read(cx).request.method;

        let rows = HttpMethod::ALL.map(|candidate| {
            let tab = tab.clone();
            Button::new(("method-option", candidate as usize))
                .ghost()
                .w_full()
                .justify_start()
                .child(
                    div()
                        .font_bold()
                        .text_color(candidate.color(cx))
                        .child(candidate.as_str()),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    tab.update(cx, |state, cx| {
                        state.request.method = candidate;
                        state.request.dirty = true;
                        cx.notify();
                    });
                    this.method_menu_open = false;
                    cx.notify();
                }))
        });

        Popover::new("method-picker")
            .open(self.method_menu_open)
            .on_open_change(cx.listener(|this, open, _, cx| {
                this.method_menu_open = *open;
                cx.notify();
            }))
            .trigger(
                Button::new("method-trigger").outline().child(
                    h_flex()
                        .items_center()
                        .gap_1()
                        .child(
                            div()
                                .font_bold()
                                .text_color(method.color(cx))
                                .child(method.as_str()),
                        )
                        .child(Icon::new(AppIcon::ChevronDown).size(px(12.))),
                ),
            )
            .p_1()
            .w(px(140.))
            .children(rows)
    }

    /// Names the request for this session and clears the unsaved dot.
    ///
    /// Session-scoped on purpose: dodo persists nothing across restarts today
    /// (see the `dodo-theming-settings` skill), and inventing a store here
    /// would be a bigger decision than this button.
    fn save_button(
        &self,
        tab: &Entity<RequestTabState>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let tab = tab.clone();
        let name_input = self.name_input.clone();
        let confirm_input = self.name_input.clone();

        Popover::new("save-request")
            .open(self.save_menu_open)
            .on_open_change(cx.listener(|this, open, window, cx| {
                this.save_menu_open = *open;
                if *open {
                    // Open with the current name, so naming twice edits rather
                    // than retypes.
                    let current = this
                        .active_tab()
                        .map(|tab| tab.read(cx).request.display_name(cx))
                        .unwrap_or_default();
                    this.name_input.update(cx, |state, cx| {
                        state.set_value(current, window, cx);
                    });
                }
                cx.notify();
            }))
            .trigger(
                Button::new("save-request-trigger")
                    .ghost()
                    .icon(AppIcon::Save)
                    .tooltip(t(Str::NameRequest, cx)),
            )
            .w(px(260.))
            .child(
                v_flex()
                    .gap_2()
                    .p_1()
                    .child(div().text_xs().font_bold().child(t(Str::NameRequest, cx)))
                    .child(Input::new(&name_input).small())
                    .child(
                        Button::new("save-request-confirm")
                            .primary()
                            .small()
                            .label(t(Str::SaveName, cx))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let name = confirm_input.read(cx).value();
                                let trimmed = name.trim();
                                if !trimmed.is_empty() {
                                    let name = trimmed.to_string();
                                    tab.update(cx, |state, cx| {
                                        state.request.name = Some(name.into());
                                        state.request.dirty = false;
                                        cx.notify();
                                    });
                                }
                                this.save_menu_open = false;
                                cx.notify();
                            })),
                    ),
            )
    }

    fn request_tab_bar(
        &self,
        tab: &Entity<RequestTabState>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let active = tab.read(cx).request.active_tab;
        let selected = RequestTab::ALL
            .iter()
            .position(|candidate| *candidate == active)
            .unwrap_or(0);
        let tab = tab.clone();

        h_flex()
            .w_full()
            .min_w_0()
            .px_2()
            .overflow_hidden()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                TabBar::new("request-panes")
                    .selected_index(selected)
                    .children(
                        RequestTab::ALL.map(|pane| Tab::new().label(t(pane.label(), cx))),
                    )
                    .on_click(cx.listener(move |_, index: &usize, _, cx| {
                        let Some(pane) = RequestTab::ALL.get(*index).copied() else {
                            return;
                        };
                        tab.update(cx, |state, cx| {
                            state.request.active_tab = pane;
                            cx.notify();
                        });
                        cx.notify();
                    })),
            )
    }

    fn request_pane(
        &self,
        tab: &Entity<RequestTabState>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let pane = tab.read(cx).request.active_tab;
        if !pane.is_implemented() {
            return later_step(
                AppIcon::SquareCode,
                t(pane.label(), cx),
                t(Str::ArrivesLater, cx),
                cx,
            )
            .into_any_element();
        }

        match pane {
            RequestTab::Headers => key_value_table(Table::Headers, tab, cx).into_any_element(),
            // Params is the default pane, and the only other implemented one.
            _ => key_value_table(Table::Params, tab, cx).into_any_element(),
        }
    }
}
