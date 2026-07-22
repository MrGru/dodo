//! The Auth tab: a scheme picker, and the fields that scheme needs.
//!
//! Nothing here builds a header. The fields are collected and handed on as
//! `AuthDraft`; `services::http::auth` is the only place that knows a bearer
//! token becomes `Authorization: Bearer …` or that an API key can ride in the
//! query string. That is why adding a scheme with different mechanics later
//! touches one service module rather than this view.

use gpui::prelude::FluentBuilder as _;
use gpui::{
    Context, Entity, InteractiveElement as _, IntoElement, ParentElement as _, Pixels,
    SharedString, StatefulInteractiveElement as _, Styled as _, div, px,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Input, InputState};
use gpui_component::popover::Popover;
use gpui_component::{
    ActiveTheme as _, Disableable as _, Icon, Selectable as _, Sizable as _, h_flex, v_flex,
};

use crate::api_explorer::components::empty_state::empty_state;
use crate::api_explorer::components::later_step::later_step;
use crate::api_explorer::models::auth::{ApiKeyLocation, AuthType};
use crate::api_explorer::state::tab::RequestTabState;
use crate::api_explorer::views::explorer::ApiExplorer;
use crate::app_icon::AppIcon;
use crate::i18n::{Str, t};

/// Width of the label column beside each field, so the inputs line up.
const LABEL_COLUMN: Pixels = px(96.);

impl ApiExplorer {
    pub(super) fn request_auth_pane(
        &self,
        tab: &Entity<RequestTabState>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        v_flex()
            .size_full()
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .px_3()
                    .py_2()
                    .child(self.auth_type_picker(tab, cx)),
            )
            .child(
                div()
                    .id("auth-fields")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(self.auth_fields(tab, cx)),
            )
            .into_any_element()
    }

    /// The scheme dropdown, built the same way as the method and body pickers.
    fn auth_type_picker(
        &self,
        tab: &Entity<RequestTabState>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let current = tab.read(cx).request.auth_type;

        let rows = AuthType::ALL.map(|candidate| {
            let tab = tab.clone();
            Button::new(("auth-type-option", candidate as usize))
                .ghost()
                .w_full()
                .justify_start()
                .selected(candidate == current)
                // OAuth 2.0 is shown disabled with its reason attached rather
                // than hidden, the same convention the rest of the tool uses.
                .disabled(!candidate.is_available())
                .when(!candidate.is_available(), |this| {
                    this.tooltip(t(Str::OAuth2Later, cx))
                })
                .label(t(candidate.label(), cx))
                .on_click(cx.listener(move |this, _, _, cx| {
                    tab.update(cx, |state, cx| {
                        state.request.auth_type = candidate;
                        state.request.dirty = true;
                        cx.notify();
                    });
                    this.auth_menu_open = false;
                    cx.notify();
                }))
        });

        Popover::new("auth-type-picker")
            .open(self.auth_menu_open)
            .on_open_change(cx.listener(|this, open, _, cx| {
                this.auth_menu_open = *open;
                cx.notify();
            }))
            .trigger(
                Button::new("auth-type-trigger")
                    .outline()
                    .xsmall()
                    .tooltip(t(Str::AuthTypeLabel, cx))
                    .child(
                        h_flex()
                            .items_center()
                            .gap_1()
                            .child(t(current.label(), cx))
                            .child(Icon::new(AppIcon::ChevronDown).size(px(12.))),
                    ),
            )
            .p_1()
            .w(px(200.))
            .children(rows)
    }

    fn auth_fields(
        &self,
        tab: &Entity<RequestTabState>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        // Everything the fields need is copied out before anything borrows
        // `cx` mutably: `tab.read` holds an immutable borrow of it, and the
        // builders below all want the mutable one.
        let (auth_type, location, token, username, password, key_name, key_value) = {
            let request = &tab.read(cx).request;
            (
                request.auth_type,
                request.auth_key_location,
                request.auth_token.clone(),
                request.auth_username.clone(),
                request.auth_password.clone(),
                request.auth_key_name.clone(),
                request.auth_key_value.clone(),
            )
        };

        match auth_type {
            AuthType::None => empty_state(
                AppIcon::Globe,
                t(Str::NoAuthTitle, cx),
                Some(t(Str::NoAuthHint, cx)),
                cx,
            )
            .into_any_element(),

            AuthType::Bearer => field_column()
                .child(field_row(t(Str::AuthTokenLabel, cx), &token, cx))
                .into_any_element(),

            AuthType::Basic => field_column()
                .child(field_row(t(Str::AuthUsernameLabel, cx), &username, cx))
                .child(field_row(t(Str::AuthPasswordLabel, cx), &password, cx))
                .into_any_element(),

            AuthType::ApiKey => field_column()
                .child(field_row(t(Str::ApiKeyNameLabel, cx), &key_name, cx))
                .child(field_row(t(Str::ApiKeyValueLabel, cx), &key_value, cx))
                .child(self.api_key_location_row(tab, location, cx))
                .into_any_element(),

            // Unreachable through the picker, which shows it disabled; stated
            // here as well so the pane can never render blank.
            AuthType::OAuth2 => later_step(
                AppIcon::Globe,
                t(Str::AuthTypeOAuth2, cx),
                t(Str::OAuth2Later, cx),
                cx,
            )
            .into_any_element(),
        }
    }

    /// Header or query parameter, as a pair of selectable buttons — the same
    /// treatment the response viewer gives Pretty / Raw.
    fn api_key_location_row(
        &self,
        tab: &Entity<RequestTabState>,
        current: ApiKeyLocation,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let options = ApiKeyLocation::ALL.map(|candidate| {
            let tab = tab.clone();
            Button::new(("api-key-location", candidate as usize))
                .ghost()
                .xsmall()
                .selected(candidate == current)
                .label(t(candidate.label(), cx))
                .on_click(cx.listener(move |_, _, _, cx| {
                    tab.update(cx, |state, cx| {
                        state.request.auth_key_location = candidate;
                        state.request.dirty = true;
                        cx.notify();
                    });
                    cx.notify();
                }))
        });

        h_flex()
            .w_full()
            .items_center()
            .gap_2()
            .child(
                div()
                    .w(LABEL_COLUMN)
                    .flex_shrink_0()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(t(Str::ApiKeySendAs, cx)),
            )
            .child(h_flex().items_center().gap_1().children(options))
    }
}

/// The column the field rows sit in. Bounded so the inputs do not stretch the
/// width of a wide window, which reads as a form nobody laid out.
fn field_column() -> gpui::Div {
    v_flex().w_full().max_w(px(560.)).gap_2().p_3()
}

/// One labelled field.
fn field_row(
    label: SharedString,
    input: &Entity<InputState>,
    cx: &mut Context<ApiExplorer>,
) -> impl IntoElement {
    h_flex()
        .w_full()
        .items_center()
        .gap_2()
        .child(
            div()
                .w(LABEL_COLUMN)
                .flex_shrink_0()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(label),
        )
        .child(
            div().flex_1().min_w_0().child(
                Input::new(input)
                    .small()
                    .font_family(cx.theme().mono_font_family.clone())
                    .w_full(),
            ),
        )
}
