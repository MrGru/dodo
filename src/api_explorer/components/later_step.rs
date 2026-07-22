//! The pane a not-yet-built tab shows.
//!
//! Body, Auth, Scripts, Cookies, Tests and Console are all real tabs that this
//! phase does not implement. They say so — with the feature named and the step
//! it arrives in — rather than rendering blank, which reads as a bug, or
//! carrying a TODO comment, which the user never sees.

use gpui::{App, IntoElement, ParentElement as _, SharedString, Styled as _, div, px};
use gpui_component::{ActiveTheme as _, Icon, IconNamed, StyledExt as _, v_flex};

/// `title` names the feature; `detail` says when it arrives.
pub fn later_step(
    icon: impl IconNamed,
    title: SharedString,
    detail: SharedString,
    cx: &App,
) -> impl IntoElement {
    v_flex()
        .size_full()
        .items_center()
        .justify_center()
        .gap_2()
        .p_6()
        .text_color(cx.theme().muted_foreground)
        .child(Icon::new(icon).size(px(24.)))
        .child(div().text_sm().font_bold().child(title))
        .child(div().text_xs().text_center().max_w(px(320.)).child(detail))
}
