//! A [`Driver`] with no server behind it.
//!
//! The test seam for everything above `services/`, exactly as `FakeTransport`
//! is for the API Explorer's send pipeline. Two things it is used to prove:
//!
//! 1. **The state layer's behaviour** — lazy expansion, what happens to a tree
//!    when a node fails to load, what the result footer says — with no
//!    PostgreSQL and no `Window`.
//! 2. **That the trait really is backend-agnostic.**
//!    [`FakeDriver::key_value`] answers as a store with no schemas, no tables
//!    and a non-SQL console, and everything above takes it without a special
//!    case. If a future change makes that awkward, this is where it shows.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::models::catalog::{CatalogNode, GroupLabel, NodeId, NodeKind};
use crate::models::detail::{DATA_PAGE_SIZE, DdlSource, DetailRequest, DetailTab};
use crate::models::error::DbError;
use crate::models::identity::{
    Editability, IdentityMetadata, ReadOnlyReason, TableRef, UniqueKey, prove,
};
use crate::models::page::{Flow, RowSink};
use crate::models::query::{Execution, QueryRequest};
use crate::models::statement::GeneratedBatch;
use crate::models::value::{ColumnMeta, ColumnOrigin, Row, Value};
use crate::services::{CancelHandle, Capabilities, DetailResult, Driver, MutationFailure};

/// What a fake answers with for one node.
type Children = Vec<CatalogNode>;

pub struct FakeDriver {
    editor_language: &'static str,
    /// `None` keyed as the empty string.
    tree: Vec<(String, Result<Children, DbError>)>,
    /// The rows every `execute` streams, and the columns above them.
    columns: Vec<ColumnMeta>,
    rows: Vec<Row>,
    /// What `execute` returns instead of rows, when set.
    failure: Option<DbError>,
    /// Whether this fake offers a [`CancelHandle`] at all — the `false` case is
    /// how a backend that cannot cancel is exercised.
    cancel: bool,
    /// Set by the handle. A subsequent `execute` fails with
    /// [`DbError::Cancelled`], which is the *server's* half of the deal: no
    /// real driver reports a cancellation it was not told about either.
    cancelled: Arc<AtomicBool>,
    /// Every statement the driver was asked to run, in order. The whole point
    /// of holding it: a test can assert dodo sent the statement *verbatim*.
    pub executed: Mutex<Vec<String>>,
    /// How many rows the last `execute` actually handed over before the sink
    /// said stop — the evidence that streaming is streaming.
    pub offered: Mutex<usize>,
    mutation_counts: Vec<Result<u64, DbError>>,
    pub committed: Arc<AtomicBool>,
    pub rolled_back: Arc<AtomicBool>,
}

impl Default for FakeDriver {
    fn default() -> Self {
        Self::sql()
    }
}

impl FakeDriver {
    /// A small SQL-shaped database: one schema, two tables.
    pub fn sql() -> Self {
        Self {
            editor_language: "sql",
            tree: vec![
                (
                    String::new(),
                    Ok(vec![CatalogNode::branch(
                        "db:shop",
                        NodeKind::Database,
                        "shop",
                    )]),
                ),
                (
                    "db:shop".into(),
                    Ok(vec![CatalogNode::branch(
                        "schema:public",
                        NodeKind::Schema,
                        "public",
                    )]),
                ),
                (
                    "schema:public".into(),
                    Ok(vec![
                        CatalogNode::group("group:tables", GroupLabel::Tables),
                        CatalogNode::group("group:views", GroupLabel::Views),
                    ]),
                ),
                (
                    "group:tables".into(),
                    Ok(vec![
                        CatalogNode::branch("table:users", NodeKind::Table, "users"),
                        CatalogNode::branch("table:orders", NodeKind::Table, "orders"),
                    ]),
                ),
                ("group:views".into(), Ok(Vec::new())),
                (
                    "table:users".into(),
                    Ok(vec![CatalogNode::group(
                        "group:users.columns",
                        GroupLabel::Columns,
                    )]),
                ),
                (
                    "group:users.columns".into(),
                    Ok(vec![
                        CatalogNode::leaf("col:users.id", NodeKind::Column, "id")
                            .with_detail("int4"),
                        CatalogNode::leaf("col:users.name", NodeKind::Column, "name")
                            .with_detail("text"),
                    ]),
                ),
                (
                    "table:orders".into(),
                    Err(DbError::server("permission denied for table orders")),
                ),
            ],
            columns: vec![
                ColumnMeta::new("id", "int4").with_origin(ColumnOrigin {
                    schema: Some("public".into()),
                    table: "users".into(),
                    column: "id".into(),
                }),
                ColumnMeta::new("name", "text").with_origin(ColumnOrigin {
                    schema: Some("public".into()),
                    table: "users".into(),
                    column: "name".into(),
                }),
            ],
            rows: (1..=3)
                .map(|n| vec![Value::Int(n), Value::Text(format!("row-{n}"))])
                .collect(),
            failure: None,
            cancel: true,
            cancelled: Arc::new(AtomicBool::new(false)),
            executed: Mutex::new(Vec::new()),
            offered: Mutex::new(0),
            mutation_counts: Vec::new(),
            committed: Arc::new(AtomicBool::new(false)),
            rolled_back: Arc::new(AtomicBool::new(false)),
        }
    }

    /// A store with no schemas, no tables and a console that is not SQL — the
    /// shape a key/value backend has. Nothing above `services/` may need a
    /// special case for it.
    pub fn key_value() -> Self {
        Self {
            editor_language: "text",
            tree: vec![
                (
                    String::new(),
                    Ok(vec![
                        CatalogNode::branch("ns:0", NodeKind::Other, "db0").with_detail("2 keys"),
                        CatalogNode::branch("ns:1", NodeKind::Other, "db1").with_detail("0 keys"),
                    ]),
                ),
                (
                    "ns:0".into(),
                    Ok(vec![
                        CatalogNode::leaf("key:greeting", NodeKind::Other, "greeting")
                            .with_detail("string"),
                        CatalogNode::leaf("key:session", NodeKind::Other, "session")
                            .with_detail("hash, ttl 60s"),
                    ]),
                ),
                ("ns:1".into(), Ok(Vec::new())),
            ],
            columns: vec![ColumnMeta::new("reply", "string")],
            rows: vec![vec![Value::Text("PONG".into())]],
            failure: None,
            cancel: true,
            cancelled: Arc::new(AtomicBool::new(false)),
            executed: Mutex::new(Vec::new()),
            offered: Mutex::new(0),
            mutation_counts: Vec::new(),
            committed: Arc::new(AtomicBool::new(false)),
            rolled_back: Arc::new(AtomicBool::new(false)),
        }
    }

    /// The same driver, with `count` identical rows to stream. Used to exercise
    /// the page budget against a driver rather than against the sink alone.
    pub fn with_rows(mut self, count: usize) -> Self {
        self.rows = (0..count)
            .map(|n| vec![Value::Int(n as i64), Value::Text(format!("row-{n}"))])
            .collect();
        self
    }

    /// The same driver, whose `execute` fails.
    pub fn failing(mut self, error: DbError) -> Self {
        self.failure = Some(error);
        self
    }

    /// The same driver, with no way to cancel — a backend whose protocol has
    /// none. `capabilities().cancel` is false and `cancel_handle` is `None`,
    /// and nothing above `services/` may need a branch for it beyond hiding the
    /// control.
    pub fn without_cancel(mut self) -> Self {
        self.cancel = false;
        self
    }

    pub fn with_mutation_counts(mut self, counts: Vec<Result<u64, DbError>>) -> Self {
        self.mutation_counts = counts;
        self
    }

    fn lookup(&self, parent: Option<&NodeId>) -> Result<Children, DbError> {
        let key = parent.map(NodeId::as_str).unwrap_or("");
        self.tree
            .iter()
            .find(|(id, _)| id == key)
            .map(|(_, children)| children.clone())
            .unwrap_or_else(|| Ok(Vec::new()))
    }
}

impl Driver for FakeDriver {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            editor_language: self.editor_language,
            cancel: self.cancel,
            explain: self.editor_language == "sql",
            detail: self.editor_language == "sql",
            ddl: if self.editor_language == "sql" {
                DdlSource::Server
            } else {
                DdlSource::None
            },
            mutation: (self.editor_language == "sql")
                .then_some(crate::models::statement::Dialect::Sqlite),
        }
    }

    fn explain_statement(&self, statement: &str) -> Option<String> {
        self.capabilities()
            .explain
            .then(|| format!("EXPLAIN {statement}"))
    }

    fn cancel_handle(&self) -> Option<CancelHandle> {
        if !self.cancel {
            return None;
        }
        let cancelled = self.cancelled.clone();
        Some(CancelHandle::new(move || {
            cancelled.store(true, Ordering::SeqCst);
            Ok(())
        }))
    }

    fn ping(&self) -> Result<(), DbError> {
        Ok(())
    }

    fn children(&self, parent: Option<&NodeId>) -> Result<Vec<CatalogNode>, DbError> {
        self.lookup(parent)
    }

    fn detail(
        &self,
        request: &DetailRequest,
        sink: &mut dyn RowSink,
    ) -> Result<DetailResult, DbError> {
        if !self.capabilities().detail {
            return Ok(DetailResult::Unavailable);
        }
        if let Some(error) = &self.failure {
            return Err(error.clone());
        }
        if request.tab == DetailTab::Ddl {
            return Ok(DetailResult::Ddl("CREATE TABLE users (id INTEGER);".into()));
        }

        sink.columns(self.columns.clone());
        let start = if request.tab == DetailTab::Data {
            request.offset as usize
        } else {
            0
        };
        let limit = if request.tab == DetailTab::Data {
            DATA_PAGE_SIZE as usize + 1
        } else {
            self.rows.len()
        };
        let mut truncated = false;
        for row in self.rows.iter().skip(start).take(limit) {
            if sink.row(row.clone()) == Flow::Stop {
                truncated = true;
                break;
            }
        }
        Ok(DetailResult::Rows {
            fields: None,
            truncated,
            notice: None,
        })
    }

    fn execute(
        &self,
        request: &QueryRequest,
        sink: &mut dyn RowSink,
    ) -> Result<Execution, DbError> {
        if let Ok(mut executed) = self.executed.lock() {
            executed.push(request.statement.clone());
        }
        // Checked before the failure, so a driver told to cancel reports the
        // cancellation rather than whatever else it was going to say — which is
        // what a real server does too.
        if self.cancelled.swap(false, Ordering::SeqCst) {
            return Err(DbError::Cancelled);
        }
        if let Some(error) = &self.failure {
            return Err(error.clone());
        }

        sink.columns(self.columns.clone());
        let mut offered = 0usize;
        for row in &self.rows {
            offered += 1;
            if sink.row(row.clone()) == Flow::Stop {
                break;
            }
        }
        if let Ok(mut held) = self.offered.lock() {
            *held = offered;
        }

        Ok(Execution {
            rows_affected: None,
            // The sink refused a row it was offered, which is exactly what
            // truncation means. A real driver reports the same way.
            truncated: offered < self.rows.len() || offered == 0 && !self.rows.is_empty(),
            elapsed: std::time::Duration::from_millis(1),
        })
    }

    fn editability(&self, columns: &[ColumnMeta]) -> Editability {
        if self.editor_language != "sql" {
            return Editability::ReadOnly(ReadOnlyReason::Unsupported);
        }
        let mut metadata = IdentityMetadata::new(TableRef {
            schema: Some("public".into()),
            table: "users".into(),
        });
        metadata.keys.push(UniqueKey {
            columns: vec!["id".into()],
            primary: true,
            all_non_null: true,
        });
        metadata.generated_columns.insert("id".into());
        prove(columns, metadata)
    }

    fn commit(&self, batch: &GeneratedBatch) -> Result<(), MutationFailure> {
        for (index, statement) in batch.statements.iter().enumerate() {
            match self.mutation_counts.get(index).cloned().unwrap_or(Ok(1)) {
                Err(error) => {
                    self.rolled_back.store(true, Ordering::SeqCst);
                    return Err(MutationFailure::Statement {
                        index,
                        sql: statement.sql.clone(),
                        error,
                    });
                }
                Ok(actual) if actual != 1 => {
                    self.rolled_back.store(true, Ordering::SeqCst);
                    return Err(MutationFailure::Affected {
                        index,
                        sql: statement.sql.clone(),
                        actual,
                    });
                }
                Ok(_) => {}
            }
        }
        self.committed.store(true, Ordering::SeqCst);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::FakeDriver;
    use crate::models::catalog::NodeId;
    use crate::models::page::{PageBudget, PageBuffer};
    use crate::models::query::QueryRequest;
    use crate::services::Driver;

    #[test]
    fn the_sql_fake_has_a_tree_that_loads_one_level_at_a_time() {
        let driver = FakeDriver::sql();

        let roots = driver.children(None).expect("roots");
        assert_eq!(roots.len(), 1);
        assert!(roots[0].expandable);

        let schemas = driver.children(Some(&roots[0].id)).expect("schemas");
        assert_eq!(schemas.len(), 1);

        let groups = driver.children(Some(&schemas[0].id)).expect("groups");
        assert_eq!(groups.len(), 2);
    }

    #[test]
    fn an_unknown_node_has_no_children_rather_than_an_error() {
        let driver = FakeDriver::sql();
        let children = driver
            .children(Some(&NodeId::new("nothing:here")))
            .expect("an unknown node is empty, not broken");
        assert!(children.is_empty());
    }

    #[test]
    fn a_node_can_fail_to_load_on_its_own() {
        let driver = FakeDriver::sql();
        assert!(driver.children(Some(&NodeId::new("table:orders"))).is_err());
    }

    #[test]
    fn the_statement_reaches_the_driver_verbatim() {
        let driver = FakeDriver::sql();
        let statement = "SELECT 1 -- with my comment";
        let mut sink = PageBuffer::default();
        driver
            .execute(&QueryRequest::new(statement), &mut sink)
            .expect("runs");

        assert_eq!(driver.executed.lock().unwrap().as_slice(), [statement]);
    }

    /// The page budget is enforced against a *driver*, not just against the
    /// sink in isolation: the driver stops offering rows and says truncated.
    #[test]
    fn a_driver_stops_streaming_when_the_page_budget_is_full() {
        let driver = FakeDriver::sql().with_rows(500);
        let mut sink = PageBuffer::new(PageBudget {
            max_rows: 10,
            ..PageBudget::default()
        });

        let execution = driver
            .execute(&QueryRequest::new("SELECT * FROM big"), &mut sink)
            .expect("runs");

        assert_eq!(sink.rows().len(), 10);
        assert!(sink.truncated());
        assert!(execution.truncated);
        assert_eq!(
            *driver.offered.lock().unwrap(),
            11,
            "exactly one row past the budget is read, and that is what proves there was more"
        );
    }

    #[test]
    fn a_result_that_fits_is_not_reported_as_truncated() {
        let driver = FakeDriver::sql();
        let mut sink = PageBuffer::default();
        let execution = driver
            .execute(&QueryRequest::new("SELECT * FROM small"), &mut sink)
            .expect("runs");

        assert_eq!(sink.rows().len(), 3);
        assert!(!execution.truncated);
        assert!(!sink.truncated());
    }
}
