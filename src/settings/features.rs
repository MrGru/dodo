use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::setting::{SettingGroup, SettingItem, SettingPage};
use gpui_component::switch::Switch;
use gpui_component::tooltip::Tooltip;
use gpui_component::{ActiveTheme as _, Disableable as _, Sizable as _, h_flex, v_flex};

use crate::app_icon::AppIcon;
use crate::i18n::{Str, shell, t};
use crate::layout::Layout;
use crate::session::models::features::FeatureError;
use crate::tools::View;

/// Height of one row on the Features page.
///
/// Fixed so that the rows are a regular ladder however long a tool's name is,
/// which is what makes a drag land where it looks like it will. `h_8` is the
/// height a `SidebarMenuItem` row uses, so the list reads as the sidebar it
/// edits.
const FEATURE_ROW_HEIGHT: Pixels = px(34.);

/// The Features page: one row per tool, in the sidebar's own order.
///
/// **One custom element for the whole list, not one [`SettingItem`] per tool.**
/// A `SettingItem` is a label with a control beside it and no way to reach the
/// row itself, so it can carry a switch but not a drag handle, a drop target or
/// a position — and the position is the feature. [`SettingItem::render`] hands
/// over the whole row instead.
///
/// The state it edits is `Layout`'s rather than a global's, which is the other
/// reason this page cannot use [`SettingField`]: those are get/set closure pairs
/// over `&App`/`&mut App`, and the two side effects that must follow every
/// change — persisting the list, and moving the pane off a tool that is no
/// longer listed — belong to the pane. [`Layout::set_tool_enabled`] and
/// [`Layout::move_tool`] are the seam.
pub(super) fn features_page(layout: &WeakEntity<Layout>, cx: &App) -> SettingPage {
    let title = t(shell::Text::Features, cx);
    let layout = layout.clone();

    SettingPage::new(title.clone())
        .icon(AppIcon::Layers)
        .resettable(false)
        .group(
            SettingGroup::new().title(title.clone()).item(
                SettingItem::render(move |_, _, cx| feature_list(&layout, cx))
                    .keywords([title.clone()]),
            ),
        )
}

/// The description, then the rows.
fn feature_list(layout: &WeakEntity<Layout>, cx: &mut App) -> AnyElement {
    let Some(pane) = layout.upgrade() else {
        return div().into_any_element();
    };
    let features = pane.read(cx).features().clone();
    let rows: Vec<AnyElement> = features
        .all()
        .iter()
        .enumerate()
        .map(|(ix, feature)| {
            feature_row(
                layout,
                feature.code,
                ix,
                features.all().len(),
                feature.enabled,
                !features.can_toggle(feature.code),
                cx,
            )
        })
        .collect();

    v_flex()
        .w_full()
        .gap_2()
        .child(
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(t(shell::Text::FeaturesDescription, cx)),
        )
        .child(v_flex().w_full().gap_1().children(rows))
        .into_any_element()
}

/// One tool's row: the handle, the tool, the two move buttons, the switch.
///
/// `locked` is "this is the last tool the sidebar has", which
/// `Features::can_toggle` answers — **not** `can_hide`, which is also false for
/// a tool that is already hidden and would draw every hidden tool's switch dead,
/// making switching one off a one-way door.
///
/// A locked switch is drawn **disabled with the reason beside it** rather than
/// left live to refuse a press: the refusal is the same either way —
/// [`Layout::set_tool_enabled`] enforces it whatever the control does — but a
/// control that visibly cannot be pressed, next to a sentence saying why,
/// explains itself before the user is puzzled rather than after.
#[allow(clippy::too_many_arguments)]
fn feature_row(
    layout: &WeakEntity<Layout>,
    code: &'static str,
    ix: usize,
    count: usize,
    enabled: bool,
    locked: bool,
    cx: &mut App,
) -> AnyElement {
    let Some(view) = View::lookup(code) else {
        // Unreachable: every code here came out of `View::codes`.
        return div().into_any_element();
    };
    let title = t(view.title(), cx);

    h_flex()
        .id(SharedString::from(format!("feature-row-{code}")))
        .w_full()
        .h(FEATURE_ROW_HEIGHT)
        .items_center()
        .gap_2()
        .px_1()
        .rounded(cx.theme().radius)
        .border_2()
        .border_color(cx.theme().transparent)
        // The drop half of the drag. `drag_over` draws the row the tool would
        // land on, so the gesture says where it is going before it is let go.
        .drag_over::<DragTool>(|this, _, _, cx| {
            this.border_color(cx.theme().drag_border)
                .bg(cx.theme().accent.opacity(0.4))
        })
        .on_drop({
            let layout = layout.clone();
            move |drag: &DragTool, _, cx| {
                let code = drag.code;
                _ = layout.update(cx, |pane, cx| pane.move_tool(code, ix, cx));
            }
        })
        .child({
            // The grab half. Only the handle starts a drag, so pressing the
            // switch or a move button never does.
            let hint = t(shell::Text::FeatureDragToReorder, cx);

            div()
                .id(SharedString::from(format!("feature-grip-{code}")))
                .flex_shrink_0()
                .cursor_grab()
                .text_color(cx.theme().muted_foreground)
                .tooltip(move |window, cx| Tooltip::new(hint.clone()).build(window, cx))
                .on_drag(
                    DragTool {
                        code,
                        title: title.clone(),
                    },
                    |drag, _, _, cx| {
                        cx.stop_propagation();
                        cx.new(|_| drag.clone())
                    },
                )
                .child(AppIcon::GripVertical.view())
        })
        .child(div().flex_shrink_0().child(view.icon().view()))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .whitespace_nowrap()
                .child(title),
        )
        .when(locked, |this| {
            this.child(
                div()
                    .flex_shrink_0()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(t(FeatureError::LastVisibleTool.message(), cx)),
            )
        })
        .child(move_button(
            layout,
            code,
            "up",
            AppIcon::ArrowUp,
            shell::Text::FeatureMoveUp.into(),
            -1,
            ix == 0,
            cx,
        ))
        .child(move_button(
            layout,
            code,
            "down",
            AppIcon::ArrowDown,
            shell::Text::FeatureMoveDown.into(),
            1,
            ix + 1 >= count,
            cx,
        ))
        .child(
            Switch::new(SharedString::from(format!("feature-switch-{code}")))
                .checked(enabled)
                .disabled(locked)
                .tooltip(if locked {
                    t(FeatureError::LastVisibleTool.message(), cx)
                } else {
                    t(shell::Text::FeatureShowInSidebar, cx)
                })
                .on_click({
                    let layout = layout.clone();
                    move |checked: &bool, _, cx| {
                        let checked = *checked;
                        _ = layout.update(cx, |pane, cx| {
                            // The refusal is already drawn by the row: the
                            // switch that could produce one is disabled, so a
                            // press cannot reach here.
                            let _ = pane.set_tool_enabled(code, checked, cx);
                        });
                    }
                }),
        )
        .into_any_element()
}

/// One of the two reorder buttons, which are also the whole keyboard path
/// through this page — a drag has no keyboard equivalent, and the sidebar it
/// edits is keyboard-navigable.
///
/// **The keyboard activation here is Space, not Enter, and that is not a
/// choice.** `Button` is a tab stop by default and gpui synthesizes a click from
/// either key on a focused element — but `Dialog` binds `enter` to
/// `ConfirmDialog` in its own key context, whose default `on_ok` returns `true`
/// and closes the card. That is how every control in this dialog has always
/// behaved, so it is not this page's to change; Space is the conventional
/// button key and it reaches the button. A disabled button is not a tab stop
/// (`Button::render` only calls `track_focus` when it is enabled), so Tab skips
/// the top row's Up and the bottom row's Down rather than landing on a control
/// that does nothing.
#[allow(clippy::too_many_arguments)]
fn move_button(
    layout: &WeakEntity<Layout>,
    code: &'static str,
    direction: &'static str,
    icon: AppIcon,
    label: Str,
    delta: isize,
    at_the_end: bool,
    cx: &App,
) -> Button {
    let layout = layout.clone();

    Button::new(SharedString::from(format!("feature-{direction}-{code}")))
        .ghost()
        .xsmall()
        .icon(icon)
        .tooltip(t(label, cx))
        .disabled(at_the_end)
        .on_click(move |_, _, cx| {
            _ = layout.update(cx, |pane, cx| pane.move_tool_by(code, delta, cx));
        })
}

/// The tool under the pointer during a drag, and the card that follows it.
///
/// gpui's drag-and-drop wants an entity to render the thing being dragged;
/// this is it. The card is deliberately the row's own label rather than a copy
/// of the row: a floating switch that cannot be pressed reads as a bug.
#[derive(Clone)]
struct DragTool {
    code: &'static str,
    title: SharedString,
}

impl Render for DragTool {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .id("drag-tool")
            .gap_2()
            .px_2()
            .py_1()
            .items_center()
            .overflow_hidden()
            .whitespace_nowrap()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .children(View::lookup(self.code).map(|view| view.icon().view()))
            .child(self.title.clone())
    }
}
