//! Small render helpers the round-3 list pages (Images, Volumes, Networks)
//! share, factored out so the three views do not each re-declare them.
//!
//! These are the container view's private helpers generalised: a header cell, a
//! per-row action button, the "now" clock for relative times, and the count cell
//! the "containers using" column renders. Anything that depends on a specific
//! view's `Self` (its refresh listener, its delete confirmation) stays in that
//! view; only the `Self`-free pieces live here.

use std::time::{SystemTime, UNIX_EPOCH};

use gpui::{
    App, ClickEvent, Div, ParentElement as _, SharedString, Styled as _, Window, div,
};
use gpui_component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_component::{ActiveTheme as _, Disableable as _, Sizable as _};

use crate::app_icon::AppIcon;

/// A header cell: a `div` carrying the caption, truncating if the column is
/// squeezed. The caller sets the width.
pub fn header_cell(label: SharedString) -> Div {
    div().truncate().child(label)
}

/// A muted cell rendering a "containers using" count as plain text. The number
/// is not language, so it is not translated.
pub fn count_cell(count: usize, cx: &App) -> Div {
    div()
        .text_color(cx.theme().muted_foreground)
        .child(SharedString::from(count.to_string()))
}

/// One small, tooltip-bearing action button, disabled when the action is not
/// available (a placeholder Inspect, or Delete on a predefined network).
pub fn action_button(
    id: SharedString,
    icon: AppIcon,
    tooltip: SharedString,
    enabled: bool,
    variant: ButtonVariant,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Button {
    Button::new(id)
        .xsmall()
        .with_variant(variant)
        .icon(icon)
        .tooltip(tooltip)
        .disabled(!enabled)
        .on_click(on_click)
}

/// A disabled placeholder button — the Inspect action later rounds fill in. Kept
/// present but inert so the row's action set does not shift when it lands.
pub fn placeholder_button(id: SharedString, icon: AppIcon, tooltip: SharedString) -> Button {
    Button::new(id)
        .xsmall()
        .ghost()
        .icon(icon)
        .tooltip(tooltip)
        .disabled(true)
}

/// One value cell in the muted foreground tone, the treatment every secondary
/// column uses.
pub fn muted_cell(text: SharedString, cx: &App) -> Div {
    div().text_color(cx.theme().muted_foreground).child(text)
}

/// Now, in Unix seconds, for relative-time formatting. A clock before the epoch
/// is impossible in practice; `0` is a harmless fallback.
pub fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|delta| delta.as_secs() as i64)
        .unwrap_or(0)
}
