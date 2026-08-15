use gpui::*;
use gpui_component::setting::{SettingField, SettingGroup, SettingItem, SettingPage};

use super::pages::highlighted;
use super::search::Setting;
use crate::app_icon::AppIcon;
use crate::i18n::{shell, t};
use crate::quick_nav::QuickNav;
use crate::quick_nav::models::detect::Detector;

/// The Quick navigation section: the master switch, then one pattern field per
/// detector, in detection order.
///
/// The pattern items are **generated from [`Detector::ORDER`]**, so a sixth
/// detector appears here with no edit to this file. Each one's description is
/// chosen by [`Detector::has_parser`], which is the "a pattern selects
/// candidates, the parser confirms" rule showing through to the user: a gate in
/// front of a real parser reads differently from a shape test, because it *is*
/// different.
pub(super) fn quick_nav_page(highlight: Option<Setting>, cx: &App) -> SettingPage {
    let title = t(shell::Text::QuickNavigation, cx);
    let lit = |setting: Setting| highlight == Some(setting);

    let mut group = SettingGroup::new().title(title.clone()).item(
        SettingItem::new(
            t(shell::Text::QuickNavEnabled, cx),
            highlighted(
                SettingField::switch(
                    |cx: &App| QuickNav::enabled(cx),
                    |value: bool, cx: &mut App| QuickNav::set_enabled(value, cx),
                )
                .default_value(true),
                lit(Setting::QuickNavEnabled),
                cx,
            ),
        )
        .description(t(shell::Text::QuickNavEnabledDescription, cx))
        .keywords([title.clone()]),
    );

    for detector in Detector::ORDER {
        // An unreadable pattern says so here, in place of the description: the
        // detector is meanwhile running on its built-in default, so the user
        // needs to know their pattern is not the thing being used.
        let description = match QuickNav::pattern_error(detector, cx) {
            Some(error) => t(error.message(), cx),
            None if detector.has_parser() => t(shell::Text::QuickNavGateDescription, cx),
            None => t(shell::Text::QuickNavShapeDescription, cx),
        };

        group = group.item(
            input_item(
                t(detector.label(), cx),
                highlighted(
                    pattern_field(detector),
                    lit(Setting::QuickNavPattern(detector)),
                    cx,
                ),
            )
            .description(description)
            .keywords([title.clone()]),
        );
    }

    // Only when there is something wrong with the file. In the ordinary case —
    // including a first run with no file at all — this section is not there.
    if let Some(problem) = QuickNav::store_error(cx) {
        group = group.item(
            // The message is the *description*; the control is empty because
            // there is nothing here to change. `SettingItem` requires a field,
            // so an empty render closure is the way to say "none".
            SettingItem::new(
                t(shell::Text::QuickNavStorageProblem, cx),
                SettingField::render(|_, _, _| div()),
            )
            .description(t(problem, cx))
            .keywords([title.clone()]),
        );
    }

    SettingPage::new(title)
        .icon(AppIcon::Binary)
        .resettable(false)
        .group(group)
}

/// A setting item whose control is a text input — **use this rather than
/// [`SettingItem::new`] for every [`SettingField::input`]**, here or on a future
/// page.
///
/// The only difference is `layout(Axis::Vertical)`, and it is not cosmetic: an
/// input is the widest of the *fixed*-width controls the library builds, wide
/// enough that a side-by-side row here cannot hold it. [`pattern_field`] has the
/// numbers. Stacked, the control is `w_full` and therefore bounded by the row at
/// any dialog or window size, which is also what the library falls back to on
/// its own once the panel is narrow enough.
pub(super) fn input_item(title: SharedString, field: SettingField<SharedString>) -> SettingItem {
    SettingItem::new(title, field).layout(Axis::Vertical)
}

/// One detector's pattern, as a plain text field.
///
/// The library's string field emits a change per keystroke; the coalescing that
/// keeps that from writing the file once per character lives in
/// [`QuickNav::set_pattern`], not here. The value is stored **raw** — untrimmed,
/// uncompiled — so the field never fights the user mid-edit.
///
/// **Its item is built by [`input_item`], which stacks it.**
/// `setting::fields::string::StringField::render` gives an input `w_64` (256px)
/// in a horizontal row, where a switch or a dropdown is content-sized and a
/// number input is half as wide. `SettingItem::render_item` caps the label
/// column at `max_w_3_5` and lets it grow into whatever the 256px control
/// leaves, but nothing shrinks the control, so a row narrower than
/// `256 + gap + 0.6 * row` cannot hold both and the input is laid out past the
/// row's right edge, where the dialog clips it. At [`super::DIALOG_WIDTH`] each row is 494px and the input
/// lands 70px outside — the regression the stacked layout fixes. The library
/// reaches for the same stacked layout by itself, but only once the whole panel
/// has dropped to 480px. `row_layout` measures both halves of that.
fn pattern_field(detector: Detector) -> SettingField<SharedString> {
    SettingField::input(
        move |cx: &App| QuickNav::pattern(detector, cx).into(),
        move |value: SharedString, cx: &mut App| QuickNav::set_pattern(detector, value, cx),
    )
    .default_value(SharedString::default())
}
