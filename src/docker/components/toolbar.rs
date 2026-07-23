//! The pre-styled row a page's controls sit in.
//!
//! Returns a bordered, padded [`gpui::Div`] the caller fills with its search
//! bar, refresh button and (future) filter/create/bulk controls. Kept a bare
//! container rather than a builder so a page composes it with the flex helpers
//! it already uses, and so the Images/Volumes/Networks pages get the identical
//! frame for free.

use gpui::{Div, Styled as _};
use gpui_component::{ActiveTheme as _, h_flex};

/// A horizontal toolbar container: full width, spaced, with a bottom rule.
pub fn toolbar(cx: &gpui::App) -> Div {
    h_flex()
        .w_full()
        .flex_shrink_0()
        .items_center()
        .gap_2()
        .px_3()
        .py_2()
        .border_b_1()
        .border_color(cx.theme().border)
}
