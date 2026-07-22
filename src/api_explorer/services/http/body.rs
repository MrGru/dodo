//! Deciding what a response body *is*, and rendering it readably.

use crate::api_explorer::models::exchange::BodyKind;
use crate::api_explorer::services::TransportError;

/// Reads the media type out of a `Content-Type` header value, discarding
/// parameters (`; charset=utf-8`) and case.
fn media_type(content_type: &str) -> String {
    content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_lowercase()
}

/// Which highlighter grammar to show a body with.
///
/// Driven by `Content-Type` and falling back to plain text, which renders
/// uncoloured rather than failing. Suffix matching covers the `+json` /
/// `+yaml` structured-syntax convention (`application/problem+json`).
pub fn kind_of(content_type: Option<&str>) -> BodyKind {
    let Some(media) = content_type.map(media_type) else {
        return BodyKind::Text;
    };

    if media.ends_with("/json") || media.ends_with("+json") {
        BodyKind::Json
    } else if media == "text/html" || media == "application/xhtml+xml" {
        BodyKind::Html
    } else if media.ends_with("/yaml") || media.ends_with("+yaml") || media == "text/x-yaml" {
        BodyKind::Yaml
    } else {
        BodyKind::Text
    }
}

/// Decodes response bytes as UTF-8.
///
/// A body that is not UTF-8 — a JPEG, or text in a legacy encoding — is a
/// reportable condition rather than something to render as mojibake, so this
/// returns the error that becomes a calm banner.
pub fn decode(bytes: &[u8], content_type: Option<&str>) -> Result<String, TransportError> {
    match std::str::from_utf8(bytes) {
        Ok(text) => Ok(text.to_string()),
        Err(err) => Err(TransportError::BodyNotText {
            // The media type is the useful half of "why is this not text", so
            // it goes in beside the position of the bad byte.
            detail: match content_type {
                Some(content_type) => format!("{}, {err}", media_type(content_type)),
                None => err.to_string(),
            },
        }),
    }
}

/// The body as the Pretty toggle shows it.
///
/// Only JSON has a pretty form this phase can produce. A body that claims to be
/// JSON but does not parse is returned untouched: the server's actual bytes are
/// more useful than an error at that point, and the status and headers are
/// still there to explain it.
pub fn prettify(body: &str, kind: BodyKind) -> String {
    if !kind.is_prettifiable() {
        return body.to_string();
    }

    match serde_json::from_str::<serde_json::Value>(body) {
        Ok(value) => serde_json::to_string_pretty(&value).unwrap_or_else(|_| body.to_string()),
        Err(_) => body.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{decode, kind_of, prettify};
    use crate::api_explorer::models::exchange::BodyKind;
    use crate::api_explorer::services::TransportError;

    #[test]
    fn content_type_selects_the_grammar() {
        assert_eq!(kind_of(Some("application/json")), BodyKind::Json);
        assert_eq!(
            kind_of(Some("application/json; charset=utf-8")),
            BodyKind::Json
        );
        assert_eq!(kind_of(Some("APPLICATION/JSON")), BodyKind::Json);
        assert_eq!(kind_of(Some("application/problem+json")), BodyKind::Json);
        assert_eq!(kind_of(Some("text/html; charset=UTF-8")), BodyKind::Html);
        assert_eq!(kind_of(Some("application/yaml")), BodyKind::Yaml);
        assert_eq!(kind_of(Some("text/plain")), BodyKind::Text);
        assert_eq!(kind_of(Some("image/png")), BodyKind::Text);
        assert_eq!(kind_of(None), BodyKind::Text);
    }

    #[test]
    fn utf8_bodies_decode() {
        assert_eq!(decode("hello".as_bytes(), None).expect("valid"), "hello");
        assert_eq!(decode("xin chào".as_bytes(), None).expect("valid"), "xin chào");
    }

    #[test]
    fn a_binary_body_is_reported_not_rendered() {
        let error = decode(&[0xff, 0xfe, 0x00], Some("image/png")).expect_err("not text");
        match error {
            TransportError::BodyNotText { detail } => assert!(
                detail.contains("image/png"),
                "the media type should reach the message, got {detail:?}"
            ),
            other => panic!("expected BodyNotText, got {other:?}"),
        }
    }

    #[test]
    fn json_is_pretty_printed() {
        let pretty = prettify(r#"{"a":[1,2]}"#, BodyKind::Json);
        assert_eq!(pretty, "{\n  \"a\": [\n    1,\n    2\n  ]\n}");
    }

    #[test]
    fn malformed_json_is_shown_as_sent() {
        let raw = "{not json";
        assert_eq!(prettify(raw, BodyKind::Json), raw);
    }

    #[test]
    fn non_json_bodies_are_left_alone() {
        let html = "<html>  <body>x</body></html>";
        assert_eq!(prettify(html, BodyKind::Html), html);
    }
}
