use std::{cell::Cell, rc::Rc};

use gpui::*;
use gpui_component::ActiveTheme as _;
use gpui_component::setting::{SettingField, SettingGroup, SettingItem, SettingPage};

use super::appearance::{font_size_field, radius_field, theme_field};
use super::features::features_page;
use super::general::{StartupStatus, language_field, run_scripts_field, start_with_os_field};
use super::quick_nav::quick_nav_page;
use super::search::Setting;
use crate::app_icon::AppIcon;
use crate::i18n::{shell, t};
use crate::layout::Layout;
use crate::session::Session;

/// The dialog's sections, in sidebar order.
///
/// `highlight` is the setting the search box last jumped to; its control is
/// drawn with the accent colour so the user can see where they landed.
///
/// Each item still repeats its section name as a search keyword. The library's
/// own search box — which is what those keywords feed — is styled away in
/// [`super::view::SettingsView::render`] so the dialog does not end up with two search
/// boxes, but the keywords cost nothing and keep it working if it comes back.
pub(super) fn pages(
    layout: &WeakEntity<Layout>,
    highlight: Option<Setting>,
    start_with_os: Rc<Cell<StartupStatus>>,
    cx: &App,
) -> Vec<SettingPage> {
    let general = t(shell::Text::General, cx);
    let appearance = t(shell::Text::Appearance, cx);
    let lit = |setting: Setting| highlight == Some(setting);

    let mut general_group = SettingGroup::new()
        .title(general.clone())
        .item(
            SettingItem::new(
                t(shell::Text::Language, cx),
                highlighted(language_field(), lit(Setting::Language), cx),
            )
            .description(t(shell::Text::LanguageDescription, cx))
            .keywords([general.clone()]),
        )
        .item(
            SettingItem::new(
                t(shell::Text::RunScripts, cx),
                highlighted(run_scripts_field(cx), lit(Setting::RunScripts), cx),
            )
            .description(t(shell::Text::RunScriptsDescription, cx))
            .keywords([general.clone()]),
        );

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        general_group = general_group.item(
            SettingItem::new(
                t(shell::Text::StartWithOs, cx),
                highlighted(
                    start_with_os_field(start_with_os),
                    lit(Setting::StartWithOs),
                    cx,
                ),
            )
            .description(t(shell::Text::StartWithOsDescription, cx))
            .keywords([general.clone()]),
        );
    }

    // Only when there is something wrong with `session.json` — and there is
    // something to say, because a refused file means nothing is being saved at
    // all this run. In the ordinary case, first run included, this row is not
    // there. Same treatment as quick navigation's storage row below.
    if let Some(problem) = Session::store_error(cx) {
        general_group = general_group.item(
            // The message is the *description*; the control is empty because
            // there is nothing here to change.
            SettingItem::new(
                t(shell::Text::SessionStorageProblem, cx),
                SettingField::render(|_, _, _| div()),
            )
            .description(t(problem, cx))
            .keywords([general.clone()]),
        );
    }

    vec![
        SettingPage::new(general.clone())
            .icon(AppIcon::Sliders)
            .resettable(false)
            .default_open(true)
            .group(general_group),
        SettingPage::new(appearance.clone())
            .icon(AppIcon::Palette)
            .resettable(false)
            .group(
                SettingGroup::new()
                    .title(appearance.clone())
                    .item(
                        SettingItem::new(
                            t(shell::Text::FontSize, cx),
                            highlighted(font_size_field(cx), lit(Setting::FontSize), cx),
                        )
                        .description(t(shell::Text::FontSizeDescription, cx))
                        .keywords([appearance.clone()]),
                    )
                    .item(
                        SettingItem::new(
                            t(shell::Text::BorderRadius, cx),
                            highlighted(radius_field(), lit(Setting::BorderRadius), cx),
                        )
                        .description(t(shell::Text::BorderRadiusDescription, cx))
                        .keywords([appearance.clone()]),
                    )
                    .item(
                        SettingItem::new(
                            t(shell::Text::Theme, cx),
                            highlighted(theme_field(), lit(Setting::Theme), cx),
                        )
                        .description(t(shell::Text::ThemeDescription, cx))
                        .keywords([appearance]),
                    ),
            ),
        quick_nav_page(highlight, cx),
        features_page(layout, cx),
    ]
}

/// Marks the field the search box jumped to. The style refines the field's own
/// control (the dropdown button), which is the thing the user came to change.
pub(super) fn highlighted<T>(field: SettingField<T>, on: bool, cx: &App) -> SettingField<T> {
    if !on {
        return field;
    }

    field
        .border_color(cx.theme().primary)
        .bg(cx.theme().primary.opacity(0.1))
}
