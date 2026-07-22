//! What came back: the response as the viewer needs it, independent of which
//! protocol produced it.

use std::time::Duration;

use gpui::{App, Hsla};
use gpui_component::ActiveTheme as _;

use crate::i18n::Str;

/// The class of an HTTP status code, which is what the badge is coloured by
/// and what the caption beside it names.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StatusClass {
    Informational,
    Success,
    Redirect,
    ClientError,
    ServerError,
    /// A status outside 100..=599. Servers do emit these; showing the number
    /// plainly beats pretending it is one of the above.
    Unknown,
}

impl StatusClass {
    pub fn of(status: u16) -> Self {
        match status {
            100..=199 => StatusClass::Informational,
            200..=299 => StatusClass::Success,
            300..=399 => StatusClass::Redirect,
            400..=499 => StatusClass::ClientError,
            500..=599 => StatusClass::ServerError,
            _ => StatusClass::Unknown,
        }
    }

    /// The short caption shown beside the status number ("CLIENT ERR" in the
    /// reference).
    pub fn label(self) -> Str {
        match self {
            StatusClass::Informational => Str::StatusClassInfo,
            StatusClass::Success => Str::StatusClassSuccess,
            StatusClass::Redirect => Str::StatusClassRedirect,
            StatusClass::ClientError => Str::StatusClassClientError,
            StatusClass::ServerError => Str::StatusClassServerError,
            StatusClass::Unknown => Str::StatusClassUnknown,
        }
    }

    /// Theme colour for the badge. As with method colours, these are semantic
    /// theme fields so every theme re-skins them.
    pub fn color(self, cx: &App) -> Hsla {
        match self {
            StatusClass::Success => cx.theme().success,
            StatusClass::Informational | StatusClass::Redirect => cx.theme().info,
            StatusClass::ClientError => cx.theme().warning,
            StatusClass::ServerError => cx.theme().danger,
            StatusClass::Unknown => cx.theme().muted_foreground,
        }
    }
}

/// Which highlighter grammar the body is shown with.
///
/// The variants are exactly the grammars compiled into this build (see the
/// `gpui-component` features in `Cargo.toml`); anything else is [`BodyKind::Text`],
/// which renders uncoloured rather than failing.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum BodyKind {
    Json,
    Html,
    Yaml,
    #[default]
    Text,
}

impl BodyKind {
    /// The language id `InputState::code_editor` / `set_highlighter` expects.
    pub fn language(self) -> &'static str {
        match self {
            BodyKind::Json => "json",
            BodyKind::Html => "html",
            BodyKind::Yaml => "yaml",
            BodyKind::Text => "text",
        }
    }

    /// Only JSON has a pretty form that this phase can produce; the Pretty
    /// toggle is a no-op for the rest rather than a lie.
    pub fn is_prettifiable(self) -> bool {
        matches!(self, BodyKind::Json)
    }
}

/// A completed request/response round trip, as the response viewer needs it.
///
/// Protocol-neutral on purpose: a future WebSocket or gRPC transport fills the
/// same struct, so the viewer does not learn a second shape.
pub struct Exchange {
    pub status: u16,
    /// In wire order, duplicates preserved.
    pub headers: Vec<(String, String)>,
    pub body: String,
    pub kind: BodyKind,
    /// Bytes actually received, which is what the size metric reports — not
    /// the length of `body` after decoding.
    pub size_bytes: usize,
    /// Set when the body hit the read cap and the rest was never fetched.
    pub truncated: bool,
    pub elapsed: Duration,
}

impl Exchange {
    pub fn status_class(&self) -> StatusClass {
        StatusClass::of(self.status)
    }
}

/// Formats a byte count the way a response size is conventionally read.
///
/// Uses 1024-byte units, one decimal place above the KB threshold, and no
/// decimal for a raw byte count.
pub fn format_size(bytes: usize) -> String {
    const KB: f64 = 1024.;
    const MB: f64 = KB * 1024.;

    let bytes = bytes as f64;
    if bytes < KB {
        format!("{bytes:.0} B")
    } else if bytes < MB {
        format!("{:.1} KB", bytes / KB)
    } else {
        format!("{:.1} MB", bytes / MB)
    }
}

/// Formats a duration the way a request timing is conventionally read:
/// milliseconds until a second, then seconds.
pub fn format_duration(elapsed: Duration) -> String {
    let millis = elapsed.as_millis();
    if millis < 1000 {
        format!("{millis} ms")
    } else {
        format!("{:.2} s", elapsed.as_secs_f64())
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{BodyKind, StatusClass, format_duration, format_size};

    #[test]
    fn status_classes_cover_the_ranges() {
        assert_eq!(StatusClass::of(100), StatusClass::Informational);
        assert_eq!(StatusClass::of(200), StatusClass::Success);
        assert_eq!(StatusClass::of(204), StatusClass::Success);
        assert_eq!(StatusClass::of(301), StatusClass::Redirect);
        assert_eq!(StatusClass::of(404), StatusClass::ClientError);
        assert_eq!(StatusClass::of(500), StatusClass::ServerError);
        assert_eq!(StatusClass::of(0), StatusClass::Unknown);
        assert_eq!(StatusClass::of(999), StatusClass::Unknown);
    }

    #[test]
    fn sizes_read_in_sensible_units() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1536), "1.5 KB");
        assert_eq!(format_size(1024 * 1024 * 3), "3.0 MB");
    }

    #[test]
    fn durations_switch_units_at_one_second() {
        assert_eq!(format_duration(Duration::from_millis(314)), "314 ms");
        assert_eq!(format_duration(Duration::from_millis(999)), "999 ms");
        assert_eq!(format_duration(Duration::from_millis(1500)), "1.50 s");
    }

    #[test]
    fn only_json_claims_a_pretty_form() {
        assert!(BodyKind::Json.is_prettifiable());
        assert!(!BodyKind::Html.is_prettifiable());
        assert!(!BodyKind::Text.is_prettifiable());
    }
}
