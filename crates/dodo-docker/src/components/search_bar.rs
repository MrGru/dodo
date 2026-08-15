//! A search input with a leading magnifier and a clear button.
//!
//! Generic: it renders whatever `InputState` it is handed. The placeholder and
//! the change subscription belong to the owning view, so filtering is instant
//! and the language sweep can re-push the placeholder — the same arrangement the
//! API Explorer's collections search uses.

use gpui::{Entity, IntoElement, Styled as _, px};
use gpui_component::Sizable as _;
use gpui_component::input::{Input, InputState};

use crate::app_icon::AppIcon;

/// The search field, sized to `width`, with a magnifier prefix and a clear
/// button that appears once there is text.
pub fn search_bar(state: &Entity<InputState>, width: gpui::Pixels) -> impl IntoElement {
    Input::new(state)
        .small()
        .cleanable(true)
        .prefix(AppIcon::Search.view().size(px(15.)))
        .w(width)
}
