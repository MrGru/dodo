//! The PostgreSQL driver.
//!
//! # No tokio runtime is constructed here, and this module never names tokio
//!
//! The `postgres` crate is a synchronous façade over `tokio-postgres`: it
//! builds a private **current-thread** runtime per `Client` and blocks on it.
//! That adds no threads — the thread driving it is the GPUI background-executor
//! thread that called in — and it is structurally what `reqwest::blocking`
//! already does inside dodo. It is also why `dodo-docker-internals` says `docker::services`
//! owns the only tokio runtime dodo *constructs*.
//!
//! # Why the client sits behind a `Mutex`
//!
//! Every `postgres::Client` method takes `&mut self`, and [`Driver`] hands out
//! `&self` because a live connection is shared as an `Arc`. Serializing is also
//! the right model: a desktop client runs one statement per connection at a
//! time, which is what makes "cancel" refer to an unambiguous statement.
//!
//! # Cancelling, and why the token is taken at connect time
//!
//! `Client::cancel_token()` needs the client, and the client is behind the
//! `Mutex` that the running statement holds — so asking for a token *while* a
//! query runs would block behind the query it is meant to stop. The token is
//! therefore taken once, in [`connect`], and kept beside the client: it is a
//! plain `Clone` value (the socket config, the backend process id and its
//! secret key) with no borrow of the connection at all.
//!
//! `CancelToken::cancel_query` opens a **second** connection and sends the
//! protocol's CancelRequest, which is why it needs the same TLS decision the
//! first one made — hence [`PostgresDriver::tls`]. The server answers nothing;
//! the evidence a cancel worked is the running statement failing with SQLSTATE
//! `57014`, which [`server_error`] maps to [`DbError::Cancelled`].
//!
//! # Rows arrive as binary, and this module decodes them
//!
//! `query_raw` gives a streaming `RowIter` — the whole point, since a
//! materialised `Vec<Row>` would defeat the page budget before it could act —
//! but tokio-postgres binds every result column in **binary** format. So this
//! module decodes.
//!
//! [`decode`] handles, from the wire: `bool`, the integer family, `oid`,
//! `float4`/`float8`, `numeric`, the text family, `bytea`, `json`/`jsonb`,
//! `uuid`, `date`, `time`, `timestamp`, `timestamptz`, and **arrays of any of
//! them**. Everything else — intervals, ranges, `inet`, composites, and any
//! extension type — falls back to: valid UTF-8 becomes text (which is exactly
//! right for an enum, whose binary form *is* its label), and anything else
//! becomes [`Value::Bytes`]. The column header always shows the server's own
//! type name via `format_type`, so a value dodo could not decode is still
//! labelled honestly rather than silently rendered as something else.
//!
//! Two alternatives were rejected. `simple_query` returns every value as text —
//! the server does the rendering, which is tempting — but the blocking crate's
//! version materialises the whole result into a `Vec`, which is the exact
//! failure the budget exists to prevent. Casting unknown columns to `::text`
//! would work and is what some clients do, but it means rewriting a statement
//! the user wrote, which this module does not do anywhere.
//!
//! # Safe-write provenance and the catalog
//!
//! Preparing a result exposes each column's table OID and attribute number;
//! this driver resolves those to exact schema/table/column names, then reads
//! `pg_index`/`pg_attribute` for the primary or all-NOT-NULL unique key. The
//! state and views never inspect SQL text, and generated writes come back only
//! through `models::statement` as bound parameters.
//!
//! Roots are **the connected database, and only it**. Listing the server's
//! other databases is easy; letting the user *open* one is not — a
//! `postgres::Client` is bound to one database, so expanding another means a
//! second connection with its own lifetime and failure modes. Offering rows
//! that cannot be expanded would be worse than not offering them, so the tree
//! starts at what this connection can actually reach.
//!
//! Node ids are this module's private business (see [`id`]) and are built
//! around `\u{1f}`, the ASCII unit separator: an identifier may legally contain
//! a dot, a colon or a slash, so those cannot be delimiters.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use postgres::config::SslMode as PgSslMode;
// `RowIter` is a *fallible* iterator, not a `std` one — a row can fail to
// arrive. The trait comes from `postgres`'s own re-export rather than from a
// direct `fallible-iterator` dependency, which matters: two incompatible major
// versions of that crate are in this graph (0.2 for `postgres`, 0.3 for
// `rusqlite`), and naming the wrong one produces a "no method named `next`"
// error that says nothing about versions.
use postgres::fallible_iterator::FallibleIterator as _;
use postgres::types::{Format, FromSql, IsNull, Kind, ToSql, Type};
use postgres::{Client, Config, NoTls, Statement};

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

/// How long to wait for a server to answer the connection attempt. Long enough
/// for a container that is still starting, short enough that a wrong host does
/// not look like a hang.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// What the server records as the client's name, visible in `pg_stat_activity`.
/// A courtesy to whoever is looking at their server wondering who is connected.
const APPLICATION_NAME: &str = "dodo";

/// The SQLSTATE PostgreSQL reports for a statement it stopped because a
/// CancelRequest arrived: `query_canceled`. Named rather than inlined because
/// it is the one code this module reads rather than merely reports.
const SQLSTATE_QUERY_CANCELED: &str = "57014";

pub struct PostgresDriver {
    client: Mutex<Client>,
    /// Taken once at connect time — see the module doc for why it cannot be
    /// taken later.
    cancel: postgres::CancelToken,
    /// Whether the cancel connection uses TLS, which has to match the decision
    /// the first connection made.
    tls: bool,
}

/// Opens a connection for `profile`.
pub fn connect(profile: &ConnectionProfile) -> Result<Arc<PostgresDriver>, DbError> {
    let mut config = Config::new();
    config
        .host(profile.host.trim())
        .port(profile.port)
        .dbname(profile.database.trim())
        .application_name(APPLICATION_NAME)
        .connect_timeout(CONNECT_TIMEOUT)
        .ssl_mode(ssl_mode(profile.ssl_mode));

    if !profile.user.trim().is_empty() {
        config.user(profile.user.trim());
    }
    if !profile.password.is_empty() {
        config.password(&profile.password);
    }

    let tls = !matches!(profile.ssl_mode, SslMode::Disable);
    let client = match tls {
        // No connector is built at all when TLS is off, rather than building
        // one and asking the server not to use it.
        false => config.connect(NoTls),
        true => config.connect(tls_connector()?),
    }
    .map_err(unreachable)?;

    Ok(Arc::new(PostgresDriver {
        cancel: client.cancel_token(),
        client: Mutex::new(client),
        tls,
    }))
}

fn ssl_mode(mode: SslMode) -> PgSslMode {
    match mode {
        SslMode::Disable => PgSslMode::Disable,
        SslMode::Prefer => PgSslMode::Prefer,
        SslMode::Require => PgSslMode::Require,
    }
}

/// The rustls connector, verifying against the platform's own trust store.
///
/// dodo ships no root certificate bundle of its own and does not intend to: the
/// platform verifier is the same one `reqwest` already uses here, so a server
/// certificate is trusted on exactly the terms the operating system says.
fn tls_connector() -> Result<tokio_postgres_rustls::MakeRustlsConnect, DbError> {
    use rustls_platform_verifier::BuilderVerifierExt as _;

    let config = rustls::ClientConfig::builder()
        .with_platform_verifier()
        .map_err(|err| DbError::Unreachable(format!("TLS could not be set up: {err}")))?
        .with_no_client_auth();
    Ok(tokio_postgres_rustls::MakeRustlsConnect::new(config))
}

/// A driver error from a failed connection attempt.
fn unreachable(err: postgres::Error) -> DbError {
    DbError::Unreachable(error_detail(&err))
}

/// A driver error from a statement the server refused, carrying the SQLSTATE
/// when there is one — so the few cases worth special-casing later can be,
/// without re-parsing English.
///
/// One case is special-cased already: `57014` is not a fault, it is the server
/// reporting that it stopped the statement because dodo asked. Reading it here
/// — from the *server's* answer, not from the fact that a Cancel button was
/// pressed — is what makes [`DbError::Cancelled`] evidence rather than a label.
fn server_error(err: postgres::Error) -> DbError {
    let code = err.code().map(|code| code.code().to_string());
    if code.as_deref() == Some(SQLSTATE_QUERY_CANCELED) {
        return DbError::Cancelled;
    }
    DbError::Server {
        code,
        detail: error_detail(&err),
    }
}

/// The most useful sentence in a `postgres::Error`.
///
/// The `Display` of the outer error is often just "db error"; the server's own
/// message is in the source. Both are third-party English and are kept verbatim
/// inside a translated frame.
fn error_detail(err: &postgres::Error) -> String {
    use std::error::Error as _;
    match err.source() {
        Some(source) => source.to_string(),
        None => err.to_string(),
    }
}

// ---------------------------------------------------------------- node ids

/// This driver's private node-id vocabulary.
///
/// The delimiter is `\u{1f}` (ASCII unit separator) because a PostgreSQL
/// identifier may legally contain a dot, a colon, a slash or a space — a
/// quoted identifier may contain almost anything — so none of the obvious
/// delimiters is safe. Nothing above `services/` parses these.
mod id {
    pub const SEP: char = '\u{1f}';

    pub const DATABASE: &str = "db";

    /// `s␟<schema>`
    pub fn schema(name: &str) -> String {
        format!("s{SEP}{name}")
    }
    /// `gt␟<schema>` — the Tables group of a schema.
    pub fn tables_group(schema: &str) -> String {
        format!("gt{SEP}{schema}")
    }
    /// `gv␟<schema>` — the Views group of a schema.
    pub fn views_group(schema: &str) -> String {
        format!("gv{SEP}{schema}")
    }
    /// `t␟<schema>␟<table>` / `v␟<schema>␟<view>`
    pub fn relation(prefix: &str, schema: &str, name: &str) -> String {
        format!("{prefix}{SEP}{schema}{SEP}{name}")
    }
    /// `gc␟…` / `gi␟…` / `gk␟…` — a relation's Columns / Indexes / Constraints.
    pub fn relation_group(prefix: &str, schema: &str, name: &str) -> String {
        format!("{prefix}{SEP}{schema}{SEP}{name}")
    }
    /// A leaf, which is never expanded and so needs only to be unique.
    pub fn leaf(kind: &str, schema: &str, relation: &str, name: &str) -> String {
        format!("{kind}{SEP}{schema}{SEP}{relation}{SEP}{name}")
    }

    /// The tag and the parts of an id, or `None` if it is not one of ours.
    pub fn parse(id: &str) -> Option<(&str, Vec<&str>)> {
        let mut parts = id.split(SEP);
        let tag = parts.next()?;
        Some((tag, parts.collect()))
    }
}

// ------------------------------------------------------------ catalog SQL

/// Every schema a user put there. The `pg_%` and `information_schema` families
/// are the server's own bookkeeping and are hidden: they are enormous, they are
/// the same on every database, and nobody browsing their own schema wants to
/// scroll past them.
const SCHEMAS: &str = "SELECT nspname FROM pg_catalog.pg_namespace \
     WHERE nspname NOT LIKE 'pg\\_%' AND nspname <> 'information_schema' \
     ORDER BY nspname";

/// Ordinary and partitioned tables. Sequences, indexes and TOAST tables are
/// other `relkind`s and are not tables.
const TABLES: &str = "SELECT c.relname FROM pg_catalog.pg_class c \
     JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
     WHERE n.nspname = $1 AND c.relkind IN ('r', 'p') ORDER BY c.relname";

/// Views and materialized views together: both are things you select from, and
/// the design report defers giving materialized views a node type of their own.
const VIEWS: &str = "SELECT c.relname FROM pg_catalog.pg_class c \
     JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
     WHERE n.nspname = $1 AND c.relkind IN ('v', 'm') ORDER BY c.relname";

/// `format_type` is what `\d` uses: it renders `varchar(255)` and `numeric(9,2)`
/// rather than the bare type name, which is what makes the tree's dimmed detail
/// worth reading.
const COLUMNS: &str = "SELECT a.attname, \
     pg_catalog.format_type(a.atttypid, a.atttypmod), a.attnotnull \
     FROM pg_catalog.pg_attribute a \
     JOIN pg_catalog.pg_class c ON c.oid = a.attrelid \
     JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
     WHERE n.nspname = $1 AND c.relname = $2 AND a.attnum > 0 AND NOT a.attisdropped \
     ORDER BY a.attnum";

const INDEXES: &str = "SELECT ic.relname, i.indisunique, i.indisprimary \
     FROM pg_catalog.pg_index i \
     JOIN pg_catalog.pg_class ic ON ic.oid = i.indexrelid \
     JOIN pg_catalog.pg_class c ON c.oid = i.indrelid \
     JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
     WHERE n.nspname = $1 AND c.relname = $2 ORDER BY ic.relname";

const CONSTRAINTS: &str = "SELECT con.conname, \
     pg_catalog.pg_get_constraintdef(con.oid) \
     FROM pg_catalog.pg_constraint con \
     JOIN pg_catalog.pg_class c ON c.oid = con.conrelid \
     JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
     WHERE n.nspname = $1 AND c.relname = $2 ORDER BY con.conname";

const DETAIL_COLUMNS: &str = "SELECT a.attname, \
     pg_catalog.format_type(a.atttypid, a.atttypmod), NOT a.attnotnull, \
     pg_catalog.pg_get_expr(d.adbin, d.adrelid) \
     FROM pg_catalog.pg_attribute a \
     JOIN pg_catalog.pg_class c ON c.oid = a.attrelid \
     JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
     LEFT JOIN pg_catalog.pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum \
     WHERE n.nspname = $1 AND c.relname = $2 AND a.attnum > 0 AND NOT a.attisdropped \
     ORDER BY a.attnum";

const DETAIL_INDEXES: &str = "SELECT ic.relname, i.indisunique, i.indisprimary, \
     pg_catalog.pg_get_indexdef(i.indexrelid) \
     FROM pg_catalog.pg_index i \
     JOIN pg_catalog.pg_class ic ON ic.oid = i.indexrelid \
     JOIN pg_catalog.pg_class c ON c.oid = i.indrelid \
     JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
     WHERE n.nspname = $1 AND c.relname = $2 ORDER BY ic.relname";

const DDL_COLUMNS: &str = "SELECT a.attname, \
     pg_catalog.format_type(a.atttypid, a.atttypmod), a.attnotnull, \
     pg_catalog.pg_get_expr(d.adbin, d.adrelid), a.attidentity::text, a.attgenerated::text \
     FROM pg_catalog.pg_attribute a \
     JOIN pg_catalog.pg_class c ON c.oid = a.attrelid \
     JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
     LEFT JOIN pg_catalog.pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum \
     WHERE n.nspname = $1 AND c.relname = $2 AND a.attnum > 0 AND NOT a.attisdropped \
     ORDER BY a.attnum";

const DDL_INDEXES: &str = "SELECT pg_catalog.pg_get_indexdef(i.indexrelid) \
     FROM pg_catalog.pg_index i \
     JOIN pg_catalog.pg_class c ON c.oid = i.indrelid \
     JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
     WHERE n.nspname = $1 AND c.relname = $2 \
       AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_constraint con WHERE con.conindid = i.indexrelid) \
     ORDER BY i.indexrelid";

const VIEW_DDL: &str = "SELECT c.relkind::text, pg_catalog.pg_get_viewdef(c.oid, true) \
     FROM pg_catalog.pg_class c \
     JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
     WHERE n.nspname = $1 AND c.relname = $2 AND c.relkind IN ('v', 'm')";

/// The database this connection is attached to.
const CURRENT_DATABASE: &str = "SELECT current_database()";

impl PostgresDriver {
    fn with_client<T>(
        &self,
        f: impl FnOnce(&mut Client) -> Result<T, postgres::Error>,
    ) -> Result<T, DbError> {
        let mut client = self
            .client
            .lock()
            .map_err(|_| DbError::Unreachable("the connection was poisoned by a panic".into()))?;
        f(&mut client).map_err(server_error)
    }

    fn identity_metadata(&self, origin: &ColumnOrigin) -> Result<IdentityMetadata, DbError> {
        self.with_client(|client| {
            let table = TableRef {
                schema: origin.schema.clone(),
                table: origin.table.clone(),
            };
            let mut metadata = IdentityMetadata::new(table);
            let rows = client.query(
                "SELECT i.indexrelid::oid, i.indisprimary, i.indnkeyatts::int4, \
                        a.attname, ord.ordinality::int4, a.attnotnull \
                 FROM pg_catalog.pg_index i \
                 JOIN pg_catalog.pg_class c ON c.oid = i.indrelid \
                 JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
                 JOIN LATERAL unnest(i.indkey) WITH ORDINALITY ord(attnum, ordinality) \
                      ON ord.ordinality <= i.indnkeyatts \
                 JOIN pg_catalog.pg_attribute a \
                      ON a.attrelid = c.oid AND a.attnum = ord.attnum \
                 WHERE n.nspname = $1 AND c.relname = $2 AND i.indisunique \
                   AND i.indisvalid AND i.indpred IS NULL AND i.indexprs IS NULL \
                 ORDER BY i.indisprimary DESC, i.indexrelid, ord.ordinality",
                &[&origin.schema.as_deref().unwrap_or("public"), &origin.table],
            )?;
            type KeyRows = std::collections::BTreeMap<(u32, bool, i32), Vec<(i32, String, bool)>>;
            let mut grouped = KeyRows::new();
            for row in rows {
                grouped
                    .entry((row.get(0), row.get(1), row.get(2)))
                    .or_default()
                    .push((row.get(4), row.get(3), row.get(5)));
            }
            let mut keys = Vec::new();
            for ((_, primary, expected), mut columns) in grouped {
                columns.sort_by_key(|(ordinal, _, _)| *ordinal);
                if columns.len() == expected as usize {
                    keys.push(UniqueKey {
                        columns: columns.iter().map(|(_, name, _)| name.clone()).collect(),
                        primary,
                        all_non_null: primary || columns.iter().all(|(_, _, not_null)| *not_null),
                    });
                }
            }
            keys.sort_by_key(|key| !key.primary);
            metadata.keys = keys;

            let generated = client.query(
                "SELECT a.attname \
                 FROM pg_catalog.pg_attribute a \
                 JOIN pg_catalog.pg_class c ON c.oid = a.attrelid \
                 JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
                 WHERE n.nspname = $1 AND c.relname = $2 AND a.attnum > 0 \
                   AND NOT a.attisdropped \
                   AND (a.attidentity <> '' OR a.attgenerated <> '' OR a.atthasdef)",
                &[&origin.schema.as_deref().unwrap_or("public"), &origin.table],
            )?;
            metadata.generated_columns = generated
                .into_iter()
                .map(|row| row.get::<_, String>(0))
                .collect();
            Ok(metadata)
        })
    }

    fn roots(&self) -> Result<Vec<CatalogNode>, DbError> {
        let name: String = self.with_client(|client| {
            client
                .query_one(CURRENT_DATABASE, &[])
                .map(|row| row.get::<_, String>(0))
        })?;
        Ok(vec![CatalogNode::branch(
            id::DATABASE,
            NodeKind::Database,
            name,
        )])
    }

    fn schemas(&self) -> Result<Vec<CatalogNode>, DbError> {
        let rows = self.with_client(|client| client.query(SCHEMAS, &[]))?;
        Ok(rows
            .iter()
            .map(|row| {
                let name: String = row.get(0);
                CatalogNode::branch(id::schema(&name), NodeKind::Schema, name)
            })
            .collect())
    }

    fn schema_groups(&self, schema: &str) -> Vec<CatalogNode> {
        vec![
            CatalogNode::group(id::tables_group(schema), GroupLabel::Tables),
            CatalogNode::group(id::views_group(schema), GroupLabel::Views),
        ]
    }

    fn relations(
        &self,
        sql: &str,
        prefix: &str,
        kind: NodeKind,
        schema: &str,
    ) -> Result<Vec<CatalogNode>, DbError> {
        let rows = self.with_client(|client| client.query(sql, &[&schema]))?;
        Ok(rows
            .iter()
            .map(|row| {
                let name: String = row.get(0);
                CatalogNode::branch(id::relation(prefix, schema, &name), kind, name)
            })
            .collect())
    }

    /// A table's groups. A view gets Columns only: an index or a constraint on
    /// a view is not a thing, and offering empty groups would be noise.
    fn relation_groups(&self, schema: &str, relation: &str, is_table: bool) -> Vec<CatalogNode> {
        let mut groups = vec![CatalogNode::group(
            id::relation_group("gc", schema, relation),
            GroupLabel::Columns,
        )];
        if is_table {
            groups.push(CatalogNode::group(
                id::relation_group("gi", schema, relation),
                GroupLabel::Indexes,
            ));
            groups.push(CatalogNode::group(
                id::relation_group("gk", schema, relation),
                GroupLabel::Constraints,
            ));
        }
        groups
    }

    fn columns(&self, schema: &str, relation: &str) -> Result<Vec<CatalogNode>, DbError> {
        let rows = self.with_client(|client| client.query(COLUMNS, &[&schema, &relation]))?;
        Ok(rows
            .iter()
            .map(|row| {
                let name: String = row.get(0);
                let type_name: String = row.get(1);
                let not_null: bool = row.get(2);
                CatalogNode::leaf(
                    id::leaf("c", schema, relation, &name),
                    NodeKind::Column,
                    name,
                )
                .with_detail(column_detail(&type_name, not_null))
            })
            .collect())
    }

    fn indexes(&self, schema: &str, relation: &str) -> Result<Vec<CatalogNode>, DbError> {
        let rows = self.with_client(|client| client.query(INDEXES, &[&schema, &relation]))?;
        Ok(rows
            .iter()
            .map(|row| {
                let name: String = row.get(0);
                let unique: bool = row.get(1);
                let primary: bool = row.get(2);
                CatalogNode::leaf(
                    id::leaf("i", schema, relation, &name),
                    NodeKind::Index,
                    name,
                )
                .with_detail(index_detail(unique, primary))
            })
            .collect())
    }

    fn constraints(&self, schema: &str, relation: &str) -> Result<Vec<CatalogNode>, DbError> {
        let rows = self.with_client(|client| client.query(CONSTRAINTS, &[&schema, &relation]))?;
        Ok(rows
            .iter()
            .map(|row| {
                let name: String = row.get(0);
                let definition: String = row.get(1);
                CatalogNode::leaf(
                    id::leaf("k", schema, relation, &name),
                    NodeKind::Constraint,
                    name,
                )
                .with_detail(definition)
            })
            .collect())
    }

    fn detail_columns(
        &self,
        schema: &str,
        relation: &str,
        sink: &mut dyn RowSink,
    ) -> Result<DetailResult, DbError> {
        let rows =
            self.with_client(|client| client.query(DETAIL_COLUMNS, &[&schema, &relation]))?;
        let values = rows
            .iter()
            .map(|row| {
                vec![
                    Value::Text(row.get::<_, String>(0)),
                    Value::Text(row.get::<_, String>(1)),
                    Value::Bool(row.get::<_, bool>(2)),
                    row.get::<_, Option<String>>(3)
                        .map(Value::Text)
                        .unwrap_or(Value::Null),
                ]
            })
            .collect();
        Ok(metadata_rows(
            vec![
                DetailField::Name,
                DetailField::Type,
                DetailField::Nullable,
                DetailField::Default,
            ],
            values,
            sink,
        ))
    }

    fn detail_indexes(
        &self,
        schema: &str,
        relation: &str,
        sink: &mut dyn RowSink,
    ) -> Result<DetailResult, DbError> {
        let rows =
            self.with_client(|client| client.query(DETAIL_INDEXES, &[&schema, &relation]))?;
        let values = rows
            .iter()
            .map(|row| {
                vec![
                    Value::Text(row.get::<_, String>(0)),
                    Value::Bool(row.get::<_, bool>(1)),
                    Value::Bool(row.get::<_, bool>(2)),
                    Value::Text(row.get::<_, String>(3)),
                ]
            })
            .collect();
        Ok(metadata_rows(
            vec![
                DetailField::Name,
                DetailField::Unique,
                DetailField::Primary,
                DetailField::Definition,
            ],
            values,
            sink,
        ))
    }

    fn detail_constraints(
        &self,
        schema: &str,
        relation: &str,
        sink: &mut dyn RowSink,
    ) -> Result<DetailResult, DbError> {
        let rows = self.with_client(|client| client.query(CONSTRAINTS, &[&schema, &relation]))?;
        let values = rows
            .iter()
            .map(|row| {
                vec![
                    Value::Text(row.get::<_, String>(0)),
                    Value::Text(row.get::<_, String>(1)),
                ]
            })
            .collect();
        Ok(metadata_rows(
            vec![DetailField::Name, DetailField::Definition],
            values,
            sink,
        ))
    }

    fn table_ddl(&self, schema: &str, relation: &str) -> Result<String, DbError> {
        let columns =
            self.with_client(|client| client.query(DDL_COLUMNS, &[&schema, &relation]))?;
        let constraints =
            self.with_client(|client| client.query(CONSTRAINTS, &[&schema, &relation]))?;
        let indexes =
            self.with_client(|client| client.query(DDL_INDEXES, &[&schema, &relation]))?;

        let mut definitions = Vec::new();
        for row in columns {
            let name: String = row.get(0);
            let type_name: String = row.get(1);
            let not_null: bool = row.get(2);
            let default: Option<String> = row.get(3);
            let identity: String = row.get(4);
            let generated: String = row.get(5);
            let mut definition = format!("{} {type_name}", quote_identifier(&name));
            if generated == "s" {
                if let Some(expression) = default {
                    definition.push_str(&format!(" GENERATED ALWAYS AS ({expression}) STORED"));
                }
            } else if identity == "a" {
                definition.push_str(" GENERATED ALWAYS AS IDENTITY");
            } else if identity == "d" {
                definition.push_str(" GENERATED BY DEFAULT AS IDENTITY");
            } else if let Some(default) = default {
                definition.push_str(&format!(" DEFAULT {default}"));
            }
            if not_null {
                definition.push_str(" NOT NULL");
            }
            definitions.push(definition);
        }
        for row in constraints {
            let name: String = row.get(0);
            let definition: String = row.get(1);
            definitions.push(format!(
                "CONSTRAINT {} {definition}",
                quote_identifier(&name)
            ));
        }

        let qualified = format!(
            "{}.{}",
            quote_identifier(schema),
            quote_identifier(relation)
        );
        let mut ddl = format!(
            "CREATE TABLE {qualified} (\n    {}\n);",
            definitions.join(",\n    ")
        );
        for row in indexes {
            let definition: String = row.get(0);
            ddl.push_str(&format!("\n\n{definition};"));
        }
        Ok(ddl)
    }

    fn view_ddl(&self, schema: &str, relation: &str) -> Result<String, DbError> {
        let row = self.with_client(|client| client.query_one(VIEW_DDL, &[&schema, &relation]))?;
        let kind: String = row.get(0);
        let definition: String = row.get(1);
        let materialized = if kind == "m" { "MATERIALIZED " } else { "" };
        Ok(format!(
            "CREATE {materialized}VIEW {}.{} AS\n{};",
            quote_identifier(schema),
            quote_identifier(relation),
            definition.trim_end_matches(';')
        ))
    }
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

fn quote_identifier(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

/// The dimmed text beside a column: its rendered type, and whether it is
/// required. `NOT NULL` is the server's own words and stays in them.
pub(crate) fn column_detail(type_name: &str, not_null: bool) -> String {
    if not_null {
        format!("{type_name} NOT NULL")
    } else {
        type_name.to_string()
    }
}

/// The dimmed text beside an index. A primary key is also unique, so saying
/// both would be noise.
pub(crate) fn index_detail(unique: bool, primary: bool) -> String {
    if primary {
        "PRIMARY KEY".into()
    } else if unique {
        "UNIQUE".into()
    } else {
        String::new()
    }
}

impl Driver for PostgresDriver {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            editor_language: "sql",
            cancel: true,
            explain: true,
            detail: true,
            ddl: DdlSource::Reconstructed,
            mutation: Some(crate::models::statement::Dialect::PostgreSql),
        }
    }

    /// A plain `EXPLAIN`, with no `ANALYZE`.
    ///
    /// `EXPLAIN ANALYZE` **runs** the statement — including an `UPDATE` or a
    /// `DELETE` — and a button labelled Explain that quietly executed the user's
    /// `DELETE FROM users` would be indefensible. The plan without it is the
    /// planner's estimate, which is what "explain" means to everyone who has
    /// not opted into the other thing.
    fn explain_statement(&self, statement: &str) -> Option<String> {
        Some(format!("EXPLAIN {statement}"))
    }

    fn cancel_handle(&self) -> Option<CancelHandle> {
        let token = self.cancel.clone();
        let tls = self.tls;
        Some(CancelHandle::new(move || {
            // Blocking, and it opens a second connection: the caller runs it on
            // the background executor. The TLS choice has to match the one the
            // first connection made, or the server refuses the cancel socket.
            let sent = if tls {
                token.cancel_query(tls_connector()?)
            } else {
                token.cancel_query(NoTls)
            };
            // A failure here means dodo could not *reach* the server to ask —
            // not that the query survived — so it is reported as unreachable
            // rather than as a rejected statement.
            sent.map_err(unreachable)
        }))
    }

    fn ping(&self) -> Result<(), DbError> {
        self.with_client(|client| client.simple_query("SELECT 1").map(|_| ()))
    }

    fn children(&self, parent: Option<&NodeId>) -> Result<Vec<CatalogNode>, DbError> {
        let Some(parent) = parent else {
            return self.roots();
        };
        let Some((tag, parts)) = id::parse(parent.as_str()) else {
            return Ok(Vec::new());
        };

        match (tag, parts.as_slice()) {
            (id::DATABASE, _) => self.schemas(),
            ("s", [schema]) => Ok(self.schema_groups(schema)),
            ("gt", [schema]) => self.relations(TABLES, "t", NodeKind::Table, schema),
            ("gv", [schema]) => self.relations(VIEWS, "v", NodeKind::View, schema),
            ("t", [schema, table]) => Ok(self.relation_groups(schema, table, true)),
            ("v", [schema, view]) => Ok(self.relation_groups(schema, view, false)),
            ("gc", [schema, relation]) => self.columns(schema, relation),
            ("gi", [schema, relation]) => self.indexes(schema, relation),
            ("gk", [schema, relation]) => self.constraints(schema, relation),
            // A leaf, or an id from a build that knew more tags than this one.
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
        let (schema, relation, is_table) = match (tag, parts.as_slice()) {
            ("t", [schema, relation]) => (*schema, *relation, true),
            ("v", [schema, relation]) => (*schema, *relation, false),
            _ => return Ok(DetailResult::Unavailable),
        };

        match request.tab {
            DetailTab::Data => {
                let statement = format!(
                    "SELECT * FROM {}.{} LIMIT {} OFFSET {}",
                    quote_identifier(schema),
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
            DetailTab::Columns => self.detail_columns(schema, relation, sink),
            DetailTab::Indexes if is_table => self.detail_indexes(schema, relation, sink),
            DetailTab::Constraints if is_table => self.detail_constraints(schema, relation, sink),
            DetailTab::Ddl if is_table => self.table_ddl(schema, relation).map(DetailResult::Ddl),
            DetailTab::Ddl => self.view_ddl(schema, relation).map(DetailResult::Ddl),
            DetailTab::Indexes | DetailTab::Constraints => Ok(DetailResult::Unavailable),
        }
    }

    fn execute(
        &self,
        request: &QueryRequest,
        sink: &mut dyn RowSink,
    ) -> Result<Execution, DbError> {
        let started = Instant::now();
        let mut client = self
            .client
            .lock()
            .map_err(|_| DbError::Unreachable("the connection was poisoned by a panic".into()))?;

        // Preparing is how PostgreSQL exposes table OIDs and column numbers
        // before the first row. It does not execute or rewrite the statement.
        let statement = client.prepare(&request.statement).map_err(server_error)?;
        let (columns, types) = describe(&mut client, &statement).map_err(server_error)?;
        if !columns.is_empty() {
            sink.columns(columns);
        }
        let mut rows = client
            .query_raw::<_, &str, _>(&statement, [])
            .map_err(server_error)?;

        let mut count = 0u64;
        let mut truncated = false;
        while let Some(row) = rows.next().map_err(server_error)? {
            count += 1;
            let values = (0..types.len())
                .map(|index| decode::cell(&row, index, &types[index]))
                .collect();
            if sink.row(values) == Flow::Stop {
                truncated = true;
                break;
            }
        }

        let rows_affected = match count {
            0 => rows.rows_affected(),
            _ => None,
        };
        drop(rows);

        Ok(Execution {
            rows_affected,
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
        if batch.dialect != Dialect::PostgreSql {
            return Err(MutationFailure::Transaction(DbError::Server {
                code: None,
                detail: "generated mutation dialect did not match PostgreSQL".into(),
            }));
        }
        let mut client = self.client.lock().map_err(|_| {
            MutationFailure::Transaction(DbError::Unreachable(
                "the connection was poisoned by a panic".into(),
            ))
        })?;
        let mut transaction = client
            .transaction()
            .map_err(|error| MutationFailure::Transaction(server_error(error)))?;

        for (index, statement) in batch.statements.iter().enumerate() {
            let parameters = statement.params.iter().map(PgParameter).collect::<Vec<_>>();
            let refs = parameters
                .iter()
                .map(|parameter| parameter as &(dyn ToSql + Sync))
                .collect::<Vec<_>>();
            let affected = match transaction.execute(&statement.sql, &refs) {
                Ok(affected) => affected,
                Err(error) => {
                    let failure = MutationFailure::Statement {
                        index,
                        sql: statement.sql.clone(),
                        error: server_error(error),
                    };
                    let _ = transaction.rollback();
                    return Err(failure);
                }
            };
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

#[derive(Debug)]
struct PgParameter<'a>(&'a Value);

impl ToSql for PgParameter<'_> {
    fn to_sql(
        &self,
        _: &Type,
        out: &mut postgres::types::private::BytesMut,
    ) -> Result<IsNull, Box<dyn std::error::Error + Sync + Send>> {
        let text = match self.0 {
            Value::Null => return Ok(IsNull::Yes),
            Value::Bool(value) => value.to_string(),
            Value::Int(value) => value.to_string(),
            Value::Float(value) if value.is_infinite() && value.is_sign_positive() => {
                "Infinity".into()
            }
            Value::Float(value) if value.is_infinite() => "-Infinity".into(),
            Value::Float(value) => value.to_string(),
            Value::Text(value) | Value::Json(value) => value.clone(),
            Value::Bytes(value) => {
                let hex: String = value.iter().map(|byte| format!("{byte:02x}")).collect();
                format!(r"\x{hex}")
            }
            Value::Truncated { .. } => unreachable!("statement generation rejects truncation"),
        };
        out.extend_from_slice(text.as_bytes());
        Ok(IsNull::No)
    }

    fn accepts(_: &Type) -> bool {
        true
    }

    fn encode_format(&self, _: &Type) -> Format {
        Format::Text
    }

    postgres::types::to_sql_checked!();
}

/// Hands the sink the result's shape and keeps the types the rows are decoded
/// with.
fn describe(
    client: &mut Client,
    statement: &Statement,
) -> Result<(Vec<ColumnMeta>, Vec<Type>), postgres::Error> {
    let mut origins = std::collections::BTreeMap::new();
    let table_oids = statement
        .columns()
        .iter()
        .filter_map(|column| column.table_oid())
        .collect::<std::collections::BTreeSet<_>>();
    for table_oid in table_oids {
        let rows = client.query(
            "SELECT n.nspname, c.relname, a.attnum, a.attname \
             FROM pg_catalog.pg_class c \
             JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
             JOIN pg_catalog.pg_attribute a ON a.attrelid = c.oid \
             WHERE c.oid = $1 AND a.attnum > 0 AND NOT a.attisdropped",
            &[&table_oid],
        )?;
        for row in rows {
            origins.insert(
                (table_oid, row.get::<_, i16>(2)),
                ColumnOrigin {
                    schema: Some(row.get(0)),
                    table: row.get(1),
                    column: row.get(3),
                },
            );
        }
    }

    let columns = statement
        .columns()
        .iter()
        .map(|column| {
            let mut meta = ColumnMeta::new(column.name(), type_name(column.type_()));
            if let (Some(table_oid), Some(column_id)) = (column.table_oid(), column.column_id())
                && let Some(origin) = origins.get(&(table_oid, column_id))
            {
                meta = meta.with_origin(origin.clone());
            }
            meta
        })
        .collect();
    let types = statement
        .columns()
        .iter()
        .map(|column| column.type_().clone())
        .collect();
    Ok((columns, types))
}

fn type_name(ty: &Type) -> String {
    match ty.kind() {
        Kind::Array(element) => format!("{}[]", type_name(element)),
        _ => ty.name().to_string(),
    }
}

// ------------------------------------------------------------- decoding

/// Turning PostgreSQL's binary wire format into a [`Value`].
///
/// Pure, and every piece of it is unit tested against bytes rather than against
/// a server — which matters, because these are the functions where being subtly
/// wrong shows up as a plausible-looking wrong number.
pub(crate) mod decode {
    use super::{Raw, Value};
    use postgres::Row as PgRow;
    use postgres::types::{Kind, Type};

    /// One cell.
    pub fn cell(row: &PgRow, index: usize, ty: &Type) -> Value {
        match row.try_get::<_, Option<Raw>>(index) {
            Ok(Some(raw)) => from_bytes(ty, &raw.0),
            // NULL, or a value the driver refused to hand over at all. `Raw`
            // accepts every type, so the second cannot happen — but reporting
            // NULL is the safe reading either way, and it is what the grid
            // draws distinctly from an empty string.
            Ok(None) | Err(_) => Value::Null,
        }
    }

    /// One value, from its binary representation.
    pub fn from_bytes(ty: &Type, raw: &[u8]) -> Value {
        if let Kind::Array(element) = ty.kind() {
            return array(element, raw);
        }

        match *ty {
            Type::BOOL => raw
                .first()
                .map_or(Value::Null, |byte| Value::Bool(*byte != 0)),
            Type::INT2 => int(raw, 2),
            Type::INT4 => int(raw, 4),
            Type::INT8 => int(raw, 8),
            Type::OID => u32_be(raw).map_or_else(|| fallback(raw), |n| Value::Int(i64::from(n))),
            Type::FLOAT4 => {
                f32_be(raw).map_or_else(|| fallback(raw), |n| Value::Float(f64::from(n)))
            }
            Type::FLOAT8 => f64_be(raw).map_or_else(|| fallback(raw), Value::Float),
            Type::NUMERIC => numeric(raw).map_or_else(|| fallback(raw), Value::Text),
            Type::CHAR => int(raw, 1),
            Type::TEXT | Type::VARCHAR | Type::BPCHAR | Type::NAME | Type::UNKNOWN => text(raw),
            Type::BYTEA => Value::Bytes(raw.to_vec()),
            Type::JSON => json(raw, 0),
            // `jsonb`'s binary form is a one-byte version marker followed by
            // the JSON text. Only version 1 exists; anything else is not
            // something this build knows how to read.
            Type::JSONB => match raw.first() {
                Some(1) => json(raw, 1),
                _ => fallback(raw),
            },
            Type::UUID => uuid(raw).map_or_else(|| fallback(raw), Value::Text),
            Type::DATE => date(raw).map_or_else(|| fallback(raw), Value::Text),
            Type::TIME => time(raw).map_or_else(|| fallback(raw), Value::Text),
            Type::TIMESTAMP => timestamp(raw, false).map_or_else(|| fallback(raw), Value::Text),
            Type::TIMESTAMPTZ => timestamp(raw, true).map_or_else(|| fallback(raw), Value::Text),
            _ => fallback(raw),
        }
    }

    /// What a type this module cannot decode becomes.
    ///
    /// Valid UTF-8 is shown as text, which is exactly right for an enum — whose
    /// binary form *is* its label — and for most extension types that are text
    /// underneath. Anything else is shown as bytes rather than as a mangled
    /// string, because a wrong-looking value is better than a wrong one.
    fn fallback(raw: &[u8]) -> Value {
        match std::str::from_utf8(raw) {
            Ok(text) if !text.contains('\0') => Value::Text(text.to_string()),
            _ => Value::Bytes(raw.to_vec()),
        }
    }

    fn text(raw: &[u8]) -> Value {
        Value::Text(String::from_utf8_lossy(raw).into_owned())
    }

    fn json(raw: &[u8], skip: usize) -> Value {
        match std::str::from_utf8(&raw[skip.min(raw.len())..]) {
            Ok(text) => Value::Json(text.to_string()),
            Err(_) => fallback(raw),
        }
    }

    /// A big-endian signed integer of `width` bytes.
    fn int(raw: &[u8], width: usize) -> Value {
        if raw.len() != width || width == 0 {
            return fallback(raw);
        }
        let mut value = i64::from(raw[0] as i8);
        for byte in &raw[1..] {
            value = (value << 8) | i64::from(*byte);
        }
        Value::Int(value)
    }

    fn u32_be(raw: &[u8]) -> Option<u32> {
        raw.try_into().ok().map(u32::from_be_bytes)
    }

    fn f32_be(raw: &[u8]) -> Option<f32> {
        raw.try_into().ok().map(f32::from_be_bytes)
    }

    fn f64_be(raw: &[u8]) -> Option<f64> {
        raw.try_into().ok().map(f64::from_be_bytes)
    }

    fn i16_at(raw: &[u8], at: usize) -> Option<i16> {
        raw.get(at..at + 2)
            .and_then(|slice| slice.try_into().ok())
            .map(i16::from_be_bytes)
    }

    fn i32_at(raw: &[u8], at: usize) -> Option<i32> {
        raw.get(at..at + 4)
            .and_then(|slice| slice.try_into().ok())
            .map(i32::from_be_bytes)
    }

    fn i64_at(raw: &[u8], at: usize) -> Option<i64> {
        raw.get(at..at + 8)
            .and_then(|slice| slice.try_into().ok())
            .map(i64::from_be_bytes)
    }

    /// `numeric`, which has no fixed-width machine form: a sign, a base-10000
    /// digit vector, the position of the first digit, and how many decimal
    /// places to display.
    ///
    /// Rendered as text rather than as an `f64` on purpose. `numeric` is what
    /// people store money in, and the entire reason they chose it over
    /// `double precision` is that it does not round — turning it into a float
    /// here would throw away the one property it was picked for.
    pub fn numeric(raw: &[u8]) -> Option<String> {
        let digit_count = i16_at(raw, 0)? as usize;
        let weight = i16_at(raw, 2)?;
        let sign = i16_at(raw, 4)? as u16;
        let scale = i16_at(raw, 6)?.max(0) as usize;

        match sign {
            0xC000 => return Some("NaN".into()),
            0xD000 => return Some("Infinity".into()),
            0xF000 => return Some("-Infinity".into()),
            _ => {}
        }

        let digits: Vec<i16> = (0..digit_count)
            .map(|n| i16_at(raw, 8 + n * 2))
            .collect::<Option<_>>()?;

        let mut out = String::new();
        if sign == 0x4000 {
            out.push('-');
        }

        // digits[i] carries base-10000 place (weight - i).
        if weight < 0 {
            out.push('0');
        } else {
            for i in 0..=weight {
                let digit = digits.get(i as usize).copied().unwrap_or(0);
                if i == 0 {
                    out.push_str(&digit.to_string());
                } else {
                    out.push_str(&format!("{digit:04}"));
                }
            }
        }

        if scale > 0 {
            out.push('.');
            let mut fraction = String::new();
            let mut step = 0i32;
            while fraction.len() < scale {
                let index = i32::from(weight) + 1 + step;
                let digit = if index < 0 {
                    0
                } else {
                    digits.get(index as usize).copied().unwrap_or(0)
                };
                fraction.push_str(&format!("{digit:04}"));
                step += 1;
            }
            fraction.truncate(scale);
            out.push_str(&fraction);
        }

        Some(out)
    }

    pub fn uuid(raw: &[u8]) -> Option<String> {
        if raw.len() != 16 {
            return None;
        }
        let hex: String = raw.iter().map(|byte| format!("{byte:02x}")).collect();
        Some(format!(
            "{}-{}-{}-{}-{}",
            &hex[0..8],
            &hex[8..12],
            &hex[12..16],
            &hex[16..20],
            &hex[20..32]
        ))
    }

    /// PostgreSQL counts days from 2000-01-01, not from the Unix epoch.
    const PG_EPOCH_DAYS_FROM_UNIX: i64 = 10_957;

    pub fn date(raw: &[u8]) -> Option<String> {
        let days = i64::from(i32_at(raw, 0)?);
        Some(civil_date(days + PG_EPOCH_DAYS_FROM_UNIX))
    }

    pub fn time(raw: &[u8]) -> Option<String> {
        let micros = i64_at(raw, 0)?;
        Some(clock_time(micros))
    }

    /// `timestamp` and `timestamptz`, both microseconds from 2000-01-01.
    ///
    /// A `timestamptz` is rendered in **UTC** with an explicit `+00`, rather
    /// than in the session's time zone. The server sends the same instant
    /// either way; showing it in UTC and saying so is unambiguous, where
    /// silently applying a time zone would make the same row read differently
    /// on two machines.
    pub fn timestamp(raw: &[u8], with_zone: bool) -> Option<String> {
        let micros = i64_at(raw, 0)?;
        let day_micros = 86_400_000_000i64;
        // Floor division: an instant before 2000 has a negative microsecond
        // count, and truncating towards zero would put it on the wrong day.
        let days = micros.div_euclid(day_micros);
        let rest = micros.rem_euclid(day_micros);
        let mut out = format!(
            "{} {}",
            civil_date(days + PG_EPOCH_DAYS_FROM_UNIX),
            clock_time(rest)
        );
        if with_zone {
            out.push_str("+00");
        }
        Some(out)
    }

    /// Microseconds since midnight as `HH:MM:SS`, with a fractional part only
    /// when there is one — a trailing `.000000` on every row is noise.
    fn clock_time(micros: i64) -> String {
        let seconds_total = micros.div_euclid(1_000_000);
        let fraction = micros.rem_euclid(1_000_000);
        let hours = seconds_total / 3_600;
        let minutes = (seconds_total % 3_600) / 60;
        let seconds = seconds_total % 60;
        if fraction == 0 {
            format!("{hours:02}:{minutes:02}:{seconds:02}")
        } else {
            let mut text = format!("{hours:02}:{minutes:02}:{seconds:02}.{fraction:06}");
            while text.ends_with('0') {
                text.pop();
            }
            text
        }
    }

    /// Days from the Unix epoch to a `YYYY-MM-DD` civil date.
    ///
    /// Howard Hinnant's `civil_from_days`, which is exact for the whole
    /// proleptic Gregorian range and needs no calendar crate.
    fn civil_date(days_from_epoch: i64) -> String {
        let z = days_from_epoch + 719_468;
        let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
        let day_of_era = z - era * 146_097;
        let year_of_era =
            (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
        let year = year_of_era + era * 400;
        let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
        let mp = (5 * day_of_year + 2) / 153;
        let day = day_of_year - (153 * mp + 2) / 5 + 1;
        let month = if mp < 10 { mp + 3 } else { mp - 9 };
        let year = if month <= 2 { year + 1 } else { year };
        format!("{year:04}-{month:02}-{day:02}")
    }

    /// An array of any element type this module can decode.
    ///
    /// Rendered the way PostgreSQL itself writes an array literal — `{a,b,c}`,
    /// with `NULL` for an absent element — and flattened for a multi-dimensional
    /// array, which is a simplification stated here rather than hidden: the wire
    /// format carries the dimensions, and nesting the braces properly is a job
    /// for the round that gives a cell its own detail view.
    fn array(element: &Type, raw: &[u8]) -> Value {
        let Some(dimensions) = i32_at(raw, 0) else {
            return fallback(raw);
        };
        if dimensions == 0 {
            return Value::Text("{}".into());
        }
        // Header: ndim, has-null flag, element oid, then two i32 per dimension.
        let mut at = 12 + (dimensions.max(0) as usize) * 8;
        let mut rendered = Vec::new();

        while at + 4 <= raw.len() {
            let Some(length) = i32_at(raw, at) else {
                return fallback(raw);
            };
            at += 4;
            if length < 0 {
                rendered.push("NULL".to_string());
                continue;
            }
            let end = at + length as usize;
            let Some(slice) = raw.get(at..end) else {
                return fallback(raw);
            };
            rendered.push(from_bytes(element, slice).display());
            at = end;
        }

        Value::Text(format!("{{{}}}", rendered.join(",")))
    }
}

/// A `FromSql` that accepts every type and keeps the bytes.
///
/// This is what lets one decoding path serve every column: without it, reading
/// an unexpected type would need a `FromSql` impl for it, and a type nobody has
/// written an impl for could not be read at all.
struct Raw(Vec<u8>);

impl<'a> FromSql<'a> for Raw {
    fn from_sql(
        _ty: &Type,
        raw: &'a [u8],
    ) -> Result<Self, Box<dyn std::error::Error + Sync + Send>> {
        Ok(Raw(raw.to_vec()))
    }

    fn accepts(_ty: &Type) -> bool {
        true
    }
}

/// Tests that need a real PostgreSQL server.
///
/// **They skip themselves when `DODO_PG_TEST_HOST` is unset**, which is how
/// `cargo test` stays a no-dependency command on a laptop and in CI. Everything
/// they cover is the half of this module that cannot be faked: whether the
/// catalog SQL is valid against a live server, and whether the binary decoders
/// above agree with what PostgreSQL actually sends. The unit tests beside them
/// cover the decoders against fixed bytes; only a server can say those bytes
/// were right.
///
/// To run them, start a throwaway server and point the variables at it:
///
/// ```text
/// podman run -d --name dodo-pg -e POSTGRES_PASSWORD=pw -e POSTGRES_USER=dodo \
///     -e POSTGRES_DB=dodo_probe -p 55432:5432 docker.io/library/postgres:16-alpine
/// DODO_PG_TEST_HOST=127.0.0.1 DODO_PG_TEST_PORT=55432 DODO_PG_TEST_USER=dodo \
///     DODO_PG_TEST_PASSWORD=pw DODO_PG_TEST_DB=dodo_probe cargo test postgres::live
/// ```
///
/// Each test creates its own schema and drops it, so they neither collide with
/// each other nor leave anything behind.
#[cfg(test)]
mod live {
    use super::{PostgresDriver, connect};
    use crate::models::catalog::{NodeId, NodeKind, NodeLabel};
    use crate::models::connection::{ConnectionProfile, SslMode};
    use crate::models::detail::{DetailRequest, DetailTab, DetailTarget};
    use crate::models::engine::Engine;
    use crate::models::error::DbError;
    use crate::models::identity::{Editability, ReadOnlyReason};
    use crate::models::page::{PageBudget, PageBuffer};
    use crate::models::query::QueryRequest;
    use crate::models::statement::{Dialect, generate};
    use crate::models::value::Value;
    use crate::services::Driver;
    use crate::state::detail::{DetailLoad, load as load_detail};
    use crate::state::edit::PendingGrid;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn profile() -> Option<ConnectionProfile> {
        let host = std::env::var("DODO_PG_TEST_HOST").ok()?;
        let mut profile = ConnectionProfile::new(1, Engine::PostgreSql);
        profile.host = host;
        profile.port = std::env::var("DODO_PG_TEST_PORT")
            .ok()
            .and_then(|port| port.parse().ok())
            .unwrap_or(5432);
        profile.user = std::env::var("DODO_PG_TEST_USER").unwrap_or_else(|_| "postgres".into());
        profile.password = std::env::var("DODO_PG_TEST_PASSWORD").unwrap_or_default();
        profile.database = std::env::var("DODO_PG_TEST_DB").unwrap_or_else(|_| "postgres".into());
        profile.ssl_mode = SslMode::Disable;
        Some(profile)
    }

    /// A driver plus a schema of its own, dropped when the test ends.
    struct Fixture {
        driver: Arc<PostgresDriver>,
        schema: String,
    }

    impl Fixture {
        /// `None` when no server is configured, which is the skip signal.
        fn new(ddl: &str) -> Option<Self> {
            let driver = connect(&profile()?).expect("connects to the configured test server");

            static COUNTER: AtomicU64 = AtomicU64::new(0);
            let schema = format!(
                "dodo_test_{}_{}",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::Relaxed)
            );
            let fixture = Fixture {
                driver,
                schema: schema.clone(),
            };
            fixture.run(&format!("CREATE SCHEMA {schema}"));
            fixture.run(&format!("SET search_path TO {schema}"));
            for statement in ddl.split(";\n") {
                if !statement.trim().is_empty() {
                    fixture.run(statement);
                }
            }
            Some(fixture)
        }

        fn run(&self, statement: &str) {
            let mut sink = PageBuffer::default();
            self.driver
                .execute(&QueryRequest::new(statement), &mut sink)
                .unwrap_or_else(|err| panic!("{statement}: {err:?}"));
        }

        fn query(&self, statement: &str) -> PageBuffer {
            let mut sink = PageBuffer::default();
            self.driver
                .execute(&QueryRequest::new(statement), &mut sink)
                .unwrap_or_else(|err| panic!("{statement}: {err:?}"));
            sink
        }

        fn children(&self, id: Option<&str>) -> Vec<crate::models::catalog::CatalogNode> {
            let node = id.map(NodeId::new);
            self.driver.children(node.as_ref()).expect("children load")
        }

        fn names(&self, id: &str) -> Vec<String> {
            self.children(Some(id))
                .iter()
                .map(|node| match &node.label {
                    NodeLabel::Name(name) => name.clone(),
                    other => panic!("expected a server-named node, got {other:?}"),
                })
                .collect()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let mut sink = PageBuffer::default();
            let _ = self.driver.execute(
                &QueryRequest::new(format!("DROP SCHEMA IF EXISTS {} CASCADE", self.schema)),
                &mut sink,
            );
        }
    }

    macro_rules! fixture {
        ($ddl:expr) => {
            match Fixture::new($ddl) {
                Some(fixture) => fixture,
                // No server configured: this is a skip, not a pass.
                None => return,
            }
        };
    }

    const DDL: &str = "CREATE TABLE users (
            id serial PRIMARY KEY,
            name varchar(50) NOT NULL,
            balance numeric(12,2),
            active boolean,
            tags text[],
            profile jsonb,
            token uuid,
            born date,
            seen timestamptz,
            avatar bytea
        );
CREATE INDEX users_name_idx ON users (name);
CREATE TABLE orders (id serial PRIMARY KEY, user_id integer REFERENCES users(id));
CREATE VIEW active_users AS SELECT id, name FROM users WHERE active;
INSERT INTO users (name, balance, active, tags, profile, token, born, seen, avatar)
    VALUES ('ada', 1234.56, true, ARRAY['a','b c'], '{\"k\": 1}'::jsonb,
            '00010203-0405-0607-0809-0a0b0c0d0e0f'::uuid, DATE '2000-02-29',
            TIMESTAMPTZ '2024-03-04 05:06:07.5+00', '\\x0102'::bytea)";

    #[test]
    fn a_live_server_connects_and_answers_a_ping() {
        let fixture = fixture!("");
        assert!(fixture.driver.ping().is_ok());
    }

    #[test]
    fn safe_mutations_use_wire_identity_and_one_atomic_transaction() {
        let fixture = fixture!(
            "CREATE TABLE users (id serial PRIMARY KEY, name text NOT NULL CHECK(name <> 'boom'));
INSERT INTO users(name) VALUES ('Ada'), ('Grace');
CREATE TABLE roles (id serial PRIMARY KEY, name text NOT NULL);
INSERT INTO roles(name) VALUES ('admin');
CREATE TABLE nullable_only (email text UNIQUE, name text)"
        );
        let page = fixture.query("SELECT id, name FROM users ORDER BY id");
        let editable = fixture.driver.editability(page.columns());
        assert!(matches!(editable, Editability::Editable(_)));

        let join = fixture
            .query("SELECT users.id, roles.name FROM users JOIN roles ON roles.id = users.id");
        assert!(matches!(
            fixture.driver.editability(join.columns()),
            Editability::ReadOnly(ReadOnlyReason::MultipleTables)
        ));
        let missing = fixture.query("SELECT name FROM users");
        assert!(matches!(
            fixture.driver.editability(missing.columns()),
            Editability::ReadOnly(ReadOnlyReason::MissingIdentityColumns { .. })
        ));
        let nullable = fixture.query("SELECT email, name FROM nullable_only");
        assert!(matches!(
            fixture.driver.editability(nullable.columns()),
            Editability::ReadOnly(ReadOnlyReason::NoUniqueIdentity(_))
        ));
        let union = fixture.query("SELECT id, name FROM users UNION SELECT id, name FROM users");
        assert!(matches!(
            fixture.driver.editability(union.columns()),
            Editability::ReadOnly(ReadOnlyReason::MissingOrigin(_))
        ));

        let mut pending = PendingGrid::new(page.rows().to_vec(), editable);
        pending
            .edit(0, 1, Value::Text("Ada Lovelace".into()))
            .unwrap();
        pending.delete(1).unwrap();
        let mut duplicate = pending.duplicate_template(0).unwrap();
        duplicate[1] = Value::Text("Ada Copy".into());
        pending.insert(duplicate).unwrap();
        let mut added = pending.add_template().unwrap();
        added[1] = Value::Text("Katherine".into());
        pending.insert(added).unwrap();
        let batch = generate(
            pending.source().unwrap(),
            &pending.mutations(),
            Dialect::PostgreSql,
        )
        .unwrap();
        fixture.driver.commit(&batch).unwrap();
        assert_eq!(
            fixture
                .query("SELECT name FROM users ORDER BY id")
                .rows()
                .iter()
                .map(|row| row[0].display())
                .collect::<Vec<_>>(),
            ["Ada Lovelace", "Ada Copy", "Katherine"]
        );

        let page = fixture.query("SELECT id, name FROM users ORDER BY id");
        let mut pending = PendingGrid::new(
            page.rows().to_vec(),
            fixture.driver.editability(page.columns()),
        );
        pending.edit(0, 1, Value::Text("changed".into())).unwrap();
        pending.edit(1, 1, Value::Text("boom".into())).unwrap();
        let batch = generate(
            pending.source().unwrap(),
            &pending.mutations(),
            Dialect::PostgreSql,
        )
        .unwrap();
        assert!(fixture.driver.commit(&batch).is_err());
        assert_eq!(
            fixture.query("SELECT name FROM users ORDER BY id").rows()[0][0],
            Value::Text("Ada Lovelace".into()),
            "the first update must roll back when the second fails"
        );
    }

    /// The catalog SQL is the half of this module that cannot be faked: it is
    /// only correct if a real server accepts it and answers what was expected.
    #[test]
    fn the_catalog_walks_database_to_schema_to_table_to_column() {
        let fixture = fixture!(DDL);

        let roots = fixture.children(None);
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].kind, NodeKind::Database);

        let schemas = fixture.names(super::id::DATABASE);
        assert!(
            schemas.contains(&fixture.schema),
            "the test's own schema is missing from {schemas:?}"
        );
        assert!(
            !schemas
                .iter()
                .any(|name| name.starts_with("pg_") || name == "information_schema"),
            "the server's own bookkeeping schemas leaked into {schemas:?}"
        );

        let groups = fixture.children(Some(&super::id::schema(&fixture.schema)));
        assert_eq!(groups.len(), 2, "Tables and Views");

        let tables = fixture.names(&super::id::tables_group(&fixture.schema));
        assert_eq!(tables, ["orders", "users"]);

        let views = fixture.names(&super::id::views_group(&fixture.schema));
        assert_eq!(views, ["active_users"]);

        let columns = fixture.children(Some(&super::id::relation_group(
            "gc",
            &fixture.schema,
            "users",
        )));
        let names: Vec<String> = columns
            .iter()
            .map(|node| match &node.label {
                NodeLabel::Name(name) => name.clone(),
                other => panic!("got {other:?}"),
            })
            .collect();
        assert_eq!(names[0], "id");
        assert_eq!(names[1], "name");
        assert_eq!(
            columns[1].detail.as_deref(),
            Some("character varying(50) NOT NULL"),
            "format_type is what makes the length worth reading"
        );
    }

    #[test]
    fn indexes_and_constraints_load_for_a_real_table() {
        let fixture = fixture!(DDL);

        let indexes = fixture.names(&super::id::relation_group("gi", &fixture.schema, "users"));
        assert!(indexes.iter().any(|name| name == "users_name_idx"));
        assert!(
            indexes.iter().any(|name| name.ends_with("_pkey")),
            "the primary key's own index is missing from {indexes:?}"
        );

        let constraints = fixture.children(Some(&super::id::relation_group(
            "gk",
            &fixture.schema,
            "orders",
        )));
        let definitions: Vec<String> = constraints
            .iter()
            .filter_map(|node| node.detail.clone())
            .collect();
        assert!(
            definitions.iter().any(|text| text.contains("FOREIGN KEY")),
            "no foreign key in {definitions:?}"
        );
    }

    #[test]
    fn object_detail_metadata_and_reconstructed_ddl_are_valid_on_a_live_server() {
        let fixture = fixture!(DDL);
        let target = DetailTarget::new(
            NodeId::new(super::id::relation("t", &fixture.schema, "users")),
            NodeKind::Table,
            "users",
        );
        let request = |tab| DetailRequest::new(target.clone(), tab, 0);

        let DetailLoad::Grid(columns) =
            load_detail(fixture.driver.as_ref(), &request(DetailTab::Columns))
        else {
            panic!("columns did not load");
        };
        assert_eq!(columns.grid.rows()[0][0], Value::Text("id".into()));
        assert!(
            columns
                .grid
                .rows()
                .iter()
                .any(|row| { row[1] == Value::Text("character varying(50)".into()) })
        );

        let DetailLoad::Grid(indexes) =
            load_detail(fixture.driver.as_ref(), &request(DetailTab::Indexes))
        else {
            panic!("indexes did not load");
        };
        assert!(
            indexes
                .grid
                .rows()
                .iter()
                .any(|row| { row[0] == Value::Text("users_name_idx".into()) })
        );

        let DetailLoad::Grid(constraints) =
            load_detail(fixture.driver.as_ref(), &request(DetailTab::Constraints))
        else {
            panic!("constraints did not load");
        };
        assert!(
            constraints
                .grid
                .rows()
                .iter()
                .any(|row| { row[1].display().contains("PRIMARY KEY") })
        );

        let DetailLoad::Ddl(ddl) = load_detail(fixture.driver.as_ref(), &request(DetailTab::Ddl))
        else {
            panic!("DDL did not load");
        };
        assert!(ddl.starts_with(&format!("CREATE TABLE \"{}\".\"users\"", fixture.schema)));
        assert!(ddl.contains("character varying(50) NOT NULL"));
        assert!(ddl.contains("CREATE INDEX users_name_idx"));
    }

    #[test]
    fn a_view_offers_columns_and_no_indexes() {
        let fixture = fixture!(DDL);
        let groups = fixture.children(Some(&super::id::relation(
            "v",
            &fixture.schema,
            "active_users",
        )));
        assert_eq!(groups.len(), 1);
    }

    /// The other half that cannot be faked: whether the binary decoders agree
    /// with what the server actually sends.
    #[test]
    fn every_decoded_type_round_trips_through_a_real_server() {
        let fixture = fixture!(DDL);
        let page = fixture.query(
            "SELECT id, name, balance, active, tags, profile, token, born, seen, avatar \
             FROM users ORDER BY id",
        );

        let types: Vec<&str> = page
            .columns()
            .iter()
            .map(|column| column.type_name.as_str())
            .collect();
        assert_eq!(
            types,
            [
                "int4",
                "varchar",
                "numeric",
                "bool",
                "text[]",
                "jsonb",
                "uuid",
                "date",
                "timestamptz",
                "bytea"
            ]
        );

        let row = &page.rows()[0];
        assert_eq!(row[0], Value::Int(1));
        assert_eq!(row[1], Value::Text("ada".into()));
        assert_eq!(
            row[2],
            Value::Text("1234.56".into()),
            "numeric must not be rounded through a float"
        );
        assert_eq!(row[3], Value::Bool(true));
        assert_eq!(row[4], Value::Text("{a,b c}".into()));
        assert_eq!(row[5], Value::Json(r#"{"k": 1}"#.into()));
        assert_eq!(
            row[6],
            Value::Text("00010203-0405-0607-0809-0a0b0c0d0e0f".into())
        );
        assert_eq!(row[7], Value::Text("2000-02-29".into()));
        assert_eq!(row[8], Value::Text("2024-03-04 05:06:07.5+00".into()));
        assert_eq!(row[9], Value::Bytes(vec![1, 2]));
    }

    #[test]
    fn a_null_from_a_real_server_is_null_and_not_an_empty_string() {
        let fixture = fixture!(DDL);
        let page = fixture.query("SELECT NULL::text, ''::text");
        assert_eq!(page.rows()[0][0], Value::Null);
        assert_eq!(page.rows()[0][1], Value::Text(String::new()));
    }

    /// An enum is the case the fallback exists for, and the only way to know it
    /// works is to make one.
    #[test]
    fn a_user_defined_enum_reads_as_its_label() {
        let fixture = fixture!("CREATE TYPE mood AS ENUM ('sad', 'happy')");
        let page = fixture.query(&format!("SELECT 'happy'::{}.mood", fixture.schema));
        assert_eq!(page.rows()[0][0], Value::Text("happy".into()));
        assert_eq!(page.columns()[0].type_name, "mood");
    }

    #[test]
    fn a_statement_that_changes_rows_reports_how_many() {
        let fixture = fixture!(DDL);
        let mut sink = PageBuffer::default();
        let execution = fixture
            .driver
            .execute(
                &QueryRequest::new("UPDATE users SET active = false"),
                &mut sink,
            )
            .expect("runs");
        assert_eq!(execution.rows_affected, Some(1));
        assert!(sink.rows().is_empty());
    }

    /// The memory bound, against a server rather than against the sink alone.
    #[test]
    fn the_page_budget_stops_a_real_result_and_says_there_was_more() {
        let fixture = fixture!("");
        let mut sink = PageBuffer::new(PageBudget {
            max_rows: 25,
            ..PageBudget::default()
        });
        let execution = fixture
            .driver
            .execute(
                &QueryRequest::new("SELECT generate_series(1, 100000) AS n"),
                &mut sink,
            )
            .expect("runs");

        assert_eq!(sink.rows().len(), 25);
        assert!(execution.truncated);
        assert!(sink.truncated());
    }

    /// **The round's centrepiece, proved against a real server.**
    ///
    /// Dropping a task is not cancelling: the statement keeps running, the
    /// server keeps burning CPU and the connection stays held. So this asserts
    /// three separate things, and the third is the one that matters:
    ///
    /// 1. The interrupted `execute` comes back as [`DbError::Cancelled`] — the
    ///    server's own `57014`, mapped in [`server_error`], not a label dodo
    ///    applied because it pressed a button.
    /// 2. It comes back *early*. `pg_sleep(30)` finishing in under a second is
    ///    only possible if the server abandoned it.
    /// 3. **A second connection sees no trace of it left running.** That is the
    ///    only observation made from outside the cancelled session, and it is
    ///    what rules out "dodo stopped waiting while the server carried on":
    ///    `pg_stat_activity` is the server's own account of what it is doing.
    #[test]
    fn cancelling_stops_the_statement_at_the_server_and_not_merely_in_dodo() {
        let fixture = fixture!("");
        let Some(profile) = profile() else { return };
        // A second connection, so the check is made from outside the session
        // being cancelled — the same way a DBA would look.
        let observer = connect(&profile).expect("a second connection");

        // The query names itself, so `pg_stat_activity` can be searched for it
        // without matching this test's own lookup.
        const MARKER: &str = "dodo_r2_cancel_probe";
        let statement = format!("SELECT pg_sleep(30) /* {MARKER} */");

        let handle = fixture
            .driver
            .cancel_handle()
            .expect("PostgreSQL reports the cancel capability");
        let started = std::time::Instant::now();
        let cancelling = std::thread::spawn(move || {
            // Long enough for the sleep to be underway, so this is a genuine
            // cancellation of a running statement rather than a race with its
            // start-up.
            std::thread::sleep(std::time::Duration::from_millis(400));
            handle
                .cancel()
                .expect("the cancel request reaches the server")
        });

        let mut sink = PageBuffer::default();
        let outcome = fixture
            .driver
            .execute(&QueryRequest::new(statement), &mut sink);
        let elapsed = started.elapsed();
        cancelling.join().expect("the cancelling thread finished");

        assert!(
            matches!(outcome, Err(DbError::Cancelled)),
            "expected the server's own 57014, got {outcome:?}"
        );
        assert!(
            elapsed < std::time::Duration::from_secs(5),
            "a 30-second sleep that took {elapsed:?} was not cancelled, it was waited out"
        );

        // The proof. `pg_sleep` is interruptible, so the backend either stopped
        // or is still sitting in it; only the server can say which. The
        // `pg_stat_activity` clause excludes this very lookup, which also
        // contains the marker.
        let mut observed = PageBuffer::default();
        observer
            .execute(
                &QueryRequest::new(format!(
                    "SELECT count(*) FROM pg_stat_activity \
                     WHERE state = 'active' AND query LIKE '%{MARKER}%' \
                     AND query NOT LIKE '%pg_stat_activity%'"
                )),
                &mut observed,
            )
            .expect("the observer can read pg_stat_activity");
        assert_eq!(
            observed.rows()[0][0],
            Value::Int(0),
            "the statement is still active on the server: it was abandoned, not cancelled"
        );
    }

    /// The connection is usable afterwards, which is the other half of "the
    /// server stopped it": a cancel that killed the session would look the same
    /// from the cancelled statement's point of view and be far worse.
    #[test]
    fn a_cancelled_connection_still_works_for_the_next_statement() {
        let fixture = fixture!("");
        let handle = fixture.driver.cancel_handle().expect("a handle");
        let cancelling = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(300));
            let _ = handle.cancel();
        });

        let mut sink = PageBuffer::default();
        let _ = fixture
            .driver
            .execute(&QueryRequest::new("SELECT pg_sleep(30)"), &mut sink);
        cancelling.join().expect("the cancelling thread finished");

        let page = fixture.query("SELECT 1 AS still_here");
        assert_eq!(page.rows()[0][0], Value::Int(1));
    }

    #[test]
    fn a_rejected_statement_carries_the_servers_sqlstate() {
        let fixture = fixture!("");
        let mut sink = PageBuffer::default();
        match fixture
            .driver
            .execute(&QueryRequest::new("SELECT * FROM no_such_table"), &mut sink)
        {
            Err(DbError::Server { code, detail }) => {
                assert_eq!(code.as_deref(), Some("42P01"), "undefined_table");
                assert!(
                    detail.contains("no_such_table"),
                    "lost the message: {detail}"
                );
            }
            other => panic!("expected a server error, got {other:?}"),
        }
    }

    #[test]
    fn a_wrong_password_is_unreachable_rather_than_a_server_error() {
        let Some(mut profile) = profile() else {
            return;
        };
        profile.password = "definitely-not-the-password".into();
        assert!(matches!(connect(&profile), Err(DbError::Unreachable(_))));
    }
}

#[cfg(test)]
mod tests {
    use super::decode::{date, from_bytes, numeric, time, timestamp, uuid};
    use super::{column_detail, id, index_detail, quote_identifier, type_name};
    use crate::models::value::Value;
    use postgres::types::Type;

    // ---- node ids -------------------------------------------------------

    /// The reason the delimiter is a control character: every obvious
    /// alternative is legal inside a quoted identifier.
    #[test]
    fn a_node_id_survives_an_identifier_full_of_punctuation() {
        let awkward = "my.odd:table/name with spaces";
        let built = id::relation("t", "public", awkward);
        let (tag, parts) = id::parse(&built).expect("parses");
        assert_eq!(tag, "t");
        assert_eq!(parts, ["public", awkward]);
    }

    #[test]
    fn every_id_shape_round_trips_to_its_tag_and_parts() {
        for (built, tag, parts) in [
            (id::schema("public"), "s", vec!["public"]),
            (id::tables_group("public"), "gt", vec!["public"]),
            (id::views_group("public"), "gv", vec!["public"]),
            (
                id::relation_group("gc", "public", "users"),
                "gc",
                vec!["public", "users"],
            ),
            (
                id::leaf("c", "public", "users", "id"),
                "c",
                vec!["public", "users", "id"],
            ),
        ] {
            let (found_tag, found_parts) = id::parse(&built).expect("parses");
            assert_eq!(found_tag, tag, "for {built:?}");
            assert_eq!(found_parts, parts, "for {built:?}");
        }
        assert_eq!(quote_identifier("odd\"name"), "\"odd\"\"name\"");
    }

    // ---- tree detail text ----------------------------------------------

    #[test]
    fn a_columns_detail_names_its_type_and_says_when_it_is_required() {
        assert_eq!(column_detail("integer", false), "integer");
        assert_eq!(
            column_detail("character varying(255)", true),
            "character varying(255) NOT NULL"
        );
    }

    #[test]
    fn an_index_that_is_a_primary_key_does_not_also_say_unique() {
        assert_eq!(index_detail(true, true), "PRIMARY KEY");
        assert_eq!(index_detail(true, false), "UNIQUE");
        assert_eq!(index_detail(false, false), "");
    }

    #[test]
    fn an_array_type_reads_the_way_a_person_writes_it() {
        assert_eq!(type_name(&Type::INT4), "int4");
        assert_eq!(
            type_name(&Type::INT4_ARRAY),
            "int4[]",
            "the internal name is `_int4`, which nobody writes"
        );
    }

    // ---- scalar decoding ------------------------------------------------

    #[test]
    fn integers_decode_from_big_endian_including_negatives() {
        assert_eq!(from_bytes(&Type::INT2, &[0x00, 0x2a]), Value::Int(42));
        assert_eq!(from_bytes(&Type::INT2, &[0xff, 0xd6]), Value::Int(-42));
        assert_eq!(
            from_bytes(&Type::INT4, &[0x00, 0x00, 0x01, 0x00]),
            Value::Int(256)
        );
        assert_eq!(
            from_bytes(&Type::INT4, &[0xff, 0xff, 0xff, 0xff]),
            Value::Int(-1)
        );
        assert_eq!(
            from_bytes(&Type::INT8, &(-9_000_000_000i64).to_be_bytes()),
            Value::Int(-9_000_000_000)
        );
    }

    #[test]
    fn booleans_and_floats_decode() {
        assert_eq!(from_bytes(&Type::BOOL, &[1]), Value::Bool(true));
        assert_eq!(from_bytes(&Type::BOOL, &[0]), Value::Bool(false));
        assert_eq!(
            from_bytes(&Type::FLOAT8, &1.5f64.to_be_bytes()),
            Value::Float(1.5)
        );
        assert_eq!(
            from_bytes(&Type::FLOAT4, &2.5f32.to_be_bytes()),
            Value::Float(2.5)
        );
    }

    #[test]
    fn text_and_bytes_decode_to_their_own_kinds() {
        assert_eq!(
            from_bytes(&Type::TEXT, "xin chào".as_bytes()),
            Value::Text("xin chào".into())
        );
        assert_eq!(
            from_bytes(&Type::BYTEA, &[0u8, 1, 2]),
            Value::Bytes(vec![0, 1, 2])
        );
    }

    #[test]
    fn json_decodes_as_json_and_jsonb_skips_its_version_byte() {
        assert_eq!(
            from_bytes(&Type::JSON, br#"{"a":1}"#),
            Value::Json(r#"{"a":1}"#.into())
        );

        let mut jsonb = vec![1u8];
        jsonb.extend_from_slice(br#"{"a":1}"#);
        assert_eq!(
            from_bytes(&Type::JSONB, &jsonb),
            Value::Json(r#"{"a":1}"#.into())
        );
    }

    /// The property that makes `numeric` worth decoding by hand: it must not
    /// round, because not rounding is the reason the column is `numeric`.
    #[test]
    fn numeric_keeps_every_digit_it_was_given() {
        // 1234.5678 — two base-10000 digits, weight 0, scale 4.
        let raw = numeric_bytes(0x0000, 0, 4, &[1234, 5678]);
        assert_eq!(numeric(&raw).as_deref(), Some("1234.5678"));

        // A money-shaped value that a float would not hold exactly.
        let raw = numeric_bytes(0x0000, 0, 2, &[1, 1000]);
        assert_eq!(numeric(&raw).as_deref(), Some("1.10"));
    }

    #[test]
    fn numeric_handles_sign_zero_and_a_value_below_one() {
        assert_eq!(
            numeric(&numeric_bytes(0x4000, 0, 2, &[5, 5000])).as_deref(),
            Some("-5.50")
        );
        assert_eq!(
            numeric(&numeric_bytes(0x0000, 0, 0, &[])).as_deref(),
            Some("0")
        );
        // 0.0001234 — weight -1, so there is no integer part and the digits
        // start at the first group of four decimal places.
        assert_eq!(
            numeric(&numeric_bytes(0x0000, -1, 7, &[1, 2340])).as_deref(),
            Some("0.0001234")
        );
        // 0.00001234 — weight -2, so the *first* group of four decimal places
        // comes from no digit at all and has to be filled with zeroes. This is
        // the case a loop that starts at `digits[0]` gets wrong.
        assert_eq!(
            numeric(&numeric_bytes(0x0000, -2, 8, &[1234])).as_deref(),
            Some("0.00001234")
        );
    }

    #[test]
    fn numeric_special_values_are_named_rather_than_decoded() {
        assert_eq!(
            numeric(&numeric_bytes(0xC000, 0, 0, &[])).as_deref(),
            Some("NaN")
        );
        assert_eq!(
            numeric(&numeric_bytes(0xD000, 0, 0, &[])).as_deref(),
            Some("Infinity")
        );
        assert_eq!(
            numeric(&numeric_bytes(0xF000, 0, 0, &[])).as_deref(),
            Some("-Infinity")
        );
    }

    #[test]
    fn a_uuid_reads_in_its_canonical_form() {
        let raw: Vec<u8> = (0..16).collect();
        assert_eq!(
            uuid(&raw).as_deref(),
            Some("00010203-0405-0607-0809-0a0b0c0d0e0f")
        );
        assert_eq!(uuid(&[0, 1]), None);
    }

    #[test]
    fn dates_decode_around_the_postgres_epoch_and_across_leap_years() {
        assert_eq!(date(&0i32.to_be_bytes()).as_deref(), Some("2000-01-01"));
        assert_eq!(date(&1i32.to_be_bytes()).as_deref(), Some("2000-01-02"));
        // 2000 is a leap year: day 59 is 29 February.
        assert_eq!(date(&59i32.to_be_bytes()).as_deref(), Some("2000-02-29"));
        // Before the epoch.
        assert_eq!(date(&(-1i32).to_be_bytes()).as_deref(), Some("1999-12-31"));
        // 1900 was *not* a leap year, which is what catches a naive algorithm.
        assert_eq!(
            date(&(-36_524i32).to_be_bytes()).as_deref(),
            Some("1900-01-01")
        );
    }

    #[test]
    fn times_drop_a_fractional_part_they_do_not_have() {
        assert_eq!(time(&0i64.to_be_bytes()).as_deref(), Some("00:00:00"));
        assert_eq!(
            time(&(13 * 3_600_000_000i64 + 45 * 60_000_000).to_be_bytes()).as_deref(),
            Some("13:45:00")
        );
        assert_eq!(
            time(&(1_500_000i64).to_be_bytes()).as_deref(),
            Some("00:00:01.5")
        );
    }

    #[test]
    fn timestamps_say_which_zone_they_are_in_and_floor_before_the_epoch() {
        assert_eq!(
            timestamp(&0i64.to_be_bytes(), false).as_deref(),
            Some("2000-01-01 00:00:00")
        );
        assert_eq!(
            timestamp(&0i64.to_be_bytes(), true).as_deref(),
            Some("2000-01-01 00:00:00+00")
        );
        // One microsecond before the epoch is the last instant of 1999, not
        // the first of 2000 — the bug a truncating division would introduce.
        assert_eq!(
            timestamp(&(-1i64).to_be_bytes(), false).as_deref(),
            Some("1999-12-31 23:59:59.999999")
        );
    }

    // ---- the fallback ---------------------------------------------------

    /// An enum's binary form is its label, so the fallback is not a
    /// consolation prize for it — it is the right answer.
    #[test]
    fn an_undecodable_type_that_is_text_underneath_reads_as_text() {
        let enum_like = Type::new(
            "mood".into(),
            16_384,
            postgres::types::Kind::Enum(vec!["happy".into()]),
            "public".into(),
        );
        assert_eq!(
            from_bytes(&enum_like, b"happy"),
            Value::Text("happy".into())
        );
    }

    #[test]
    fn an_undecodable_type_that_is_not_text_reads_as_bytes_rather_than_mangled() {
        assert_eq!(
            from_bytes(&Type::INTERVAL, &[0xff, 0xfe, 0x00, 0x01]),
            Value::Bytes(vec![0xff, 0xfe, 0x00, 0x01])
        );
    }

    // ---- arrays ---------------------------------------------------------

    #[test]
    fn an_array_reads_the_way_postgres_writes_one() {
        let raw = array_bytes(&[Some(&1i32.to_be_bytes()), Some(&2i32.to_be_bytes()), None]);
        assert_eq!(
            from_bytes(&Type::INT4_ARRAY, &raw),
            Value::Text("{1,2,NULL}".into())
        );
    }

    #[test]
    fn an_empty_array_is_empty_braces() {
        assert_eq!(
            from_bytes(&Type::INT4_ARRAY, &0i32.to_be_bytes()),
            Value::Text("{}".into())
        );
    }

    #[test]
    fn a_text_array_keeps_its_elements() {
        let raw = array_bytes(&[Some(b"a"), Some(b"b c")]);
        assert_eq!(
            from_bytes(&Type::TEXT_ARRAY, &raw),
            Value::Text("{a,b c}".into())
        );
    }

    // ---- fixtures -------------------------------------------------------

    /// The wire form of a `numeric`: digit count, weight, sign, scale, then the
    /// base-10000 digits.
    fn numeric_bytes(sign: u16, weight: i16, scale: i16, digits: &[i16]) -> Vec<u8> {
        let mut raw = Vec::new();
        raw.extend_from_slice(&(digits.len() as i16).to_be_bytes());
        raw.extend_from_slice(&weight.to_be_bytes());
        raw.extend_from_slice(&sign.to_be_bytes());
        raw.extend_from_slice(&scale.to_be_bytes());
        for digit in digits {
            raw.extend_from_slice(&digit.to_be_bytes());
        }
        raw
    }

    /// The wire form of a one-dimensional array: header, then a length and the
    /// bytes per element, with -1 for a NULL.
    fn array_bytes(elements: &[Option<&[u8]>]) -> Vec<u8> {
        let mut raw = Vec::new();
        raw.extend_from_slice(&1i32.to_be_bytes()); // one dimension
        raw.extend_from_slice(&0i32.to_be_bytes()); // has-null flag
        raw.extend_from_slice(&0i32.to_be_bytes()); // element oid
        raw.extend_from_slice(&(elements.len() as i32).to_be_bytes());
        raw.extend_from_slice(&1i32.to_be_bytes()); // lower bound
        for element in elements {
            match element {
                Some(bytes) => {
                    raw.extend_from_slice(&(bytes.len() as i32).to_be_bytes());
                    raw.extend_from_slice(bytes);
                }
                None => raw.extend_from_slice(&(-1i32).to_be_bytes()),
            }
        }
        raw
    }
}
