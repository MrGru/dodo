use gpui::*;
use gpui_component::setting::SettingField;
use gpui_component::{Theme, ThemeRegistry};

use crate::i18n::{Language, LanguageExt, shell, t};
use crate::session::Session;

/// Base text size in px, largest first. `Theme::font_size` drives the window's
/// rem size (see the library's `Root::render`), so these scale the whole UI.
const FONT_SIZES: [(shell::Text, f32); 3] = [
    (shell::Text::Large, 18.),
    (shell::Text::Medium, 16.),
    (shell::Text::Small, 14.),
];
const DEFAULT_FONT_SIZE: f32 = 16.;

const RADII: [f32; 4] = [8., 6., 4., 0.];
const DEFAULT_RADIUS: f32 = 6.;

/// Themes offered in the dialog, by the `name` inside `assets/themes/*.json`.
/// "Default Light"/"Default Dark" are built into the library's registry; the
/// rest come from the vendored files loaded in [`super::init`].
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

pub(super) fn font_size_field(cx: &App) -> SettingField<SharedString> {
    let options = FONT_SIZES
        .map(|(label, size)| (size_value(size), t(label, cx)))
        .to_vec();

    SettingField::dropdown(
        options,
        |cx: &App| size_value(f32::from(Theme::global(cx).font_size)),
        |value: SharedString, cx: &mut App| {
            let size = value.parse().unwrap_or(DEFAULT_FONT_SIZE);
            set_font_size(size, cx);
            Session::set_font_size(size, cx);
        },
    )
    .default_value(size_value(DEFAULT_FONT_SIZE))
}

pub(super) fn radius_field() -> SettingField<SharedString> {
    let options = RADII
        .map(|radius| (size_value(radius), format!("{radius}px").into()))
        .to_vec();

    SettingField::dropdown(
        options,
        |cx: &App| size_value(f32::from(Theme::global(cx).radius)),
        |value: SharedString, cx: &mut App| {
            let radius = value.parse().unwrap_or(DEFAULT_RADIUS);
            set_radius(radius, cx);
            Session::set_border_radius(radius, cx);
        },
    )
    .default_value(size_value(DEFAULT_RADIUS))
}

pub(super) fn theme_field() -> SettingField<SharedString> {
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
        |value: SharedString, cx: &mut App| {
            set_theme(&value, cx);
            Session::set_theme(value.to_string(), cx);
        },
    )
    .default_value(THEMES[0])
}

/// Applies the appearance choices `session.json` held, if any.
///
/// Called from `main` **after** `session::load` and **before** the window is
/// opened, so the first frame is already the user's theme rather than a flash
/// of the default one.
///
/// The order matters and is the same order the dialog produces by hand: the
/// theme first, because [`set_theme`] re-asserts the font size and radius that
/// were current over whatever the theme config brought with it, then the two
/// explicit values over that. Doing it the other way round would let a theme's
/// own numbers win over the user's.
///
/// A field the user never touched stays `None` and is not applied at all —
/// notably the theme, because `gpui_component::init` picks light or dark from
/// the *system appearance* and forcing "Default Light" over that merely because
/// it was what the app happened to show would break appearance following for
/// everyone who never opened this dialog. `session::models::document` argues it
/// at more length.
pub fn apply_session(cx: &mut App) {
    if let Some(name) = Session::theme(cx) {
        set_theme(&name, cx);
    }
    if let Some(size) = Session::font_size(cx) {
        set_font_size(size, cx);
    }
    if let Some(radius) = Session::border_radius(cx) {
        set_radius(radius, cx);
    }
    if let Some(code) = Session::language(cx) {
        Language::from_code(&code).set(cx);
    }
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

/// Applies a registered theme by name, keeping the user's font size and radius.
///
/// **Does not persist**, unlike the three field writers above, because
/// [`apply_session`] calls it on the restore path too and a restore that wrote
/// back what it had just read would be noise. The dialog's own writer persists;
/// see [`theme_field`].
///
/// An unregistered name is a no-op, which is also what makes a `session.json`
/// naming a theme this build dropped harmless rather than fatal.
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
