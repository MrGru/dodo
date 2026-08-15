//! The request body as plain data: which kind it is, and what the user typed.
//!
//! The *encoding* of a body — percent-escaping a form, laying out a multipart
//! document, choosing a `Content-Type` — belongs to the service layer and lives
//! in `services::http::request_body`. This module only says what a body is.

use crate::i18n::{Str, api_explorer};
use crate::models::key_value::KeyValue;

/// The kinds of body the Body tab can build.
///
/// A closed enum for the same reason [`HttpMethod`] is one: every branch that
/// maps a kind to a grammar, a media type or a label is exhaustive, so adding a
/// kind cannot silently miss one.
///
/// [`HttpMethod`]: crate::models::method::HttpMethod
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, serde::Serialize, serde::Deserialize)]
pub enum BodyType {
    #[default]
    None,
    Json,
    Text,
    Xml,
    Html,
    FormData,
    UrlEncoded,
    Binary,
}

impl BodyType {
    pub const ALL: [BodyType; 8] = [
        BodyType::None,
        BodyType::Json,
        BodyType::Text,
        BodyType::Xml,
        BodyType::Html,
        BodyType::FormData,
        BodyType::UrlEncoded,
        BodyType::Binary,
    ];

    pub fn label(self) -> Str {
        match self {
            BodyType::None => api_explorer::Text::BodyTypeNone.into(),
            BodyType::Json => api_explorer::Text::BodyTypeJson.into(),
            BodyType::Text => api_explorer::Text::BodyTypeText.into(),
            BodyType::Xml => api_explorer::Text::BodyTypeXml.into(),
            BodyType::Html => api_explorer::Text::BodyTypeHtml.into(),
            BodyType::FormData => api_explorer::Text::BodyTypeFormData.into(),
            BodyType::UrlEncoded => api_explorer::Text::BodyTypeUrlEncoded.into(),
            BodyType::Binary => api_explorer::Text::BodyTypeBinary.into(),
        }
    }

    /// The `Content-Type` this kind implies, used only when the user has not
    /// written one of their own in the Headers tab.
    ///
    /// Two kinds are deliberately absent. Multipart's media type carries the
    /// boundary, which does not exist until the body has been laid out; a
    /// binary body's is sniffed from the chosen file's extension. Both are
    /// returned by the encoder instead.
    pub fn content_type(self) -> Option<&'static str> {
        match self {
            BodyType::Json => Some("application/json"),
            BodyType::Text => Some("text/plain"),
            BodyType::Xml => Some("application/xml"),
            BodyType::Html => Some("text/html"),
            BodyType::UrlEncoded => Some("application/x-www-form-urlencoded"),
            BodyType::None | BodyType::FormData | BodyType::Binary => None,
        }
    }

    /// The code-editor grammar this kind is edited with, for the kinds that are
    /// edited as text at all.
    ///
    /// Only `json` and `html` are compiled into this build (see
    /// `gpui-component-recipes`); `text` renders uncoloured, which is the
    /// graceful default rather than a failure.
    pub fn editor_language(self) -> Option<&'static str> {
        match self {
            BodyType::Json => Some("json"),
            BodyType::Html => Some("html"),
            BodyType::Text | BodyType::Xml => Some("text"),
            BodyType::None | BodyType::FormData | BodyType::UrlEncoded | BodyType::Binary => None,
        }
    }

    /// Whether the Body tab shows the code editor for this kind.
    pub fn is_text(self) -> bool {
        self.editor_language().is_some()
    }

    /// Whether the Body tab shows the key/value table for this kind.
    pub fn is_form(self) -> bool {
        matches!(self, BodyType::FormData | BodyType::UrlEncoded)
    }

    /// Whether rows of this kind carry a [`FieldKind`] — only multipart can
    /// send a file part, so only multipart shows the TYPE column.
    ///
    /// [`FieldKind`]: crate::models::key_value::FieldKind
    pub fn is_typed_form(self) -> bool {
        matches!(self, BodyType::FormData)
    }

    /// Whether the Body tab shows the single-file picker for this kind.
    pub fn is_file(self) -> bool {
        matches!(self, BodyType::Binary)
    }

    /// Whether "format document" can do anything with this kind. Only JSON has
    /// a pretty form this app can produce.
    pub fn is_formattable(self) -> bool {
        matches!(self, BodyType::Json)
    }
}

/// A snapshot of the Body tab, taken when Send is pressed.
///
/// Every editing surface is carried regardless of `kind`, because the tab keeps
/// what was typed when the kind is switched — swapping JSON for Raw and back
/// must not lose the document, and swapping Binary out and back must not lose
/// the chosen file.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BodyDraft {
    pub kind: BodyType,
    /// What the code editor holds, for the text-shaped kinds.
    pub text: String,
    /// What the table holds, for the two form kinds.
    pub fields: Vec<KeyValue>,
    /// The file [`BodyType::Binary`] sends, as an absolute path. Empty means
    /// none chosen.
    ///
    /// `#[serde(default)]` for the same reason every other field here is: a
    /// collection written before Binary worked must still load.
    #[serde(default)]
    pub file_path: String,
}

#[cfg(test)]
mod tests {
    use super::BodyType;

    #[test]
    fn every_kind_is_listed_once() {
        for kind in BodyType::ALL {
            assert_eq!(
                BodyType::ALL.iter().filter(|other| **other == kind).count(),
                1,
                "{kind:?} appears more than once in BodyType::ALL"
            );
        }
    }

    #[test]
    fn a_kind_is_edited_as_text_or_as_a_table_but_never_both() {
        for kind in BodyType::ALL {
            assert!(
                !(kind.is_text() && kind.is_form()),
                "{kind:?} claims both editing surfaces"
            );
        }
    }

    #[test]
    fn every_kind_has_exactly_one_editing_surface_or_is_the_empty_one() {
        for kind in BodyType::ALL {
            let surfaces = [kind.is_text(), kind.is_form(), kind.is_file()]
                .into_iter()
                .filter(|shown| *shown)
                .count();
            let expected = usize::from(kind != BodyType::None);
            assert_eq!(
                surfaces, expected,
                "{kind:?} offers {surfaces} editing surfaces"
            );
        }
    }

    #[test]
    fn only_multipart_types_its_rows() {
        for kind in BodyType::ALL {
            assert_eq!(
                kind.is_typed_form(),
                kind == BodyType::FormData,
                "{kind:?} disagrees about whether its rows carry a type"
            );
        }
    }

    #[test]
    fn multipart_declares_no_static_media_type() {
        // Its boundary is only known after encoding, so the encoder owns it.
        assert_eq!(BodyType::FormData.content_type(), None);
        assert_eq!(
            BodyType::UrlEncoded.content_type(),
            Some("application/x-www-form-urlencoded")
        );
    }

    #[test]
    fn a_binary_body_declares_no_static_media_type_either() {
        // It is sniffed from the chosen file's extension, so the encoder owns
        // it the same way multipart owns its boundary.
        assert_eq!(BodyType::Binary.content_type(), None);
        assert!(BodyType::Binary.is_file());
    }

    #[test]
    fn a_draft_written_before_binary_worked_still_loads() {
        let draft: super::BodyDraft =
            serde_json::from_str(r#"{"kind":"Json","text":"{}","fields":[]}"#).expect("loads");
        assert_eq!(draft.kind, BodyType::Json);
        assert!(draft.file_path.is_empty());
    }
}
