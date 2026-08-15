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
    App, ClickEvent, Context, Div, FocusHandle, InteractiveElement as _, ParentElement as _,
    SharedString, Stateful, StatefulInteractiveElement as _, Styled as _, Window, div,
};
use gpui_component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_component::menu::PopupMenu;
use gpui_component::tooltip::Tooltip;
use gpui_component::{ActiveTheme as _, Disableable as _, Sizable as _, StyledExt as _};

use crate::app_icon::AppIcon;
use crate::docker::{DockerContextDelete, DockerContextInspect};
use crate::i18n::{docker, shared, t};

/// A header cell: a `div` carrying the caption, truncating if the column is
/// squeezed. The caller sets the width.
pub fn header_cell(label: SharedString) -> Div {
    div().truncate().child(label)
}

/// A row's identifying cell — Name on Containers, Networks and Volumes,
/// Repository on Images — as the click target that opens the row's detail dialog.
///
/// This replaced the per-row eye and logs icons, so the affordance has to be
/// *visible*: it takes the theme's link colour with an underline on hover, a
/// pointer cursor throughout, and a tooltip naming what a click does. The same
/// dialog is reachable from the row's `enter` key (see
/// [`DockerOpenDetail`](crate::docker::DockerOpenDetail)) and from the row's
/// right-click menu, so nothing here is the only route to it.
///
/// Left mouse-down is deliberately *not* stopped: it has to reach the page root's
/// `track_focus`, so the list is the focused handle when the dialog opens and is
/// therefore the handle `Root::close_dialog` restores.
///
/// The caller sets the width (every page's identifying column is `flex_1`).
pub fn name_cell(
    id: SharedString,
    text: SharedString,
    tooltip: SharedString,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> Stateful<Div> {
    let link = cx.theme().link;
    div()
        .id(id)
        .font_medium()
        .truncate()
        .cursor_pointer()
        .hover(|this| {
            this.text_color(link)
                .text_decoration_1()
                .text_decoration_color(link)
        })
        .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
        .on_click(on_click)
        .child(text)
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
pub fn coming_soon_button(
    id: SharedString,
    icon: AppIcon,
    label: SharedString,
    cx: &App,
) -> Button {
    Button::new(id)
        .small()
        .icon(icon)
        .label(label)
        .tooltip(t(docker::Text::ComingSoonLabel, cx))
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
        .menu_with_icon(
            t(docker::Text::Inspect, cx),
            AppIcon::Eye,
            Box::new(DockerContextInspect),
        )
        .separator()
        .menu_with_icon_and_disabled(
            t(shared::Text::Delete, cx),
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
