//! A deliberately small localization mechanism: one enum per translatable
//! string, one column per language, and a global holding the active choice.
//!
//! Adding a string means adding a [`Str`] variant and its row in [`Str::text`];
//! adding a language means a [`Language`] variant, a row in [`Language::ALL`],
//! and a column in every `Str::text` row (the compiler lists the ones you
//! missed). No catalogue files, no runtime key lookup, no missing-key fallback
//! to get wrong.
//!
//! Messages that carry runtime values — a position, a count, a third-party
//! parser's own text — are [`Str`] variants with fields, so each language owns
//! the whole sentence and word order rather than a translated prefix glued onto
//! an English tail. Third-party error text (serde_json, base64, …) is English
//! and stays English inside the translated frame; there is nothing to translate
//! it with.

use std::borrow::Cow;

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

/// Which part of a JWT an error is about. Its own row per language so that a
/// new language has to say how it names the part, even if — as in Vietnamese —
/// the answer is to keep the English term of art.
#[derive(Clone, Copy)]
pub enum JwtPart {
    Header,
    Payload,
}

impl JwtPart {
    /// The part's name as it reads mid-sentence.
    fn name(self, language: Language) -> &'static str {
        match (self, language) {
            (JwtPart::Header, Language::English) => "header",
            (JwtPart::Header, Language::Vietnamese) => "header",
            (JwtPart::Payload, Language::English) => "payload",
            (JwtPart::Payload, Language::Vietnamese) => "payload",
        }
    }
}

/// Every string this app localizes.
///
/// "Dodo" is the product name and is never translated, so it has no variant
/// here. Neither do the technical terms that stay put in both languages —
/// JSON, Base64, hex, JWT, URL — they appear inside the strings below.
#[derive(Clone)]
pub enum Str {
    // Settings dialog.
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

    // Sidebar.
    Tools,
    JsonFormatterTitle,
    EncoderDecoderTitle,

    // JSON formatter.
    JsonPlaceholder,
    FormatButton,
    IndentLabel,
    /// "{count} spaces" — the indent-width dropdown options.
    IndentSpaces(usize),
    /// serde_json's message is third-party English and is kept verbatim.
    InvalidJson {
        line: usize,
        column: usize,
        detail: String,
    },

    // Encoder / decoder.
    FormatLabel,
    EncodeButton,
    DecodeButton,
    DecodeJwtButton,
    InputLabel,
    OutputLabel,
    JwtHeaderLabel,
    JwtPayloadLabel,
    JwtSignatureLabel,
    EncoderInputPlaceholder,
    EncoderOutputPlaceholder,
    FormatBase64,
    FormatBase64UrlSafe,
    FormatUrl,
    FormatHex,
    FormatJwt,

    // Encoder / decoder errors.
    JwtEncodeUnsupported,
    InvalidHexOddLength(usize),
    InvalidHexDigit {
        digit: char,
        position: usize,
    },
    /// base64's message is third-party English and is kept verbatim.
    InvalidBase64(String),
    InvalidPercentAt(usize),
    /// The UTF-8 error is third-party English and is kept verbatim.
    InvalidPercentEncoding(String),
    NotUtf8(String),
    JwtEmpty,
    JwtPartCount(usize),
    JwtPartNotBase64 {
        part: JwtPart,
        detail: String,
    },
    JwtPartNotJson {
        part: JwtPart,
        detail: String,
    },
    JwtPartNotRenderable {
        part: JwtPart,
        detail: String,
    },
}

impl Str {
    fn text(self, language: Language) -> Cow<'static, str> {
        match (self, language) {
            (Str::Settings, Language::English) => "Settings".into(),
            (Str::Settings, Language::Vietnamese) => "Cài đặt".into(),
            (Str::General, Language::English) => "General".into(),
            (Str::General, Language::Vietnamese) => "Chung".into(),
            (Str::Appearance, Language::English) => "Appearance".into(),
            (Str::Appearance, Language::Vietnamese) => "Giao diện".into(),
            (Str::Language, Language::English) => "Language".into(),
            (Str::Language, Language::Vietnamese) => "Ngôn ngữ".into(),
            (Str::LanguageDescription, Language::English) => {
                "The language used for the app's own labels.".into()
            }
            (Str::LanguageDescription, Language::Vietnamese) => {
                "Ngôn ngữ dùng cho các nhãn của ứng dụng.".into()
            }
            (Str::Theme, Language::English) => "Theme".into(),
            (Str::Theme, Language::Vietnamese) => "Chủ đề".into(),
            (Str::ThemeDescription, Language::English) => {
                "The colour scheme of the whole app.".into()
            }
            (Str::ThemeDescription, Language::Vietnamese) => "Bảng màu của toàn bộ ứng dụng.".into(),
            (Str::FontSize, Language::English) => "Font size".into(),
            (Str::FontSize, Language::Vietnamese) => "Cỡ chữ".into(),
            (Str::FontSizeDescription, Language::English) => "The base text size of the app.".into(),
            (Str::FontSizeDescription, Language::Vietnamese) => {
                "Cỡ chữ cơ bản của ứng dụng.".into()
            }
            (Str::BorderRadius, Language::English) => "Border radius".into(),
            (Str::BorderRadius, Language::Vietnamese) => "Bo góc".into(),
            (Str::BorderRadiusDescription, Language::English) => {
                "How rounded buttons, inputs and panels are.".into()
            }
            (Str::BorderRadiusDescription, Language::Vietnamese) => {
                "Độ bo góc của nút, ô nhập và khung.".into()
            }
            (Str::Large, Language::English) => "Large".into(),
            (Str::Large, Language::Vietnamese) => "Lớn".into(),
            (Str::Medium, Language::English) => "Medium".into(),
            (Str::Medium, Language::Vietnamese) => "Vừa".into(),
            (Str::Small, Language::English) => "Small".into(),
            (Str::Small, Language::Vietnamese) => "Nhỏ".into(),

            (Str::Tools, Language::English) => "Tools".into(),
            (Str::Tools, Language::Vietnamese) => "Công cụ".into(),
            (Str::JsonFormatterTitle, Language::English) => "Json formatter".into(),
            (Str::JsonFormatterTitle, Language::Vietnamese) => "Định dạng JSON".into(),
            (Str::EncoderDecoderTitle, Language::English) => "Encoder / Decoder".into(),
            (Str::EncoderDecoderTitle, Language::Vietnamese) => "Mã hoá / Giải mã".into(),

            (Str::JsonPlaceholder, Language::English) => {
                "Paste JSON here, then click Format.".into()
            }
            (Str::JsonPlaceholder, Language::Vietnamese) => {
                "Dán JSON vào đây rồi bấm Định dạng.".into()
            }
            (Str::FormatButton, Language::English) => "Format".into(),
            (Str::FormatButton, Language::Vietnamese) => "Định dạng".into(),
            (Str::IndentLabel, Language::English) => "Indent:".into(),
            (Str::IndentLabel, Language::Vietnamese) => "Thụt lề:".into(),
            (Str::IndentSpaces(count), Language::English) => format!("{count} spaces").into(),
            (Str::IndentSpaces(count), Language::Vietnamese) => {
                format!("{count} khoảng trắng").into()
            }
            (
                Str::InvalidJson {
                    line,
                    column,
                    detail,
                },
                Language::English,
            ) => format!("Invalid JSON at line {line}, column {column}: {detail}").into(),
            (
                Str::InvalidJson {
                    line,
                    column,
                    detail,
                },
                Language::Vietnamese,
            ) => format!("JSON không hợp lệ tại dòng {line}, cột {column}: {detail}").into(),

            (Str::FormatLabel, Language::English) => "Format:".into(),
            (Str::FormatLabel, Language::Vietnamese) => "Định dạng:".into(),
            (Str::EncodeButton, Language::English) => "Encode".into(),
            (Str::EncodeButton, Language::Vietnamese) => "Mã hoá".into(),
            (Str::DecodeButton, Language::English) => "Decode".into(),
            (Str::DecodeButton, Language::Vietnamese) => "Giải mã".into(),
            (Str::DecodeJwtButton, Language::English) => "Decode JWT".into(),
            (Str::DecodeJwtButton, Language::Vietnamese) => "Giải mã JWT".into(),
            (Str::InputLabel, Language::English) => "Input".into(),
            (Str::InputLabel, Language::Vietnamese) => "Đầu vào".into(),
            (Str::OutputLabel, Language::English) => "Output".into(),
            (Str::OutputLabel, Language::Vietnamese) => "Đầu ra".into(),
            (Str::JwtHeaderLabel, Language::English) => "Header".into(),
            (Str::JwtHeaderLabel, Language::Vietnamese) => "Header".into(),
            (Str::JwtPayloadLabel, Language::English) => "Payload".into(),
            (Str::JwtPayloadLabel, Language::Vietnamese) => "Payload".into(),
            (Str::JwtSignatureLabel, Language::English) => "Signature (not verified)".into(),
            (Str::JwtSignatureLabel, Language::Vietnamese) => "Chữ ký (chưa xác thực)".into(),
            (Str::EncoderInputPlaceholder, Language::English) => {
                "Paste the text or token to convert here.".into()
            }
            (Str::EncoderInputPlaceholder, Language::Vietnamese) => {
                "Dán văn bản hoặc token cần chuyển đổi vào đây.".into()
            }
            (Str::EncoderOutputPlaceholder, Language::English) => "Result appears here.".into(),
            (Str::EncoderOutputPlaceholder, Language::Vietnamese) => {
                "Kết quả hiển thị ở đây.".into()
            }
            (Str::FormatBase64, Language::English) => "Base64 (standard)".into(),
            (Str::FormatBase64, Language::Vietnamese) => "Base64 (chuẩn)".into(),
            (Str::FormatBase64UrlSafe, Language::English) => "Base64 (URL-safe)".into(),
            (Str::FormatBase64UrlSafe, Language::Vietnamese) => "Base64 (an toàn cho URL)".into(),
            (Str::FormatUrl, Language::English) => "URL percent-encoding".into(),
            (Str::FormatUrl, Language::Vietnamese) => "Mã hoá phần trăm URL".into(),
            (Str::FormatHex, Language::English) => "Hex".into(),
            (Str::FormatHex, Language::Vietnamese) => "Hex".into(),
            (Str::FormatJwt, Language::English) => "JWT (decode only)".into(),
            (Str::FormatJwt, Language::Vietnamese) => "JWT (chỉ giải mã)".into(),

            (Str::JwtEncodeUnsupported, Language::English) => {
                "JWT is decode-only: no signing key is available.".into()
            }
            (Str::JwtEncodeUnsupported, Language::Vietnamese) => {
                "JWT chỉ hỗ trợ giải mã: không có khoá ký.".into()
            }
            (Str::InvalidHexOddLength(count), Language::English) => {
                format!("Invalid hex: expected an even number of digits, got {count}.").into()
            }
            (Str::InvalidHexOddLength(count), Language::Vietnamese) => {
                format!("Hex không hợp lệ: cần số ký tự chẵn, nhận được {count}.").into()
            }
            (Str::InvalidHexDigit { digit, position }, Language::English) => {
                format!("Invalid hex: '{digit}' at position {position} is not a hex digit.").into()
            }
            (Str::InvalidHexDigit { digit, position }, Language::Vietnamese) => {
                format!("Hex không hợp lệ: '{digit}' ở vị trí {position} không phải ký tự hex.")
                    .into()
            }
            (Str::InvalidBase64(detail), Language::English) => {
                format!("Invalid base64: {detail}").into()
            }
            (Str::InvalidBase64(detail), Language::Vietnamese) => {
                format!("Base64 không hợp lệ: {detail}").into()
            }
            (Str::InvalidPercentAt(position), Language::English) => format!(
                "Invalid percent-encoding: '%' at position {position} is not followed by two hex digits."
            )
            .into(),
            (Str::InvalidPercentAt(position), Language::Vietnamese) => format!(
                "Mã hoá phần trăm không hợp lệ: '%' ở vị trí {position} không được theo sau bởi hai ký tự hex."
            )
            .into(),
            (Str::InvalidPercentEncoding(detail), Language::English) => {
                format!("Invalid percent-encoding: {detail}").into()
            }
            (Str::InvalidPercentEncoding(detail), Language::Vietnamese) => {
                format!("Mã hoá phần trăm không hợp lệ: {detail}").into()
            }
            (Str::NotUtf8(detail), Language::English) => {
                format!("Decoded bytes are not valid UTF-8 text: {detail}").into()
            }
            (Str::NotUtf8(detail), Language::Vietnamese) => {
                format!("Dữ liệu giải mã không phải văn bản UTF-8 hợp lệ: {detail}").into()
            }
            (Str::JwtEmpty, Language::English) => "Invalid JWT: the input is empty.".into(),
            (Str::JwtEmpty, Language::Vietnamese) => {
                "JWT không hợp lệ: chưa có dữ liệu đầu vào.".into()
            }
            (Str::JwtPartCount(count), Language::English) => {
                format!("Invalid JWT: expected 3 dot-separated parts, got {count}.").into()
            }
            (Str::JwtPartCount(count), Language::Vietnamese) => {
                format!("JWT không hợp lệ: cần 3 phần ngăn cách bởi dấu chấm, nhận được {count}.")
                    .into()
            }
            (Str::JwtPartNotBase64 { part, detail }, Language::English) => {
                let part = part.name(Language::English);
                format!("Invalid JWT: the {part} is not valid base64url ({detail}).").into()
            }
            (Str::JwtPartNotBase64 { part, detail }, Language::Vietnamese) => {
                let part = part.name(Language::Vietnamese);
                format!("JWT không hợp lệ: phần {part} không phải base64url hợp lệ ({detail}).")
                    .into()
            }
            (Str::JwtPartNotJson { part, detail }, Language::English) => {
                let part = part.name(Language::English);
                format!("Invalid JWT: the {part} is not valid JSON ({detail}).").into()
            }
            (Str::JwtPartNotJson { part, detail }, Language::Vietnamese) => {
                let part = part.name(Language::Vietnamese);
                format!("JWT không hợp lệ: phần {part} không phải JSON hợp lệ ({detail}).").into()
            }
            (Str::JwtPartNotRenderable { part, detail }, Language::English) => {
                let part = part.name(Language::English);
                format!("Invalid JWT: could not render the {part} ({detail}).").into()
            }
            (Str::JwtPartNotRenderable { part, detail }, Language::Vietnamese) => {
                let part = part.name(Language::Vietnamese);
                format!("JWT không hợp lệ: không thể hiển thị phần {part} ({detail}).").into()
            }
        }
    }
}

/// Translates `str` into the active language.
pub fn t(str: Str, cx: &App) -> SharedString {
    match str.text(Language::current(cx)) {
        Cow::Borrowed(text) => SharedString::new_static(text),
        Cow::Owned(text) => SharedString::from(text),
    }
}

/// What these tests protect
/// ------------------------
///
/// The `match` in [`Str::text`] already makes a *missing* language a compile
/// error. Three things it cannot catch, and that these tests do:
///
/// 1. A language arm that is present but empty, or whitespace only.
/// 2. A parameterized arm that forgot its `{placeholder}`, so the runtime value
///    (a line number, a parser's message) silently never reaches the screen.
/// 3. A language arm that was filled in by pasting the English text. Asserting
///    "every language differs" would be false — `Hex`, `Header` and `Payload`
///    are the same word in both languages by design — so every variant declares
///    which it is via [`Expect`], and the test holds it to that declaration in
///    *both* directions.
///
/// Adding a `Str` variant is a compile error in `position` below until it is
/// given a slot, and the slot then has to line up with a real entry in
/// `samples`. (The one thing that slips through is deliberately reusing another
/// variant's index; nothing here can detect that.)
#[cfg(test)]
mod tests {
    use super::{JwtPart, Language, Str};

    /// Stands in for a third-party parser's own message. Deliberately unlike
    /// any word in the catalogue so `contains` cannot match by accident.
    const DETAIL: &str = "<<detail-sentinel>>";
    /// Ditto for numeric values: no catalogue string contains this digit run.
    const NUMBER: usize = 4242;
    const NUMBER_TEXT: &str = "4242";

    /// Whether a variant is expected to read differently in each language.
    #[derive(Clone, Copy)]
    enum Expect {
        /// Prose. Every language must produce its own wording.
        Translated,
        /// A term of art that is the same word in every language we ship.
        /// Asserted as equality, so "translating" one later fails here and
        /// forces the declaration to be updated rather than quietly diverging.
        SameEverywhere,
    }

    struct Sample {
        str: Str,
        /// Runtime values the rendered text must surface, in every language.
        parts: &'static [&'static str],
        expect: Expect,
    }

    fn plain(str: Str) -> Sample {
        Sample {
            str,
            parts: &[],
            expect: Expect::Translated,
        }
    }

    fn term(str: Str) -> Sample {
        Sample {
            str,
            parts: &[],
            expect: Expect::SameEverywhere,
        }
    }

    fn with(str: Str, parts: &'static [&'static str]) -> Sample {
        Sample {
            str,
            parts,
            expect: Expect::Translated,
        }
    }

    /// One entry per `Str` variant, in `position` order.
    fn samples() -> Vec<Sample> {
        vec![
            plain(Str::Settings),
            plain(Str::General),
            plain(Str::Appearance),
            plain(Str::Language),
            plain(Str::LanguageDescription),
            plain(Str::Theme),
            plain(Str::ThemeDescription),
            plain(Str::FontSize),
            plain(Str::FontSizeDescription),
            plain(Str::BorderRadius),
            plain(Str::BorderRadiusDescription),
            plain(Str::Large),
            plain(Str::Medium),
            plain(Str::Small),
            plain(Str::Tools),
            plain(Str::JsonFormatterTitle),
            plain(Str::EncoderDecoderTitle),
            plain(Str::JsonPlaceholder),
            plain(Str::FormatButton),
            plain(Str::IndentLabel),
            with(Str::IndentSpaces(NUMBER), &[NUMBER_TEXT]),
            with(
                Str::InvalidJson {
                    line: NUMBER,
                    column: 77,
                    detail: DETAIL.into(),
                },
                &[NUMBER_TEXT, "77", DETAIL],
            ),
            plain(Str::FormatLabel),
            plain(Str::EncodeButton),
            plain(Str::DecodeButton),
            plain(Str::DecodeJwtButton),
            plain(Str::InputLabel),
            plain(Str::OutputLabel),
            term(Str::JwtHeaderLabel),
            term(Str::JwtPayloadLabel),
            plain(Str::JwtSignatureLabel),
            plain(Str::EncoderInputPlaceholder),
            plain(Str::EncoderOutputPlaceholder),
            plain(Str::FormatBase64),
            plain(Str::FormatBase64UrlSafe),
            plain(Str::FormatUrl),
            term(Str::FormatHex),
            plain(Str::FormatJwt),
            plain(Str::JwtEncodeUnsupported),
            with(Str::InvalidHexOddLength(NUMBER), &[NUMBER_TEXT]),
            with(
                Str::InvalidHexDigit {
                    digit: 'Z',
                    position: NUMBER,
                },
                &["Z", NUMBER_TEXT],
            ),
            with(Str::InvalidBase64(DETAIL.into()), &[DETAIL]),
            with(Str::InvalidPercentAt(NUMBER), &[NUMBER_TEXT]),
            with(Str::InvalidPercentEncoding(DETAIL.into()), &[DETAIL]),
            with(Str::NotUtf8(DETAIL.into()), &[DETAIL]),
            plain(Str::JwtEmpty),
            with(Str::JwtPartCount(NUMBER), &[NUMBER_TEXT]),
            // The part name is checked separately: it is language-dependent, so
            // it cannot be a fixed fragment here.
            with(
                Str::JwtPartNotBase64 {
                    part: JwtPart::Header,
                    detail: DETAIL.into(),
                },
                &[DETAIL],
            ),
            with(
                Str::JwtPartNotJson {
                    part: JwtPart::Payload,
                    detail: DETAIL.into(),
                },
                &[DETAIL],
            ),
            with(
                Str::JwtPartNotRenderable {
                    part: JwtPart::Header,
                    detail: DETAIL.into(),
                },
                &[DETAIL],
            ),
        ]
    }

    /// Exhaustive over `Str`: a new variant does not compile until it is given
    /// a position, and `samples` must then have an entry at that position.
    fn position(str: &Str) -> usize {
        match str {
            Str::Settings => 0,
            Str::General => 1,
            Str::Appearance => 2,
            Str::Language => 3,
            Str::LanguageDescription => 4,
            Str::Theme => 5,
            Str::ThemeDescription => 6,
            Str::FontSize => 7,
            Str::FontSizeDescription => 8,
            Str::BorderRadius => 9,
            Str::BorderRadiusDescription => 10,
            Str::Large => 11,
            Str::Medium => 12,
            Str::Small => 13,
            Str::Tools => 14,
            Str::JsonFormatterTitle => 15,
            Str::EncoderDecoderTitle => 16,
            Str::JsonPlaceholder => 17,
            Str::FormatButton => 18,
            Str::IndentLabel => 19,
            Str::IndentSpaces(_) => 20,
            Str::InvalidJson { .. } => 21,
            Str::FormatLabel => 22,
            Str::EncodeButton => 23,
            Str::DecodeButton => 24,
            Str::DecodeJwtButton => 25,
            Str::InputLabel => 26,
            Str::OutputLabel => 27,
            Str::JwtHeaderLabel => 28,
            Str::JwtPayloadLabel => 29,
            Str::JwtSignatureLabel => 30,
            Str::EncoderInputPlaceholder => 31,
            Str::EncoderOutputPlaceholder => 32,
            Str::FormatBase64 => 33,
            Str::FormatBase64UrlSafe => 34,
            Str::FormatUrl => 35,
            Str::FormatHex => 36,
            Str::FormatJwt => 37,
            Str::JwtEncodeUnsupported => 38,
            Str::InvalidHexOddLength(_) => 39,
            Str::InvalidHexDigit { .. } => 40,
            Str::InvalidBase64(_) => 41,
            Str::InvalidPercentAt(_) => 42,
            Str::InvalidPercentEncoding(_) => 43,
            Str::NotUtf8(_) => 44,
            Str::JwtEmpty => 45,
            Str::JwtPartCount(_) => 46,
            Str::JwtPartNotBase64 { .. } => 47,
            Str::JwtPartNotJson { .. } => 48,
            Str::JwtPartNotRenderable { .. } => 49,
        }
    }

    #[test]
    fn every_str_variant_has_a_sample() {
        for (index, sample) in samples().iter().enumerate() {
            assert_eq!(
                position(&sample.str),
                index,
                "samples() is out of step with position() at index {index}: add the \
                 missing entry rather than renumbering position()"
            );
        }
    }

    #[test]
    fn every_language_renders_every_string() {
        for sample in samples() {
            let english = sample.str.clone().text(Language::English).into_owned();

            for language in Language::ALL {
                let text = sample.str.clone().text(language).into_owned();
                let code = language.code();

                assert!(
                    !text.trim().is_empty(),
                    "{code} translation of \"{english}\" is empty"
                );
                for part in sample.parts {
                    assert!(
                        text.contains(part),
                        "{code} translation of \"{english}\" dropped the runtime value \
                         `{part}`; it rendered as \"{text}\""
                    );
                }
            }
        }
    }

    #[test]
    fn translations_match_their_declared_kind() {
        for sample in samples() {
            let english = sample.str.clone().text(Language::English).into_owned();

            for language in Language::ALL {
                if language == Language::English {
                    continue;
                }
                let text = sample.str.clone().text(language).into_owned();
                let code = language.code();

                match sample.expect {
                    Expect::Translated => assert_ne!(
                        text, english,
                        "{code} still shows the English text for \"{english}\" — translate it, \
                         or declare it with term() if it really is the same word"
                    ),
                    Expect::SameEverywhere => assert_eq!(
                        text, english,
                        "\"{english}\" is declared as a term of art that is identical in every \
                         language, but {code} differs — declare it with plain() instead"
                    ),
                }
            }
        }
    }

    #[test]
    fn every_language_names_every_jwt_part() {
        for part in [JwtPart::Header, JwtPart::Payload] {
            for language in Language::ALL {
                assert!(
                    !part.name(language).trim().is_empty(),
                    "{} has no name for a JWT part",
                    language.code()
                );
            }
        }
    }
}
