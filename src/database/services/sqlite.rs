//! The SQLite driver.
//!
//! # Why this driver is short and the PostgreSQL one is not
//!
//! `rusqlite` speaks to a library in the same process, so there is no wire
//! format to decode and no connection to negotiate: a value arrives as a
//! `ValueRef` in one of five kinds, and a statement's columns are described by
//! the library before the first row. Almost all of this file is the catalog.
//!
//! # The tree has no schema level, and that is the point
//!
//! PostgreSQL puts schemas between a database and its tables; SQLite has
//! nothing there. Because a driver answers "the children of this node" rather
//! than filling a fixed ladder, that difference costs exactly one thing: this
//! file's [`Driver::children`] returns groups where the PostgreSQL one returns
//! schemas. Nothing above `services/` knows or has a branch for it.
//!
//! # `column_metadata` and what it is for
//!
//! The `bundled` build is compiled with `SQLITE_ENABLE_COLUMN_METADATA`, so
//! `Statement::column_table_name` / `column_origin_name` report which base
//! table and column each result column came from. Nothing in this round reads
//! it — editing a result safely is a later round, and that is what will need it
//! — but it is filled in here because it is free at describe time, and because
//! having it lets a later round find out whether the feature really is enabled
//! in a shipped build without a rebuild to check.
//!
//! # Threading
//!
//! `rusqlite::Connection` is `!Sync`, so it sits behind a `Mutex`. Blocking by
//! contract, like every other driver.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags};

use crate::database::models::catalog::{CatalogNode, GroupLabel, NodeId, NodeKind};
use crate::database::models::connection::ConnectionProfile;
use crate::database::models::error::DbError;
use crate::database::models::page::{Flow, RowSink};
use crate::database::models::query::{Execution, QueryRequest};
use crate::database::models::value::{ColumnMeta, ColumnOrigin, Value};
use crate::database::services::{Capabilities, Driver};

pub struct SqliteDriver {
    connection: Mutex<Connection>,
}

/// Opens `profile`'s file.
///
/// **An absent file is refused rather than created.** `sqlite3_open` happily
/// creates one, which would turn a mistyped path into a silently empty database
/// that looks like a working connection — the least helpful possible outcome.
/// Creating a database is a deliberate act and belongs behind a control that
/// says so.
pub fn connect(profile: &ConnectionProfile) -> Result<Arc<SqliteDriver>, DbError> {
    let path = profile.file.trim();
    if path.is_empty() {
        return Err(DbError::Unreachable("no database file was given".into()));
    }
    if !Path::new(path).exists() {
        return Err(DbError::Unreachable(format!("{path}: no such file")));
    }

    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_URI,
    )
    .map_err(|err| DbError::Unreachable(format!("{path}: {err}")))?;

    Ok(Arc::new(SqliteDriver {
        connection: Mutex::new(connection),
    }))
}

fn server_error(err: rusqlite::Error) -> DbError {
    // SQLite's extended result code is the closest thing it has to a SQLSTATE,
    // and it is what distinguishes "constraint violated" from "file is locked".
    let code = match &err {
        rusqlite::Error::SqliteFailure(error, _) => Some(format!("SQLITE_{}", error.extended_code)),
        _ => None,
    };
    DbError::Server {
        code,
        detail: err.to_string(),
    }
}

// ---------------------------------------------------------------- node ids

/// This driver's private node-id vocabulary. Same `\u{1f}` delimiter and same
/// reasoning as the PostgreSQL driver's: a SQLite identifier may contain
/// anything a quoted name allows.
mod id {
    pub const SEP: char = '\u{1f}';

    pub const DATABASE: &str = "db";
    pub const TABLES_GROUP: &str = "gt";
    pub const VIEWS_GROUP: &str = "gv";

    /// `t␟<table>` / `v␟<view>`
    pub fn relation(prefix: &str, name: &str) -> String {
        format!("{prefix}{SEP}{name}")
    }
    /// `gc␟…` / `gi␟…` / `gk␟…`
    pub fn relation_group(prefix: &str, name: &str) -> String {
        format!("{prefix}{SEP}{name}")
    }
    pub fn leaf(kind: &str, relation: &str, name: &str) -> String {
        format!("{kind}{SEP}{relation}{SEP}{name}")
    }

    pub fn parse(id: &str) -> Option<(&str, Vec<&str>)> {
        let mut parts = id.split(SEP);
        let tag = parts.next()?;
        Some((tag, parts.collect()))
    }
}

// ------------------------------------------------------------ catalog SQL

/// `sqlite_%` covers the library's own bookkeeping tables — `sqlite_sequence`,
/// `sqlite_stat1` — which are on every database and are not the user's.
const TABLES: &str = "SELECT name FROM sqlite_master \
     WHERE type = 'table' AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\' ORDER BY name";

const VIEWS: &str = "SELECT name FROM sqlite_master WHERE type = 'view' ORDER BY name";

impl SqliteDriver {
    fn with_connection<T>(
        &self,
        f: impl FnOnce(&Connection) -> Result<T, rusqlite::Error>,
    ) -> Result<T, DbError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| DbError::Unreachable("the connection was poisoned by a panic".into()))?;
        f(&connection).map_err(server_error)
    }

    /// Every row of a one-column query. Used for the catalog, whose results are
    /// small and bounded by the schema's size — unlike a user's statement,
    /// which goes through the streaming path.
    fn names(&self, sql: &str) -> Result<Vec<String>, DbError> {
        self.with_connection(|connection| {
            let mut statement = connection.prepare(sql)?;
            let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
            rows.collect()
        })
    }

    fn roots(&self) -> Result<Vec<CatalogNode>, DbError> {
        // The database's own name. `PRAGMA database_list` reports the file for
        // the `main` schema; the file's stem is what a person calls it.
        let file: Option<String> = self
            .with_connection(|connection| {
                connection.query_row("PRAGMA database_list", [], |row| row.get::<_, String>(2))
            })
            .ok();
        let name = file
            .as_deref()
            .filter(|file| !file.is_empty())
            .and_then(|file| {
                Path::new(file)
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
            })
            .unwrap_or_else(|| "main".to_string());

        Ok(vec![CatalogNode::branch(
            id::DATABASE,
            NodeKind::Database,
            name,
        )])
    }

    fn relations(
        &self,
        sql: &str,
        prefix: &str,
        kind: NodeKind,
    ) -> Result<Vec<CatalogNode>, DbError> {
        Ok(self
            .names(sql)?
            .into_iter()
            .map(|name| CatalogNode::branch(id::relation(prefix, &name), kind, name))
            .collect())
    }

    fn columns(&self, relation: &str) -> Result<Vec<CatalogNode>, DbError> {
        // `pragma_table_info` as a table-valued function rather than
        // `PRAGMA table_info(x)`: the pragma form cannot take a bound
        // parameter, so the table name would have to be pasted into the SQL —
        // which is exactly the quoting-bug class this avoids entirely.
        self.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT name, type, \"notnull\", pk FROM pragma_table_info(?1) ORDER BY cid",
            )?;
            let rows = statement.query_map([relation], |row| {
                let name: String = row.get(0)?;
                let declared: String = row.get(1)?;
                let not_null: i64 = row.get(2)?;
                let primary_key: i64 = row.get(3)?;
                Ok(
                    CatalogNode::leaf(id::leaf("c", relation, &name), NodeKind::Column, name)
                        .with_detail(column_detail(&declared, not_null != 0, primary_key != 0)),
                )
            })?;
            rows.collect()
        })
    }

    fn indexes(&self, relation: &str) -> Result<Vec<CatalogNode>, DbError> {
        self.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT name, \"unique\", origin FROM pragma_index_list(?1) ORDER BY name",
            )?;
            let rows = statement.query_map([relation], |row| {
                let name: String = row.get(0)?;
                let unique: i64 = row.get(1)?;
                let origin: String = row.get(2)?;
                Ok(
                    CatalogNode::leaf(id::leaf("i", relation, &name), NodeKind::Index, name)
                        .with_detail(index_detail(unique != 0, &origin)),
                )
            })?;
            rows.collect()
        })
    }

    /// SQLite has no `pg_constraint`. What it has is a foreign-key list and the
    /// primary key marked on the columns, so this reports exactly those two —
    /// rather than an empty group, which would read as "this table has no
    /// constraints" when it has several.
    fn constraints(&self, relation: &str) -> Result<Vec<CatalogNode>, DbError> {
        let mut nodes = Vec::new();

        let primary_key: Vec<String> = self.with_connection(|connection| {
            let mut statement = connection
                .prepare("SELECT name FROM pragma_table_info(?1) WHERE pk > 0 ORDER BY pk")?;
            let rows = statement.query_map([relation], |row| row.get::<_, String>(0))?;
            rows.collect()
        })?;
        if !primary_key.is_empty() {
            nodes.push(
                CatalogNode::leaf(
                    id::leaf("k", relation, "primary-key"),
                    NodeKind::Constraint,
                    "PRIMARY KEY",
                )
                .with_detail(format!("({})", primary_key.join(", "))),
            );
        }

        let foreign_keys: Vec<CatalogNode> = self.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT id, \"table\", \"from\", \"to\" FROM pragma_foreign_key_list(?1) ORDER BY id, seq",
            )?;
            let rows = statement.query_map([relation], |row| {
                let index: i64 = row.get(0)?;
                let target: String = row.get(1)?;
                let from: String = row.get(2)?;
                // A foreign key that names no target column references the
                // target's primary key, and SQLite reports that as NULL.
                let to: Option<String> = row.get(3)?;
                Ok(CatalogNode::leaf(
                    id::leaf("k", relation, &format!("fk-{index}-{from}")),
                    NodeKind::Constraint,
                    "FOREIGN KEY",
                )
                .with_detail(foreign_key_detail(&from, &target, to.as_deref())))
            })?;
            rows.collect()
        })?;
        nodes.extend(foreign_keys);

        Ok(nodes)
    }
}

/// The dimmed text beside a column. A SQLite column may have no declared type
/// at all, which is legal and means "no affinity" — saying nothing is more
/// honest than inventing `BLOB`.
pub(crate) fn column_detail(declared: &str, not_null: bool, primary_key: bool) -> String {
    let mut parts = Vec::new();
    if !declared.trim().is_empty() {
        parts.push(declared.trim().to_string());
    }
    if primary_key {
        parts.push("PRIMARY KEY".into());
    }
    if not_null {
        parts.push("NOT NULL".into());
    }
    parts.join(" ")
}

/// The dimmed text beside an index.
///
/// `origin` is SQLite's own word for where the index came from: `pk` for the
/// implicit primary-key index, `u` for one a `UNIQUE` clause created, `c` for
/// one a `CREATE INDEX` created. Naming the first two is what stops a table
/// looking like it has indexes nobody wrote.
pub(crate) fn index_detail(unique: bool, origin: &str) -> String {
    match origin {
        "pk" => "PRIMARY KEY".into(),
        "u" => "UNIQUE".into(),
        _ if unique => "UNIQUE".into(),
        _ => String::new(),
    }
}

pub(crate) fn foreign_key_detail(from: &str, target: &str, to: Option<&str>) -> String {
    match to {
        Some(to) => format!("{from} → {target}({to})"),
        None => format!("{from} → {target}"),
    }
}

impl Driver for SqliteDriver {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            editor_language: "sql",
        }
    }

    fn ping(&self) -> Result<(), DbError> {
        self.with_connection(|connection| connection.execute_batch("SELECT 1"))
    }

    fn children(&self, parent: Option<&NodeId>) -> Result<Vec<CatalogNode>, DbError> {
        let Some(parent) = parent else {
            return self.roots();
        };
        let Some((tag, parts)) = id::parse(parent.as_str()) else {
            return Ok(Vec::new());
        };

        match (tag, parts.as_slice()) {
            (id::DATABASE, _) => Ok(vec![
                CatalogNode::group(id::TABLES_GROUP, GroupLabel::Tables),
                CatalogNode::group(id::VIEWS_GROUP, GroupLabel::Views),
            ]),
            (id::TABLES_GROUP, _) => self.relations(TABLES, "t", NodeKind::Table),
            (id::VIEWS_GROUP, _) => self.relations(VIEWS, "v", NodeKind::View),
            ("t", [table]) => Ok(vec![
                CatalogNode::group(id::relation_group("gc", table), GroupLabel::Columns),
                CatalogNode::group(id::relation_group("gi", table), GroupLabel::Indexes),
                CatalogNode::group(id::relation_group("gk", table), GroupLabel::Constraints),
            ]),
            // A view has columns and nothing else: an index or a constraint on
            // a view is not a thing, and an empty group would be noise.
            ("v", [view]) => Ok(vec![CatalogNode::group(
                id::relation_group("gc", view),
                GroupLabel::Columns,
            )]),
            ("gc", [relation]) => self.columns(relation),
            ("gi", [relation]) => self.indexes(relation),
            ("gk", [relation]) => self.constraints(relation),
            _ => Ok(Vec::new()),
        }
    }

    fn execute(
        &self,
        request: &QueryRequest,
        sink: &mut dyn RowSink,
    ) -> Result<Execution, DbError> {
        let started = Instant::now();
        let connection = self
            .connection
            .lock()
            .map_err(|_| DbError::Unreachable("the connection was poisoned by a panic".into()))?;

        // Exactly what the user wrote, with no parameters and no appended
        // `LIMIT`. The bound on what comes back is the sink's.
        let mut statement = connection
            .prepare(&request.statement)
            .map_err(server_error)?;

        let column_count = statement.column_count();
        if column_count == 0 {
            // A statement with no result set — `INSERT`, `CREATE TABLE`,
            // `PRAGMA` with no output. `raw_execute` runs it and reports the
            // change count, which is the server's answer rather than a guess
            // from the statement's first word.
            let affected = statement.raw_execute().map_err(server_error)?;
            return Ok(Execution {
                rows_affected: Some(affected as u64),
                truncated: false,
                elapsed: started.elapsed(),
            });
        }

        sink.columns(describe(&statement, column_count));

        let mut rows = statement.raw_query();
        let mut truncated = false;
        while let Some(row) = rows.next().map_err(server_error)? {
            let values = (0..column_count)
                .map(|index| match row.get_ref(index) {
                    Ok(value) => cell(value),
                    // Only reachable on an index this loop cannot produce.
                    Err(_) => Value::Null,
                })
                .collect();
            if sink.row(values) == Flow::Stop {
                truncated = true;
                break;
            }
        }

        Ok(Execution {
            rows_affected: None,
            truncated,
            elapsed: started.elapsed(),
        })
    }
}

/// The result's shape.
///
/// `decl_type` is what the column was *declared* as in its table — SQLite has
/// no per-value type in the schema sense — and is absent for an expression, in
/// which case the header says nothing rather than guessing.
fn describe(statement: &rusqlite::Statement<'_>, column_count: usize) -> Vec<ColumnMeta> {
    let declared = statement.columns();
    let metadata = statement.columns_with_metadata();

    (0..column_count)
        .map(|index| {
            let name = declared
                .get(index)
                .map(|column| column.name().to_string())
                .unwrap_or_default();
            let type_name = declared
                .get(index)
                .and_then(|column| column.decl_type())
                .unwrap_or("")
                .to_string();

            let mut column = ColumnMeta::new(name, type_name);
            if let Some(origin) = metadata.get(index) {
                if let (Some(table), Some(name)) = (origin.table_name(), origin.origin_name()) {
                    column = column.with_origin(ColumnOrigin {
                        schema: origin.database_name().map(str::to_string),
                        table: table.to_string(),
                        column: name.to_string(),
                    });
                }
            }
            column
        })
        .collect()
}

/// One cell. SQLite has exactly five storage classes, so this needs no fallback
/// arm and no guessing.
fn cell(value: ValueRef<'_>) -> Value {
    match value {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(number) => Value::Int(number),
        ValueRef::Real(number) => Value::Float(number),
        ValueRef::Text(bytes) => Value::Text(String::from_utf8_lossy(bytes).into_owned()),
        ValueRef::Blob(bytes) => Value::Bytes(bytes.to_vec()),
    }
}

#[cfg(test)]
mod tests {
    use super::{SqliteDriver, cell, column_detail, connect, foreign_key_detail, id, index_detail};
    use crate::database::models::catalog::{GroupLabel, NodeId, NodeKind, NodeLabel};
    use crate::database::models::connection::ConnectionProfile;
    use crate::database::models::engine::Engine;
    use crate::database::models::error::DbError;
    use crate::database::models::page::{PageBudget, PageBuffer};
    use crate::database::models::query::QueryRequest;
    use crate::database::models::value::Value;
    use crate::database::services::Driver;
    use rusqlite::types::ValueRef;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// A real SQLite file, in the system temp directory. Not a fake: SQLite is
    /// the one backend whose whole server fits in the test binary, so its
    /// catalog SQL and its type mapping are exercised for real here rather than
    /// only by hand against a running database.
    struct Fixture {
        path: PathBuf,
        driver: Arc<SqliteDriver>,
    }

    impl Fixture {
        fn new(schema: &str) -> Self {
            static COUNTER: AtomicU64 = AtomicU64::new(0);
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let pid = std::process::id();
            let dir = std::env::temp_dir().join(format!("dodo-sqlite-test-{pid}-{n}"));
            std::fs::create_dir_all(&dir).expect("temp dir");
            let path = dir.join("test.db");

            let seed = rusqlite::Connection::open(&path).expect("creates");
            seed.execute_batch(schema).expect("seeds");
            drop(seed);

            let mut profile = ConnectionProfile::new(1, Engine::Sqlite);
            profile.file = path.to_string_lossy().into_owned();
            let driver = connect(&profile).expect("connects");

            Self { path, driver }
        }

        fn children(&self, id: Option<&str>) -> Vec<crate::database::models::catalog::CatalogNode> {
            let node = id.map(NodeId::new);
            self.driver.children(node.as_ref()).expect("children load")
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            if let Some(dir) = self.path.parent() {
                let _ = std::fs::remove_dir_all(dir);
            }
        }
    }

    const SCHEMA: &str = "
        CREATE TABLE users (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            score REAL,
            avatar BLOB,
            note
        );
        CREATE UNIQUE INDEX users_name ON users(name);
        CREATE TABLE orders (
            id INTEGER PRIMARY KEY,
            user_id INTEGER REFERENCES users(id)
        );
        CREATE VIEW recent AS SELECT id, name FROM users;
        INSERT INTO users (id, name, score, avatar, note)
            VALUES (1, 'ada', 9.5, x'0102', NULL),
                   (2, 'grace', 8.25, NULL, 'hello');
    ";

    // ---- connecting -----------------------------------------------------

    /// The failure this refusal exists to prevent: a typo silently becoming an
    /// empty database that looks like it connected.
    #[test]
    fn a_missing_file_is_refused_rather_than_created() {
        let dir = std::env::temp_dir().join(format!("dodo-sqlite-absent-{}", std::process::id()));
        let path = dir.join("nope.db");

        let mut profile = ConnectionProfile::new(1, Engine::Sqlite);
        profile.file = path.to_string_lossy().into_owned();

        assert!(matches!(connect(&profile), Err(DbError::Unreachable(_))));
        assert!(!path.exists(), "connecting must not have created the file");
    }

    #[test]
    fn an_empty_path_is_refused_before_sqlite_sees_it() {
        let profile = ConnectionProfile::new(1, Engine::Sqlite);
        assert!(matches!(connect(&profile), Err(DbError::Unreachable(_))));
    }

    #[test]
    fn a_real_file_connects_and_answers_a_ping() {
        let fixture = Fixture::new(SCHEMA);
        assert!(fixture.driver.ping().is_ok());
        assert_eq!(fixture.driver.capabilities().editor_language, "sql");
    }

    // ---- the catalog ----------------------------------------------------

    #[test]
    fn the_root_is_the_file_and_it_has_no_schema_level_under_it() {
        let fixture = Fixture::new(SCHEMA);

        let roots = fixture.children(None);
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].kind, NodeKind::Database);
        assert_eq!(roots[0].label, NodeLabel::Name("test.db".into()));

        let groups = fixture.children(Some(roots[0].id.as_str()));
        assert_eq!(
            groups
                .iter()
                .map(|node| node.label.clone())
                .collect::<Vec<_>>(),
            vec![
                NodeLabel::Group(GroupLabel::Tables),
                NodeLabel::Group(GroupLabel::Views),
            ],
            "SQLite has no schemas, so a schema level would be an empty lie"
        );
    }

    #[test]
    fn tables_and_views_are_listed_separately_and_sqlites_own_tables_are_hidden() {
        let fixture = Fixture::new(SCHEMA);

        let tables: Vec<String> = fixture
            .children(Some(id::TABLES_GROUP))
            .iter()
            .map(|node| match &node.label {
                NodeLabel::Name(name) => name.clone(),
                other => panic!("a table is named by the server, got {other:?}"),
            })
            .collect();
        assert_eq!(tables, ["orders", "users"]);
        assert!(
            !tables.iter().any(|name| name.starts_with("sqlite_")),
            "SQLite's own bookkeeping tables are not the user's"
        );

        let views = fixture.children(Some(id::VIEWS_GROUP));
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].kind, NodeKind::View);
    }

    #[test]
    fn a_table_offers_three_groups_and_a_view_offers_only_columns() {
        let fixture = Fixture::new(SCHEMA);

        let table_groups = fixture.children(Some(&id::relation("t", "users")));
        assert_eq!(table_groups.len(), 3);

        let view_groups = fixture.children(Some(&id::relation("v", "recent")));
        assert_eq!(view_groups.len(), 1);
        assert_eq!(
            view_groups[0].label,
            NodeLabel::Group(GroupLabel::Columns),
            "an index or a constraint on a view is not a thing"
        );
    }

    #[test]
    fn columns_are_listed_in_schema_order_with_their_declared_types() {
        let fixture = Fixture::new(SCHEMA);
        let columns = fixture.children(Some(&id::relation_group("gc", "users")));

        let names: Vec<String> = columns
            .iter()
            .map(|node| match &node.label {
                NodeLabel::Name(name) => name.clone(),
                other => panic!("got {other:?}"),
            })
            .collect();
        assert_eq!(
            names,
            ["id", "name", "score", "avatar", "note"],
            "declaration order, not alphabetical"
        );

        assert_eq!(columns[0].detail.as_deref(), Some("INTEGER PRIMARY KEY"));
        assert_eq!(columns[1].detail.as_deref(), Some("TEXT NOT NULL"));
        assert_eq!(
            columns[4].detail, None,
            "a column with no declared type says nothing rather than inventing one"
        );
    }

    #[test]
    fn indexes_name_where_they_came_from() {
        let fixture = Fixture::new(SCHEMA);
        let indexes = fixture.children(Some(&id::relation_group("gi", "users")));

        assert_eq!(indexes.len(), 1);
        assert_eq!(indexes[0].kind, NodeKind::Index);
        assert_eq!(indexes[0].detail.as_deref(), Some("UNIQUE"));
    }

    /// SQLite has no `pg_constraint`, so an empty group would read as "no
    /// constraints" on a table that plainly has two.
    #[test]
    fn constraints_are_derived_from_the_primary_key_and_the_foreign_keys() {
        let fixture = Fixture::new(SCHEMA);
        let constraints = fixture.children(Some(&id::relation_group("gk", "orders")));

        let labels: Vec<String> = constraints
            .iter()
            .map(|node| match &node.label {
                NodeLabel::Name(name) => name.clone(),
                other => panic!("got {other:?}"),
            })
            .collect();
        assert_eq!(labels, ["PRIMARY KEY", "FOREIGN KEY"]);
        assert_eq!(constraints[0].detail.as_deref(), Some("(id)"));
        assert_eq!(
            constraints[1].detail.as_deref(),
            Some("user_id → users(id)")
        );
    }

    #[test]
    fn an_unknown_node_id_has_no_children_rather_than_failing() {
        let fixture = Fixture::new(SCHEMA);
        assert!(fixture.children(Some("something:else")).is_empty());
    }

    // ---- executing ------------------------------------------------------

    #[test]
    fn a_select_streams_its_rows_with_column_names_and_declared_types() {
        let fixture = Fixture::new(SCHEMA);
        let mut sink = PageBuffer::default();
        let execution = fixture
            .driver
            .execute(
                &QueryRequest::new("SELECT id, name, score FROM users ORDER BY id"),
                &mut sink,
            )
            .expect("runs");

        assert_eq!(execution.rows_affected, None, "a SELECT changed no rows");
        assert!(!execution.truncated);

        let columns: Vec<(&str, &str)> = sink
            .columns()
            .iter()
            .map(|column| (column.name.as_str(), column.type_name.as_str()))
            .collect();
        assert_eq!(
            columns,
            [("id", "INTEGER"), ("name", "TEXT"), ("score", "REAL")]
        );

        assert_eq!(sink.rows().len(), 2);
        assert_eq!(sink.rows()[0][0], Value::Int(1));
        assert_eq!(sink.rows()[0][1], Value::Text("ada".into()));
        assert_eq!(sink.rows()[0][2], Value::Float(9.5));
    }

    /// `column_metadata` is compiled into the bundled SQLite. Nothing reads
    /// this yet; the test exists so that the round which *does* need it finds
    /// out here rather than in a rebuild.
    #[test]
    fn a_result_column_reports_the_base_table_it_came_from() {
        let fixture = Fixture::new(SCHEMA);
        let mut sink = PageBuffer::default();
        fixture
            .driver
            .execute(
                &QueryRequest::new("SELECT name, 1 + 1 AS computed FROM users"),
                &mut sink,
            )
            .expect("runs");

        let origin = sink.columns()[0].origin.as_ref().expect("a base column");
        assert_eq!(origin.table, "users");
        assert_eq!(origin.column, "name");

        assert_eq!(
            sink.columns()[1].origin,
            None,
            "an expression came from no column, and must not claim to"
        );
    }

    #[test]
    fn every_sqlite_storage_class_reaches_the_grid_as_itself() {
        let fixture = Fixture::new(SCHEMA);
        let mut sink = PageBuffer::default();
        fixture
            .driver
            .execute(
                &QueryRequest::new("SELECT avatar, note FROM users ORDER BY id"),
                &mut sink,
            )
            .expect("runs");

        assert_eq!(sink.rows()[0][0], Value::Bytes(vec![1, 2]));
        assert_eq!(
            sink.rows()[0][1],
            Value::Null,
            "NULL is not an empty string"
        );
        assert_eq!(sink.rows()[1][0], Value::Null);
        assert_eq!(sink.rows()[1][1], Value::Text("hello".into()));
    }

    #[test]
    fn a_statement_with_no_result_set_reports_what_it_changed() {
        let fixture = Fixture::new(SCHEMA);
        let mut sink = PageBuffer::default();
        let execution = fixture
            .driver
            .execute(
                &QueryRequest::new("UPDATE users SET score = 1 WHERE id IN (1, 2)"),
                &mut sink,
            )
            .expect("runs");

        assert_eq!(execution.rows_affected, Some(2));
        assert!(sink.columns().is_empty());
        assert!(sink.rows().is_empty());
    }

    #[test]
    fn ddl_runs_and_shows_up_in_the_tree() {
        let fixture = Fixture::new(SCHEMA);
        let mut sink = PageBuffer::default();
        fixture
            .driver
            .execute(
                &QueryRequest::new("CREATE TABLE fresh (a INTEGER)"),
                &mut sink,
            )
            .expect("runs");

        let tables: Vec<String> = fixture
            .children(Some(id::TABLES_GROUP))
            .iter()
            .map(|node| match &node.label {
                NodeLabel::Name(name) => name.clone(),
                other => panic!("got {other:?}"),
            })
            .collect();
        assert!(tables.contains(&"fresh".to_string()));
    }

    /// The budget holds against a real database, not only against the sink.
    #[test]
    fn the_page_budget_stops_a_real_result_and_reports_it() {
        let fixture = Fixture::new(SCHEMA);
        let mut sink = PageBuffer::new(PageBudget {
            max_rows: 1,
            ..PageBudget::default()
        });
        let execution = fixture
            .driver
            .execute(&QueryRequest::new("SELECT id FROM users"), &mut sink)
            .expect("runs");

        assert_eq!(sink.rows().len(), 1);
        assert!(execution.truncated);
        assert!(sink.truncated());
    }

    #[test]
    fn a_broken_statement_is_a_server_error_that_keeps_sqlites_own_words() {
        let fixture = Fixture::new(SCHEMA);
        let mut sink = PageBuffer::default();
        match fixture
            .driver
            .execute(&QueryRequest::new("SELECT * FROM nope"), &mut sink)
        {
            Err(DbError::Server { detail, .. }) => {
                assert!(detail.contains("nope"), "lost the message: {detail}");
            }
            other => panic!("expected a server error, got {other:?}"),
        }
    }

    #[test]
    fn the_statement_is_sent_exactly_as_written_comments_included() {
        let fixture = Fixture::new(SCHEMA);
        let mut sink = PageBuffer::default();
        // If anything rewrote the statement, this alias would not survive.
        fixture
            .driver
            .execute(
                &QueryRequest::new("SELECT 1 AS \"odd; name\" -- trailing comment"),
                &mut sink,
            )
            .expect("runs");
        assert_eq!(sink.columns()[0].name, "odd; name");
    }

    // ---- pure helpers ---------------------------------------------------

    #[test]
    fn a_column_with_no_declared_type_says_nothing() {
        assert_eq!(column_detail("", false, false), "");
        assert_eq!(column_detail("  ", true, false), "NOT NULL");
        assert_eq!(
            column_detail("TEXT", true, true),
            "TEXT PRIMARY KEY NOT NULL"
        );
    }

    #[test]
    fn an_indexs_origin_is_named_rather_than_implied() {
        assert_eq!(index_detail(true, "pk"), "PRIMARY KEY");
        assert_eq!(index_detail(true, "u"), "UNIQUE");
        assert_eq!(index_detail(true, "c"), "UNIQUE");
        assert_eq!(index_detail(false, "c"), "");
    }

    #[test]
    fn a_foreign_key_without_a_named_target_column_still_reads() {
        assert_eq!(
            foreign_key_detail("user_id", "users", Some("id")),
            "user_id → users(id)"
        );
        assert_eq!(
            foreign_key_detail("user_id", "users", None),
            "user_id → users"
        );
    }

    #[test]
    fn every_storage_class_maps_to_its_own_value_kind() {
        assert_eq!(cell(ValueRef::Null), Value::Null);
        assert_eq!(cell(ValueRef::Integer(7)), Value::Int(7));
        assert_eq!(cell(ValueRef::Real(1.5)), Value::Float(1.5));
        assert_eq!(cell(ValueRef::Text(b"hi")), Value::Text("hi".into()));
        assert_eq!(cell(ValueRef::Blob(&[9])), Value::Bytes(vec![9]));
    }

    #[test]
    fn a_node_id_survives_an_identifier_full_of_punctuation() {
        let awkward = "my.odd:table name";
        let built = id::relation("t", awkward);
        let (tag, parts) = id::parse(&built).expect("parses");
        assert_eq!(tag, "t");
        assert_eq!(parts, [awkward]);
    }
}
