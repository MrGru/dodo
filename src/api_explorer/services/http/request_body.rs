//! Turning the Body tab into bytes on the wire.
//!
//! Kept beside the transport rather than in `models` because this is where a
//! body stops being "what the user typed" and becomes an encoding decision —
//! percent-escaping, a multipart layout, a media type. There is still no IO
//! here and nothing names `reqwest`, so all of it is unit tested.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};

use crate::api_explorer::models::body::{BodyDraft, BodyType};
use crate::api_explorer::models::key_value::effective_pairs;

/// The characters `application/x-www-form-urlencoded` leaves alone.
///
/// The WHATWG form serializer keeps `*-._` and the alphanumerics, escapes
/// everything else, and writes a space as `+`. Space is removed from the set
/// here so it survives the percent encoder and is swapped for `+` afterwards;
/// a `+` the user actually typed is escaped to `%2B` by the encoder first, so
/// the two can never be confused.
const FORM_COMPONENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'*')
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b' ');

/// Distinguishes one multipart boundary from the next within a process.
static BOUNDARY_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// A body ready to hand to the transport.
#[derive(Debug, PartialEq, Eq)]
pub struct EncodedBody {
    pub bytes: Vec<u8>,
    /// The media type this encoding implies. Applied only if the user has not
    /// set `Content-Type` themselves — see `http::headers`.
    pub content_type: Option<String>,
}

/// Encodes what the Body tab holds, or `None` when there is nothing to send.
///
/// "Nothing to send" covers more than [`BodyType::None`]: an empty JSON editor
/// or a form with no filled-in rows also produces no body, because sending
/// zero bytes under a `Content-Type` that promises a document is worse than
/// sending no body at all. [`BodyType::Binary`] lands here too — the tab shows
/// it disabled, and this is the matching refusal to invent bytes for it.
pub fn encode(body: &BodyDraft) -> Option<EncodedBody> {
    match body.kind {
        BodyType::None | BodyType::Binary => None,

        BodyType::Json | BodyType::Text | BodyType::Xml | BodyType::Html => {
            // Only entirely blank text counts as "no body": a document whose
            // meaning is its whitespace is still a document.
            if body.text.trim().is_empty() {
                return None;
            }
            Some(EncodedBody {
                bytes: body.text.clone().into_bytes(),
                content_type: body.kind.content_type().map(str::to_string),
            })
        }

        BodyType::UrlEncoded => {
            let pairs = effective_pairs(&body.fields);
            if pairs.is_empty() {
                return None;
            }
            Some(EncodedBody {
                bytes: urlencoded_body(&pairs).into_bytes(),
                content_type: body.kind.content_type().map(str::to_string),
            })
        }

        BodyType::FormData => {
            let pairs = effective_pairs(&body.fields);
            if pairs.is_empty() {
                return None;
            }
            let boundary = next_boundary();
            Some(EncodedBody {
                bytes: multipart_body(&pairs, &boundary),
                content_type: Some(format!("multipart/form-data; boundary={boundary}")),
            })
        }
    }
}

/// `a=1&b=two+words`, the WHATWG form serialization.
fn urlencoded_body(pairs: &[(String, String)]) -> String {
    pairs
        .iter()
        .map(|(key, value)| format!("{}={}", form_escape(key), form_escape(value)))
        .collect::<Vec<_>>()
        .join("&")
}

fn form_escape(text: &str) -> String {
    utf8_percent_encode(text, FORM_COMPONENT)
        .to_string()
        .replace(' ', "+")
}

/// An RFC 7578 multipart document with one text part per row.
///
/// Every part is text: file parts need the Binary body type, which this phase
/// does not build. `\r\n` throughout, because multipart is one of the few
/// places where the line ending is part of the grammar rather than a habit.
fn multipart_body(pairs: &[(String, String)], boundary: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for (key, value) in pairs {
        out.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        out.extend_from_slice(
            format!(
                "Content-Disposition: form-data; name=\"{}\"\r\n\r\n",
                escape_part_name(key)
            )
            .as_bytes(),
        );
        out.extend_from_slice(value.as_bytes());
        out.extend_from_slice(b"\r\n");
    }
    out.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    out
}

/// Makes a field name safe to sit inside the quoted `name="…"` parameter.
///
/// RFC 7578 §5.1 recommends percent-encoding rather than backslash escapes,
/// because receivers disagree about the latter. Only the three characters that
/// could end the quoted string or the header line are touched, so ordinary
/// names — including non-ASCII ones — pass through readable.
fn escape_part_name(name: &str) -> String {
    name.replace('%', "%25")
        .replace('"', "%22")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
}

/// A boundary that cannot collide with another request's.
///
/// The clock supplies uniqueness across runs and the counter across a single
/// millisecond; a clock that refuses to answer degrades to the counter alone
/// rather than panicking. The `dodo` infix makes a stray boundary in a server
/// log traceable back here.
fn next_boundary() -> String {
    let sequence = BOUNDARY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos());
    format!("----dodo-boundary-{nanos:x}-{sequence:x}")
}

#[cfg(test)]
mod tests {
    use super::{encode, escape_part_name, multipart_body, next_boundary, urlencoded_body};
    use crate::api_explorer::models::body::{BodyDraft, BodyType};
    use crate::api_explorer::models::key_value::KeyValue;

    fn field(enabled: bool, key: &str, value: &str) -> KeyValue {
        KeyValue {
            enabled,
            key: key.into(),
            value: value.into(),
        }
    }

    fn text_body(kind: BodyType, text: &str) -> BodyDraft {
        BodyDraft {
            kind,
            text: text.into(),
            fields: Vec::new(),
        }
    }

    fn form_body(kind: BodyType, fields: Vec<KeyValue>) -> BodyDraft {
        BodyDraft {
            kind,
            text: String::new(),
            fields,
        }
    }

    #[test]
    fn no_body_kinds_encode_to_nothing() {
        assert!(encode(&text_body(BodyType::None, "ignored")).is_none());
        assert!(encode(&text_body(BodyType::Binary, "ignored")).is_none());
    }

    #[test]
    fn a_json_body_keeps_its_bytes_and_declares_json() {
        let encoded = encode(&text_body(BodyType::Json, r#"{"a":1}"#)).expect("has a body");
        assert_eq!(encoded.bytes, br#"{"a":1}"#);
        assert_eq!(encoded.content_type.as_deref(), Some("application/json"));
    }

    #[test]
    fn a_body_is_sent_verbatim_and_not_reformatted() {
        // Formatting is an explicit action in the Body tab, never a side
        // effect of sending: a server that cares about byte-for-byte payloads
        // must get what is on screen.
        let ugly = "{\n  \"a\" :   1 }";
        let encoded = encode(&text_body(BodyType::Json, ugly)).expect("has a body");
        assert_eq!(encoded.bytes, ugly.as_bytes());
    }

    #[test]
    fn a_blank_text_body_sends_nothing_rather_than_an_empty_document() {
        assert!(encode(&text_body(BodyType::Json, "   \n ")).is_none());
        assert!(encode(&text_body(BodyType::Text, "")).is_none());
    }

    #[test]
    fn whitespace_that_is_the_document_survives() {
        let encoded = encode(&text_body(BodyType::Text, " x ")).expect("has a body");
        assert_eq!(encoded.bytes, b" x ");
    }

    #[test]
    fn every_text_kind_declares_its_media_type() {
        for (kind, expected) in [
            (BodyType::Json, "application/json"),
            (BodyType::Text, "text/plain"),
            (BodyType::Xml, "application/xml"),
            (BodyType::Html, "text/html"),
        ] {
            let encoded = encode(&text_body(kind, "x")).expect("has a body");
            assert_eq!(encoded.content_type.as_deref(), Some(expected));
        }
    }

    #[test]
    fn urlencoded_escapes_the_way_a_form_does() {
        let pairs = [
            ("q".to_string(), "a b&c".to_string()),
            ("plus".to_string(), "1+1".to_string()),
            ("kept".to_string(), "a*b-c.d_e".to_string()),
        ];
        assert_eq!(
            urlencoded_body(&pairs),
            "q=a+b%26c&plus=1%2B1&kept=a*b-c.d_e"
        );
    }

    #[test]
    fn urlencoded_uses_only_the_rows_that_count() {
        let encoded = encode(&form_body(
            BodyType::UrlEncoded,
            vec![
                field(true, "a", "1"),
                field(false, "skipped", "yes"),
                field(true, "  ", "no key"),
                field(true, "b", "2"),
            ],
        ))
        .expect("has a body");
        assert_eq!(encoded.bytes, b"a=1&b=2");
        assert_eq!(
            encoded.content_type.as_deref(),
            Some("application/x-www-form-urlencoded")
        );
    }

    #[test]
    fn a_form_with_no_usable_rows_sends_nothing() {
        assert!(
            encode(&form_body(
                BodyType::UrlEncoded,
                vec![field(false, "a", "1")]
            ))
            .is_none()
        );
        assert!(encode(&form_body(BodyType::FormData, Vec::new())).is_none());
    }

    #[test]
    fn multipart_lays_out_one_part_per_row() {
        let pairs = [
            ("name".to_string(), "Ada".to_string()),
            ("note".to_string(), "two\nlines".to_string()),
        ];
        let document = String::from_utf8(multipart_body(&pairs, "BOUND")).expect("utf-8");
        assert_eq!(
            document,
            "--BOUND\r\n\
             Content-Disposition: form-data; name=\"name\"\r\n\r\n\
             Ada\r\n\
             --BOUND\r\n\
             Content-Disposition: form-data; name=\"note\"\r\n\r\n\
             two\nlines\r\n\
             --BOUND--\r\n"
        );
    }

    #[test]
    fn a_part_name_cannot_break_out_of_its_quotes() {
        assert_eq!(escape_part_name("a\"b"), "a%22b");
        assert_eq!(escape_part_name("a\r\nb"), "a%0D%0Ab");
        // The escape character itself is escaped first, so the mapping is
        // reversible rather than ambiguous.
        assert_eq!(escape_part_name("100%"), "100%25");
        assert_eq!(escape_part_name("xin chào"), "xin chào");
    }

    #[test]
    fn multipart_declares_the_boundary_it_used() {
        let encoded = encode(&form_body(BodyType::FormData, vec![field(true, "a", "1")]))
            .expect("has a body");
        let content_type = encoded.content_type.expect("multipart declares one");
        let boundary = content_type
            .split("boundary=")
            .nth(1)
            .expect("the media type carries a boundary");
        let document = String::from_utf8(encoded.bytes).expect("utf-8");
        assert!(
            document.starts_with(&format!("--{boundary}\r\n")),
            "the document does not open with the boundary it declared: {document:?}"
        );
        assert!(document.ends_with(&format!("--{boundary}--\r\n")));
    }

    #[test]
    fn boundaries_do_not_repeat() {
        let first = next_boundary();
        let second = next_boundary();
        assert_ne!(first, second);
        // A boundary may only contain a conservative ASCII set, or receivers
        // reject the document.
        assert!(
            first
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
            "{first} is not a legal multipart boundary"
        );
    }
}
