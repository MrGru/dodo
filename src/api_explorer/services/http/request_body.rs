//! Turning the Body tab into bytes on the wire.
//!
//! Kept beside the transport rather than in `models` because this is where a
//! body stops being "what the user typed" and becomes an encoding decision —
//! percent-escaping, a multipart layout, a media type.
//!
//! Nothing here names `reqwest`. The one outside-world dependency is
//! [`upload`], which reads the files a multipart or binary body sends; that is
//! also why [`encode`] returns a `Result` and why the whole path runs on the
//! background executor (see `upload`'s module doc for where the boundary is).
//! Everything else is arithmetic over strings and is unit tested.

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};

use crate::api_explorer::models::body::{BodyDraft, BodyType};
use crate::api_explorer::models::key_value::{FieldKind, KeyValue, effective_pairs};
use crate::api_explorer::services::TransportError;
use crate::api_explorer::services::http::upload;

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

/// One piece of a multipart document, already resolved: a file part carries the
/// bytes that were read, not the path they came from.
#[derive(Debug, PartialEq, Eq)]
enum Part {
    Text {
        name: String,
        value: String,
    },
    File {
        name: String,
        file_name: String,
        media_type: &'static str,
        bytes: Vec<u8>,
    },
}

/// Encodes what the Body tab holds, or `Ok(None)` when there is nothing to
/// send.
///
/// "Nothing to send" covers more than [`BodyType::None`]: an empty JSON editor,
/// a form with no filled-in rows, or Binary with no file chosen also produce no
/// body, because sending zero bytes under a `Content-Type` that promises a
/// document is worse than sending no body at all.
///
/// `Err` is reserved for a body the user *asked* for and this cannot build —
/// a file that has moved, or one past [`upload::MAX_UPLOAD_BYTES`]. A saved
/// request pointing at a file that no longer exists fails here, loudly, rather
/// than sending an empty part that a server would happily accept.
pub fn encode(body: &BodyDraft) -> Result<Option<EncodedBody>, TransportError> {
    match body.kind {
        BodyType::None => Ok(None),

        BodyType::Json | BodyType::Text | BodyType::Xml | BodyType::Html => {
            // Only entirely blank text counts as "no body": a document whose
            // meaning is its whitespace is still a document.
            if body.text.trim().is_empty() {
                return Ok(None);
            }
            Ok(Some(EncodedBody {
                bytes: body.text.clone().into_bytes(),
                content_type: body.kind.content_type().map(str::to_string),
            }))
        }

        BodyType::UrlEncoded => {
            let pairs = effective_pairs(&body.fields);
            if pairs.is_empty() {
                return Ok(None);
            }
            Ok(Some(EncodedBody {
                bytes: urlencoded_body(&pairs).into_bytes(),
                content_type: body.kind.content_type().map(str::to_string),
            }))
        }

        BodyType::FormData => {
            let parts = multipart_parts(&body.fields)?;
            if parts.is_empty() {
                return Ok(None);
            }
            let boundary = next_boundary();
            Ok(Some(EncodedBody {
                bytes: multipart_body(&parts, &boundary),
                content_type: Some(format!("multipart/form-data; boundary={boundary}")),
            }))
        }

        BodyType::Binary => {
            let path = body.file_path.trim();
            if path.is_empty() {
                return Ok(None);
            }
            let path = Path::new(path);
            Ok(Some(EncodedBody {
                bytes: upload::read_file(path)?,
                content_type: Some(upload::media_type_of(path).to_string()),
            }))
        }
    }
}

/// Resolves the form rows into parts, reading each file row's file.
///
/// A file row with no file chosen is **skipped**, not an error: the row is
/// half-written rather than wrong, and the Body tab marks it on screen (see
/// [`KeyValue::is_incomplete_file`]). A row that names a file which cannot be
/// read *is* an error — that is a request that would silently lose a payload.
fn multipart_parts(rows: &[KeyValue]) -> Result<Vec<Part>, TransportError> {
    let mut parts = Vec::new();
    for row in rows.iter().filter(|row| row.is_effective()) {
        let name = row.key.trim().to_string();
        match row.kind {
            FieldKind::Text => parts.push(Part::Text {
                name,
                value: row.value.trim().to_string(),
            }),
            FieldKind::File => {
                let path = row.file_path.trim();
                if path.is_empty() {
                    continue;
                }
                let path = Path::new(path);
                parts.push(Part::File {
                    name,
                    file_name: upload::file_name_of(path),
                    media_type: upload::media_type_of(path),
                    bytes: upload::read_file(path)?,
                });
            }
        }
    }
    Ok(parts)
}

/// `a=1&b=two+words`, the WHATWG form serialization.
fn urlencoded_body(pairs: &[(String, String)]) -> String {
    pairs
        .iter()
        .map(|(key, value)| format!("{}={}", form_escape(key), form_escape(value)))
        .collect::<Vec<_>>()
        .join("&")
}

/// One field of a `x-www-form-urlencoded` document.
///
/// `pub` so that `services::codegen` escapes a query parameter with the *same*
/// function the wire uses rather than a second one that agrees today. The
/// generated snippet has to spell the URL the way dodo would send it, and `+`
/// versus `%20` is exactly the kind of near-agreement that goes unnoticed.
pub fn form_escape(text: &str) -> String {
    utf8_percent_encode(text, FORM_COMPONENT)
        .to_string()
        .replace(' ', "+")
}

/// An RFC 7578 multipart document, one part per row.
///
/// A text part carries only its name; a file part adds `filename` and its own
/// `Content-Type`, which is what makes a server treat it as an upload rather
/// than as a string. `\r\n` throughout, because multipart is one of the few
/// places where the line ending is part of the grammar rather than a habit.
fn multipart_body(parts: &[Part], boundary: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for part in parts {
        out.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        match part {
            Part::Text { name, value } => {
                out.extend_from_slice(
                    format!(
                        "Content-Disposition: form-data; name=\"{}\"\r\n\r\n",
                        escape_part_name(name)
                    )
                    .as_bytes(),
                );
                out.extend_from_slice(value.as_bytes());
            }
            Part::File {
                name,
                file_name,
                media_type,
                bytes,
            } => {
                out.extend_from_slice(
                    format!(
                        "Content-Disposition: form-data; name=\"{}\"; filename=\"{}\"\r\n\
                         Content-Type: {media_type}\r\n\r\n",
                        escape_part_name(name),
                        escape_part_name(file_name)
                    )
                    .as_bytes(),
                );
                out.extend_from_slice(bytes);
            }
        }
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
    use std::path::PathBuf;

    use super::{
        Part, encode, escape_part_name, multipart_body, multipart_parts, next_boundary,
        urlencoded_body,
    };
    use crate::api_explorer::models::body::{BodyDraft, BodyType};
    use crate::api_explorer::models::key_value::KeyValue;
    use crate::api_explorer::services::TransportError;

    /// A scratch file that removes itself, so the multipart tests exercise the
    /// real read path rather than a stub.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str, bytes: &[u8]) -> Self {
            let path = std::env::temp_dir().join(format!("dodo-body-test-{name}"));
            std::fs::write(&path, bytes).expect("scratch file is writable");
            Self(path)
        }

        fn path(&self) -> String {
            self.0.display().to_string()
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    fn field(enabled: bool, key: &str, value: &str) -> KeyValue {
        KeyValue {
            enabled,
            ..KeyValue::text(key, value)
        }
    }

    fn text_body(kind: BodyType, text: &str) -> BodyDraft {
        BodyDraft {
            kind,
            text: text.into(),
            ..BodyDraft::default()
        }
    }

    fn form_body(kind: BodyType, fields: Vec<KeyValue>) -> BodyDraft {
        BodyDraft {
            kind,
            fields,
            ..BodyDraft::default()
        }
    }

    fn encoded(body: &BodyDraft) -> super::EncodedBody {
        encode(body).expect("encodes").expect("has a body")
    }

    #[test]
    fn the_no_body_kind_encodes_to_nothing() {
        assert!(
            encode(&text_body(BodyType::None, "ignored"))
                .expect("encodes")
                .is_none()
        );
    }

    #[test]
    fn a_json_body_keeps_its_bytes_and_declares_json() {
        let encoded = encoded(&text_body(BodyType::Json, r#"{"a":1}"#));
        assert_eq!(encoded.bytes, br#"{"a":1}"#);
        assert_eq!(encoded.content_type.as_deref(), Some("application/json"));
    }

    #[test]
    fn a_body_is_sent_verbatim_and_not_reformatted() {
        // Formatting is an explicit action in the Body tab, never a side
        // effect of sending: a server that cares about byte-for-byte payloads
        // must get what is on screen.
        let ugly = "{\n  \"a\" :   1 }";
        assert_eq!(
            encoded(&text_body(BodyType::Json, ugly)).bytes,
            ugly.as_bytes()
        );
    }

    #[test]
    fn a_blank_text_body_sends_nothing_rather_than_an_empty_document() {
        assert!(
            encode(&text_body(BodyType::Json, "   \n "))
                .expect("encodes")
                .is_none()
        );
        assert!(
            encode(&text_body(BodyType::Text, ""))
                .expect("encodes")
                .is_none()
        );
    }

    #[test]
    fn whitespace_that_is_the_document_survives() {
        assert_eq!(encoded(&text_body(BodyType::Text, " x ")).bytes, b" x ");
    }

    #[test]
    fn every_text_kind_declares_its_media_type() {
        for (kind, expected) in [
            (BodyType::Json, "application/json"),
            (BodyType::Text, "text/plain"),
            (BodyType::Xml, "application/xml"),
            (BodyType::Html, "text/html"),
        ] {
            assert_eq!(
                encoded(&text_body(kind, "x")).content_type.as_deref(),
                Some(expected)
            );
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
        let encoded = encoded(&form_body(
            BodyType::UrlEncoded,
            vec![
                field(true, "a", "1"),
                field(false, "skipped", "yes"),
                field(true, "  ", "no key"),
                field(true, "b", "2"),
            ],
        ));
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
            .expect("encodes")
            .is_none()
        );
        assert!(
            encode(&form_body(BodyType::FormData, Vec::new()))
                .expect("encodes")
                .is_none()
        );
    }

    #[test]
    fn multipart_lays_out_one_part_per_row() {
        let parts = [
            Part::Text {
                name: "name".into(),
                value: "Ada".into(),
            },
            Part::Text {
                name: "note".into(),
                value: "two\nlines".into(),
            },
        ];
        let document = String::from_utf8(multipart_body(&parts, "BOUND")).expect("utf-8");
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
    fn a_file_part_carries_its_filename_and_sniffed_type() {
        let scratch = Scratch::new("avatar.png", b"PNGbytes");
        let parts = multipart_parts(&[
            KeyValue::text("name", "Ada"),
            KeyValue::file("avatar", scratch.path()),
        ])
        .expect("both rows resolve");

        let document = String::from_utf8_lossy(&multipart_body(&parts, "BOUND")).to_string();
        assert!(
            document.contains(
                "Content-Disposition: form-data; name=\"avatar\"; \
                 filename=\"dodo-body-test-avatar.png\"\r\n\
                 Content-Type: image/png\r\n\r\nPNGbytes\r\n"
            ),
            "the file part is not laid out as expected: {document}"
        );
        // The text row beside it is still a plain text part.
        assert!(document.contains("name=\"name\"\r\n\r\nAda\r\n"));
    }

    #[test]
    fn a_file_part_sends_bytes_that_are_not_text() {
        let scratch = Scratch::new("raw.bin", &[0u8, 159, 146, 150, 255]);
        let parts = multipart_parts(&[KeyValue::file("blob", scratch.path())]).expect("resolves");
        let document = multipart_body(&parts, "BOUND");
        assert!(
            document
                .windows(5)
                .any(|window| window == [0u8, 159, 146, 150, 255]),
            "the raw bytes did not survive the multipart layout"
        );
    }

    #[test]
    fn an_unrecognised_extension_falls_back_to_octet_stream() {
        let scratch = Scratch::new("thing.qqq", b"x");
        let parts = multipart_parts(&[KeyValue::file("blob", scratch.path())]).expect("resolves");
        assert!(matches!(
            &parts[0],
            Part::File { media_type, .. } if *media_type == "application/octet-stream"
        ));
    }

    #[test]
    fn a_file_row_with_no_file_is_skipped_rather_than_sent_empty() {
        let parts = multipart_parts(&[
            KeyValue::text("name", "Ada"),
            KeyValue::file("avatar", "   "),
        ])
        .expect("an unfinished row is not an error");
        assert_eq!(parts.len(), 1, "the incomplete row reached the wire");
        assert!(matches!(&parts[0], Part::Text { name, .. } if name == "name"));
    }

    #[test]
    fn a_form_of_only_incomplete_file_rows_sends_no_body_at_all() {
        assert!(
            encode(&form_body(
                BodyType::FormData,
                vec![KeyValue::file("avatar", "")]
            ))
            .expect("encodes")
            .is_none()
        );
    }

    #[test]
    fn a_file_that_has_moved_fails_by_name_instead_of_sending_an_empty_part() {
        let missing = std::env::temp_dir().join("dodo-body-test-gone.png");
        let error = encode(&form_body(
            BodyType::FormData,
            vec![KeyValue::file("avatar", missing.display().to_string())],
        ))
        .expect_err("a named file that is not there cannot be sent");
        match error {
            TransportError::FileUnreadable { path, .. } => {
                assert_eq!(path, missing.display().to_string())
            }
            other => panic!("expected FileUnreadable, got {other:?}"),
        }
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
        let encoded = encoded(&form_body(BodyType::FormData, vec![field(true, "a", "1")]));
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

    #[test]
    fn a_binary_body_is_the_files_bytes_typed_from_its_extension() {
        let scratch = Scratch::new("payload.pdf", &[37u8, 80, 68, 70, 0, 1, 2]);
        let encoded = encoded(&BodyDraft {
            kind: BodyType::Binary,
            file_path: scratch.path(),
            ..BodyDraft::default()
        });
        assert_eq!(encoded.bytes, [37u8, 80, 68, 70, 0, 1, 2]);
        assert_eq!(encoded.content_type.as_deref(), Some("application/pdf"));
    }

    #[test]
    fn a_binary_body_with_no_file_chosen_sends_nothing() {
        assert!(
            encode(&BodyDraft {
                kind: BodyType::Binary,
                file_path: "  ".into(),
                ..BodyDraft::default()
            })
            .expect("encodes")
            .is_none()
        );
    }

    #[test]
    fn a_binary_body_whose_file_is_gone_is_an_error() {
        let missing = std::env::temp_dir().join("dodo-body-test-binary-gone.bin");
        assert!(matches!(
            encode(&BodyDraft {
                kind: BodyType::Binary,
                file_path: missing.display().to_string(),
                ..BodyDraft::default()
            }),
            Err(TransportError::FileUnreadable { .. })
        ));
    }

    #[test]
    fn the_editors_of_the_other_kinds_are_ignored_by_the_one_in_use() {
        // Switching kind keeps every surface's contents; only the one the kind
        // names may reach the wire.
        let scratch = Scratch::new("kept.txt", b"file bytes");
        let draft = BodyDraft {
            kind: BodyType::Json,
            text: r#"{"a":1}"#.into(),
            fields: vec![KeyValue::text("ignored", "row")],
            file_path: scratch.path(),
        };
        assert_eq!(encoded(&draft).bytes, br#"{"a":1}"#);
    }
}
