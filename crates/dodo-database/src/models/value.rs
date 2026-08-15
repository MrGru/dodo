//! A result cell, in display terms — deliberately not a driver's own type.
//!
//! This is the seam that keeps `models/` free of every driver crate and every
//! result-grid rule unit-testable with no server. A driver converts whatever
//! the wire gave it into one of these; nothing above `services/` ever sees a
//! `postgres::Row` or a `rusqlite::ValueRef`.
//!
//! # Why the enum is small
//!
//! It is not trying to model SQL's type system — PostgreSQL alone has hundreds
//! of types and user-defined ones on top. It models what a *grid cell* can be:
//! absent, a number, a truth value, text, opaque bytes, or something the detail
//! view should open as JSON. The server's own type name travels separately, in
//! [`ColumnMeta::type_name`], and is what the column header shows underneath the
//! column's name. So an unknown type is rendered as its text form and labelled
//! honestly, rather than being lost.

use super::page::PageBudget;

/// One cell.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// SQL `NULL`. Distinct from an empty string, and the grid draws it
    /// differently — confusing the two is a classic way to misread a result.
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Text(String),
    /// Bytes with no text meaning. Summarised in the grid rather than dumped.
    Bytes(Vec<u8>),
    /// Text that is already valid JSON, so a detail view can open it in the
    /// library's highlighted JSON code editor.
    Json(String),
    /// A value larger than [`PageBudget::max_cell_bytes`]: the prefix that was
    /// kept, and how many bytes the whole value was.
    ///
    /// This exists so truncation is **visible in the data** rather than being a
    /// silently shortened string. A grid that shows `abc…` cannot tell the user
    /// whether the row really ended there.
    Truncated {
        prefix: String,
        full_bytes: usize,
    },
}

impl Value {
    /// Text for a grid cell. `NULL` renders as an empty string here and is
    /// drawn by the grid itself, so that the word "NULL" is never mistaken for
    /// a value that happens to spell it.
    pub fn display(&self) -> String {
        match self {
            Value::Null => String::new(),
            Value::Bool(value) => value.to_string(),
            Value::Int(value) => value.to_string(),
            Value::Float(value) => format_float(*value),
            Value::Text(text) | Value::Json(text) => text.clone(),
            Value::Bytes(bytes) => format!("[{} bytes]", bytes.len()),
            Value::Truncated { prefix, .. } => format!("{prefix}…"),
        }
    }

    /// Roughly what this cell costs in memory. Used by the page budget, so it
    /// counts the payload rather than trying to be exact about the enum's own
    /// footprint — the aim is a bound that holds, not an allocator's answer.
    pub fn byte_size(&self) -> usize {
        match self {
            Value::Null | Value::Bool(_) | Value::Int(_) | Value::Float(_) => {
                std::mem::size_of::<Value>()
            }
            Value::Text(text) | Value::Json(text) => text.len() + std::mem::size_of::<Value>(),
            Value::Bytes(bytes) => bytes.len() + std::mem::size_of::<Value>(),
            Value::Truncated { prefix, .. } => prefix.len() + std::mem::size_of::<Value>(),
        }
    }

    /// This value, cut down to `max_bytes` if it is over.
    ///
    /// Cutting happens on a **character boundary**, so a multi-byte character
    /// is never split into invalid UTF-8 — the reason this is a method with
    /// tests rather than a `truncate` call at the call site.
    pub fn capped(self, max_bytes: usize) -> Self {
        let full_bytes = match &self {
            Value::Text(text) | Value::Json(text) => text.len(),
            Value::Bytes(bytes) => bytes.len(),
            // A number, a bool, a null or an already-truncated value is either
            // small or already accounted for.
            _ => return self,
        };
        if full_bytes <= max_bytes {
            return self;
        }

        let prefix = match &self {
            Value::Text(text) | Value::Json(text) => floor_char_boundary(text, max_bytes).into(),
            Value::Bytes(bytes) => format!("[{} bytes]", bytes.len()),
            _ => unreachable!("guarded by the match above"),
        };
        Value::Truncated { prefix, full_bytes }
    }
}

/// The longest prefix of `text` that is at most `max_bytes` long and ends on a
/// character boundary.
fn floor_char_boundary(text: &str, max_bytes: usize) -> &str {
    if max_bytes >= text.len() {
        return text;
    }
    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// A float, rendered the way a database client should: no exponent for
/// ordinary magnitudes, no trailing `.0` invented for a value that has one, and
/// the full precision Rust round-trips.
fn format_float(value: f64) -> String {
    if value.is_nan() {
        return "NaN".into();
    }
    if value.is_infinite() {
        return if value.is_sign_negative() {
            "-Infinity".into()
        } else {
            "Infinity".into()
        };
    }
    value.to_string()
}

/// Where a result column came from.
///
/// `None` on a [`ColumnMeta`] means an expression, a join artefact, or a driver
/// that does not report it. Nothing in this round reads it; it is filled in
/// because the wire hands it over for free at describe time and re-running
/// every query to get it later would be the expensive way to learn it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ColumnOrigin {
    pub schema: Option<String>,
    pub table: String,
    pub column: String,
}

/// One result column.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ColumnMeta {
    pub name: String,
    /// The server's own type name — `int4`, `VARCHAR(255)`, `TEXT`. Shown under
    /// the column name in the grid's header. Data, never translated.
    pub type_name: String,
    pub origin: Option<ColumnOrigin>,
}

impl ColumnMeta {
    pub fn new(name: impl Into<String>, type_name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            type_name: type_name.into(),
            origin: None,
        }
    }

    pub fn with_origin(mut self, origin: ColumnOrigin) -> Self {
        self.origin = Some(origin);
        self
    }
}

/// One result row, in column order.
pub type Row = Vec<Value>;

/// The total payload of a row, for the page budget.
pub fn row_bytes(row: &Row) -> usize {
    row.iter().map(Value::byte_size).sum()
}

/// Every value in `row`, capped at the budget's per-cell limit.
pub fn cap_row(row: Row, budget: &PageBudget) -> Row {
    row.into_iter()
        .map(|value| value.capped(budget.max_cell_bytes))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{ColumnMeta, ColumnOrigin, Value, cap_row, format_float, row_bytes};
    use crate::models::page::PageBudget;

    #[test]
    fn null_renders_as_nothing_so_the_grid_can_draw_it_itself() {
        assert_eq!(Value::Null.display(), "");
        assert_eq!(
            Value::Text("NULL".into()).display(),
            "NULL",
            "a string that spells NULL is not a NULL"
        );
    }

    #[test]
    fn scalars_render_the_way_a_database_client_should() {
        assert_eq!(Value::Bool(true).display(), "true");
        assert_eq!(Value::Int(-42).display(), "-42");
        assert_eq!(Value::Float(1.5).display(), "1.5");
        assert_eq!(Value::Float(3.0).display(), "3");
        assert_eq!(format_float(f64::NAN), "NaN");
        assert_eq!(format_float(f64::INFINITY), "Infinity");
        assert_eq!(format_float(f64::NEG_INFINITY), "-Infinity");
    }

    #[test]
    fn bytes_are_summarised_rather_than_dumped_into_a_cell() {
        assert_eq!(Value::Bytes(vec![0u8; 2048]).display(), "[2048 bytes]");
    }

    #[test]
    fn a_value_under_the_cap_is_returned_untouched() {
        let value = Value::Text("short".into());
        assert_eq!(value.clone().capped(64), value);
        assert_eq!(Value::Int(9).capped(0), Value::Int(9));
        assert_eq!(Value::Null.capped(0), Value::Null);
    }

    #[test]
    fn an_oversized_value_records_the_length_it_really_was() {
        let text: String = "x".repeat(100);
        match Value::Text(text).capped(10) {
            Value::Truncated { prefix, full_bytes } => {
                assert_eq!(prefix.len(), 10);
                assert_eq!(full_bytes, 100);
            }
            other => panic!("expected Truncated, got {other:?}"),
        }
    }

    /// The bug this method exists to prevent: `&text[..n]` panics mid-character.
    #[test]
    fn truncation_never_splits_a_multibyte_character() {
        // Each "é" is two bytes, so a cap of 5 lands inside the third one.
        let text = "ééé".to_string();
        assert_eq!(text.len(), 6);
        match Value::Text(text).capped(5) {
            Value::Truncated { prefix, full_bytes } => {
                assert_eq!(prefix, "éé", "cut back to a character boundary");
                assert_eq!(full_bytes, 6);
            }
            other => panic!("expected Truncated, got {other:?}"),
        }
    }

    #[test]
    fn a_truncated_cell_shows_that_it_was_cut() {
        let value = Value::Truncated {
            prefix: "abc".into(),
            full_bytes: 900,
        };
        assert_eq!(value.display(), "abc…");
    }

    #[test]
    fn capping_bytes_replaces_them_with_their_summary() {
        match Value::Bytes(vec![7u8; 500]).capped(10) {
            Value::Truncated { prefix, full_bytes } => {
                assert_eq!(prefix, "[500 bytes]");
                assert_eq!(full_bytes, 500);
            }
            other => panic!("expected Truncated, got {other:?}"),
        }
    }

    #[test]
    fn a_rows_size_grows_with_its_payload() {
        let small = vec![Value::Int(1), Value::Text("a".into())];
        let large = vec![Value::Int(1), Value::Text("a".repeat(1000))];
        assert!(row_bytes(&large) > row_bytes(&small) + 900);
    }

    #[test]
    fn capping_a_row_caps_every_cell_in_it() {
        let budget = PageBudget {
            max_cell_bytes: 4,
            ..PageBudget::default()
        };
        let row = cap_row(vec![Value::Text("abcdefgh".into()), Value::Int(3)], &budget);
        assert!(matches!(row[0], Value::Truncated { .. }));
        assert_eq!(row[1], Value::Int(3));
    }

    #[test]
    fn a_column_has_a_name_a_server_type_and_an_optional_origin() {
        let column = ColumnMeta::new("id", "int4");
        assert_eq!(column.origin, None);

        let owned = column.with_origin(ColumnOrigin {
            schema: Some("public".into()),
            table: "users".into(),
            column: "id".into(),
        });
        assert_eq!(
            owned.origin.as_ref().map(|o| o.table.as_str()),
            Some("users")
        );
    }
}
