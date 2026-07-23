//! Deciding what a response body *is*, and rendering it readably.

use crate::api_explorer::models::exchange::BodyKind;

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

/// Reads the `charset` parameter out of a `Content-Type` header value,
/// lowercased and unquoted. `text/html; charset=ISO-8859-1` yields `iso-8859-1`.
fn charset_of(content_type: &str) -> Option<String> {
    content_type.split(';').skip(1).find_map(|param| {
        let (name, value) = param.split_once('=')?;
        name.trim()
            .eq_ignore_ascii_case("charset")
            .then(|| value.trim().trim_matches('"').to_lowercase())
    })
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

/// Decodes response bytes into a displayable string, never failing.
///
/// A browser renders whatever bytes arrive; so does this. Bytes that are valid
/// UTF-8 come back verbatim. When `Content-Type` declares a single-byte legacy
/// charset (`ISO-8859-1` and friends — what `https://www.google.com/` serves),
/// each byte is that encoding's code point. Anything else — a declared UTF-8
/// body with a stray byte, an unknown label, or no charset at all — is read as
/// UTF-8 with invalid sequences replaced, so even a JPEG shows *something*
/// rather than tripping the failure banner. A genuine transport failure (DNS,
/// timeout, TLS, refused connection) still errors upstream in `client`; this
/// only ever sees bytes that already arrived.
pub fn decode(bytes: &[u8], content_type: Option<&str>) -> String {
    match content_type.and_then(charset_of).as_deref() {
        // Single-byte legacy encodings map every byte into U+0000..=U+00FF.
        // windows-1252 differs from latin-1 only across 0x80..=0x9F; latin-1 is
        // a faithful-enough rendering without vendoring a code-page table.
        Some("iso-8859-1" | "latin1" | "latin-1" | "iso8859-1" | "windows-1252" | "cp1252") => {
            bytes.iter().map(|&byte| byte as char).collect()
        }
        // utf-8, us-ascii, an unknown label, or no charset: read as UTF-8 and
        // replace any invalid sequence rather than reject the whole body.
        _ => String::from_utf8_lossy(bytes).into_owned(),
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
        assert_eq!(decode("hello".as_bytes(), None), "hello");
        assert_eq!(decode("xin chào".as_bytes(), None), "xin chào");
    }

    /// Regression for the `https://google.com` failure: `www.google.com` serves
    /// HTML declared `charset=ISO-8859-1` whose bytes are not valid UTF-8. A
    /// strict UTF-8 decode reported it as a transport failure and showed the red
    /// banner; honoring the declared charset renders it instead.
    #[test]
    fn latin1_declared_body_renders_instead_of_failing() {
        // 0xE9 is `é` in ISO-8859-1 but an invalid lone continuation byte in UTF-8.
        let bytes = b"caf\xe9";
        assert_eq!(decode(bytes, Some("text/html; charset=ISO-8859-1")), "café");
    }

    /// Any received bytes must render, not error — even a binary body with no
    /// usable charset. Invalid UTF-8 sequences become the replacement character
    /// rather than tripping a failure.
    #[test]
    fn a_non_utf8_body_renders_rather_than_erroring() {
        let text = decode(&[0xff, 0xfe, 0x00], Some("image/png"));
        assert!(
            text.contains('\u{fffd}'),
            "invalid bytes should decode to replacement chars, got {text:?}"
        );
    }

    #[test]
    fn charset_is_read_case_and_quote_insensitively() {
        // Quoted, upper-cased, extra parameters: still latin-1.
        let bytes = b"\xe9";
        assert_eq!(decode(bytes, Some("text/plain; CharSet=\"iso-8859-1\"")), "é");
        // No charset falls back to lossy UTF-8, so the same byte is replaced.
        assert_eq!(decode(bytes, Some("text/plain")), "\u{fffd}");
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
