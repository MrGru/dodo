//! MySQL and MariaDB through their shared blocking wire protocol.
//!
//! One `Conn` is serialized behind a mutex. Cancellation opens a second
//! connection and sends `KILL QUERY <connection_id>`; error 1317 from the
//! running connection is the evidence that the server stopped it. Query rows
//! use the text protocol and are handed to `RowSink` one at a time.
//!
//! Safe writes request `CLIENT_FOUND_ROWS` at connect time: Commit checks for
//! exactly one *matched* row, so assigning an unchanged value must not be
//! misreported as zero changed rows. Column origins plus
//! `information_schema.STATISTICS`/`COLUMNS` prove the identity before
//! `models::statement` can generate anything.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mysql::consts::{CapabilityFlags, ColumnFlags, ColumnType};
use mysql::prelude::Queryable as _;
use mysql::{
    Column, Conn, Opts, OptsBuilder, Params, Row as MyRow, SslOpts, TxOpts, Value as MyValue,
};

use crate::models::catalog::{CatalogNode, GroupLabel, NodeId, NodeKind};
use crate::models::connection::{ConnectionProfile, SslMode};
use crate::models::detail::{DATA_PAGE_SIZE, DdlSource, DetailField, DetailRequest, DetailTab};
use crate::models::error::DbError;
use crate::models::identity::{
    Editability, IdentityMetadata, ReadOnlyReason, TableRef, UniqueKey, prove,
};
use crate::models::page::{Flow, RowSink};
use crate::models::query::{Execution, QueryRequest};
use crate::models::statement::{Dialect, GeneratedBatch};
use crate::models::value::{ColumnMeta, ColumnOrigin, Value};
use crate::services::{CancelHandle, Capabilities, DetailResult, Driver, MutationFailure};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const ER_QUERY_INTERRUPTED: u16 = 1317;

pub struct MySqlDriver {
    connection: Mutex<Conn>,
    cancel_opts: Opts,
    connection_id: u32,
    database: String,
}

pub fn connect(profile: &ConnectionProfile) -> Result<Arc<MySqlDriver>, DbError> {
    let plain = options(profile, false);
    let tls = options(profile, true);
    let (connection, cancel_opts) = match profile.ssl_mode {
        SslMode::Disable => Conn::new(plain.clone()).map(|conn| (conn, plain)),
        SslMode::Require => Conn::new(tls.clone()).map(|conn| (conn, tls)),
        SslMode::Prefer => match Conn::new(tls.clone()) {
            Ok(conn) => Ok((conn, tls)),
            Err(mysql::Error::DriverError(mysql::DriverError::TlsNotSupported)) => {
                Conn::new(plain.clone()).map(|conn| (conn, plain))
            }
            Err(error) => Err(error),
        },
    }
    .map_err(unreachable)?;

    Ok(Arc::new(MySqlDriver {
        connection_id: connection.connection_id(),
        connection: Mutex::new(connection),
        cancel_opts,
        database: profile.database.trim().to_string(),
    }))
}

fn options(profile: &ConnectionProfile, tls: bool) -> Opts {
    let mut builder = OptsBuilder::new()
        .ip_or_hostname(Some(profile.host.trim()))
        .tcp_port(profile.port)
        .db_name(Some(profile.database.trim()))
        .tcp_connect_timeout(Some(CONNECT_TIMEOUT))
        .prefer_socket(false)
        // UPDATE must report matched rows, not only changed rows: assigning an
        // unchanged value is still one safely identified row, not a false zero.
        .additional_capabilities(CapabilityFlags::CLIENT_FOUND_ROWS);
    if !profile.user.trim().is_empty() {
        builder = builder.user(Some(profile.user.trim()));
    }
    if !profile.password.is_empty() {
        builder = builder.pass(Some(profile.password.as_str()));
    }
    if tls {
        builder = builder.ssl_opts(Some(SslOpts::default()));
    }
    Opts::from(builder)
}

fn unreachable(error: mysql::Error) -> DbError {
    DbError::Unreachable(error.to_string())
}

fn server_error(error: mysql::Error) -> DbError {
    match error {
        mysql::Error::MySqlError(error) if error.code == ER_QUERY_INTERRUPTED => DbError::Cancelled,
        mysql::Error::MySqlError(error) => DbError::Server {
            code: Some(error.code.to_string()),
            detail: error.message,
        },
        other => DbError::Server {
            code: None,
            detail: other.to_string(),
        },
    }
}

mod id {
    pub const SEP: char = '\u{1f}';
    pub const DATABASE: &str = "db";

    pub fn group(prefix: &str) -> String {
        prefix.to_string()
    }
    pub fn relation(prefix: &str, name: &str) -> String {
        format!("{prefix}{SEP}{name}")
    }
    pub fn relation_group(prefix: &str, name: &str) -> String {
        format!("{prefix}{SEP}{name}")
    }
    pub fn leaf(prefix: &str, relation: &str, name: &str) -> String {
        format!("{prefix}{SEP}{relation}{SEP}{name}")
    }
    pub fn parse(id: &str) -> Option<(&str, Vec<&str>)> {
        let mut parts = id.split(SEP);
        Some((parts.next()?, parts.collect()))
    }
}

impl MySqlDriver {
    fn with_connection<T>(
        &self,
        f: impl FnOnce(&mut Conn) -> Result<T, mysql::Error>,
    ) -> Result<T, DbError> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| DbError::Unreachable("the connection was poisoned by a panic".into()))?;
        f(&mut connection).map_err(server_error)
    }

    fn identity_metadata(&self, origin: &ColumnOrigin) -> Result<IdentityMetadata, DbError> {
        self.with_connection(|connection| {
            let schema = origin.schema.as_deref().unwrap_or(&self.database);
            let rows: Vec<(String, u64, Option<String>, String)> = connection.exec(
                "SELECT s.INDEX_NAME, s.SEQ_IN_INDEX, s.COLUMN_NAME, c.IS_NULLABLE \
                 FROM information_schema.STATISTICS s \
                 JOIN information_schema.COLUMNS c \
                   ON c.TABLE_SCHEMA = s.TABLE_SCHEMA \
                  AND c.TABLE_NAME = s.TABLE_NAME \
                  AND c.COLUMN_NAME = s.COLUMN_NAME \
                 WHERE s.TABLE_SCHEMA = ? AND s.TABLE_NAME = ? AND s.NON_UNIQUE = 0 \
                 ORDER BY (s.INDEX_NAME = 'PRIMARY') DESC, s.INDEX_NAME, s.SEQ_IN_INDEX",
                (schema, &origin.table),
            )?;
            let mut grouped: std::collections::BTreeMap<String, Vec<(u64, Option<String>, bool)>> =
                std::collections::BTreeMap::new();
            for (index, ordinal, column, nullable) in rows {
                grouped.entry(index).or_default().push((
                    ordinal,
                    column,
                    nullable.eq_ignore_ascii_case("NO"),
                ));
            }
            let mut metadata = IdentityMetadata::new(TableRef {
                schema: origin.schema.clone(),
                table: origin.table.clone(),
            });
            for (name, mut columns) in grouped {
                columns.sort_by_key(|(ordinal, _, _)| *ordinal);
                let Some(names) = columns
                    .iter()
                    .map(|(_, name, _)| name.clone())
                    .collect::<Option<Vec<_>>>()
                else {
                    continue;
                };
                metadata.keys.push(UniqueKey {
                    primary: name == "PRIMARY",
                    all_non_null: name == "PRIMARY"
                        || columns.iter().all(|(_, _, not_null)| *not_null),
                    columns: names,
                });
            }
            metadata.keys.sort_by_key(|key| !key.primary);

            let generated: Vec<String> = connection.exec_map(
                "SELECT COLUMN_NAME FROM information_schema.COLUMNS \
                 WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? \
                   AND (EXTRA LIKE '%auto_increment%' \
                        OR EXTRA LIKE '%GENERATED%' \
                        OR GENERATION_EXPRESSION <> '' \
                        OR COLUMN_DEFAULT IS NOT NULL)",
                (schema, &origin.table),
                |name: String| name,
            )?;
            metadata.generated_columns = generated.into_iter().collect();
            Ok(metadata)
        })
    }

    fn relations(
        &self,
        table_type: &str,
        prefix: &str,
        kind: NodeKind,
    ) -> Result<Vec<CatalogNode>, DbError> {
        let sql = "SELECT TABLE_NAME FROM information_schema.TABLES \
                   WHERE TABLE_SCHEMA = ? AND TABLE_TYPE = ? ORDER BY TABLE_NAME";
        let names: Vec<String> = self.with_connection(|conn| {
            conn.exec_map(sql, (&self.database, table_type), |name: String| name)
        })?;
        Ok(names
            .into_iter()
            .map(|name| CatalogNode::branch(id::relation(prefix, &name), kind, name))
            .collect())
    }

    fn columns(&self, relation: &str) -> Result<Vec<CatalogNode>, DbError> {
        let rows: Vec<(String, String, String)> = self.with_connection(|conn| {
            conn.exec(
                "SELECT COLUMN_NAME, COLUMN_TYPE, IS_NULLABLE \
                 FROM information_schema.COLUMNS \
                 WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? ORDER BY ORDINAL_POSITION",
                (&self.database, relation),
            )
        })?;
        Ok(rows
            .into_iter()
            .map(|(name, type_name, nullable)| {
                let detail = if nullable == "NO" {
                    format!("{type_name} NOT NULL")
                } else {
                    type_name
                };
                CatalogNode::leaf(id::leaf("c", relation, &name), NodeKind::Column, name)
                    .with_detail(detail)
            })
            .collect())
    }

    fn indexes(&self, relation: &str) -> Result<Vec<CatalogNode>, DbError> {
        let rows: Vec<(String, u8)> = self.with_connection(|conn| {
            conn.exec(
                "SELECT INDEX_NAME, MIN(NON_UNIQUE) FROM information_schema.STATISTICS \
                 WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? \
                 GROUP BY INDEX_NAME ORDER BY INDEX_NAME",
                (&self.database, relation),
            )
        })?;
        Ok(rows
            .into_iter()
            .map(|(name, non_unique)| {
                let detail = if name == "PRIMARY" {
                    "PRIMARY KEY"
                } else if non_unique == 0 {
                    "UNIQUE"
                } else {
                    ""
                };
                CatalogNode::leaf(id::leaf("i", relation, &name), NodeKind::Index, name)
                    .with_detail(detail)
            })
            .collect())
    }

    fn constraints(&self, relation: &str) -> Result<Vec<CatalogNode>, DbError> {
        let rows: Vec<(String, String)> = self.with_connection(|conn| {
            conn.exec(
                "SELECT CONSTRAINT_NAME, CONSTRAINT_TYPE \
                 FROM information_schema.TABLE_CONSTRAINTS \
                 WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? ORDER BY CONSTRAINT_NAME",
                (&self.database, relation),
            )
        })?;
        Ok(rows
            .into_iter()
            .map(|(name, kind)| {
                CatalogNode::leaf(id::leaf("k", relation, &name), NodeKind::Constraint, name)
                    .with_detail(kind)
            })
            .collect())
    }

    fn detail_columns(
        &self,
        relation: &str,
        sink: &mut dyn RowSink,
    ) -> Result<DetailResult, DbError> {
        let rows: Vec<(String, String, String, Option<String>)> = self.with_connection(|conn| {
            conn.exec(
                "SELECT COLUMN_NAME, COLUMN_TYPE, IS_NULLABLE, COLUMN_DEFAULT \
                 FROM information_schema.COLUMNS \
                 WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? ORDER BY ORDINAL_POSITION",
                (&self.database, relation),
            )
        })?;
        Ok(metadata_rows(
            vec![
                DetailField::Name,
                DetailField::Type,
                DetailField::Nullable,
                DetailField::Default,
            ],
            rows.into_iter()
                .map(|(name, kind, nullable, default)| {
                    vec![
                        Value::Text(name),
                        Value::Text(kind),
                        Value::Bool(nullable == "YES"),
                        default.map(Value::Text).unwrap_or(Value::Null),
                    ]
                })
                .collect(),
            sink,
        ))
    }

    fn detail_indexes(
        &self,
        relation: &str,
        sink: &mut dyn RowSink,
    ) -> Result<DetailResult, DbError> {
        let rows: Vec<(String, u8, String)> = self.with_connection(|conn| {
            conn.exec(
                "SELECT INDEX_NAME, MIN(NON_UNIQUE), \
                 GROUP_CONCAT(COLUMN_NAME ORDER BY SEQ_IN_INDEX SEPARATOR ', ') \
                 FROM information_schema.STATISTICS \
                 WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? \
                 GROUP BY INDEX_NAME ORDER BY INDEX_NAME",
                (&self.database, relation),
            )
        })?;
        Ok(metadata_rows(
            vec![
                DetailField::Name,
                DetailField::Unique,
                DetailField::Primary,
                DetailField::Definition,
            ],
            rows.into_iter()
                .map(|(name, non_unique, columns)| {
                    let primary = name == "PRIMARY";
                    let definition = format!(
                        "{}INDEX {} ({columns})",
                        if non_unique == 0 { "UNIQUE " } else { "" },
                        quote_identifier(&name)
                    );
                    vec![
                        Value::Text(name),
                        Value::Bool(non_unique == 0),
                        Value::Bool(primary),
                        Value::Text(definition),
                    ]
                })
                .collect(),
            sink,
        ))
    }

    fn detail_constraints(
        &self,
        relation: &str,
        sink: &mut dyn RowSink,
    ) -> Result<DetailResult, DbError> {
        let rows: Vec<(String, String)> = self.with_connection(|conn| {
            conn.exec(
                "SELECT CONSTRAINT_NAME, CONSTRAINT_TYPE \
                 FROM information_schema.TABLE_CONSTRAINTS \
                 WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? ORDER BY CONSTRAINT_NAME",
                (&self.database, relation),
            )
        })?;
        Ok(metadata_rows(
            vec![DetailField::Name, DetailField::Type],
            rows.into_iter()
                .map(|(name, definition)| vec![Value::Text(name), Value::Text(definition)])
                .collect(),
            sink,
        ))
    }

    fn ddl(&self, relation: &str, view: bool) -> Result<DetailResult, DbError> {
        let statement = format!(
            "SHOW CREATE {} {}.{}",
            if view { "VIEW" } else { "TABLE" },
            quote_identifier(&self.database),
            quote_identifier(relation)
        );
        let ddl = self.with_connection(|conn| {
            let mut result = conn.query_iter(statement)?;
            let row = result.next().transpose()?;
            Ok(row
                .and_then(|row| row.unwrap().into_iter().nth(1))
                .and_then(text_value))
        })?;
        Ok(ddl
            .map(DetailResult::Ddl)
            .unwrap_or(DetailResult::Unavailable))
    }
}

impl Driver for MySqlDriver {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            editor_language: "sql",
            cancel: true,
            explain: true,
            detail: true,
            ddl: DdlSource::Server,
            mutation: Some(crate::models::statement::Dialect::MySql),
        }
    }

    fn ping(&self) -> Result<(), DbError> {
        self.with_connection(|conn| conn.query_drop("SELECT 1"))
    }

    fn cancel_handle(&self) -> Option<CancelHandle> {
        let opts = self.cancel_opts.clone();
        let connection_id = self.connection_id;
        Some(CancelHandle::new(move || {
            let mut cancel = Conn::new(opts.clone()).map_err(unreachable)?;
            cancel
                .query_drop(format!("KILL QUERY {connection_id}"))
                .map_err(server_error)
        }))
    }

    fn explain_statement(&self, statement: &str) -> Option<String> {
        Some(format!("EXPLAIN {statement}"))
    }

    fn children(&self, parent: Option<&NodeId>) -> Result<Vec<CatalogNode>, DbError> {
        let Some(parent) = parent else {
            return Ok(vec![CatalogNode::branch(
                id::DATABASE,
                NodeKind::Database,
                self.database.clone(),
            )]);
        };
        let Some((tag, parts)) = id::parse(parent.as_str()) else {
            return Ok(Vec::new());
        };
        match (tag, parts.as_slice()) {
            (id::DATABASE, _) => Ok(vec![
                CatalogNode::group(id::group("gt"), GroupLabel::Tables),
                CatalogNode::group(id::group("gv"), GroupLabel::Views),
            ]),
            ("gt", _) => self.relations("BASE TABLE", "t", NodeKind::Table),
            ("gv", _) => self.relations("VIEW", "v", NodeKind::View),
            ("t", [table]) => Ok(vec![
                CatalogNode::group(id::relation_group("gc", table), GroupLabel::Columns),
                CatalogNode::group(id::relation_group("gi", table), GroupLabel::Indexes),
                CatalogNode::group(id::relation_group("gk", table), GroupLabel::Constraints),
            ]),
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

    fn detail(
        &self,
        request: &DetailRequest,
        sink: &mut dyn RowSink,
    ) -> Result<DetailResult, DbError> {
        if !request.tab.applies_to(request.target.kind) {
            return Ok(DetailResult::Unavailable);
        }
        let Some((tag, parts)) = id::parse(request.target.node.as_str()) else {
            return Ok(DetailResult::Unavailable);
        };
        let (relation, is_table) = match (tag, parts.as_slice()) {
            ("t", [relation]) => (*relation, true),
            ("v", [relation]) => (*relation, false),
            _ => return Ok(DetailResult::Unavailable),
        };
        match request.tab {
            DetailTab::Data => {
                let statement = format!(
                    "SELECT * FROM {}.{} LIMIT {} OFFSET {}",
                    quote_identifier(&self.database),
                    quote_identifier(relation),
                    DATA_PAGE_SIZE + 1,
                    request.offset
                );
                let execution = self.execute(&QueryRequest::new(statement), sink)?;
                Ok(DetailResult::Rows {
                    fields: None,
                    truncated: execution.truncated,
                    notice: None,
                })
            }
            DetailTab::Columns => self.detail_columns(relation, sink),
            DetailTab::Indexes if is_table => self.detail_indexes(relation, sink),
            DetailTab::Constraints if is_table => self.detail_constraints(relation, sink),
            DetailTab::Ddl => self.ddl(relation, !is_table),
            DetailTab::Indexes | DetailTab::Constraints => Ok(DetailResult::Unavailable),
        }
    }

    fn execute(
        &self,
        request: &QueryRequest,
        sink: &mut dyn RowSink,
    ) -> Result<Execution, DbError> {
        let started = Instant::now();
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| DbError::Unreachable("the connection was poisoned by a panic".into()))?;
        let mut result = connection
            .query_iter(request.statement.as_str())
            .map_err(server_error)?;
        let columns = result.columns().as_ref().to_vec();
        if columns.is_empty() {
            let affected = result.affected_rows();
            return Ok(Execution {
                rows_affected: Some(affected),
                truncated: false,
                elapsed: started.elapsed(),
            });
        }

        sink.columns(describe(&columns));
        let mut truncated = false;
        for row in result.by_ref() {
            let values = decode_row(row.map_err(server_error)?, &columns);
            if sink.row(values) == Flow::Stop {
                truncated = true;
                break;
            }
        }
        drop(result);
        Ok(Execution {
            rows_affected: None,
            truncated,
            elapsed: started.elapsed(),
        })
    }

    fn editability(&self, columns: &[ColumnMeta]) -> Editability {
        let Some(first) = columns.first() else {
            return Editability::ReadOnly(ReadOnlyReason::NoColumns);
        };
        let Some(origin) = first.origin.as_ref() else {
            return Editability::ReadOnly(ReadOnlyReason::MissingOrigin(first.name.clone()));
        };
        match self.identity_metadata(origin) {
            Ok(metadata) => prove(columns, metadata),
            Err(error) => Editability::ReadOnly(ReadOnlyReason::Metadata(error.detail().into())),
        }
    }

    fn commit(&self, batch: &GeneratedBatch) -> Result<(), MutationFailure> {
        if batch.dialect != Dialect::MySql {
            return Err(MutationFailure::Transaction(DbError::Server {
                code: None,
                detail: "generated mutation dialect did not match MySQL".into(),
            }));
        }
        let mut connection = self.connection.lock().map_err(|_| {
            MutationFailure::Transaction(DbError::Unreachable(
                "the connection was poisoned by a panic".into(),
            ))
        })?;
        let mut transaction = connection
            .start_transaction(TxOpts::default())
            .map_err(|error| MutationFailure::Transaction(server_error(error)))?;

        for (index, statement) in batch.statements.iter().enumerate() {
            let params = Params::Positional(statement.params.iter().map(my_value).collect());
            if let Err(error) = transaction.exec_drop(&statement.sql, params) {
                let failure = MutationFailure::Statement {
                    index,
                    sql: statement.sql.clone(),
                    error: server_error(error),
                };
                let _ = transaction.rollback();
                return Err(failure);
            }
            let affected = transaction.affected_rows();
            if affected != 1 {
                let failure = MutationFailure::Affected {
                    index,
                    sql: statement.sql.clone(),
                    actual: affected,
                };
                let _ = transaction.rollback();
                return Err(failure);
            }
        }
        transaction
            .commit()
            .map_err(|error| MutationFailure::Transaction(server_error(error)))
    }
}

fn my_value(value: &Value) -> MyValue {
    match value {
        Value::Null => MyValue::NULL,
        Value::Bool(value) => MyValue::Int(i64::from(*value)),
        Value::Int(value) => MyValue::Int(*value),
        Value::Float(value) => MyValue::Double(*value),
        Value::Text(value) | Value::Json(value) => MyValue::Bytes(value.as_bytes().to_vec()),
        Value::Bytes(value) => MyValue::Bytes(value.clone()),
        Value::Truncated { .. } => unreachable!("statement generation rejects truncation"),
    }
}

fn describe(columns: &[Column]) -> Vec<ColumnMeta> {
    columns
        .iter()
        .map(|column| {
            let mut meta = ColumnMeta::new(column.name_str(), type_name(column.column_type()));
            let table = column.org_table_str();
            let name = column.org_name_str();
            if !table.is_empty() && !name.is_empty() {
                meta = meta.with_origin(ColumnOrigin {
                    schema: (!column.schema_ref().is_empty())
                        .then(|| column.schema_str().into_owned()),
                    table: table.into_owned(),
                    column: name.into_owned(),
                });
            }
            meta
        })
        .collect()
}

fn decode_row(row: MyRow, columns: &[Column]) -> Vec<Value> {
    row.unwrap()
        .into_iter()
        .zip(columns)
        .map(|(value, column)| decode(value, column))
        .collect()
}

fn decode(value: MyValue, column: &Column) -> Value {
    match value {
        MyValue::NULL => Value::Null,
        MyValue::Int(value) => Value::Int(value),
        MyValue::UInt(value) => i64::try_from(value)
            .map(Value::Int)
            .unwrap_or_else(|_| Value::Text(value.to_string())),
        MyValue::Float(value) => Value::Float(f64::from(value)),
        MyValue::Double(value) => Value::Float(value),
        MyValue::Date(year, month, day, hour, minute, second, micros) => Value::Text(format!(
            "{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}{}",
            fraction(micros)
        )),
        MyValue::Time(negative, days, hours, minutes, seconds, micros) => Value::Text(format!(
            "{}{}:{minutes:02}:{seconds:02}{}",
            if negative { "-" } else { "" },
            u64::from(days) * 24 + u64::from(hours),
            fraction(micros)
        )),
        MyValue::Bytes(bytes) => decode_bytes(bytes, column),
    }
}

fn decode_bytes(bytes: Vec<u8>, column: &Column) -> Value {
    use ColumnType::*;
    match column.column_type() {
        MYSQL_TYPE_TINY | MYSQL_TYPE_SHORT | MYSQL_TYPE_LONG | MYSQL_TYPE_LONGLONG
        | MYSQL_TYPE_INT24 | MYSQL_TYPE_YEAR => std::str::from_utf8(&bytes)
            .ok()
            .and_then(|text| text.parse::<i64>().ok())
            .map(Value::Int)
            .unwrap_or_else(|| Value::Text(String::from_utf8_lossy(&bytes).into_owned())),
        MYSQL_TYPE_FLOAT | MYSQL_TYPE_DOUBLE => std::str::from_utf8(&bytes)
            .ok()
            .and_then(|text| text.parse::<f64>().ok())
            .map(Value::Float)
            .unwrap_or_else(|| Value::Text(String::from_utf8_lossy(&bytes).into_owned())),
        MYSQL_TYPE_JSON => match String::from_utf8(bytes) {
            Ok(text) => Value::Json(text),
            Err(error) => Value::Bytes(error.into_bytes()),
        },
        _ if column.flags().contains(ColumnFlags::BINARY_FLAG) => Value::Bytes(bytes),
        _ => Value::Text(String::from_utf8_lossy(&bytes).into_owned()),
    }
}

fn text_value(value: MyValue) -> Option<String> {
    match value {
        MyValue::Bytes(bytes) => Some(String::from_utf8_lossy(&bytes).into_owned()),
        MyValue::NULL => None,
        other => Some(format!("{other:?}")),
    }
}

fn type_name(kind: ColumnType) -> String {
    format!("{kind:?}")
        .strip_prefix("MYSQL_TYPE_")
        .unwrap_or("UNKNOWN")
        .to_string()
}

fn fraction(micros: u32) -> String {
    if micros == 0 {
        String::new()
    } else {
        let mut fraction = format!(".{micros:06}");
        while fraction.ends_with('0') {
            fraction.pop();
        }
        fraction
    }
}

fn quote_identifier(identifier: &str) -> String {
    format!("`{}`", identifier.replace('`', "``"))
}

fn metadata_rows(
    fields: Vec<DetailField>,
    rows: Vec<Vec<Value>>,
    sink: &mut dyn RowSink,
) -> DetailResult {
    sink.columns(
        fields
            .iter()
            .enumerate()
            .map(|(index, _)| ColumnMeta::new(index.to_string(), ""))
            .collect(),
    );
    let mut truncated = false;
    for row in rows {
        if sink.row(row) == Flow::Stop {
            truncated = true;
            break;
        }
    }
    DetailResult::Rows {
        fields: Some(fields),
        truncated,
        notice: None,
    }
}

#[cfg(test)]
mod tests {
    use super::{fraction, quote_identifier, type_name};
    use mysql::consts::ColumnType;

    #[test]
    fn identifiers_escape_backticks_and_protocol_types_are_readable() {
        assert_eq!(quote_identifier("odd`name"), "`odd``name`");
        assert_eq!(type_name(ColumnType::MYSQL_TYPE_LONGLONG), "LONGLONG");
    }

    #[test]
    fn fractional_seconds_drop_only_trailing_zeroes() {
        assert_eq!(fraction(0), "");
        assert_eq!(fraction(120_000), ".12");
        assert_eq!(fraction(123_456), ".123456");
    }
}

/// Live coverage shared by MySQL and MariaDB. It skips unless explicitly
/// pointed at a throwaway server; run the same test once per image.
#[cfg(test)]
mod live {
    use super::{MySqlDriver, connect, id, quote_identifier};
    use crate::models::catalog::{NodeId, NodeKind, NodeLabel};
    use crate::models::connection::{ConnectionProfile, SslMode};
    use crate::models::detail::{DetailRequest, DetailTab, DetailTarget};
    use crate::models::engine::Engine;
    use crate::models::error::DbError;
    use crate::models::identity::{Editability, ReadOnlyReason};
    use crate::models::page::PageBuffer;
    use crate::models::query::QueryRequest;
    use crate::models::statement::{Dialect, generate};
    use crate::models::value::Value;
    use crate::services::Driver;
    use crate::state::detail::{DetailLoad, load as load_detail};
    use crate::state::edit::PendingGrid;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn profile() -> Option<ConnectionProfile> {
        let host = std::env::var("DODO_MYSQL_TEST_HOST").ok()?;
        let mut profile = ConnectionProfile::new(1, Engine::MySql);
        profile.host = host;
        profile.port = std::env::var("DODO_MYSQL_TEST_PORT")
            .ok()
            .and_then(|port| port.parse().ok())
            .unwrap_or(3306);
        profile.user = std::env::var("DODO_MYSQL_TEST_USER").unwrap_or_else(|_| "root".into());
        profile.password = std::env::var("DODO_MYSQL_TEST_PASSWORD").unwrap_or_default();
        profile.database = std::env::var("DODO_MYSQL_TEST_DB").unwrap_or_else(|_| "test".into());
        profile.ssl_mode = SslMode::Disable;
        Some(profile)
    }

    fn execute(driver: &MySqlDriver, statement: impl Into<String>) -> Result<PageBuffer, DbError> {
        let mut sink = PageBuffer::default();
        driver.execute(&QueryRequest::new(statement), &mut sink)?;
        Ok(sink)
    }

    struct Fixture {
        driver: Arc<MySqlDriver>,
        table: String,
        view: String,
        other: String,
        nullable: String,
    }

    impl Fixture {
        fn new() -> Option<Self> {
            let driver = connect(&profile()?).expect("connects to configured MySQL/MariaDB");
            static COUNTER: AtomicU64 = AtomicU64::new(0);
            let table = format!(
                "dodo_r5_{}_{}",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::Relaxed)
            );
            let view = format!("{table}_view");
            let other = format!("{table}_other");
            let nullable = format!("{table}_nullable");
            let _ = execute(
                driver.as_ref(),
                format!(
                    "DROP VIEW IF EXISTS {}; DROP TABLE IF EXISTS {}, {}, {}",
                    quote_identifier(&view),
                    quote_identifier(&table),
                    quote_identifier(&other),
                    quote_identifier(&nullable)
                ),
            );
            execute(
                driver.as_ref(),
                format!(
                    "CREATE TABLE {} (id BIGINT PRIMARY KEY, name VARCHAR(50) NOT NULL, payload JSON); \
                     INSERT INTO {} VALUES (1, 'ada', '{{\"ok\":true}}'), (2, 'grace', NULL); \
                     CREATE VIEW {} AS SELECT id, name FROM {}; \
                     CREATE TABLE {} (id BIGINT PRIMARY KEY, name VARCHAR(50)); \
                     INSERT INTO {} VALUES (1, 'admin'); \
                     CREATE TABLE {} (email VARCHAR(50) UNIQUE NULL, name VARCHAR(50))",
                    quote_identifier(&table),
                    quote_identifier(&table),
                    quote_identifier(&view),
                    quote_identifier(&table),
                    quote_identifier(&other),
                    quote_identifier(&other),
                    quote_identifier(&nullable),
                ),
            )
            .expect("seeds live fixture");
            Some(Self {
                driver,
                table,
                view,
                other,
                nullable,
            })
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = execute(
                self.driver.as_ref(),
                format!(
                    "DROP VIEW IF EXISTS {}; DROP TABLE IF EXISTS {}, {}, {}",
                    quote_identifier(&self.view),
                    quote_identifier(&self.table),
                    quote_identifier(&self.other),
                    quote_identifier(&self.nullable)
                ),
            );
        }
    }

    #[test]
    fn mysql_and_mariadb_catalog_query_detail_origin_ddl_and_cancel() {
        let Some(fixture) = Fixture::new() else {
            return;
        };
        assert!(fixture.driver.ping().is_ok());

        let tables = fixture
            .driver
            .children(Some(&NodeId::new(id::group("gt"))))
            .expect("tables load");
        let table = tables
            .iter()
            .find(|node| node.label == NodeLabel::Name(fixture.table.clone()))
            .expect("fixture table is listed");
        assert_eq!(table.kind, NodeKind::Table);
        let columns = fixture
            .driver
            .children(Some(&NodeId::new(id::relation_group("gc", &fixture.table))))
            .expect("columns load");
        assert!(columns.iter().any(|column| {
            column.label == NodeLabel::Name("name".into())
                && column.detail.as_deref() == Some("varchar(50) NOT NULL")
        }));

        let sink = execute(
            fixture.driver.as_ref(),
            format!(
                "SELECT id, name, payload FROM {} ORDER BY id",
                quote_identifier(&fixture.table)
            ),
        )
        .expect("query runs");
        assert_eq!(sink.rows().len(), 2);
        assert_eq!(sink.rows()[0][0], Value::Int(1));
        assert!(
            sink.columns()[0]
                .origin
                .as_ref()
                .is_some_and(|origin| { origin.table == fixture.table && origin.column == "id" })
        );

        let target = DetailTarget::new(table.id.clone(), NodeKind::Table, fixture.table.clone());
        let DetailLoad::Grid(data) = load_detail(
            fixture.driver.as_ref(),
            &DetailRequest::new(target.clone(), DetailTab::Data, 0),
        ) else {
            panic!("table data did not load")
        };
        assert_eq!(data.grid.rows().len(), 2);
        let DetailLoad::Grid(detail_columns) = load_detail(
            fixture.driver.as_ref(),
            &DetailRequest::new(target.clone(), DetailTab::Columns, 0),
        ) else {
            panic!("column metadata did not load")
        };
        assert_eq!(detail_columns.grid.rows().len(), 3);
        let DetailLoad::Grid(indexes) = load_detail(
            fixture.driver.as_ref(),
            &DetailRequest::new(target.clone(), DetailTab::Indexes, 0),
        ) else {
            panic!("index metadata did not load")
        };
        assert!(
            indexes
                .grid
                .rows()
                .iter()
                .any(|row| row[0] == Value::Text("PRIMARY".into()))
        );
        let DetailLoad::Grid(constraints) = load_detail(
            fixture.driver.as_ref(),
            &DetailRequest::new(target.clone(), DetailTab::Constraints, 0),
        ) else {
            panic!("constraint metadata did not load")
        };
        assert!(
            constraints
                .grid
                .rows()
                .iter()
                .any(|row| { row[1] == Value::Text("PRIMARY KEY".into()) })
        );
        let DetailLoad::Ddl(ddl) = load_detail(
            fixture.driver.as_ref(),
            &DetailRequest::new(target, DetailTab::Ddl, 0),
        ) else {
            panic!("server DDL did not load")
        };
        assert!(ddl.to_ascii_uppercase().contains("CREATE TABLE"));

        let cancel = fixture.driver.cancel_handle().expect("cancel is supported");
        let driver = fixture.driver.clone();
        let running = std::thread::spawn(move || {
            execute(
                driver.as_ref(),
                "SELECT COUNT(*) FROM information_schema.COLUMNS a \
                 CROSS JOIN information_schema.COLUMNS b \
                 CROSS JOIN information_schema.COLUMNS c \
                 CROSS JOIN information_schema.COLUMNS d",
            )
        });
        std::thread::sleep(std::time::Duration::from_millis(300));
        cancel.cancel().expect("KILL QUERY is delivered");
        let cancelled = running.join().expect("query thread");
        assert!(
            matches!(cancelled, Err(DbError::Cancelled)),
            "server returned {cancelled:?}"
        );
    }

    #[test]
    fn mysql_and_mariadb_safe_mutations_are_atomic_and_use_matched_rows() {
        let Some(fixture) = Fixture::new() else {
            return;
        };
        let page = execute(
            fixture.driver.as_ref(),
            format!(
                "SELECT id, name FROM {} ORDER BY id",
                quote_identifier(&fixture.table)
            ),
        )
        .unwrap();
        let editable = fixture.driver.editability(page.columns());
        assert!(matches!(editable, Editability::Editable(_)));

        let join = execute(
            fixture.driver.as_ref(),
            format!(
                "SELECT a.id, b.name FROM {} a JOIN {} b ON b.id = a.id",
                quote_identifier(&fixture.table),
                quote_identifier(&fixture.other)
            ),
        )
        .unwrap();
        assert!(matches!(
            fixture.driver.editability(join.columns()),
            Editability::ReadOnly(ReadOnlyReason::MultipleTables)
        ));
        let missing = execute(
            fixture.driver.as_ref(),
            format!("SELECT name FROM {}", quote_identifier(&fixture.table)),
        )
        .unwrap();
        assert!(matches!(
            fixture.driver.editability(missing.columns()),
            Editability::ReadOnly(ReadOnlyReason::MissingIdentityColumns { .. })
        ));
        let nullable = execute(
            fixture.driver.as_ref(),
            format!(
                "SELECT email, name FROM {}",
                quote_identifier(&fixture.nullable)
            ),
        )
        .unwrap();
        assert!(matches!(
            fixture.driver.editability(nullable.columns()),
            Editability::ReadOnly(ReadOnlyReason::NoUniqueIdentity(_))
        ));
        let union = execute(
            fixture.driver.as_ref(),
            format!(
                "SELECT id, name FROM {} UNION SELECT id, name FROM {}",
                quote_identifier(&fixture.table),
                quote_identifier(&fixture.table)
            ),
        )
        .unwrap();
        assert!(matches!(
            fixture.driver.editability(union.columns()),
            Editability::ReadOnly(ReadOnlyReason::MissingOrigin(_))
        ));

        // CLIENT_FOUND_ROWS makes this unchanged assignment report one matched
        // row instead of zero changed rows.
        let mut unchanged = PendingGrid::new(page.rows().to_vec(), editable.clone());
        unchanged.edit(0, 1, Value::Text("ada".into())).unwrap();
        // Force an UPDATE even though the displayed value decodes identically.
        let batch = crate::models::statement::generate(
            unchanged.source().unwrap(),
            &[crate::models::statement::Mutation::Update {
                identity: vec![Value::Int(1)],
                values: vec![(1, Value::Text("ada".into()))],
            }],
            Dialect::MySql,
        )
        .unwrap();
        fixture.driver.commit(&batch).unwrap();

        let mut pending = PendingGrid::new(page.rows().to_vec(), editable);
        pending
            .edit(0, 1, Value::Text("Ada Lovelace".into()))
            .unwrap();
        pending.delete(1).unwrap();
        let mut duplicate = pending.duplicate_template(0).unwrap();
        duplicate[0] = Value::Int(3);
        duplicate[1] = Value::Text("Ada Copy".into());
        pending.insert(duplicate).unwrap();
        let mut added = pending.add_template().unwrap();
        added[0] = Value::Int(4);
        added[1] = Value::Text("Katherine".into());
        pending.insert(added).unwrap();
        let batch = generate(
            pending.source().unwrap(),
            &pending.mutations(),
            Dialect::MySql,
        )
        .unwrap();
        fixture.driver.commit(&batch).unwrap();

        let page = execute(
            fixture.driver.as_ref(),
            format!(
                "SELECT id, name FROM {} ORDER BY id",
                quote_identifier(&fixture.table)
            ),
        )
        .unwrap();
        assert_eq!(page.rows().len(), 3);

        // The duplicate primary key fails after the first UPDATE; the UPDATE
        // must not survive the transaction rollback.
        let mut atomic = PendingGrid::new(
            page.rows().to_vec(),
            fixture.driver.editability(page.columns()),
        );
        atomic.edit(0, 1, Value::Text("changed".into())).unwrap();
        let mut duplicate = atomic.add_template().unwrap();
        duplicate[0] = Value::Int(1);
        duplicate[1] = Value::Text("duplicate key".into());
        atomic.insert(duplicate).unwrap();
        let batch = generate(
            atomic.source().unwrap(),
            &atomic.mutations(),
            Dialect::MySql,
        )
        .unwrap();
        assert!(fixture.driver.commit(&batch).is_err());
        let observed = execute(
            fixture.driver.as_ref(),
            format!(
                "SELECT name FROM {} WHERE id = 1",
                quote_identifier(&fixture.table)
            ),
        )
        .unwrap();
        assert_eq!(observed.rows()[0][0], Value::Text("Ada Lovelace".into()));
    }
}
