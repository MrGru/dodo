//! Pending table-data changes.
//!
//! The displayed rows and their original positions live together so add,
//! delete and duplicate cannot desynchronise row indices. Mutations are derived
//! from the original/current diff at Commit time; Rollback is therefore one
//! reset, not a fragile attempt to reverse an operation log.

use crate::models::identity::{Editability, EditableSource, ReadOnlyReason};
use crate::models::statement::Mutation;
use crate::models::value::{Row, Value};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditError {
    ReadOnly(ReadOnlyReason),
    MissingRow,
    MissingColumn,
    IdentityColumn,
    IdentityUnavailable,
    UnsupportedCell,
    MissingRequiredIdentity(Vec<String>),
}

/// One result set, including pending local changes.
#[derive(Clone, Debug, PartialEq)]
pub struct PendingGrid {
    editability: Editability,
    original: Vec<Row>,
    rows: Vec<Row>,
    /// `Some(original index)` for a server row; `None` for a pending insert.
    origins: Vec<Option<usize>>,
}

impl PendingGrid {
    pub fn new(rows: Vec<Row>, mut editability: Editability) -> Self {
        if let Editability::Editable(source) = &editability {
            // ponytail: result pages are capped at 1,000 rows; an O(n²)
            // duplicate check is smaller than hashing a Value enum with f64.
            let duplicate = rows.iter().enumerate().any(|(index, row)| {
                let identity = identity_values(source, row);
                identity.len() == source.identity_indices().len()
                    && identity
                        .iter()
                        .all(|value| !matches!(value, Value::Null | Value::Truncated { .. }))
                    && rows[index + 1..]
                        .iter()
                        .any(|other| identity_values(source, other) == identity)
            });
            if duplicate {
                editability = Editability::ReadOnly(ReadOnlyReason::DuplicateRows);
            }
        }
        let origins = (0..rows.len()).map(Some).collect();
        Self {
            original: rows.clone(),
            rows,
            origins,
            editability,
        }
    }

    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    pub fn editability(&self) -> &Editability {
        &self.editability
    }

    pub fn source(&self) -> Result<&EditableSource, EditError> {
        match &self.editability {
            Editability::Editable(source) => Ok(source),
            Editability::ReadOnly(reason) => Err(EditError::ReadOnly(reason.clone())),
        }
    }

    pub fn row(&self, row: usize) -> Option<&Row> {
        self.rows.get(row)
    }

    pub fn is_pending_insert(&self, row: usize) -> bool {
        self.origins.get(row) == Some(&None)
    }

    pub fn cell_error(&self, row: usize, column: usize) -> Option<EditError> {
        let source = match self.source() {
            Ok(source) => source,
            Err(error) => return Some(error),
        };
        let values = match self.row(row) {
            Some(values) => values,
            None => return Some(EditError::MissingRow),
        };
        if source.column(column).is_none() {
            return Some(EditError::MissingColumn);
        }
        if source.is_identity(column) {
            return Some(EditError::IdentityColumn);
        }
        if !self.is_pending_insert(row)
            && let Err(error) = ensure_identity(source, values)
        {
            return Some(error);
        }
        matches!(
            values.get(column),
            Some(Value::Bytes(_) | Value::Truncated { .. })
        )
        .then_some(EditError::UnsupportedCell)
    }

    pub fn duplicate_error(&self, row: usize) -> Option<EditError> {
        let source = match self.source() {
            Ok(source) => source,
            Err(error) => return Some(error),
        };
        let values = match self.row(row) {
            Some(values) => values,
            None => return Some(EditError::MissingRow),
        };
        ensure_identity(source, values).err().or_else(|| {
            values
                .iter()
                .any(|value| matches!(value, Value::Truncated { .. }))
                .then_some(EditError::UnsupportedCell)
        })
    }

    pub fn row_error(&self, row: usize) -> Option<EditError> {
        let source = match self.source() {
            Ok(source) => source,
            Err(error) => return Some(error),
        };
        let values = match self.row(row) {
            Some(values) => values,
            None => return Some(EditError::MissingRow),
        };
        if self.is_pending_insert(row) {
            None
        } else {
            ensure_identity(source, values).err()
        }
    }

    pub fn edit(&mut self, row: usize, column: usize, value: Value) -> Result<(), EditError> {
        let source = self.source()?;
        let existing = self.origins.get(row).is_some_and(Option::is_some);
        if source.is_identity(column) && existing {
            return Err(EditError::IdentityColumn);
        }
        if existing {
            ensure_identity(source, self.row(row).ok_or(EditError::MissingRow)?)?;
        }
        if matches!(value, Value::Truncated { .. }) {
            return Err(EditError::UnsupportedCell);
        }
        let cell = self
            .rows
            .get_mut(row)
            .ok_or(EditError::MissingRow)?
            .get_mut(column)
            .ok_or(EditError::MissingColumn)?;
        *cell = value;
        Ok(())
    }

    pub fn delete(&mut self, row: usize) -> Result<(), EditError> {
        let source = self.source()?;
        if self.origins.get(row).is_some_and(Option::is_some) {
            ensure_identity(source, self.row(row).ok_or(EditError::MissingRow)?)?;
        } else if row >= self.rows.len() {
            return Err(EditError::MissingRow);
        }
        self.rows.remove(row);
        self.origins.remove(row);
        Ok(())
    }

    /// A blank row for Add Row. Generated columns stay present in the display
    /// but statement generation omits them.
    pub fn add_template(&self) -> Result<Row, EditError> {
        Ok(vec![Value::Null; self.source()?.columns().len()])
    }

    /// A copy for Duplicate Row. Server-generated values and non-generated
    /// identity values are cleared: the former must be omitted and the latter
    /// must be supplied explicitly rather than guessed from the source row.
    pub fn duplicate_template(&self, row: usize) -> Result<Row, EditError> {
        let source = self.source()?;
        let mut values = self.row(row).ok_or(EditError::MissingRow)?.clone();
        ensure_identity(source, &values)?;
        for column in source.columns() {
            if column.generated || column.identity {
                values[column.result_index] = Value::Null;
            }
        }
        Ok(values)
    }

    pub fn required_identity_columns(&self) -> Result<Vec<(usize, String)>, EditError> {
        let source = self.source()?;
        Ok(source
            .columns()
            .iter()
            .filter(|column| column.identity && !column.generated)
            .map(|column| (column.result_index, column.name.clone()))
            .collect())
    }

    pub fn insert(&mut self, values: Row) -> Result<(), EditError> {
        let source = self.source()?;
        if values.len() != source.columns().len() {
            return Err(EditError::MissingColumn);
        }
        let missing = source
            .columns()
            .iter()
            .filter(|column| {
                column.identity
                    && !column.generated
                    && matches!(values[column.result_index], Value::Null)
            })
            .map(|column| column.name.clone())
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            return Err(EditError::MissingRequiredIdentity(missing));
        }
        if values
            .iter()
            .any(|value| matches!(value, Value::Truncated { .. }))
        {
            return Err(EditError::UnsupportedCell);
        }
        self.rows.push(values);
        self.origins.push(None);
        Ok(())
    }

    pub fn rollback(&mut self) {
        self.rows.clone_from(&self.original);
        self.origins = (0..self.rows.len()).map(Some).collect();
    }

    pub fn has_pending(&self) -> bool {
        !self.mutations().is_empty()
    }

    pub fn pending_rows(&self) -> usize {
        self.mutations().len()
    }

    /// One expected-single-row mutation per changed, deleted, or inserted row.
    pub fn mutations(&self) -> Vec<Mutation> {
        let Ok(source) = self.source() else {
            return Vec::new();
        };
        let mut mutations = Vec::new();

        for (original_index, original) in self.original.iter().enumerate() {
            match self
                .origins
                .iter()
                .position(|origin| *origin == Some(original_index))
            {
                None => mutations.push(Mutation::Delete {
                    identity: identity_values(source, original),
                }),
                Some(current_index) => {
                    let current = &self.rows[current_index];
                    let values = source
                        .columns()
                        .iter()
                        .filter(|column| !column.identity)
                        .filter_map(|column| {
                            let old = original.get(column.result_index)?;
                            let new = current.get(column.result_index)?;
                            (old != new).then(|| (column.result_index, new.clone()))
                        })
                        .collect::<Vec<_>>();
                    if !values.is_empty() {
                        mutations.push(Mutation::Update {
                            identity: identity_values(source, original),
                            values,
                        });
                    }
                }
            }
        }

        for (row, origin) in self.rows.iter().zip(&self.origins) {
            if origin.is_none() {
                mutations.push(Mutation::Insert {
                    values: row.iter().cloned().enumerate().collect(),
                });
            }
        }
        mutations
    }
}

fn identity_values(source: &EditableSource, row: &Row) -> Vec<Value> {
    source
        .identity_indices()
        .iter()
        .filter_map(|index| row.get(*index).cloned())
        .collect()
}

fn ensure_identity(source: &EditableSource, row: &Row) -> Result<(), EditError> {
    for index in source.identity_indices() {
        match row.get(*index) {
            Some(Value::Null | Value::Truncated { .. }) | None => {
                return Err(EditError::IdentityUnavailable);
            }
            Some(_) => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{EditError, PendingGrid};
    use crate::models::identity::{IdentityMetadata, TableRef, UniqueKey, prove};
    use crate::models::statement::{Dialect, Mutation, generate};
    use crate::models::value::{ColumnMeta, ColumnOrigin, Value};
    use crate::services::fake::FakeDriver;
    use crate::services::{Driver, MutationFailure};
    use std::sync::atomic::Ordering;

    fn grid() -> PendingGrid {
        let columns = ["id", "name"]
            .into_iter()
            .map(|name| {
                ColumnMeta::new(name, "text").with_origin(ColumnOrigin {
                    schema: None,
                    table: "users".into(),
                    column: name.into(),
                })
            })
            .collect::<Vec<_>>();
        let mut metadata = IdentityMetadata::new(TableRef {
            schema: None,
            table: "users".into(),
        });
        metadata.keys.push(UniqueKey {
            columns: vec!["id".into()],
            primary: true,
            all_non_null: true,
        });
        metadata.generated_columns.insert("id".into());
        PendingGrid::new(
            vec![
                vec![Value::Int(1), Value::Text("Ada".into())],
                vec![Value::Int(2), Value::Text("Grace".into())],
            ],
            prove(&columns, metadata),
        )
    }

    #[test]
    fn edits_adds_deletes_and_duplicates_stay_pending() {
        let mut grid = grid();
        grid.edit(0, 1, Value::Text("Ada Lovelace".into())).unwrap();
        grid.delete(1).unwrap();
        let mut duplicate = grid.duplicate_template(0).unwrap();
        duplicate[1] = Value::Text("Copy".into());
        grid.insert(duplicate).unwrap();

        assert_eq!(grid.rows().len(), 2);
        assert_eq!(grid.pending_rows(), 3);
        assert!(matches!(grid.mutations()[0], Mutation::Update { .. }));
        assert!(matches!(grid.mutations()[1], Mutation::Delete { .. }));
        assert!(matches!(grid.mutations()[2], Mutation::Insert { .. }));
    }

    #[test]
    fn rollback_restores_the_exact_displayed_rows() {
        let mut grid = grid();
        let original = grid.rows().to_vec();
        grid.edit(0, 1, Value::Text("changed".into())).unwrap();
        grid.delete(1).unwrap();
        grid.insert(vec![Value::Null, Value::Text("new".into())])
            .unwrap();
        grid.rollback();
        assert_eq!(grid.rows(), original);
        assert!(!grid.has_pending());
    }

    #[test]
    fn identity_cells_and_unavailable_identity_values_cannot_be_edited() {
        let mut grid = grid();
        assert_eq!(
            grid.edit(0, 0, Value::Int(9)),
            Err(EditError::IdentityColumn)
        );

        grid.rows[0][0] = Value::Null;
        assert_eq!(
            grid.edit(0, 1, Value::Text("x".into())),
            Err(EditError::IdentityUnavailable)
        );

        let row = grid.add_template().unwrap();
        grid.insert(row).unwrap();
        let inserted = grid.rows().len() - 1;
        assert!(grid.edit(inserted, 1, Value::Text("x".into())).is_ok());
    }

    #[test]
    fn deleting_a_pending_insert_cancels_it_instead_of_generating_delete() {
        let mut grid = grid();
        grid.insert(vec![Value::Null, Value::Text("new".into())])
            .unwrap();
        let inserted = grid.rows().len() - 1;
        grid.delete(inserted).unwrap();
        assert!(!grid.has_pending());
    }

    #[test]
    fn duplicate_displayed_identity_makes_the_whole_result_read_only() {
        let mut duplicate = grid();
        duplicate.rows[1][0] = Value::Int(1);
        let duplicate = PendingGrid::new(duplicate.rows, duplicate.editability);
        assert!(matches!(
            duplicate.editability(),
            crate::models::identity::Editability::ReadOnly(
                crate::models::identity::ReadOnlyReason::DuplicateRows
            )
        ));
        assert!(duplicate.mutations().is_empty());
    }

    #[test]
    fn zero_or_two_affected_rows_roll_back_the_whole_batch() {
        let mut pending = grid();
        pending.delete(0).unwrap();
        pending.delete(0).unwrap();
        let batch = generate(
            pending.source().unwrap(),
            &pending.mutations(),
            Dialect::Sqlite,
        )
        .unwrap();

        for counts in [vec![Ok(1), Ok(0)], vec![Ok(2)]] {
            let driver = FakeDriver::sql().with_mutation_counts(counts);
            assert!(matches!(
                driver.commit(&batch),
                Err(MutationFailure::Affected { .. })
            ));
            assert!(driver.rolled_back.load(Ordering::SeqCst));
            assert!(!driver.committed.load(Ordering::SeqCst));
        }
    }
}
