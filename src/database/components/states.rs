//! Centred placeholders shown in place of a panel's content.
//!
//! Deliberately the same shape as `docker::components::states` rather than
//! shared with it: the two modules are self-contained by design (see
//! `database/mod.rs`), and a shared placeholder is not worth a compile-time
//! edge between two tools.

use gpui::prelude::FluentBuilder as _;
use gpui::{App, Div, ParentElement as _, SharedString, Styled as _, div, px};
use gpui_component::{ActiveTheme as _, Icon, IconNamed, StyledExt as _, v_flex};

use crate::app_icon::AppIcon;

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
            this.child(div().text_xs().text_center().max_w(px(300.)).child(hint))
        })
}

/// A centred error panel in the danger tone. The caller appends any Retry
/// button as a further child.
pub fn error_state(title: SharedString, message: SharedString, cx: &App) -> Div {
    let danger = cx.theme().danger;
    v_flex()
        .size_full()
        .items_center()
        .justify_center()
        .gap_3()
        .p_6()
        .child(
            Icon::new(AppIcon::AlertTriangle)
                .size(px(30.))
                .text_color(danger),
        )
        .child(div().text_sm().font_medium().child(title))
        .child(
            div()
                .text_xs()
                .text_center()
                .max_w(px(420.))
                .text_color(cx.theme().muted_foreground)
                .child(message),
        )
}
