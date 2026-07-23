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

/// The body as the Preview toggle shows it.
///
/// For HTML this strips the markup to the readable text a page is made of —
/// GPUI has no browser to render a real page, so an honest text preview is what
/// "Preview" means here, and the tab says so. Any other kind falls back to its
/// pretty form, so Preview is never a worse view than Pretty.
pub fn preview(body: &str, kind: BodyKind) -> String {
    match kind {
        BodyKind::Html => strip_html(body),
        _ => prettify(body, kind),
    }
}

/// Reduces HTML to its visible text: the contents of `<script>` and `<style>`
/// are dropped, every other tag is removed, a handful of common entities are
/// decoded, and runs of blank lines are collapsed.
fn strip_html(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let bytes = html.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'<' {
            // Skip the whole contents of a script or style element, not just
            // its tags — that text is code, not something to read.
            let rest = &html[i..];
            let lower = rest
                .get(..rest.len().min(8))
                .unwrap_or_default()
                .to_ascii_lowercase();
            if let Some(tag) = ["script", "style"]
                .into_iter()
                .find(|tag| lower.starts_with(&format!("<{tag}")))
            {
                let close = format!("</{tag}");
                match rest.to_ascii_lowercase().find(&close) {
                    Some(end) => {
                        // Advance past the closing tag's `>`.
                        let after = i + end;
                        i = html[after..]
                            .find('>')
                            .map_or(html.len(), |offset| after + offset + 1);
                    }
                    None => break,
                }
                continue;
            }

            // An ordinary tag: skip to its `>`.
            match rest.find('>') {
                Some(offset) => i += offset + 1,
                None => break,
            }
        } else {
            out.push(bytes[i] as char);
            i += 1;
        }
    }

    decode_entities(collapse_blank_lines(&out))
}

/// Collapses runs of whitespace-only lines to a single blank line and trims each
/// line, so stripped markup reads as text rather than a column of indentation.
fn collapse_blank_lines(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut blank_run = 0;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            blank_run += 1;
            if blank_run <= 1 {
                out.push('\n');
            }
        } else {
            blank_run = 0;
            out.push_str(trimmed);
            out.push('\n');
        }
    }
    out.trim().to_string()
}

/// Decodes the few HTML entities common enough that leaving them raw would read
/// as noise. Not a full entity table — this is a preview, not a parser.
fn decode_entities(text: String) -> String {
    text.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
}

#[cfg(test)]
mod tests {
    use super::{decode, kind_of, preview, prettify};
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

    #[test]
    fn html_preview_keeps_text_and_drops_tags() {
        let html = "<html><body><h1>Title</h1><p>Hello &amp; welcome</p></body></html>";
        let text = preview(html, BodyKind::Html);
        assert!(text.contains("Title"));
        assert!(text.contains("Hello & welcome"));
        assert!(!text.contains('<'));
    }

    #[test]
    fn html_preview_drops_script_and_style_contents() {
        let html = "<style>.a{color:red}</style><p>Visible</p><script>alert(1)</script>";
        let text = preview(html, BodyKind::Html);
        assert!(text.contains("Visible"));
        assert!(!text.contains("color:red"));
        assert!(!text.contains("alert"));
    }

    #[test]
    fn preview_of_json_falls_back_to_pretty() {
        assert_eq!(
            preview(r#"{"a":1}"#, BodyKind::Json),
            prettify(r#"{"a":1}"#, BodyKind::Json)
        );
    }
}
