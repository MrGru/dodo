//! The request body as plain data: which kind it is, and what the user typed.
//!
//! The *encoding* of a body — percent-escaping a form, laying out a multipart
//! document, choosing a `Content-Type` — belongs to the service layer and lives
//! in `services::http::request_body`. This module only says what a body is.

use crate::api_explorer::models::key_value::KeyValue;
use crate::i18n::Str;

/// The kinds of body the Body tab can build.
///
/// A closed enum for the same reason [`HttpMethod`] is one: every branch that
/// maps a kind to a grammar, a media type or a label is exhaustive, so adding a
/// kind cannot silently miss one.
///
/// [`HttpMethod`]: crate::api_explorer::models::method::HttpMethod
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
            BodyType::None => Str::BodyTypeNone,
            BodyType::Json => Str::BodyTypeJson,
            BodyType::Text => Str::BodyTypeText,
            BodyType::Xml => Str::BodyTypeXml,
            BodyType::Html => Str::BodyTypeHtml,
            BodyType::FormData => Str::BodyTypeFormData,
            BodyType::UrlEncoded => Str::BodyTypeUrlEncoded,
            BodyType::Binary => Str::BodyTypeBinary,
        }
    }

    /// The `Content-Type` this kind implies, used only when the user has not
    /// written one of their own in the Headers tab.
    ///
    /// [`BodyType::FormData`] is deliberately absent: multipart's media type
    /// carries the boundary, which does not exist until the body has been laid
    /// out, so the encoder returns it instead.
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

    /// Whether this phase can actually build the body.
    ///
    /// Binary needs a file picker and a streaming upload path, neither of which
    /// exists yet; the option is shown disabled with a tooltip rather than
    /// hidden, so the gap is visible instead of mysterious.
    pub fn is_available(self) -> bool {
        !matches!(self, BodyType::Binary)
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

    /// Whether "format document" can do anything with this kind. Only JSON has
    /// a pretty form this app can produce.
    pub fn is_formattable(self) -> bool {
        matches!(self, BodyType::Json)
    }
}

/// A snapshot of the Body tab, taken when Send is pressed.
///
/// Both editing surfaces are carried regardless of `kind`, because the tab
/// keeps what was typed when the kind is switched — swapping JSON for Raw and
/// back must not lose the document.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BodyDraft {
    pub kind: BodyType,
    /// What the code editor holds, for the text-shaped kinds.
    pub text: String,
    /// What the table holds, for the two form kinds.
    pub fields: Vec<KeyValue>,
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
    fn the_kinds_with_no_editor_are_the_ones_that_send_nothing() {
        for kind in BodyType::ALL {
            if !kind.is_text() && !kind.is_form() {
                assert!(
                    matches!(kind, BodyType::None | BodyType::Binary),
                    "{kind:?} has no editing surface but is not a no-body kind"
                );
            }
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
    fn only_binary_is_deferred() {
        for kind in BodyType::ALL {
            assert_eq!(
                kind.is_available(),
                kind != BodyType::Binary,
                "{kind:?} is not marked as this phase expects"
            );
        }
    }
}
