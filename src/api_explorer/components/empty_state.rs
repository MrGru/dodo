//! The centred "there is nothing here yet" panel.
//!
//! Four places need one — the Collections panel and the Cookies, Tests and
//! Console response tabs — and the library has no equivalent, so this is the
//! app's own. Kept to a glyph, a line and an optional hint so that it reads the
//! same everywhere it appears.

use gpui::prelude::FluentBuilder as _;
use gpui::{App, IntoElement, ParentElement as _, SharedString, Styled as _, div, px};
use gpui_component::{ActiveTheme as _, Icon, IconNamed, v_flex};

/// Builds an empty state from an icon, a line, and an optional hint under it.
///
/// The strings arrive already translated: this is a presentation helper and
/// deliberately has no opinion about localization.
pub fn empty_state(
    icon: impl IconNamed,
    title: SharedString,
    hint: Option<SharedString>,
    cx: &App,
) -> impl IntoElement {
    v_flex()
        .size_full()
        .items_center()
        .justify_center()
        .gap_2()
        .p_6()
        // Set once on the wrapper so the glyph and both lines share it.
        .text_color(cx.theme().muted_foreground)
        .child(Icon::new(icon).size(px(28.)))
        .child(div().text_sm().child(title))
        .when_some(hint, |this, hint| {
            this.child(div().text_xs().text_center().max_w(px(240.)).child(hint))
        })
}
