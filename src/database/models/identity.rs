//! Proof that a result row has a unique identity safe enough for generated writes.
//!
//! This module never reads SQL text. A driver supplies column origins from its
//! wire metadata and unique-key facts from its catalog; [`prove`] is the only
//! constructor of [`EditableSource`]. Statement generation accepts that type,
//! so an `UPDATE` or `DELETE` cannot be built from an unproved guess.

use std::collections::BTreeSet;

use crate::database::models::value::ColumnMeta;
use crate::i18n::{Str, database};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableRef {
    pub schema: Option<String>,
    pub table: String,
}

impl TableRef {
    pub fn display_name(&self) -> String {
        self.schema
            .as_ref()
            .map(|schema| format!("{schema}.{}", self.table))
            .unwrap_or_else(|| self.table.clone())
    }
}

/// One catalog-proven unique key, in key order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UniqueKey {
    pub columns: Vec<String>,
    pub primary: bool,
    /// A nullable unique index is not a row identity: SQL permits several rows
    /// whose indexed value is `NULL`.
    pub all_non_null: bool,
}

/// The catalog facts for the one table named by a result's origins.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdentityMetadata {
    pub table: TableRef,
    pub keys: Vec<UniqueKey>,
    /// Columns whose value the server supplies on INSERT (identity,
    /// auto-increment, generated expression, or a default).
    pub generated_columns: BTreeSet<String>,
}

impl IdentityMetadata {
    pub fn new(table: TableRef) -> Self {
        Self {
            table,
            keys: Vec::new(),
            generated_columns: BTreeSet::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReadOnlyReason {
    Unsupported,
    NoColumns,
    MissingOrigin(String),
    MultipleTables,
    DuplicateBaseColumn(String),
    DuplicateRows,
    NoUniqueIdentity(String),
    MissingIdentityColumns { table: String, columns: Vec<String> },
    Metadata(String),
}

impl ReadOnlyReason {
    pub fn message(&self) -> Str {
        match self {
            Self::Unsupported => database::Text::EditUnsupported.into(),
            Self::NoColumns => database::Text::EditNoColumns.into(),
            Self::MissingOrigin(column) => database::Text::EditMissingOrigin(column.clone()).into(),
            Self::MultipleTables => database::Text::EditMultipleTables.into(),
            Self::DuplicateBaseColumn(column) => {
                database::Text::EditDuplicateColumn(column.clone()).into()
            }
            Self::DuplicateRows => database::Text::EditDuplicateRows.into(),
            Self::NoUniqueIdentity(table) => {
                database::Text::EditNoUniqueIdentity(table.clone()).into()
            }
            Self::MissingIdentityColumns { table, columns } => {
                database::Text::EditMissingIdentityColumns {
                    table: table.clone(),
                    columns: columns.join(", "),
                }
                .into()
            }
            Self::Metadata(detail) => database::Text::EditMetadataFailed(detail.clone()).into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Editability {
    Editable(EditableSource),
    ReadOnly(ReadOnlyReason),
}

impl Editability {
    pub fn reason(&self) -> Option<&ReadOnlyReason> {
        match self {
            Self::Editable(_) => None,
            Self::ReadOnly(reason) => Some(reason),
        }
    }
}

/// A result column after its base table and identity have been proved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditableColumn {
    pub result_index: usize,
    pub name: String,
    pub identity: bool,
    pub generated: bool,
}

/// The token statement generation requires for `UPDATE` and `DELETE`.
///
/// Fields are private and this module exposes no constructor. The only way to
/// obtain one is [`prove`], from wire origins plus catalog identity metadata.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditableSource {
    table: TableRef,
    columns: Vec<EditableColumn>,
    identity_indices: Vec<usize>,
}

impl EditableSource {
    pub fn table(&self) -> &TableRef {
        &self.table
    }

    pub fn columns(&self) -> &[EditableColumn] {
        &self.columns
    }

    pub fn column(&self, result_index: usize) -> Option<&EditableColumn> {
        self.columns.get(result_index)
    }

    pub fn identity_indices(&self) -> &[usize] {
        &self.identity_indices
    }

    pub fn is_identity(&self, result_index: usize) -> bool {
        self.identity_indices.contains(&result_index)
    }
}

/// Proves editability without looking at the query text.
pub fn prove(columns: &[ColumnMeta], metadata: IdentityMetadata) -> Editability {
    if columns.is_empty() {
        return Editability::ReadOnly(ReadOnlyReason::NoColumns);
    }

    let Some(first) = columns[0].origin.as_ref() else {
        return Editability::ReadOnly(ReadOnlyReason::MissingOrigin(columns[0].name.clone()));
    };
    let table = TableRef {
        schema: first.schema.clone(),
        table: first.table.clone(),
    };
    let mut seen = BTreeSet::new();
    for column in columns {
        let Some(origin) = column.origin.as_ref() else {
            return Editability::ReadOnly(ReadOnlyReason::MissingOrigin(column.name.clone()));
        };
        if origin.schema != table.schema || origin.table != table.table {
            return Editability::ReadOnly(ReadOnlyReason::MultipleTables);
        }
        if !seen.insert(origin.column.clone()) {
            return Editability::ReadOnly(ReadOnlyReason::DuplicateBaseColumn(
                origin.column.clone(),
            ));
        }
    }

    // Metadata must describe exactly the table the wire named. Treat a
    // mismatch as unavailable metadata, never as a reason to trust the key.
    if metadata.table != table {
        return Editability::ReadOnly(ReadOnlyReason::Metadata(format!(
            "catalog identity for {} did not match result table {}",
            metadata.table.display_name(),
            table.display_name()
        )));
    }

    let key = metadata
        .keys
        .iter()
        .find(|key| key.primary)
        .or_else(|| metadata.keys.iter().find(|key| key.all_non_null));
    let Some(key) = key else {
        return Editability::ReadOnly(ReadOnlyReason::NoUniqueIdentity(table.display_name()));
    };

    let mut identity_indices = Vec::with_capacity(key.columns.len());
    let mut missing = Vec::new();
    for key_column in &key.columns {
        match columns.iter().position(|column| {
            column
                .origin
                .as_ref()
                .is_some_and(|origin| origin.column == *key_column)
        }) {
            Some(index) => identity_indices.push(index),
            None => missing.push(key_column.clone()),
        }
    }
    if !missing.is_empty() {
        return Editability::ReadOnly(ReadOnlyReason::MissingIdentityColumns {
            table: table.display_name(),
            columns: missing,
        });
    }

    let editable_columns = columns
        .iter()
        .enumerate()
        .map(|(result_index, column)| {
            let name = column
                .origin
                .as_ref()
                .expect("all origins were checked above")
                .column
                .clone();
            EditableColumn {
                result_index,
                identity: identity_indices.contains(&result_index),
                generated: metadata.generated_columns.contains(&name),
                name,
            }
        })
        .collect();

    Editability::Editable(EditableSource {
        table,
        columns: editable_columns,
        identity_indices,
    })
}

#[cfg(test)]
mod tests {
    use super::{Editability, IdentityMetadata, ReadOnlyReason, TableRef, UniqueKey, prove};
    use crate::database::models::value::{ColumnMeta, ColumnOrigin};

    fn column(table: &str, name: &str) -> ColumnMeta {
        ColumnMeta::new(name, "text").with_origin(ColumnOrigin {
            schema: Some("public".into()),
            table: table.into(),
            column: name.into(),
        })
    }

    fn metadata(keys: Vec<UniqueKey>) -> IdentityMetadata {
        IdentityMetadata {
            table: TableRef {
                schema: Some("public".into()),
                table: "users".into(),
            },
            keys,
            generated_columns: ["id".to_string()].into_iter().collect(),
        }
    }

    fn key(columns: &[&str], primary: bool, all_non_null: bool) -> UniqueKey {
        UniqueKey {
            columns: columns.iter().map(|column| (*column).into()).collect(),
            primary,
            all_non_null,
        }
    }

    #[test]
    fn primary_key_proves_identity_and_generated_columns() {
        let Editability::Editable(source) = prove(
            &[column("users", "id"), column("users", "name")],
            metadata(vec![key(&["id"], true, true)]),
        ) else {
            panic!("primary key should prove the row")
        };
        assert_eq!(source.identity_indices(), [0]);
        assert!(source.column(0).unwrap().generated);
        assert!(!source.column(1).unwrap().identity);
    }

    #[test]
    fn a_non_nullable_unique_index_is_the_fallback() {
        let Editability::Editable(source) = prove(
            &[column("users", "email"), column("users", "name")],
            metadata(vec![key(&["email"], false, true)]),
        ) else {
            panic!("non-null unique key should prove the row")
        };
        assert_eq!(source.identity_indices(), [0]);
    }

    #[test]
    fn joins_computed_columns_and_unions_are_rejected_by_origin() {
        assert!(matches!(
            prove(
                &[column("users", "id"), column("roles", "name")],
                metadata(vec![key(&["id"], true, true)])
            ),
            Editability::ReadOnly(ReadOnlyReason::MultipleTables)
        ));

        assert!(matches!(
            prove(
                &[column("users", "id"), ColumnMeta::new("count", "int8")],
                metadata(vec![key(&["id"], true, true)])
            ),
            Editability::ReadOnly(ReadOnlyReason::MissingOrigin(_))
        ));
    }

    #[test]
    fn missing_keys_and_nullable_unique_indexes_never_prove_identity() {
        assert!(matches!(
            prove(
                &[column("users", "name")],
                metadata(vec![key(&["id"], true, true)])
            ),
            Editability::ReadOnly(ReadOnlyReason::MissingIdentityColumns { .. })
        ));
        assert!(matches!(
            prove(
                &[column("users", "email")],
                metadata(vec![key(&["email"], false, false)])
            ),
            Editability::ReadOnly(ReadOnlyReason::NoUniqueIdentity(_))
        ));
    }

    #[test]
    fn duplicate_base_columns_are_read_only() {
        assert!(matches!(
            prove(
                &[column("users", "id"), column("users", "id")],
                metadata(vec![key(&["id"], true, true)])
            ),
            Editability::ReadOnly(ReadOnlyReason::DuplicateBaseColumn(_))
        ));
    }
}
