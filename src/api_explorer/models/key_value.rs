//! The key/value pairs behind the Params and Headers tables.
//!
//! Deliberately a `Vec` of pairs rather than a map: HTTP allows the same header
//! name more than once (`Set-Cookie`, `Accept`), and so does a query string, so
//! collapsing to a map would silently drop the user's second row.

/// One row of a key/value table, as the request is about to be sent.
///
/// This is the plain-data form. The editable form — which owns the two text
/// inputs — lives in `state::request`, so that this stays testable without a
/// `Window`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyValue {
    pub enabled: bool,
    pub key: String,
    pub value: String,
}

impl KeyValue {
    /// A row that contributes nothing to the request: switched off, or with a
    /// blank key.
    ///
    /// A blank key is treated as "not filled in yet" rather than as an error,
    /// because the table always shows one empty trailing row to type into.
    pub fn is_effective(&self) -> bool {
        self.enabled && !self.key.trim().is_empty()
    }
}

/// The rows that will actually be sent, in table order, with keys and values
/// trimmed of the whitespace that pasting tends to bring along.
pub fn effective_pairs(rows: &[KeyValue]) -> Vec<(String, String)> {
    rows.iter()
        .filter(|row| row.is_effective())
        .map(|row| (row.key.trim().to_string(), row.value.trim().to_string()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{KeyValue, effective_pairs};

    fn row(enabled: bool, key: &str, value: &str) -> KeyValue {
        KeyValue {
            enabled,
            key: key.into(),
            value: value.into(),
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
}
