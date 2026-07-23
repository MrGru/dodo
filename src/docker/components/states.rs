//! The empty and error placeholders a page shows in place of its table.
//!
//! Both return a centred [`gpui::Div`] the caller finishes with an action — the
//! Create button under an empty state, the Retry button under an error — so the
//! action keeps its own listener while the frame stays reusable. Strings arrive
//! already translated; these are presentation helpers with no opinion on
//! localization.

use gpui::prelude::FluentBuilder as _;
use gpui::{App, Div, ParentElement as _, SharedString, Styled as _, div, px};
use gpui_component::{ActiveTheme as _, Icon, IconNamed, StyledExt as _, v_flex};

/// A centred "nothing here" panel: a glyph, a title and an optional hint. The
/// caller appends any action button as a further child.
pub fn empty_state(
    icon: impl IconNamed,
    title: SharedString,
    hint: Option<SharedString>,
    cx: &App,
) -> Div {
    v_flex()
        .size_full()
        .items_center()
        .justify_center()
        .gap_3()
        .p_6()
        .text_color(cx.theme().muted_foreground)
        .child(Icon::new(icon).size(px(30.)))
        .child(div().text_sm().child(title))
        .when_some(hint, |this, hint| {
            this.child(div().text_xs().text_center().max_w(px(280.)).child(hint))
        })
}

/// A centred error panel in the danger tone: an alert glyph, a title and the
/// engine's message. The caller appends the Retry button as a further child.
pub fn error_state(title: SharedString, message: SharedString, cx: &App) -> Div {
    let danger = cx.theme().danger;
    v_flex()
        .size_full()
        .items_center()
        .justify_center()
        .gap_3()
        .p_6()
        .child(
            Icon::new(crate::app_icon::AppIcon::AlertTriangle)
                .size(px(30.))
                .text_color(danger),
        )
        .child(div().text_sm().font_medium().child(title))
        .child(
            div()
                .text_xs()
                .text_center()
                .max_w(px(360.))
                .text_color(cx.theme().muted_foreground)
                .child(message),
        )
}
