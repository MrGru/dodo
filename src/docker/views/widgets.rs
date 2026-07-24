//! Small render helpers the list pages share, factored out so the views do not
//! each re-declare them. Most are used by the round-3 three (Images, Volumes,
//! Networks); [`coming_soon_button`] is used by Containers as well.
//!
//! These are the container view's private helpers generalised: a header cell, a
//! per-row action button, the "now" clock for relative times, the count cell the
//! "containers using" column renders, the shared right-click menu, and the
//! disabled button a not-yet-built feature shows. Anything that depends on a
//! specific view's `Self` (its refresh listener, its delete confirmation) stays
//! in that view; only the `Self`-free pieces live here.

use std::time::{SystemTime, UNIX_EPOCH};

use gpui::{
    App, ClickEvent, Context, Div, FocusHandle, ParentElement as _, SharedString, Styled as _,
    Window, div,
};
use gpui_component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_component::menu::PopupMenu;
use gpui_component::{ActiveTheme as _, Disableable as _, Sizable as _};

use crate::app_icon::AppIcon;
use crate::docker::{DockerContextDelete, DockerContextInspect};
use crate::i18n::{Str, t};

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

/// A disabled placeholder button for a feature that is not built yet, tooltipped
/// "Coming soon" so it reads as a future feature rather than something broken.
/// The toolbar's Pull and Build are these.
pub fn coming_soon_button(id: SharedString, icon: AppIcon, label: SharedString, cx: &App) -> Button {
    Button::new(id)
        .small()
        .icon(icon)
        .label(label)
        .tooltip(t(Str::DockerComingSoonLabel, cx))
        .disabled(true)
}

/// One value cell in the muted foreground tone, the treatment every secondary
/// column uses.
pub fn muted_cell(text: SharedString, cx: &App) -> Div {
    div().text_color(cx.theme().muted_foreground).child(text)
}

/// The right-click menu the Images, Volumes and Networks pages share: Inspect,
/// which opens the read-only detail panel and is always available, then Delete
/// (disabled where the resource cannot be removed, e.g. a predefined network).
/// `action_context` points the actions at the view's focus handle so its
/// `on_action` handlers catch them; the view records which row was right-clicked
/// before the menu builds.
pub fn resource_context_menu(
    menu: PopupMenu,
    focus: FocusHandle,
    delete_enabled: bool,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    menu.action_context(focus)
        .menu_with_icon(t(Str::DockerInspect, cx), AppIcon::Eye, Box::new(DockerContextInspect))
        .separator()
        .menu_with_icon_and_disabled(
            t(Str::Delete, cx),
            AppIcon::Trash,
            Box::new(DockerContextDelete),
            !delete_enabled,
        )
}

/// Now, in Unix seconds, for relative-time formatting. A clock before the epoch
/// is impossible in practice; `0` is a harmless fallback.
pub fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|delta| delta.as_secs() as i64)
        .unwrap_or(0)
}
