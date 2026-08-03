//! The sole owner of generated mutation SQL.
//!
//! Callers provide pending operations over an [`EditableSource`], which can
//! only be constructed by the row-identity proof. This module quotes every
//! identifier, chooses dialect placeholders, and keeps every value as a bound
//! parameter. Views and state never assemble mutation SQL.

use std::collections::BTreeSet;

use crate::database::models::identity::{EditableSource, TableRef};
use crate::database::models::value::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dialect {
    PostgreSql,
    Sqlite,
    MySql,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Mutation {
    Update {
        identity: Vec<Value>,
        values: Vec<(usize, Value)>,
    },
    Delete {
        identity: Vec<Value>,
    },
    Insert {
        values: Vec<(usize, Value)>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct GeneratedStatement {
    pub sql: String,
    pub params: Vec<Value>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GeneratedBatch {
    pub dialect: Dialect,
    pub statements: Vec<GeneratedStatement>,
}

impl GeneratedBatch {
    pub fn expected_rows(&self) -> usize {
        self.statements.len()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StatementError {
    NoIdentity,
    WrongIdentityValueCount,
    InvalidColumn(usize),
    IdentityColumnChanged(String),
    TruncatedValue,
    NullIdentity,
    NoValues,
}

/// Generates the complete batch. No statement is returned if any operation is
/// invalid, so callers cannot execute a safe prefix of an unsafe batch.
pub fn generate(
    source: &EditableSource,
    mutations: &[Mutation],
    dialect: Dialect,
) -> Result<GeneratedBatch, StatementError> {
    if source.identity_indices().is_empty() {
        return Err(StatementError::NoIdentity);
    }
    let statements = mutations
        .iter()
        .map(|mutation| generate_one(source, mutation, dialect))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(GeneratedBatch {
        dialect,
        statements,
    })
}

fn generate_one(
    source: &EditableSource,
    mutation: &Mutation,
    dialect: Dialect,
) -> Result<GeneratedStatement, StatementError> {
    match mutation {
        Mutation::Update { identity, values } => update(source, identity, values, dialect),
        Mutation::Delete { identity } => delete(source, identity, dialect),
        Mutation::Insert { values } => insert(source, values, dialect),
    }
}

fn update(
    source: &EditableSource,
    identity: &[Value],
    values: &[(usize, Value)],
    dialect: Dialect,
) -> Result<GeneratedStatement, StatementError> {
    validate_identity(source, identity)?;
    let mut seen = BTreeSet::new();
    let mut params = Vec::new();
    let mut assignments = Vec::new();
    for (index, value) in values {
        let column = source
            .column(*index)
            .ok_or(StatementError::InvalidColumn(*index))?;
        if column.identity {
            return Err(StatementError::IdentityColumnChanged(column.name.clone()));
        }
        if !seen.insert(*index) {
            continue;
        }
        validate_value(value)?;
        params.push(value.clone());
        assignments.push(format!(
            "{} = {}",
            quote_identifier(&column.name, dialect),
            placeholder(params.len(), dialect)
        ));
    }
    if assignments.is_empty() {
        return Err(StatementError::NoValues);
    }
    let where_clause = identity_clause(source, identity, dialect, &mut params)?;
    Ok(GeneratedStatement {
        sql: format!(
            "UPDATE {} SET {} WHERE {}",
            quote_table(source.table(), dialect),
            assignments.join(", "),
            where_clause
        ),
        params,
    })
}

fn delete(
    source: &EditableSource,
    identity: &[Value],
    dialect: Dialect,
) -> Result<GeneratedStatement, StatementError> {
    validate_identity(source, identity)?;
    let mut params = Vec::new();
    let where_clause = identity_clause(source, identity, dialect, &mut params)?;
    Ok(GeneratedStatement {
        sql: format!(
            "DELETE FROM {} WHERE {}",
            quote_table(source.table(), dialect),
            where_clause
        ),
        params,
    })
}

fn insert(
    source: &EditableSource,
    values: &[(usize, Value)],
    dialect: Dialect,
) -> Result<GeneratedStatement, StatementError> {
    let mut seen = BTreeSet::new();
    let mut names = Vec::new();
    let mut params = Vec::new();
    for (index, value) in values {
        let column = source
            .column(*index)
            .ok_or(StatementError::InvalidColumn(*index))?;
        // The server owns generated values even if a caller accidentally
        // supplied one. Omitting it here keeps that policy in the SQL owner.
        if column.generated || !seen.insert(*index) {
            continue;
        }
        validate_value(value)?;
        names.push(quote_identifier(&column.name, dialect));
        params.push(value.clone());
    }

    let table = quote_table(source.table(), dialect);
    let sql = if names.is_empty() {
        match dialect {
            Dialect::PostgreSql | Dialect::Sqlite => format!("INSERT INTO {table} DEFAULT VALUES"),
            Dialect::MySql => format!("INSERT INTO {table} () VALUES ()"),
        }
    } else {
        let placeholders = (1..=params.len())
            .map(|index| placeholder(index, dialect))
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "INSERT INTO {table} ({}) VALUES ({placeholders})",
            names.join(", ")
        )
    };
    Ok(GeneratedStatement { sql, params })
}

fn validate_identity(source: &EditableSource, identity: &[Value]) -> Result<(), StatementError> {
    if identity.len() != source.identity_indices().len() {
        return Err(StatementError::WrongIdentityValueCount);
    }
    for value in identity {
        if matches!(value, Value::Null) {
            return Err(StatementError::NullIdentity);
        }
        validate_value(value)?;
    }
    Ok(())
}

fn validate_value(value: &Value) -> Result<(), StatementError> {
    if matches!(value, Value::Truncated { .. }) {
        Err(StatementError::TruncatedValue)
    } else {
        Ok(())
    }
}

fn identity_clause(
    source: &EditableSource,
    identity: &[Value],
    dialect: Dialect,
    params: &mut Vec<Value>,
) -> Result<String, StatementError> {
    validate_identity(source, identity)?;
    let mut clauses = Vec::with_capacity(identity.len());
    for (index, value) in source.identity_indices().iter().zip(identity) {
        let column = source
            .column(*index)
            .ok_or(StatementError::InvalidColumn(*index))?;
        params.push(value.clone());
        clauses.push(format!(
            "{} = {}",
            quote_identifier(&column.name, dialect),
            placeholder(params.len(), dialect)
        ));
    }
    Ok(clauses.join(" AND "))
}

fn quote_table(table: &TableRef, dialect: Dialect) -> String {
    match &table.schema {
        Some(schema) => format!(
            "{}.{}",
            quote_identifier(schema, dialect),
            quote_identifier(&table.table, dialect)
        ),
        None => quote_identifier(&table.table, dialect),
    }
}

pub fn quote_identifier(identifier: &str, dialect: Dialect) -> String {
    match dialect {
        Dialect::PostgreSql | Dialect::Sqlite => format!("\"{}\"", identifier.replace('"', "\"\"")),
        Dialect::MySql => format!("`{}`", identifier.replace('`', "``")),
    }
}

pub fn placeholder(index: usize, dialect: Dialect) -> String {
    match dialect {
        Dialect::PostgreSql => format!("${index}"),
        Dialect::Sqlite => format!("?{index}"),
        Dialect::MySql => "?".into(),
    }
}

/// Read-only diagnostic text for one bound value in the confirmation dialog.
/// It is never sent to a database; execution uses [`GeneratedStatement::params`].
pub fn display_parameter(value: &Value) -> String {
    match value {
        Value::Null => "NULL".into(),
        Value::Bool(value) => value.to_string(),
        Value::Int(value) => value.to_string(),
        Value::Float(value) => value.to_string(),
        Value::Text(value) | Value::Json(value) => format!("'{}'", value.replace('\'', "''")),
        Value::Bytes(value) => {
            let hex: String = value.iter().map(|byte| format!("{byte:02x}")).collect();
            format!("X'{hex}'")
        }
        Value::Truncated { .. } => "<truncated>".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::{Dialect, Mutation, StatementError, display_parameter, generate, quote_identifier};
    use crate::database::models::identity::{IdentityMetadata, TableRef, UniqueKey, prove};
    use crate::database::models::value::{ColumnMeta, ColumnOrigin, Value};

    fn source() -> crate::database::models::identity::EditableSource {
        let columns = ["id", "tenant", "name", "generated"]
            .into_iter()
            .map(|name| {
                ColumnMeta::new(name, "text").with_origin(ColumnOrigin {
                    schema: Some("odd\"schema".into()),
                    table: "we`ird".into(),
                    column: name.into(),
                })
            })
            .collect::<Vec<_>>();
        let mut metadata = IdentityMetadata::new(TableRef {
            schema: Some("odd\"schema".into()),
            table: "we`ird".into(),
        });
        metadata.keys.push(UniqueKey {
            columns: vec!["id".into(), "tenant".into()],
            primary: true,
            all_non_null: true,
        });
        metadata.generated_columns.insert("generated".into());
        match prove(&columns, metadata) {
            crate::database::models::identity::Editability::Editable(source) => source,
            other => panic!("expected editable source, got {other:?}"),
        }
    }

    #[test]
    fn update_and_delete_quote_identifiers_and_bind_the_proved_key() {
        let mutations = vec![
            Mutation::Update {
                identity: vec![Value::Int(7), Value::Text("acme".into())],
                values: vec![(2, Value::Text("O'Reilly".into()))],
            },
            Mutation::Delete {
                identity: vec![Value::Int(8), Value::Text("acme".into())],
            },
        ];
        let batch = generate(&source(), &mutations, Dialect::PostgreSql).unwrap();
        assert_eq!(
            batch.statements[0].sql,
            "UPDATE \"odd\"\"schema\".\"we`ird\" SET \"name\" = $1 WHERE \"id\" = $2 AND \"tenant\" = $3"
        );
        assert_eq!(batch.statements[0].params.len(), 3);
        assert_eq!(
            batch.statements[1].sql,
            "DELETE FROM \"odd\"\"schema\".\"we`ird\" WHERE \"id\" = $1 AND \"tenant\" = $2"
        );
    }

    #[test]
    fn mysql_uses_backticks_and_positional_question_marks() {
        let batch = generate(
            &source(),
            &[Mutation::Delete {
                identity: vec![Value::Int(1), Value::Text("t".into())],
            }],
            Dialect::MySql,
        )
        .unwrap();
        assert_eq!(
            batch.statements[0].sql,
            "DELETE FROM `odd\"schema`.`we``ird` WHERE `id` = ? AND `tenant` = ?"
        );
        assert_eq!(quote_identifier("a`b", Dialect::MySql), "`a``b`");
    }

    #[test]
    fn inserts_omit_server_generated_columns() {
        let batch = generate(
            &source(),
            &[Mutation::Insert {
                values: vec![
                    (0, Value::Int(1)),
                    (1, Value::Text("t".into())),
                    (2, Value::Text("n".into())),
                    (3, Value::Text("must be omitted".into())),
                ],
            }],
            Dialect::Sqlite,
        )
        .unwrap();
        assert_eq!(
            batch.statements[0].sql,
            "INSERT INTO \"odd\"\"schema\".\"we`ird\" (\"id\", \"tenant\", \"name\") VALUES (?1, ?2, ?3)"
        );
        assert_eq!(batch.statements[0].params.len(), 3);
    }

    #[test]
    fn no_generated_update_or_delete_can_use_a_null_or_truncated_identity() {
        for value in [
            Value::Null,
            Value::Truncated {
                prefix: "x".into(),
                full_bytes: 99,
            },
        ] {
            let error = generate(
                &source(),
                &[Mutation::Delete {
                    identity: vec![value, Value::Text("t".into())],
                }],
                Dialect::PostgreSql,
            )
            .unwrap_err();
            assert!(matches!(
                error,
                StatementError::NullIdentity | StatementError::TruncatedValue
            ));
        }
    }

    #[test]
    fn parameter_diagnostics_are_never_interpolated_into_sql() {
        assert_eq!(
            display_parameter(&Value::Text("O'Reilly".into())),
            "'O''Reilly'"
        );
        assert_eq!(display_parameter(&Value::Bytes(vec![0, 255])), "X'00ff'");
    }
}
