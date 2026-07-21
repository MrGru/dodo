//! A deliberately small localization mechanism: one enum per translatable
//! string, one column per language, and a global holding the active choice.
//!
//! Adding a string means adding a [`Str`] variant and its row in [`Str::text`];
//! adding a language means a [`Language`] variant, a row in [`Language::ALL`],
//! and a column in every `Str::text` row (the compiler lists the ones you
//! missed). No catalogue files, no runtime key lookup, no missing-key fallback
//! to get wrong.

use gpui::{App, Global, SharedString};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum Language {
    #[default]
    English,
    Vietnamese,
}

impl Global for Language {}

impl Language {
    pub const ALL: [Language; 2] = [Language::English, Language::Vietnamese];

    /// The stable identifier used as the settings dropdown value.
    pub fn code(self) -> &'static str {
        match self {
            Language::English => "en",
            Language::Vietnamese => "vi",
        }
    }

    pub fn from_code(code: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|language| language.code() == code)
            .unwrap_or_default()
    }

    /// The language's name in that language, as language pickers conventionally
    /// show it.
    pub fn label(self) -> &'static str {
        match self {
            Language::English => "English",
            Language::Vietnamese => "Tiếng Việt",
        }
    }

    /// The active language. Defaults to English until [`Language::set`] runs.
    pub fn current(cx: &App) -> Language {
        cx.try_global::<Language>().copied().unwrap_or_default()
    }

    /// Switches language and repaints every window so already-rendered strings
    /// pick the new column up.
    pub fn set(self, cx: &mut App) {
        cx.set_global(self);
        cx.refresh_windows();
    }
}

/// Every string this app localizes.
#[derive(Clone, Copy)]
pub enum Str {
    Settings,
    General,
    Appearance,
    Language,
    LanguageDescription,
    Theme,
    ThemeDescription,
    FontSize,
    FontSizeDescription,
    BorderRadius,
    BorderRadiusDescription,
    Large,
    Medium,
    Small,
    Tools,
}

impl Str {
    fn text(self, language: Language) -> &'static str {
        match (self, language) {
            (Str::Settings, Language::English) => "Settings",
            (Str::Settings, Language::Vietnamese) => "Cài đặt",
            (Str::General, Language::English) => "General",
            (Str::General, Language::Vietnamese) => "Chung",
            (Str::Appearance, Language::English) => "Appearance",
            (Str::Appearance, Language::Vietnamese) => "Giao diện",
            (Str::Language, Language::English) => "Language",
            (Str::Language, Language::Vietnamese) => "Ngôn ngữ",
            (Str::LanguageDescription, Language::English) => {
                "The language used for the app's own labels."
            }
            (Str::LanguageDescription, Language::Vietnamese) => {
                "Ngôn ngữ dùng cho các nhãn của ứng dụng."
            }
            (Str::Theme, Language::English) => "Theme",
            (Str::Theme, Language::Vietnamese) => "Chủ đề",
            (Str::ThemeDescription, Language::English) => "The colour scheme of the whole app.",
            (Str::ThemeDescription, Language::Vietnamese) => "Bảng màu của toàn bộ ứng dụng.",
            (Str::FontSize, Language::English) => "Font size",
            (Str::FontSize, Language::Vietnamese) => "Cỡ chữ",
            (Str::FontSizeDescription, Language::English) => "The base text size of the app.",
            (Str::FontSizeDescription, Language::Vietnamese) => "Cỡ chữ cơ bản của ứng dụng.",
            (Str::BorderRadius, Language::English) => "Border radius",
            (Str::BorderRadius, Language::Vietnamese) => "Bo góc",
            (Str::BorderRadiusDescription, Language::English) => {
                "How rounded buttons, inputs and panels are."
            }
            (Str::BorderRadiusDescription, Language::Vietnamese) => {
                "Độ bo góc của nút, ô nhập và khung."
            }
            (Str::Large, Language::English) => "Large",
            (Str::Large, Language::Vietnamese) => "Lớn",
            (Str::Medium, Language::English) => "Medium",
            (Str::Medium, Language::Vietnamese) => "Vừa",
            (Str::Small, Language::English) => "Small",
            (Str::Small, Language::Vietnamese) => "Nhỏ",
            (Str::Tools, Language::English) => "Tools",
            (Str::Tools, Language::Vietnamese) => "Công cụ",
        }
    }
}

/// Translates `str` into the active language.
pub fn t(str: Str, cx: &App) -> SharedString {
    SharedString::new_static(str.text(Language::current(cx)))
}
