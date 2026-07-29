//! The key/value pairs behind the Params, Headers and form-body tables.
//!
//! Deliberately a `Vec` of pairs rather than a map: HTTP allows the same header
//! name more than once (`Set-Cookie`, `Accept`), and so does a query string, so
//! collapsing to a map would silently drop the user's second row.

use crate::i18n::Str;

/// What a row's value *is*.
///
/// Only the multipart form body distinguishes the two — a query parameter and a
/// header are always text — but the discriminant lives on the row rather than
/// beside it so that a row survives being duplicated, reordered, saved and
/// reloaded with its type intact.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, serde::Serialize, serde::Deserialize)]
pub enum FieldKind {
    #[default]
    Text,
    File,
}

impl FieldKind {
    pub const ALL: [FieldKind; 2] = [FieldKind::Text, FieldKind::File];

    pub fn label(self) -> Str {
        match self {
            FieldKind::Text => Str::FieldKindText,
            FieldKind::File => Str::FieldKindFile,
        }
    }
}

/// One row of a key/value table, as the request is about to be sent.
///
/// This is the plain-data form. The editable form — which owns the text inputs —
/// lives in `state::request`, so that this stays testable without a `Window`.
///
/// `kind` and `file_path` are `#[serde(default)]` so a collection written before
/// form-data rows had a type still loads, as an all-`Text` table.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct KeyValue {
    pub enabled: bool,
    pub key: String,
    pub value: String,
    #[serde(default)]
    pub kind: FieldKind,
    /// The file a [`FieldKind::File`] row sends. Empty means "not chosen yet",
    /// which is what makes the row incomplete rather than empty.
    ///
    /// Stored as the absolute path the picker returned. A saved request whose
    /// file has since moved fails at send time with the path named, rather than
    /// sending an empty part — see `services::http::upload`.
    #[serde(default)]
    pub file_path: String,
}

impl KeyValue {
    /// An enabled text row. The shape almost every caller wants.
    pub fn text(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            enabled: true,
            key: key.into(),
            value: value.into(),
            kind: FieldKind::Text,
            file_path: String::new(),
        }
    }

    /// An enabled row that uploads `path`.
    pub fn file(key: impl Into<String>, path: impl Into<String>) -> Self {
        Self {
            enabled: true,
            key: key.into(),
            value: String::new(),
            kind: FieldKind::File,
            file_path: path.into(),
        }
    }

    /// A row that contributes nothing to the request: switched off, or with a
    /// blank key.
    ///
    /// A blank key is treated as "not filled in yet" rather than as an error,
    /// because the table always shows one empty trailing row to type into.
    pub fn is_effective(&self) -> bool {
        self.enabled && !self.key.trim().is_empty()
    }

    /// A file row that has been named but never given a file.
    ///
    /// Such a row is *not* sent — there is nothing to send — and the table
    /// marks it, because a part that silently vanishes between the editor and
    /// the wire is the one failure a user cannot debug.
    pub fn is_incomplete_file(&self) -> bool {
        self.kind == FieldKind::File && self.is_effective() && self.file_path.trim().is_empty()
    }
}

/// The rows that will actually be sent, in table order, with keys and values
/// trimmed of the whitespace that pasting tends to bring along.
///
/// Row *type* is ignored here on purpose: this feeds the query string, the
/// header list and the urlencoded body, none of which can carry a file. Only
/// multipart looks at [`KeyValue::kind`], through
/// [`crate::api_explorer::services::http::request_body`].
pub fn effective_pairs(rows: &[KeyValue]) -> Vec<(String, String)> {
    rows.iter()
        .filter(|row| row.is_effective())
        .map(|row| (row.key.trim().to_string(), row.value.trim().to_string()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{FieldKind, KeyValue, effective_pairs};

    fn row(enabled: bool, key: &str, value: &str) -> KeyValue {
        KeyValue {
            enabled,
            ..KeyValue::text(key, value)
        }
    }

    #[test]
    fn disabled_and_keyless_rows_are_skipped() {
        let rows = [
            row(true, "a", "1"),
            row(false, "b", "2"),
            row(true, "   ", "3"),
            row(true, "", ""),
        ];
        assert_eq!(effective_pairs(&rows), [("a".into(), "1".into())]);
    }

    #[test]
    fn duplicate_keys_are_preserved_in_order() {
        let rows = [
            row(true, "Accept", "text/html"),
            row(true, "Accept", "application/json"),
        ];
        assert_eq!(
            effective_pairs(&rows),
            [
                ("Accept".to_string(), "text/html".to_string()),
                ("Accept".to_string(), "application/json".to_string()),
            ]
        );
    }

    #[test]
    fn surrounding_whitespace_is_trimmed() {
        let rows = [row(true, "  key  ", "  value  ")];
        assert_eq!(effective_pairs(&rows), [("key".into(), "value".into())]);
    }

    #[test]
    fn an_empty_value_is_still_sent() {
        // `?flag=` is meaningful; only a missing *key* means "unfilled".
        let rows = [row(true, "flag", "")];
        assert_eq!(effective_pairs(&rows), [("flag".into(), String::new())]);
    }

    #[test]
    fn a_row_defaults_to_text() {
        assert_eq!(KeyValue::text("a", "1").kind, FieldKind::Text);
        assert_eq!(KeyValue::default().kind, FieldKind::Text);
    }

    #[test]
    fn a_file_row_without_a_file_is_incomplete_only_once_it_is_named() {
        assert!(KeyValue::file("avatar", "").is_incomplete_file());
        assert!(!KeyValue::file("avatar", "/tmp/a.png").is_incomplete_file());
        // Unnamed and switched-off rows are "not filled in yet", not broken.
        assert!(!KeyValue::file("", "").is_incomplete_file());
        assert!(
            !KeyValue {
                enabled: false,
                ..KeyValue::file("avatar", "")
            }
            .is_incomplete_file()
        );
        // Whitespace is not a path.
        assert!(KeyValue::file("avatar", "   ").is_incomplete_file());
    }

    #[test]
    fn a_row_written_before_types_existed_still_loads() {
        let row: KeyValue =
            serde_json::from_str(r#"{"enabled":true,"key":"a","value":"1"}"#).expect("loads");
        assert_eq!(row, KeyValue::text("a", "1"));
    }

    #[test]
    fn a_typed_row_round_trips_through_json() {
        let row = KeyValue::file("avatar", "/tmp/a.png");
        let json = serde_json::to_string(&row).expect("serializes");
        assert_eq!(
            serde_json::from_str::<KeyValue>(&json).expect("deserializes"),
            row
        );
    }
}
