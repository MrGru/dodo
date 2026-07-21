//! The Settings dialog, plus the app-level state it edits.
//!
//! There is deliberately no settings struct of our own for appearance: font
//! size, border radius and colours all live on `gpui_component::Theme`, which
//! is already a global the whole app renders from, so the dialog reads and
//! writes that directly and every change is live. Language is the one setting
//! with no home in `Theme`; it lives in [`crate::i18n::Language`].
//!
//! Nothing is persisted across restarts — see the note in `AGENTS.md`.

use gpui::*;
use gpui_component::setting::{SettingField, SettingGroup, SettingItem, SettingPage, Settings};
use gpui_component::{Theme, ThemeRegistry, WindowExt as _};

use crate::app_icon::AppIcon;
use crate::assets::Assets;
use crate::i18n::{Language, Str, t};

/// Base text size in px, largest first. `Theme::font_size` drives the window's
/// rem size (see the library's `Root::render`), so these scale the whole UI.
const FONT_SIZES: [(Str, f32); 3] = [(Str::Large, 18.), (Str::Medium, 16.), (Str::Small, 14.)];
const DEFAULT_FONT_SIZE: f32 = 16.;

const RADII: [f32; 4] = [8., 6., 4., 0.];
const DEFAULT_RADIUS: f32 = 6.;

/// Themes offered in the dialog, by the `name` inside `assets/themes/*.json`.
/// "Default Light"/"Default Dark" are built into the library's registry; the
/// rest come from the vendored files loaded in [`init`].
const THEMES: [&str; 16] = [
    "Default Light",
    "Default Dark",
    "Ayu Light",
    "Catppuccin Latte",
    "Everforest Light",
    "Flexoki Light",
    "Gruvbox Light",
    "Hybrid Light",
    "macOS Classic Light",
    "Mellifluous Light",
    "Molokai Light",
    "Adventure Time",
    "Alduin",
    "Asciinema",
    "Ayu Dark",
    "Catppuccin Frappe",
];

/// Registers the vendored themes with the library's [`ThemeRegistry`].
///
/// Must run after `gpui_component::init`, which creates the registry.
pub fn init(cx: &mut App) {
    let themes: Vec<_> = Assets::themes().collect();
    let registry = ThemeRegistry::global_mut(cx);

    for (path, data) in themes {
        let Ok(json) = std::str::from_utf8(&data) else {
            eprintln!("theme {path} is not valid UTF-8");
            continue;
        };
        if let Err(err) = registry.load_themes_from_str(json) {
            eprintln!("failed to load theme {path}: {err}");
        }
    }
}

/// Opens the Settings dialog. The dialog is dismissed with Escape, the close
/// button, or a click on the overlay.
pub fn open(window: &mut Window, cx: &mut App) {
    window.open_dialog(cx, |dialog, _, cx| {
        dialog.title(t(Str::Settings, cx)).w(px(760.)).child(
            div().w_full().h(px(440.)).child(
                Settings::new("dodo-settings")
                    .sidebar_width(px(200.))
                    .pages(pages(cx)),
            ),
        )
    });
}

/// The dialog's sections, in sidebar order.
///
/// Each item repeats its section name as a search keyword so that typing a
/// section name into the search box keeps that section's items — the library
/// only matches item titles, descriptions and keywords.
fn pages(cx: &App) -> Vec<SettingPage> {
    let general = t(Str::General, cx);
    let appearance = t(Str::Appearance, cx);

    vec![
        SettingPage::new(general.clone())
            .icon(AppIcon::Sliders)
            .resettable(false)
            .default_open(true)
            .group(
                SettingGroup::new().title(general.clone()).item(
                    SettingItem::new(t(Str::Language, cx), language_field())
                        .description(t(Str::LanguageDescription, cx))
                        .keywords([general]),
                ),
            ),
        SettingPage::new(appearance.clone())
            .icon(AppIcon::Palette)
            .resettable(false)
            .group(
                SettingGroup::new()
                    .title(appearance.clone())
                    .item(
                        SettingItem::new(t(Str::FontSize, cx), font_size_field(cx))
                            .description(t(Str::FontSizeDescription, cx))
                            .keywords([appearance.clone()]),
                    )
                    .item(
                        SettingItem::new(t(Str::BorderRadius, cx), radius_field())
                            .description(t(Str::BorderRadiusDescription, cx))
                            .keywords([appearance.clone()]),
                    )
                    .item(
                        SettingItem::new(t(Str::Theme, cx), theme_field())
                            .description(t(Str::ThemeDescription, cx))
                            .keywords([appearance]),
                    ),
            ),
    ]
}

fn language_field() -> SettingField<SharedString> {
    let options = Language::ALL
        .map(|language| (language.code().into(), language.label().into()))
        .to_vec();

    SettingField::dropdown(
        options,
        |cx: &App| Language::current(cx).code().into(),
        |value: SharedString, cx: &mut App| Language::from_code(&value).set(cx),
    )
    .default_value(Language::default().code())
}

fn font_size_field(cx: &App) -> SettingField<SharedString> {
    let options = FONT_SIZES
        .map(|(label, size)| (size_value(size), t(label, cx)))
        .to_vec();

    SettingField::dropdown(
        options,
        |cx: &App| size_value(f32::from(Theme::global(cx).font_size)),
        |value: SharedString, cx: &mut App| {
            set_font_size(value.parse().unwrap_or(DEFAULT_FONT_SIZE), cx)
        },
    )
    .default_value(size_value(DEFAULT_FONT_SIZE))
}

fn radius_field() -> SettingField<SharedString> {
    let options = RADII
        .map(|radius| (size_value(radius), format!("{radius}px").into()))
        .to_vec();

    SettingField::dropdown(
        options,
        |cx: &App| size_value(f32::from(Theme::global(cx).radius)),
        |value: SharedString, cx: &mut App| set_radius(value.parse().unwrap_or(DEFAULT_RADIUS), cx),
    )
    .default_value(size_value(DEFAULT_RADIUS))
}

fn theme_field() -> SettingField<SharedString> {
    let options = THEMES
        .map(|name| {
            (
                SharedString::new_static(name),
                SharedString::new_static(name),
            )
        })
        .to_vec();

    SettingField::scrollable_dropdown(
        options,
        |cx: &App| Theme::global(cx).theme_name().clone(),
        |value: SharedString, cx: &mut App| set_theme(&value, cx),
    )
    .default_value(THEMES[0])
}

fn set_font_size(size: f32, cx: &mut App) {
    Theme::global_mut(cx).font_size = px(size);
    cx.refresh_windows();
}

/// `radius_lg` (dialogs, notifications) tracks `radius` so that picking 0px
/// squares off every corner rather than leaving overlays rounded.
fn set_radius(radius: f32, cx: &mut App) {
    let theme = Theme::global_mut(cx);
    theme.radius = px(radius);
    theme.radius_lg = px(radius);
    cx.refresh_windows();
}

fn set_theme(name: &str, cx: &mut App) {
    let Some(config) = ThemeRegistry::global(cx).themes().get(name).cloned() else {
        eprintln!("theme {name} is not registered");
        return;
    };

    // A theme config may carry its own font size and radius. Ours are explicit
    // user choices, so re-assert them over whatever the theme brought with it.
    let font_size = f32::from(Theme::global(cx).font_size);
    let radius = f32::from(Theme::global(cx).radius);
    Theme::global_mut(cx).apply_config(&config);
    set_font_size(font_size, cx);
    set_radius(radius, cx);
}

/// Dropdown values are stable identifiers, never localized labels, so the
/// stored choice does not change meaning when the language does.
fn size_value(size: f32) -> SharedString {
    format!("{size}").into()
}
