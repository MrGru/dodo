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
    SearchSettingsPlaceholder,
    NoSettingsMatch,

    // Sidebar.
    Tools,
    JsonFormatterTitle,
    EncoderDecoderTitle,
    ApiExplorerTitle,

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

    // API Explorer — collections panel.
    Collections,
    NoCollections,
    NoCollectionsHint,

    // API Explorer — request bar and tab strip.
    UrlPlaceholder,
    Send,
    NewRequest,
    CloseRequest,
    NameRequest,
    NameRequestPlaceholder,
    SaveName,
    GenerateCodeLater,
    ArrivesLater,

    // API Explorer — request tabs.
    RequestTabParams,
    RequestTabHeaders,
    RequestTabBody,
    RequestTabAuth,
    RequestTabScripts,

    // API Explorer — key/value tables.
    ColumnKey,
    ColumnValue,
    Add,
    AddParameter,
    AddHeader,
    DeleteRow,
    NoActiveParams,
    /// "{count} active params" — the summary above the params table.
    ActiveParams(usize),
    NoActiveHeaders,
    /// "{count} active headers" — the summary above the headers table.
    ActiveHeaders(usize),
    ParamKeyPlaceholder,
    ParamValuePlaceholder,
    HeaderKeyPlaceholder,
    HeaderValuePlaceholder,
    ColumnDescription,
    DescriptionPlaceholder,
    DuplicateRow,
    MoveRowUp,
    MoveRowDown,
    AddField,
    NoActiveFields,
    /// "{count} active fields" — the summary above the form-body table.
    ActiveFields(usize),
    FieldKeyPlaceholder,
    FieldValuePlaceholder,

    // API Explorer — Body tab.
    BodyTypeLabel,
    BodyTypeNone,
    BodyTypeJson,
    BodyTypeText,
    BodyTypeXml,
    BodyTypeHtml,
    BodyTypeFormData,
    BodyTypeUrlEncoded,
    BodyTypeBinary,
    BodyPlaceholder,
    NoBodyTitle,
    NoBodyHint,
    BinaryBodyLater,
    /// "GET requests are sent without a body." The method is a wire token and
    /// is not translated; the sentence around it is.
    MethodSendsNoBody(String),

    // API Explorer — Auth tab.
    AuthTypeLabel,
    AuthTypeNone,
    AuthTypeBearer,
    AuthTypeBasic,
    AuthTypeApiKey,
    AuthTypeOAuth2,
    OAuth2Later,
    NoAuthTitle,
    NoAuthHint,
    AuthTokenLabel,
    AuthTokenPlaceholder,
    AuthUsernameLabel,
    AuthUsernamePlaceholder,
    AuthPasswordLabel,
    AuthPasswordPlaceholder,
    ApiKeyNameLabel,
    ApiKeyNamePlaceholder,
    ApiKeyValueLabel,
    ApiKeyValuePlaceholder,
    ApiKeySendAs,
    ApiKeyInHeader,
    ApiKeyInQuery,

    // API Explorer — Scripts tab.
    ScriptsNotExecuted,
    PreRequestScriptLabel,
    PreRequestScriptPlaceholder,
    PostResponseScriptLabel,
    PostResponseScriptPlaceholder,

    // API Explorer — response viewer.
    ResponseTabBody,
    ResponseTabHeaders,
    ResponseTabCookies,
    ResponseTabTests,
    ResponseTabConsole,
    NoResponseYet,
    NoResponseHint,
    Sending,
    RequestFailed,
    CollapseResponse,
    ExpandResponse,
    BodyPretty,
    BodyRaw,
    Copy,
    LoadMoreLines,
    BodyTruncated,
    /// "{shown} of {total} lines" — the response body footer.
    LineRange {
        shown: usize,
        total: usize,
    },

    // API Explorer — status classes.
    StatusClassInfo,
    StatusClassSuccess,
    StatusClassRedirect,
    StatusClassClientError,
    StatusClassServerError,
    StatusClassUnknown,

    // API Explorer — request failures.
    /// The URL parser's message is third-party English and is kept verbatim.
    HttpInvalidUrl(String),
    HttpUnsupportedScheme(String),
    HttpInvalidHeader(String),
    HttpTimeout(u64),
    HttpDnsFailure(String),
    /// The underlying error chain is third-party English and is kept verbatim.
    HttpConnectFailure(String),
    HttpTlsFailure(String),
    HttpBodyNotText(String),
    HttpUnexpected(String),

    // API Explorer — collections panel (phase 3).
    ImportCollection,
    NewCollection,
    NewFolder,
    SearchCollectionsPlaceholder,
    Rename,
    Delete,
    Duplicate,
    Open,
    MoreActions,
    NamePlaceholder,
    DefaultCollectionName,
    DefaultFolderName,
    SaveToCollectionNote,
    /// The store's own IO/serde message is third-party English, kept verbatim.
    CollectionStoreError(String),
    CollectionImportError(String),

    // API Explorer — request history (phase 3).
    History,
    NoHistory,
    NoHistoryHint,
    HistoryReopen,
    HistoryResend,
    HistoryClearAll,
    HistoryJustNow,
    /// "{minutes}m ago" — how long ago a request in the history ran.
    HistoryMinutesAgo(u64),
    HistoryHoursAgo(u64),
    HistoryDaysAgo(u64),

    // API Explorer — response viewer polish (phase 3).
    BodyPreview,
    BodyTree,
    SaveToFile,
    /// "Showing the first {count} nodes — collapse some to see the rest."
    JsonTreeTruncated(usize),
    HtmlPreviewNote,
    NoCookies,
    NoCookiesHint,
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
            (Str::SearchSettingsPlaceholder, Language::English) => {
                "Search settings, then press Enter to jump".into()
            }
            (Str::SearchSettingsPlaceholder, Language::Vietnamese) => {
                "Tìm cài đặt, rồi nhấn Enter để chuyển tới".into()
            }
            (Str::NoSettingsMatch, Language::English) => "No setting matches that search.".into(),
            (Str::NoSettingsMatch, Language::Vietnamese) => {
                "Không có cài đặt nào khớp với tìm kiếm đó.".into()
            }

            (Str::Tools, Language::English) => "Tools".into(),
            (Str::Tools, Language::Vietnamese) => "Công cụ".into(),
            (Str::JsonFormatterTitle, Language::English) => "Json formatter".into(),
            (Str::JsonFormatterTitle, Language::Vietnamese) => "Định dạng JSON".into(),
            (Str::EncoderDecoderTitle, Language::English) => "Encoder / Decoder".into(),
            (Str::EncoderDecoderTitle, Language::Vietnamese) => "Mã hoá / Giải mã".into(),
            (Str::ApiExplorerTitle, Language::English) => "API Explorer".into(),
            (Str::ApiExplorerTitle, Language::Vietnamese) => "Khám phá API".into(),

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

            (Str::Collections, Language::English) => "Collections".into(),
            (Str::Collections, Language::Vietnamese) => "Bộ sưu tập".into(),
            (Str::NoCollections, Language::English) => "No collections yet".into(),
            (Str::NoCollections, Language::Vietnamese) => "Chưa có bộ sưu tập nào".into(),
            (Str::NoCollectionsHint, Language::English) => {
                "Saved requests will be grouped here.".into()
            }
            (Str::NoCollectionsHint, Language::Vietnamese) => {
                "Các yêu cầu đã lưu sẽ được nhóm ở đây.".into()
            }

            (Str::UrlPlaceholder, Language::English) => {
                "Enter a URL, then press Send.".into()
            }
            (Str::UrlPlaceholder, Language::Vietnamese) => {
                "Nhập URL rồi bấm Gửi.".into()
            }
            (Str::Send, Language::English) => "Send".into(),
            (Str::Send, Language::Vietnamese) => "Gửi".into(),
            (Str::NewRequest, Language::English) => "New request".into(),
            (Str::NewRequest, Language::Vietnamese) => "Yêu cầu mới".into(),
            (Str::CloseRequest, Language::English) => "Close request".into(),
            (Str::CloseRequest, Language::Vietnamese) => "Đóng yêu cầu".into(),
            (Str::NameRequest, Language::English) => "Name this request".into(),
            (Str::NameRequest, Language::Vietnamese) => "Đặt tên yêu cầu này".into(),
            (Str::NameRequestPlaceholder, Language::English) => "Request name".into(),
            (Str::NameRequestPlaceholder, Language::Vietnamese) => "Tên yêu cầu".into(),
            (Str::SaveName, Language::English) => "Save name".into(),
            (Str::SaveName, Language::Vietnamese) => "Lưu tên".into(),
            (Str::GenerateCodeLater, Language::English) => {
                "Code generation arrives in a later step.".into()
            }
            (Str::GenerateCodeLater, Language::Vietnamese) => {
                "Sinh mã sẽ có ở bước sau.".into()
            }
            (Str::ArrivesLater, Language::English) => "This arrives in a later step.".into(),
            (Str::ArrivesLater, Language::Vietnamese) => "Phần này sẽ có ở bước sau.".into(),

            (Str::RequestTabParams, Language::English) => "Params".into(),
            (Str::RequestTabParams, Language::Vietnamese) => "Tham số".into(),
            (Str::RequestTabHeaders, Language::English) => "Headers".into(),
            (Str::RequestTabHeaders, Language::Vietnamese) => "Header".into(),
            (Str::RequestTabBody, Language::English) => "Body".into(),
            (Str::RequestTabBody, Language::Vietnamese) => "Nội dung".into(),
            (Str::RequestTabAuth, Language::English) => "Auth".into(),
            (Str::RequestTabAuth, Language::Vietnamese) => "Xác thực".into(),
            (Str::RequestTabScripts, Language::English) => "Scripts".into(),
            (Str::RequestTabScripts, Language::Vietnamese) => "Kịch bản".into(),

            (Str::ColumnKey, Language::English) => "KEY".into(),
            (Str::ColumnKey, Language::Vietnamese) => "KHOÁ".into(),
            (Str::ColumnValue, Language::English) => "VALUE".into(),
            (Str::ColumnValue, Language::Vietnamese) => "GIÁ TRỊ".into(),
            (Str::Add, Language::English) => "Add".into(),
            (Str::Add, Language::Vietnamese) => "Thêm".into(),
            (Str::AddParameter, Language::English) => "Add parameter".into(),
            (Str::AddParameter, Language::Vietnamese) => "Thêm tham số".into(),
            (Str::AddHeader, Language::English) => "Add header".into(),
            (Str::AddHeader, Language::Vietnamese) => "Thêm header".into(),
            (Str::DeleteRow, Language::English) => "Delete row".into(),
            (Str::DeleteRow, Language::Vietnamese) => "Xoá dòng".into(),
            (Str::NoActiveParams, Language::English) => "No active params".into(),
            (Str::NoActiveParams, Language::Vietnamese) => "Không có tham số nào bật".into(),
            (Str::ActiveParams(count), Language::English) => {
                format!("{count} active params").into()
            }
            (Str::ActiveParams(count), Language::Vietnamese) => {
                format!("{count} tham số đang bật").into()
            }
            (Str::NoActiveHeaders, Language::English) => "No active headers".into(),
            (Str::NoActiveHeaders, Language::Vietnamese) => "Không có header nào bật".into(),
            (Str::ActiveHeaders(count), Language::English) => {
                format!("{count} active headers").into()
            }
            (Str::ActiveHeaders(count), Language::Vietnamese) => {
                format!("{count} header đang bật").into()
            }
            (Str::ParamKeyPlaceholder, Language::English) => "Parameter".into(),
            (Str::ParamKeyPlaceholder, Language::Vietnamese) => "Tham số".into(),
            (Str::ParamValuePlaceholder, Language::English) => "Value".into(),
            (Str::ParamValuePlaceholder, Language::Vietnamese) => "Giá trị".into(),
            (Str::HeaderKeyPlaceholder, Language::English) => "Header".into(),
            (Str::HeaderKeyPlaceholder, Language::Vietnamese) => "Tên header".into(),
            (Str::HeaderValuePlaceholder, Language::English) => "Value".into(),
            (Str::HeaderValuePlaceholder, Language::Vietnamese) => "Giá trị".into(),
            (Str::ColumnDescription, Language::English) => "DESCRIPTION".into(),
            (Str::ColumnDescription, Language::Vietnamese) => "MÔ TẢ".into(),
            (Str::DescriptionPlaceholder, Language::English) => "Description".into(),
            (Str::DescriptionPlaceholder, Language::Vietnamese) => "Mô tả".into(),
            (Str::DuplicateRow, Language::English) => "Duplicate row".into(),
            (Str::DuplicateRow, Language::Vietnamese) => "Nhân đôi dòng".into(),
            (Str::MoveRowUp, Language::English) => "Move row up".into(),
            (Str::MoveRowUp, Language::Vietnamese) => "Chuyển dòng lên".into(),
            (Str::MoveRowDown, Language::English) => "Move row down".into(),
            (Str::MoveRowDown, Language::Vietnamese) => "Chuyển dòng xuống".into(),
            (Str::AddField, Language::English) => "Add field".into(),
            (Str::AddField, Language::Vietnamese) => "Thêm trường".into(),
            (Str::NoActiveFields, Language::English) => "No active fields".into(),
            (Str::NoActiveFields, Language::Vietnamese) => "Không có trường nào đang bật".into(),
            (Str::ActiveFields(count), Language::English) => format!("{count} active fields").into(),
            (Str::ActiveFields(count), Language::Vietnamese) => {
                format!("{count} trường đang bật").into()
            }
            (Str::FieldKeyPlaceholder, Language::English) => "Field".into(),
            (Str::FieldKeyPlaceholder, Language::Vietnamese) => "Trường".into(),
            (Str::FieldValuePlaceholder, Language::English) => "Value".into(),
            (Str::FieldValuePlaceholder, Language::Vietnamese) => "Giá trị".into(),

            (Str::BodyTypeLabel, Language::English) => "Body type".into(),
            (Str::BodyTypeLabel, Language::Vietnamese) => "Loại nội dung".into(),
            (Str::BodyTypeNone, Language::English) => "None".into(),
            (Str::BodyTypeNone, Language::Vietnamese) => "Không có".into(),
            (Str::BodyTypeJson, Language::English) => "JSON".into(),
            (Str::BodyTypeJson, Language::Vietnamese) => "JSON".into(),
            (Str::BodyTypeText, Language::English) => "Raw text".into(),
            (Str::BodyTypeText, Language::Vietnamese) => "Văn bản thô".into(),
            (Str::BodyTypeXml, Language::English) => "XML".into(),
            (Str::BodyTypeXml, Language::Vietnamese) => "XML".into(),
            (Str::BodyTypeHtml, Language::English) => "HTML".into(),
            (Str::BodyTypeHtml, Language::Vietnamese) => "HTML".into(),
            (Str::BodyTypeFormData, Language::English) => "Form data".into(),
            (Str::BodyTypeFormData, Language::Vietnamese) => "Dữ liệu biểu mẫu".into(),
            (Str::BodyTypeUrlEncoded, Language::English) => "x-www-form-urlencoded".into(),
            (Str::BodyTypeUrlEncoded, Language::Vietnamese) => "x-www-form-urlencoded".into(),
            (Str::BodyTypeBinary, Language::English) => "Binary".into(),
            (Str::BodyTypeBinary, Language::Vietnamese) => "Nhị phân".into(),
            (Str::BodyPlaceholder, Language::English) => {
                "Type or paste the request body here.".into()
            }
            (Str::BodyPlaceholder, Language::Vietnamese) => {
                "Nhập hoặc dán nội dung yêu cầu vào đây.".into()
            }
            (Str::NoBodyTitle, Language::English) => "No body".into(),
            (Str::NoBodyTitle, Language::Vietnamese) => "Không có nội dung".into(),
            (Str::NoBodyHint, Language::English) => {
                "This request is sent without a body. Choose a type above to add one.".into()
            }
            (Str::NoBodyHint, Language::Vietnamese) => {
                "Yêu cầu này được gửi mà không có nội dung. Chọn một loại ở trên để thêm.".into()
            }
            (Str::BinaryBodyLater, Language::English) => {
                "A binary body needs a file picker; it arrives in a later step.".into()
            }
            (Str::BinaryBodyLater, Language::Vietnamese) => {
                "Nội dung nhị phân cần bộ chọn tệp; phần này sẽ có ở bước sau.".into()
            }
            (Str::MethodSendsNoBody(method), Language::English) => {
                format!("{method} requests are sent without a body.").into()
            }
            (Str::MethodSendsNoBody(method), Language::Vietnamese) => {
                format!("Yêu cầu {method} được gửi mà không có nội dung.").into()
            }

            (Str::AuthTypeLabel, Language::English) => "Auth type".into(),
            (Str::AuthTypeLabel, Language::Vietnamese) => "Kiểu xác thực".into(),
            (Str::AuthTypeNone, Language::English) => "No auth".into(),
            (Str::AuthTypeNone, Language::Vietnamese) => "Không xác thực".into(),
            (Str::AuthTypeBearer, Language::English) => "Bearer token".into(),
            (Str::AuthTypeBearer, Language::Vietnamese) => "Bearer token".into(),
            (Str::AuthTypeBasic, Language::English) => "Basic auth".into(),
            (Str::AuthTypeBasic, Language::Vietnamese) => "Basic auth".into(),
            (Str::AuthTypeApiKey, Language::English) => "API key".into(),
            (Str::AuthTypeApiKey, Language::Vietnamese) => "API key".into(),
            (Str::AuthTypeOAuth2, Language::English) => "OAuth 2.0".into(),
            (Str::AuthTypeOAuth2, Language::Vietnamese) => "OAuth 2.0".into(),
            (Str::OAuth2Later, Language::English) => {
                "OAuth 2.0 needs a browser redirect and a token store; it arrives in a later step."
                    .into()
            }
            (Str::OAuth2Later, Language::Vietnamese) => {
                "OAuth 2.0 cần chuyển hướng trình duyệt và nơi lưu token; phần này sẽ có ở bước sau."
                    .into()
            }
            (Str::NoAuthTitle, Language::English) => "No authorization".into(),
            (Str::NoAuthTitle, Language::Vietnamese) => "Không có xác thực".into(),
            (Str::NoAuthHint, Language::English) => {
                "This request carries no Authorization header. Choose a scheme above to add one."
                    .into()
            }
            (Str::NoAuthHint, Language::Vietnamese) => {
                "Yêu cầu này không mang header Authorization. Chọn một cách ở trên để thêm.".into()
            }
            (Str::AuthTokenLabel, Language::English) => "Token".into(),
            (Str::AuthTokenLabel, Language::Vietnamese) => "Token".into(),
            (Str::AuthTokenPlaceholder, Language::English) => "Paste the bearer token".into(),
            (Str::AuthTokenPlaceholder, Language::Vietnamese) => "Dán bearer token vào đây".into(),
            (Str::AuthUsernameLabel, Language::English) => "Username".into(),
            (Str::AuthUsernameLabel, Language::Vietnamese) => "Tên đăng nhập".into(),
            (Str::AuthUsernamePlaceholder, Language::English) => "Your username".into(),
            (Str::AuthUsernamePlaceholder, Language::Vietnamese) => {
                "Tên đăng nhập của bạn".into()
            }
            (Str::AuthPasswordLabel, Language::English) => "Password".into(),
            (Str::AuthPasswordLabel, Language::Vietnamese) => "Mật khẩu".into(),
            (Str::AuthPasswordPlaceholder, Language::English) => "Your password".into(),
            (Str::AuthPasswordPlaceholder, Language::Vietnamese) => "Mật khẩu của bạn".into(),
            (Str::ApiKeyNameLabel, Language::English) => "Key".into(),
            (Str::ApiKeyNameLabel, Language::Vietnamese) => "Khoá".into(),
            (Str::ApiKeyNamePlaceholder, Language::English) => "For example X-Api-Key".into(),
            (Str::ApiKeyNamePlaceholder, Language::Vietnamese) => "Ví dụ X-Api-Key".into(),
            (Str::ApiKeyValueLabel, Language::English) => "Value".into(),
            (Str::ApiKeyValueLabel, Language::Vietnamese) => "Giá trị".into(),
            (Str::ApiKeyValuePlaceholder, Language::English) => "The key's value".into(),
            (Str::ApiKeyValuePlaceholder, Language::Vietnamese) => "Giá trị của khoá".into(),
            (Str::ApiKeySendAs, Language::English) => "Send as".into(),
            (Str::ApiKeySendAs, Language::Vietnamese) => "Gửi dưới dạng".into(),
            (Str::ApiKeyInHeader, Language::English) => "Header".into(),
            (Str::ApiKeyInHeader, Language::Vietnamese) => "Header".into(),
            (Str::ApiKeyInQuery, Language::English) => "Query parameter".into(),
            (Str::ApiKeyInQuery, Language::Vietnamese) => "Tham số truy vấn".into(),

            (Str::ScriptsNotExecuted, Language::English) => {
                "Scripts are saved with the request for this session. Nothing runs them yet — \
                 there is no script engine in this build.".into()
            }
            (Str::ScriptsNotExecuted, Language::Vietnamese) => {
                "Kịch bản được lưu cùng yêu cầu trong phiên này. Chưa có gì chạy chúng — \
                 bản dựng này không có bộ chạy kịch bản.".into()
            }
            (Str::PreRequestScriptLabel, Language::English) => "Pre-request script".into(),
            (Str::PreRequestScriptLabel, Language::Vietnamese) => "Kịch bản trước yêu cầu".into(),
            (Str::PreRequestScriptPlaceholder, Language::English) => {
                "Would run before the request is sent.".into()
            }
            (Str::PreRequestScriptPlaceholder, Language::Vietnamese) => {
                "Sẽ chạy trước khi yêu cầu được gửi.".into()
            }
            (Str::PostResponseScriptLabel, Language::English) => "Post-response script".into(),
            (Str::PostResponseScriptLabel, Language::Vietnamese) => "Kịch bản sau phản hồi".into(),
            (Str::PostResponseScriptPlaceholder, Language::English) => {
                "Would run after the response arrives.".into()
            }
            (Str::PostResponseScriptPlaceholder, Language::Vietnamese) => {
                "Sẽ chạy sau khi phản hồi về.".into()
            }

            (Str::ResponseTabBody, Language::English) => "Body".into(),
            (Str::ResponseTabBody, Language::Vietnamese) => "Nội dung".into(),
            (Str::ResponseTabHeaders, Language::English) => "Headers".into(),
            (Str::ResponseTabHeaders, Language::Vietnamese) => "Header".into(),
            (Str::ResponseTabCookies, Language::English) => "Cookies".into(),
            (Str::ResponseTabCookies, Language::Vietnamese) => "Cookie".into(),
            (Str::ResponseTabTests, Language::English) => "Tests".into(),
            (Str::ResponseTabTests, Language::Vietnamese) => "Kiểm thử".into(),
            (Str::ResponseTabConsole, Language::English) => "Console".into(),
            (Str::ResponseTabConsole, Language::Vietnamese) => "Nhật ký".into(),
            (Str::NoResponseYet, Language::English) => "No response yet".into(),
            (Str::NoResponseYet, Language::Vietnamese) => "Chưa có phản hồi".into(),
            (Str::NoResponseHint, Language::English) => {
                "Send the request to see the response here.".into()
            }
            (Str::NoResponseHint, Language::Vietnamese) => {
                "Gửi yêu cầu để xem phản hồi ở đây.".into()
            }
            (Str::Sending, Language::English) => "Sending…".into(),
            (Str::Sending, Language::Vietnamese) => "Đang gửi…".into(),
            (Str::RequestFailed, Language::English) => "FAILED".into(),
            (Str::RequestFailed, Language::Vietnamese) => "THẤT BẠI".into(),
            (Str::CollapseResponse, Language::English) => "Collapse response".into(),
            (Str::CollapseResponse, Language::Vietnamese) => "Thu gọn phản hồi".into(),
            (Str::ExpandResponse, Language::English) => "Expand response".into(),
            (Str::ExpandResponse, Language::Vietnamese) => "Mở rộng phản hồi".into(),
            (Str::BodyPretty, Language::English) => "Pretty".into(),
            (Str::BodyPretty, Language::Vietnamese) => "Đẹp".into(),
            (Str::BodyRaw, Language::English) => "Raw".into(),
            (Str::BodyRaw, Language::Vietnamese) => "Thô".into(),
            (Str::Copy, Language::English) => "Copy".into(),
            (Str::Copy, Language::Vietnamese) => "Sao chép".into(),
            (Str::LoadMoreLines, Language::English) => "Load more lines".into(),
            (Str::LoadMoreLines, Language::Vietnamese) => "Tải thêm dòng".into(),
            (Str::BodyTruncated, Language::English) => {
                "The body was too large and was cut short.".into()
            }
            (Str::BodyTruncated, Language::Vietnamese) => {
                "Nội dung quá lớn nên đã bị cắt bớt.".into()
            }
            (Str::LineRange { shown, total }, Language::English) => {
                format!("{shown} of {total} lines").into()
            }
            (Str::LineRange { shown, total }, Language::Vietnamese) => {
                format!("{shown} trên {total} dòng").into()
            }

            (Str::StatusClassInfo, Language::English) => "INFO".into(),
            (Str::StatusClassInfo, Language::Vietnamese) => "THÔNG TIN".into(),
            (Str::StatusClassSuccess, Language::English) => "SUCCESS".into(),
            (Str::StatusClassSuccess, Language::Vietnamese) => "THÀNH CÔNG".into(),
            (Str::StatusClassRedirect, Language::English) => "REDIRECT".into(),
            (Str::StatusClassRedirect, Language::Vietnamese) => "CHUYỂN HƯỚNG".into(),
            (Str::StatusClassClientError, Language::English) => "CLIENT ERR".into(),
            (Str::StatusClassClientError, Language::Vietnamese) => "LỖI PHÍA GỌI".into(),
            (Str::StatusClassServerError, Language::English) => "SERVER ERR".into(),
            (Str::StatusClassServerError, Language::Vietnamese) => "LỖI MÁY CHỦ".into(),
            (Str::StatusClassUnknown, Language::English) => "UNKNOWN".into(),
            (Str::StatusClassUnknown, Language::Vietnamese) => "KHÔNG RÕ".into(),

            (Str::HttpInvalidUrl(detail), Language::English) => {
                if detail.is_empty() {
                    "Enter a URL before sending.".into()
                } else {
                    format!("That URL could not be read: {detail}").into()
                }
            }
            (Str::HttpInvalidUrl(detail), Language::Vietnamese) => {
                if detail.is_empty() {
                    "Hãy nhập URL trước khi gửi.".into()
                } else {
                    format!("Không đọc được URL đó: {detail}").into()
                }
            }
            (Str::HttpUnsupportedScheme(scheme), Language::English) => {
                format!("This tool can only fetch http and https, not {scheme}.").into()
            }
            (Str::HttpUnsupportedScheme(scheme), Language::Vietnamese) => {
                format!("Công cụ này chỉ gọi được http và https, không phải {scheme}.").into()
            }
            (Str::HttpInvalidHeader(name), Language::English) => {
                format!("The header \"{name}\" cannot be sent as written.").into()
            }
            (Str::HttpInvalidHeader(name), Language::Vietnamese) => {
                format!("Header \"{name}\" không gửi được như đang viết.").into()
            }
            (Str::HttpTimeout(seconds), Language::English) => {
                format!("No response within {seconds} seconds.").into()
            }
            (Str::HttpTimeout(seconds), Language::Vietnamese) => {
                format!("Không có phản hồi trong {seconds} giây.").into()
            }
            (Str::HttpDnsFailure(host), Language::English) => {
                format!("The address \"{host}\" could not be found.").into()
            }
            (Str::HttpDnsFailure(host), Language::Vietnamese) => {
                format!("Không tìm thấy địa chỉ \"{host}\".").into()
            }
            (Str::HttpConnectFailure(detail), Language::English) => {
                format!("Could not connect: {detail}").into()
            }
            (Str::HttpConnectFailure(detail), Language::Vietnamese) => {
                format!("Không kết nối được: {detail}").into()
            }
            (Str::HttpTlsFailure(detail), Language::English) => {
                format!("The secure connection was refused: {detail}").into()
            }
            (Str::HttpTlsFailure(detail), Language::Vietnamese) => {
                format!("Kết nối bảo mật bị từ chối: {detail}").into()
            }
            (Str::HttpBodyNotText(detail), Language::English) => {
                format!("The response is not text this viewer can show ({detail}).").into()
            }
            (Str::HttpBodyNotText(detail), Language::Vietnamese) => {
                format!("Phản hồi không phải văn bản có thể hiển thị ({detail}).").into()
            }
            (Str::HttpUnexpected(detail), Language::English) => {
                format!("The request failed: {detail}").into()
            }
            (Str::HttpUnexpected(detail), Language::Vietnamese) => {
                format!("Yêu cầu thất bại: {detail}").into()
            }

            (Str::ImportCollection, Language::English) => "Import a collection".into(),
            (Str::ImportCollection, Language::Vietnamese) => "Nhập bộ sưu tập".into(),
            (Str::NewCollection, Language::English) => "New collection".into(),
            (Str::NewCollection, Language::Vietnamese) => "Bộ sưu tập mới".into(),
            (Str::NewFolder, Language::English) => "New folder".into(),
            (Str::NewFolder, Language::Vietnamese) => "Thư mục mới".into(),
            (Str::SearchCollectionsPlaceholder, Language::English) => "Search collections".into(),
            (Str::SearchCollectionsPlaceholder, Language::Vietnamese) => "Tìm bộ sưu tập".into(),
            (Str::Rename, Language::English) => "Rename".into(),
            (Str::Rename, Language::Vietnamese) => "Đổi tên".into(),
            (Str::Delete, Language::English) => "Delete".into(),
            (Str::Delete, Language::Vietnamese) => "Xoá".into(),
            (Str::Duplicate, Language::English) => "Duplicate".into(),
            (Str::Duplicate, Language::Vietnamese) => "Nhân đôi".into(),
            (Str::Open, Language::English) => "Open".into(),
            (Str::Open, Language::Vietnamese) => "Mở".into(),
            (Str::MoreActions, Language::English) => "Actions".into(),
            (Str::MoreActions, Language::Vietnamese) => "Thao tác".into(),
            (Str::NamePlaceholder, Language::English) => "Name".into(),
            (Str::NamePlaceholder, Language::Vietnamese) => "Tên".into(),
            (Str::DefaultCollectionName, Language::English) => "New collection".into(),
            (Str::DefaultCollectionName, Language::Vietnamese) => "Bộ sưu tập mới".into(),
            (Str::DefaultFolderName, Language::English) => "New folder".into(),
            (Str::DefaultFolderName, Language::Vietnamese) => "Thư mục mới".into(),
            (Str::SaveToCollectionNote, Language::English) => {
                "Saved into your collections.".into()
            }
            (Str::SaveToCollectionNote, Language::Vietnamese) => {
                "Đã lưu vào bộ sưu tập của bạn.".into()
            }
            (Str::CollectionStoreError(detail), Language::English) => {
                format!("Could not save collections: {detail}").into()
            }
            (Str::CollectionStoreError(detail), Language::Vietnamese) => {
                format!("Không lưu được bộ sưu tập: {detail}").into()
            }
            (Str::CollectionImportError(detail), Language::English) => {
                format!("Could not import that file: {detail}").into()
            }
            (Str::CollectionImportError(detail), Language::Vietnamese) => {
                format!("Không nhập được tệp đó: {detail}").into()
            }

            (Str::History, Language::English) => "History".into(),
            (Str::History, Language::Vietnamese) => "Lịch sử".into(),
            (Str::NoHistory, Language::English) => "No requests yet".into(),
            (Str::NoHistory, Language::Vietnamese) => "Chưa có yêu cầu nào".into(),
            (Str::NoHistoryHint, Language::English) => {
                "Requests you send appear here, newest first.".into()
            }
            (Str::NoHistoryHint, Language::Vietnamese) => {
                "Các yêu cầu bạn gửi sẽ hiện ở đây, mới nhất trước.".into()
            }
            (Str::HistoryReopen, Language::English) => "Reopen in a new tab".into(),
            (Str::HistoryReopen, Language::Vietnamese) => "Mở lại trong thẻ mới".into(),
            (Str::HistoryResend, Language::English) => "Resend".into(),
            (Str::HistoryResend, Language::Vietnamese) => "Gửi lại".into(),
            (Str::HistoryClearAll, Language::English) => "Clear all".into(),
            (Str::HistoryClearAll, Language::Vietnamese) => "Xoá tất cả".into(),
            (Str::HistoryJustNow, Language::English) => "just now".into(),
            (Str::HistoryJustNow, Language::Vietnamese) => "vừa xong".into(),
            (Str::HistoryMinutesAgo(minutes), Language::English) => {
                format!("{minutes}m ago").into()
            }
            (Str::HistoryMinutesAgo(minutes), Language::Vietnamese) => {
                format!("{minutes} phút trước").into()
            }
            (Str::HistoryHoursAgo(hours), Language::English) => format!("{hours}h ago").into(),
            (Str::HistoryHoursAgo(hours), Language::Vietnamese) => {
                format!("{hours} giờ trước").into()
            }
            (Str::HistoryDaysAgo(days), Language::English) => format!("{days}d ago").into(),
            (Str::HistoryDaysAgo(days), Language::Vietnamese) => format!("{days} ngày trước").into(),

            (Str::BodyPreview, Language::English) => "Preview".into(),
            (Str::BodyPreview, Language::Vietnamese) => "Xem trước".into(),
            (Str::BodyTree, Language::English) => "Tree".into(),
            (Str::BodyTree, Language::Vietnamese) => "Cây".into(),
            (Str::SaveToFile, Language::English) => "Save to file".into(),
            (Str::SaveToFile, Language::Vietnamese) => "Lưu ra tệp".into(),
            (Str::JsonTreeTruncated(count), Language::English) => {
                format!("Showing the first {count} nodes — collapse some to see the rest.").into()
            }
            (Str::JsonTreeTruncated(count), Language::Vietnamese) => {
                format!("Đang hiện {count} nút đầu — thu gọn bớt để xem phần còn lại.").into()
            }
            (Str::HtmlPreviewNote, Language::English) => {
                "Text preview — markup is shown as readable text, not rendered.".into()
            }
            (Str::HtmlPreviewNote, Language::Vietnamese) => {
                "Xem trước văn bản — mã đánh dấu hiển thị dạng chữ, không kết xuất.".into()
            }
            (Str::NoCookies, Language::English) => "No cookies set".into(),
            (Str::NoCookies, Language::Vietnamese) => "Không có cookie nào".into(),
            (Str::NoCookiesHint, Language::English) => {
                "This response sent no Set-Cookie headers.".into()
            }
            (Str::NoCookiesHint, Language::Vietnamese) => {
                "Phản hồi này không gửi header Set-Cookie nào.".into()
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
            plain(Str::SearchSettingsPlaceholder),
            plain(Str::NoSettingsMatch),
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
            // API Explorer. Appended rather than slotted in beside the strings
            // they read next to, so that adding a tool does not renumber every
            // existing entry.
            plain(Str::ApiExplorerTitle),
            plain(Str::Collections),
            plain(Str::NoCollections),
            plain(Str::NoCollectionsHint),
            plain(Str::UrlPlaceholder),
            plain(Str::Send),
            plain(Str::NewRequest),
            plain(Str::CloseRequest),
            plain(Str::NameRequest),
            plain(Str::NameRequestPlaceholder),
            plain(Str::SaveName),
            plain(Str::GenerateCodeLater),
            plain(Str::ArrivesLater),
            plain(Str::RequestTabParams),
            plain(Str::RequestTabHeaders),
            plain(Str::RequestTabBody),
            plain(Str::RequestTabAuth),
            plain(Str::RequestTabScripts),
            plain(Str::ColumnKey),
            plain(Str::ColumnValue),
            plain(Str::Add),
            plain(Str::AddParameter),
            plain(Str::AddHeader),
            plain(Str::DeleteRow),
            plain(Str::NoActiveParams),
            with(Str::ActiveParams(NUMBER), &[NUMBER_TEXT]),
            plain(Str::NoActiveHeaders),
            with(Str::ActiveHeaders(NUMBER), &[NUMBER_TEXT]),
            plain(Str::ParamKeyPlaceholder),
            plain(Str::ParamValuePlaceholder),
            plain(Str::HeaderKeyPlaceholder),
            plain(Str::HeaderValuePlaceholder),
            plain(Str::ColumnDescription),
            plain(Str::DescriptionPlaceholder),
            plain(Str::DuplicateRow),
            plain(Str::MoveRowUp),
            plain(Str::MoveRowDown),
            plain(Str::AddField),
            plain(Str::NoActiveFields),
            with(Str::ActiveFields(NUMBER), &[NUMBER_TEXT]),
            plain(Str::FieldKeyPlaceholder),
            plain(Str::FieldValuePlaceholder),
            plain(Str::BodyTypeLabel),
            plain(Str::BodyTypeNone),
            term(Str::BodyTypeJson),
            plain(Str::BodyTypeText),
            term(Str::BodyTypeXml),
            term(Str::BodyTypeHtml),
            plain(Str::BodyTypeFormData),
            // The wire spelling of the media type, in both languages.
            term(Str::BodyTypeUrlEncoded),
            plain(Str::BodyTypeBinary),
            plain(Str::BodyPlaceholder),
            plain(Str::NoBodyTitle),
            plain(Str::NoBodyHint),
            plain(Str::BinaryBodyLater),
            with(Str::MethodSendsNoBody("GET".into()), &["GET"]),
            plain(Str::AuthTypeLabel),
            plain(Str::AuthTypeNone),
            term(Str::AuthTypeBearer),
            term(Str::AuthTypeBasic),
            term(Str::AuthTypeApiKey),
            term(Str::AuthTypeOAuth2),
            plain(Str::OAuth2Later),
            plain(Str::NoAuthTitle),
            plain(Str::NoAuthHint),
            term(Str::AuthTokenLabel),
            plain(Str::AuthTokenPlaceholder),
            plain(Str::AuthUsernameLabel),
            plain(Str::AuthUsernamePlaceholder),
            plain(Str::AuthPasswordLabel),
            plain(Str::AuthPasswordPlaceholder),
            plain(Str::ApiKeyNameLabel),
            plain(Str::ApiKeyNamePlaceholder),
            plain(Str::ApiKeyValueLabel),
            plain(Str::ApiKeyValuePlaceholder),
            plain(Str::ApiKeySendAs),
            term(Str::ApiKeyInHeader),
            plain(Str::ApiKeyInQuery),
            plain(Str::ScriptsNotExecuted),
            plain(Str::PreRequestScriptLabel),
            plain(Str::PreRequestScriptPlaceholder),
            plain(Str::PostResponseScriptLabel),
            plain(Str::PostResponseScriptPlaceholder),
            plain(Str::ResponseTabBody),
            plain(Str::ResponseTabHeaders),
            plain(Str::ResponseTabCookies),
            plain(Str::ResponseTabTests),
            plain(Str::ResponseTabConsole),
            plain(Str::NoResponseYet),
            plain(Str::NoResponseHint),
            plain(Str::Sending),
            plain(Str::RequestFailed),
            plain(Str::CollapseResponse),
            plain(Str::ExpandResponse),
            plain(Str::BodyPretty),
            plain(Str::BodyRaw),
            plain(Str::Copy),
            plain(Str::LoadMoreLines),
            plain(Str::BodyTruncated),
            with(
                Str::LineRange {
                    shown: NUMBER,
                    total: 77,
                },
                &[NUMBER_TEXT, "77"],
            ),
            plain(Str::StatusClassInfo),
            plain(Str::StatusClassSuccess),
            plain(Str::StatusClassRedirect),
            plain(Str::StatusClassClientError),
            plain(Str::StatusClassServerError),
            plain(Str::StatusClassUnknown),
            with(Str::HttpInvalidUrl(DETAIL.into()), &[DETAIL]),
            with(Str::HttpUnsupportedScheme(DETAIL.into()), &[DETAIL]),
            with(Str::HttpInvalidHeader(DETAIL.into()), &[DETAIL]),
            with(Str::HttpTimeout(NUMBER as u64), &[NUMBER_TEXT]),
            with(Str::HttpDnsFailure(DETAIL.into()), &[DETAIL]),
            with(Str::HttpConnectFailure(DETAIL.into()), &[DETAIL]),
            with(Str::HttpTlsFailure(DETAIL.into()), &[DETAIL]),
            with(Str::HttpBodyNotText(DETAIL.into()), &[DETAIL]),
            with(Str::HttpUnexpected(DETAIL.into()), &[DETAIL]),
            // Phase 3.
            plain(Str::ImportCollection),
            plain(Str::NewCollection),
            plain(Str::NewFolder),
            plain(Str::SearchCollectionsPlaceholder),
            plain(Str::Rename),
            plain(Str::Delete),
            plain(Str::Duplicate),
            plain(Str::Open),
            plain(Str::MoreActions),
            plain(Str::NamePlaceholder),
            plain(Str::DefaultCollectionName),
            plain(Str::DefaultFolderName),
            plain(Str::SaveToCollectionNote),
            with(Str::CollectionStoreError(DETAIL.into()), &[DETAIL]),
            with(Str::CollectionImportError(DETAIL.into()), &[DETAIL]),
            plain(Str::History),
            plain(Str::NoHistory),
            plain(Str::NoHistoryHint),
            plain(Str::HistoryReopen),
            plain(Str::HistoryResend),
            plain(Str::HistoryClearAll),
            plain(Str::HistoryJustNow),
            with(Str::HistoryMinutesAgo(NUMBER as u64), &[NUMBER_TEXT]),
            with(Str::HistoryHoursAgo(NUMBER as u64), &[NUMBER_TEXT]),
            with(Str::HistoryDaysAgo(NUMBER as u64), &[NUMBER_TEXT]),
            plain(Str::BodyPreview),
            plain(Str::BodyTree),
            plain(Str::SaveToFile),
            with(Str::JsonTreeTruncated(NUMBER), &[NUMBER_TEXT]),
            plain(Str::HtmlPreviewNote),
            plain(Str::NoCookies),
            plain(Str::NoCookiesHint),
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
            Str::SearchSettingsPlaceholder => 14,
            Str::NoSettingsMatch => 15,
            Str::Tools => 16,
            Str::JsonFormatterTitle => 17,
            Str::EncoderDecoderTitle => 18,
            Str::JsonPlaceholder => 19,
            Str::FormatButton => 20,
            Str::IndentLabel => 21,
            Str::IndentSpaces(_) => 22,
            Str::InvalidJson { .. } => 23,
            Str::FormatLabel => 24,
            Str::EncodeButton => 25,
            Str::DecodeButton => 26,
            Str::DecodeJwtButton => 27,
            Str::InputLabel => 28,
            Str::OutputLabel => 29,
            Str::JwtHeaderLabel => 30,
            Str::JwtPayloadLabel => 31,
            Str::JwtSignatureLabel => 32,
            Str::EncoderInputPlaceholder => 33,
            Str::EncoderOutputPlaceholder => 34,
            Str::FormatBase64 => 35,
            Str::FormatBase64UrlSafe => 36,
            Str::FormatUrl => 37,
            Str::FormatHex => 38,
            Str::FormatJwt => 39,
            Str::JwtEncodeUnsupported => 40,
            Str::InvalidHexOddLength(_) => 41,
            Str::InvalidHexDigit { .. } => 42,
            Str::InvalidBase64(_) => 43,
            Str::InvalidPercentAt(_) => 44,
            Str::InvalidPercentEncoding(_) => 45,
            Str::NotUtf8(_) => 46,
            Str::JwtEmpty => 47,
            Str::JwtPartCount(_) => 48,
            Str::JwtPartNotBase64 { .. } => 49,
            Str::JwtPartNotJson { .. } => 50,
            Str::JwtPartNotRenderable { .. } => 51,

            Str::ApiExplorerTitle => 52,
            Str::Collections => 53,
            Str::NoCollections => 54,
            Str::NoCollectionsHint => 55,
            Str::UrlPlaceholder => 56,
            Str::Send => 57,
            Str::NewRequest => 58,
            Str::CloseRequest => 59,
            Str::NameRequest => 60,
            Str::NameRequestPlaceholder => 61,
            Str::SaveName => 62,
            Str::GenerateCodeLater => 63,
            Str::ArrivesLater => 64,
            Str::RequestTabParams => 65,
            Str::RequestTabHeaders => 66,
            Str::RequestTabBody => 67,
            Str::RequestTabAuth => 68,
            Str::RequestTabScripts => 69,
            Str::ColumnKey => 70,
            Str::ColumnValue => 71,
            Str::Add => 72,
            Str::AddParameter => 73,
            Str::AddHeader => 74,
            Str::DeleteRow => 75,
            Str::NoActiveParams => 76,
            Str::ActiveParams(_) => 77,
            Str::NoActiveHeaders => 78,
            Str::ActiveHeaders(_) => 79,
            Str::ParamKeyPlaceholder => 80,
            Str::ParamValuePlaceholder => 81,
            Str::HeaderKeyPlaceholder => 82,
            Str::HeaderValuePlaceholder => 83,
            Str::ColumnDescription => 84,
            Str::DescriptionPlaceholder => 85,
            Str::DuplicateRow => 86,
            Str::MoveRowUp => 87,
            Str::MoveRowDown => 88,
            Str::AddField => 89,
            Str::NoActiveFields => 90,
            Str::ActiveFields(_) => 91,
            Str::FieldKeyPlaceholder => 92,
            Str::FieldValuePlaceholder => 93,
            Str::BodyTypeLabel => 94,
            Str::BodyTypeNone => 95,
            Str::BodyTypeJson => 96,
            Str::BodyTypeText => 97,
            Str::BodyTypeXml => 98,
            Str::BodyTypeHtml => 99,
            Str::BodyTypeFormData => 100,
            Str::BodyTypeUrlEncoded => 101,
            Str::BodyTypeBinary => 102,
            Str::BodyPlaceholder => 103,
            Str::NoBodyTitle => 104,
            Str::NoBodyHint => 105,
            Str::BinaryBodyLater => 106,
            Str::MethodSendsNoBody(_) => 107,
            Str::AuthTypeLabel => 108,
            Str::AuthTypeNone => 109,
            Str::AuthTypeBearer => 110,
            Str::AuthTypeBasic => 111,
            Str::AuthTypeApiKey => 112,
            Str::AuthTypeOAuth2 => 113,
            Str::OAuth2Later => 114,
            Str::NoAuthTitle => 115,
            Str::NoAuthHint => 116,
            Str::AuthTokenLabel => 117,
            Str::AuthTokenPlaceholder => 118,
            Str::AuthUsernameLabel => 119,
            Str::AuthUsernamePlaceholder => 120,
            Str::AuthPasswordLabel => 121,
            Str::AuthPasswordPlaceholder => 122,
            Str::ApiKeyNameLabel => 123,
            Str::ApiKeyNamePlaceholder => 124,
            Str::ApiKeyValueLabel => 125,
            Str::ApiKeyValuePlaceholder => 126,
            Str::ApiKeySendAs => 127,
            Str::ApiKeyInHeader => 128,
            Str::ApiKeyInQuery => 129,
            Str::ScriptsNotExecuted => 130,
            Str::PreRequestScriptLabel => 131,
            Str::PreRequestScriptPlaceholder => 132,
            Str::PostResponseScriptLabel => 133,
            Str::PostResponseScriptPlaceholder => 134,
            Str::ResponseTabBody => 135,
            Str::ResponseTabHeaders => 136,
            Str::ResponseTabCookies => 137,
            Str::ResponseTabTests => 138,
            Str::ResponseTabConsole => 139,
            Str::NoResponseYet => 140,
            Str::NoResponseHint => 141,
            Str::Sending => 142,
            Str::RequestFailed => 143,
            Str::CollapseResponse => 144,
            Str::ExpandResponse => 145,
            Str::BodyPretty => 146,
            Str::BodyRaw => 147,
            Str::Copy => 148,
            Str::LoadMoreLines => 149,
            Str::BodyTruncated => 150,
            Str::LineRange { .. } => 151,
            Str::StatusClassInfo => 152,
            Str::StatusClassSuccess => 153,
            Str::StatusClassRedirect => 154,
            Str::StatusClassClientError => 155,
            Str::StatusClassServerError => 156,
            Str::StatusClassUnknown => 157,
            Str::HttpInvalidUrl(_) => 158,
            Str::HttpUnsupportedScheme(_) => 159,
            Str::HttpInvalidHeader(_) => 160,
            Str::HttpTimeout(_) => 161,
            Str::HttpDnsFailure(_) => 162,
            Str::HttpConnectFailure(_) => 163,
            Str::HttpTlsFailure(_) => 164,
            Str::HttpBodyNotText(_) => 165,
            Str::HttpUnexpected(_) => 166,

            Str::ImportCollection => 167,
            Str::NewCollection => 168,
            Str::NewFolder => 169,
            Str::SearchCollectionsPlaceholder => 170,
            Str::Rename => 171,
            Str::Delete => 172,
            Str::Duplicate => 173,
            Str::Open => 174,
            Str::MoreActions => 175,
            Str::NamePlaceholder => 176,
            Str::DefaultCollectionName => 177,
            Str::DefaultFolderName => 178,
            Str::SaveToCollectionNote => 179,
            Str::CollectionStoreError(_) => 180,
            Str::CollectionImportError(_) => 181,
            Str::History => 182,
            Str::NoHistory => 183,
            Str::NoHistoryHint => 184,
            Str::HistoryReopen => 185,
            Str::HistoryResend => 186,
            Str::HistoryClearAll => 187,
            Str::HistoryJustNow => 188,
            Str::HistoryMinutesAgo(_) => 189,
            Str::HistoryHoursAgo(_) => 190,
            Str::HistoryDaysAgo(_) => 191,
            Str::BodyPreview => 192,
            Str::BodyTree => 193,
            Str::SaveToFile => 194,
            Str::JsonTreeTruncated(_) => 195,
            Str::HtmlPreviewNote => 196,
            Str::NoCookies => 197,
            Str::NoCookiesHint => 198,
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
