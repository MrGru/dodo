//! A colored status badge: a caption in the colour of its state.
//!
//! Generic on purpose — it takes a label and a colour, not a container status —
//! so any page can badge anything (an image's in-use state, a volume's driver)
//! with the same shape. The colour is used at low opacity behind its own
//! full-strength text, the treatment the API Explorer's status tag and the
//! app's error banner both use.

use gpui::{App, Hsla, IntoElement, ParentElement as _, SharedString, div};
use gpui_component::tag::Tag;
use gpui_component::{ActiveTheme as _, Sizable as _, StyledExt as _};

/// A pill reading `label` in `color`.
pub fn status_badge(label: SharedString, color: Hsla, cx: &App) -> impl IntoElement {
    Tag::custom(color.opacity(0.15), color, color.opacity(0.4))
        .small()
        .rounded(cx.theme().radius)
        .child(div().font_medium().child(label))
}
