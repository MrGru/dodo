//! The response status badge and the small metrics beside it.

use gpui::{App, IntoElement, ParentElement as _, SharedString, Styled as _, div, px};
use gpui_component::tag::Tag;
use gpui_component::{ActiveTheme as _, Icon, IconNamed, Sizable as _, StyledExt as _, h_flex};

use crate::api_explorer::models::exchange::StatusClass;

/// The status number, in the colour of its class.
///
/// `Tag::custom` takes background, foreground and border, so the class colour
/// is used at low opacity behind its own full-strength text — the same
/// treatment the app's error banner already uses for `danger`.
pub fn status_tag(status: u16, class: StatusClass, cx: &App) -> impl IntoElement {
    let color = class.color(cx);
    Tag::custom(color.opacity(0.15), color, color.opacity(0.4))
        .small()
        .rounded(cx.theme().radius)
        .child(div().font_bold().child(format!("{status}")))
}

/// An icon and a value, as used for duration and response size.
pub fn metric(icon: impl IconNamed, value: SharedString, cx: &App) -> impl IntoElement {
    h_flex()
        .items_center()
        .gap_1p5()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(Icon::new(icon).size(px(13.)))
        .child(div().child(value))
}
