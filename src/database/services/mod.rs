//! The driver layer the state stores talk to.
//!
//! # Why a trait
//!
//! The same discipline as `api_explorer::services::Transport` and
//! `docker::services::DockerEngine`: the state holds an `Arc<dyn Driver>` and
//! never learns which crate is behind it. It hands over a statement and gets
//! back [`models`](crate::database::models) types. A second backend is another
//! [`Driver`] and nothing above this module changes. **This module and its
//! children are the only place that may name `postgres`, `rusqlite`, `mysql`,
//! `redis`, `rustls` or `tokio-postgres-rustls`.**
//!
//! # Threading — blocking by contract
//!
//! Every method here performs blocking IO and is **always** invoked from GPUI's
//! background executor, never from the UI thread. `Send + Sync + 'static` is
//! what lets one be shared as an `Arc` across those tasks.
//!
//! `rusqlite::Connection` is `!Sync`, so [`sqlite`] holds it behind a `Mutex`.
//! That is not a workaround, it is the right model for this app: a desktop
//! client runs one statement per connection at a time, and serializing is what
//! will later make "cancel" refer to an unambiguous statement.
//!
//! # The capability set is small on purpose
//!
//! [`Capabilities`] carries what *the shipped UI reads*, and nothing else. The
//! design report proposes fields for transactions, column provenance and
//! `LIMIT`/`OFFSET` support — every one of which describes a control that is
//! not built, so every one would be a value no code reads. That is the same
//! reasoning `Cargo.toml` records for the marker features this crate declined
//! to add: a flag nothing reads is decoration, and decoration has to be
//! maintained, tested and kept honest.
//!
//! The extension point is the struct and [`Driver::capabilities`], not a
//! pre-filled list of guesses. **A field arrives with the control that reads
//! it**: round 2 added `cancel` and `explain`; round 3 adds `detail` and the DDL
//! source with the object-detail surface. Round 4 needs no new capability:
//! MySQL fills the existing SQL fields, while Redis reports only plain-text
//! editing and key detail and leaves cancel, Explain and DDL absent.
//!
//! # Cancellation is against the server, and that is the whole point
//!
//! Dropping the GPUI task that is waiting for a query is *not* cancelling it:
//! the blocking call keeps running on its background thread, the server keeps
//! burning CPU, and the connection stays held. [`CancelHandle`] is the real
//! thing — PostgreSQL's protocol CancelRequest on a second connection, SQLite's
//! `sqlite3_interrupt` — and both drivers take theirs **at connect time**,
//! because the connection's `Mutex` is held for as long as the statement runs
//! and a handle fetched afterwards would block behind the query it is meant to
//! stop.
//!
//! A handle is blocking IO like everything else here (PostgreSQL's opens a
//! whole second connection), so it is invoked from the background executor too.
//!
//! # How a driver that is not a SQL server fits
//!
//! This trait was shaped so that a key/value store — the design report works
//! the example through with Redis — fits without contorting it and without the
//! views changing:
//!
//! | Trait member | How such a driver answers |
//! |---|---|
//! | [`capabilities`](Driver::capabilities) | `editor_language: "text"` — its console is not SQL, and it does not pretend to be |
//! | [`ping`](Driver::ping) | its own no-op command |
//! | [`children`](Driver::children) | numbered keyspaces at the root, then one node per value type, then keys — no schemas, no tables, and nothing above `services/` notices |
//! | [`execute`](Driver::execute) | one [`ColumnMeta`](crate::database::models::value::ColumnMeta) per reply field, one [`Row`](crate::database::models::value::Row) per reply |
//!
//! The two things that make that work are both in `models/`: the tree is a
//! *question* (`children of this node`) rather than a fixed ladder, and a cell
//! is dodo's own [`Value`](crate::database::models::value::Value) rather than a
//! driver's type.

pub mod connection_store;
pub mod export;
pub mod mysql;
pub mod postgres;
pub mod redis;
pub mod sqlite;

#[cfg(test)]
pub mod fake;

use std::sync::Arc;

use crate::database::models::catalog::{CatalogNode, NodeId};
use crate::database::models::connection::ConnectionProfile;
use crate::database::models::detail::{DdlSource, DetailField, DetailNotice, DetailRequest};
use crate::database::models::engine::Engine;
use crate::database::models::error::DbError;
use crate::database::models::page::RowSink;
use crate::database::models::query::{Execution, QueryRequest};

/// What a driver can do, as far as the UI needs to know.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Capabilities {
    /// The language id handed to the query editor's `code_editor`, so a backend
    /// whose console is not SQL is never coloured as though it were.
    pub editor_language: &'static str,
    /// [`Driver::cancel_handle`] returns something. Read by the Cancel button,
    /// which is not offered at all when this is `false` — a control that
    /// silently does nothing teaches the user the wrong thing about their
    /// database.
    pub cancel: bool,
    /// [`Driver::explain_statement`] returns something. Read by the Explain
    /// button, which is **absent** rather than disabled where it is false — the
    /// same posture as the result grid's missing sort affordance, and for the
    /// same reason: a disabled control invites the question every time.
    pub explain: bool,
    /// Table and view nodes can open the object-detail surface.
    pub detail: bool,
    /// Where this backend obtains the DDL shown in that surface.
    pub ddl: DdlSource,
}

/// What one object-detail load produced beside rows sent to its sink.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DetailResult {
    Rows {
        /// `None` for table data, whose column names came from the database.
        fields: Option<Vec<DetailField>>,
        truncated: bool,
        notice: Option<DetailNotice>,
    },
    Ddl(String),
    Unavailable,
}

/// A way to stop whatever a connection is running, **from another thread**.
///
/// Cloneable and `Send + Sync` because the UI hands one to a background task
/// while the query it stops is executing on a different one. Calling it when
/// nothing is running is harmless on both backends, but the UI only offers it
/// while a run is in flight.
///
/// Blocking: PostgreSQL's implementation opens a second connection to send the
/// protocol's CancelRequest. Never call it on the UI thread.
#[derive(Clone)]
pub struct CancelHandle(Arc<dyn Fn() -> Result<(), DbError> + Send + Sync>);

impl CancelHandle {
    pub fn new(cancel: impl Fn() -> Result<(), DbError> + Send + Sync + 'static) -> Self {
        Self(Arc::new(cancel))
    }

    /// Asks the server to stop the statement in flight.
    ///
    /// `Ok` means the *request* was delivered, not that anything stopped —
    /// PostgreSQL's protocol says nothing back about whether a cancellation
    /// took effect, and a query that finished a microsecond earlier was never
    /// cancelled at all. The evidence that it worked is the driver returning
    /// [`DbError::Cancelled`] from `execute`, which comes from the server.
    pub fn cancel(&self) -> Result<(), DbError> {
        (self.0)()
    }
}

impl std::fmt::Debug for CancelHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CancelHandle")
    }
}

/// One live connection to one database.
///
/// `&self` rather than `&mut self` because a live connection is shared as an
/// `Arc`; a driver whose handle is `!Sync` holds it behind a `Mutex`.
pub trait Driver: Send + Sync + 'static {
    fn capabilities(&self) -> Capabilities;

    /// Cheapest round trip that proves the connection is alive. Used by Test
    /// connection and by Reconnect.
    fn ping(&self) -> Result<(), DbError>;

    /// The children of `parent`, or the roots when `parent` is `None`.
    ///
    /// **One call per expanded node — this *is* the lazy loading.** Nothing
    /// above knows or cares that PostgreSQL puts schemas under a database and
    /// SQLite does not.
    fn children(&self, parent: Option<&NodeId>) -> Result<Vec<CatalogNode>, DbError>;

    /// Runs one statement, streaming its rows into `sink`.
    ///
    /// The statement is sent **exactly as given**. The driver does not rewrite
    /// it, does not append a `LIMIT`, and does not decide from its first word
    /// whether it returns rows — the server says which, and `Execution` reports
    /// what the server said.
    fn execute(&self, request: &QueryRequest, sink: &mut dyn RowSink)
    -> Result<Execution, DbError>;

    /// Splits an editor buffer into commands. SQL is the default; a non-SQL
    /// console overrides this without teaching the state or views its syntax.
    fn statements(&self, buffer: &str) -> Vec<String> {
        crate::database::models::split::split_statements(buffer)
    }

    /// A handle that stops whatever this connection is running.
    ///
    /// `None` exactly when `capabilities().cancel` is false. Callers take one
    /// **before** starting a statement: a driver that serialises on a `Mutex`
    /// cannot hand one out while the statement it would stop holds the lock.
    fn cancel_handle(&self) -> Option<CancelHandle> {
        None
    }

    /// The statement that asks this backend for `statement`'s execution plan,
    /// or `None` when it has none worth offering.
    ///
    /// This is the **one** place a user's statement is wrapped, and it is not
    /// the rewriting `models::page` rules out: the user pressed Explain, what
    /// comes back is a plan rather than their rows, and the footer names the
    /// `EXPLAIN …` that was actually sent. `execute` still sends exactly what it
    /// is given.
    fn explain_statement(&self, _statement: &str) -> Option<String> {
        None
    }

    /// Loads one table/view detail section, streaming grid rows into `sink`.
    ///
    /// Data paging is generated by the backend from the opaque node id; this
    /// never rewrites text from the query editor.
    fn detail(
        &self,
        _request: &DetailRequest,
        _sink: &mut dyn RowSink,
    ) -> Result<DetailResult, DbError> {
        Ok(DetailResult::Unavailable)
    }
}

/// Opens a connection for `profile`.
///
/// Blocking, like everything else here. One arm per engine: adding a backend is
/// a line here and a file beside this one.
pub fn connect(profile: &ConnectionProfile) -> Result<Arc<dyn Driver>, DbError> {
    match profile.engine {
        Engine::PostgreSql => postgres::connect(profile).map(|driver| driver as Arc<dyn Driver>),
        Engine::Sqlite => sqlite::connect(profile).map(|driver| driver as Arc<dyn Driver>),
        Engine::MySql => mysql::connect(profile).map(|driver| driver as Arc<dyn Driver>),
        Engine::Redis => redis::connect(profile).map(|driver| driver as Arc<dyn Driver>),
    }
}

/// Opens a connection, proves it works, and closes it again.
///
/// What the connection form's Test button runs. Deliberately does *not* keep
/// the connection: a test that quietly left a session open on the server would
/// be a resource leak the user never asked for.
pub fn test_connection(profile: &ConnectionProfile) -> Result<(), DbError> {
    let driver = connect(profile)?;
    driver.ping()
}

#[cfg(test)]
mod tests {
    use super::{Driver, connect, test_connection};
    use crate::database::models::connection::ConnectionProfile;
    use crate::database::models::engine::Engine;
    use crate::database::models::error::DbError;
    use crate::database::services::fake::FakeDriver;

    /// The point of the trait: something that is not a SQL server at all can be
    /// a `Driver`, and everything above this layer takes it as one.
    #[test]
    fn a_driver_that_is_not_sql_is_still_a_driver() {
        let driver: std::sync::Arc<dyn Driver> = std::sync::Arc::new(FakeDriver::key_value());
        assert_eq!(driver.capabilities().editor_language, "text");
        assert!(!driver.capabilities().explain);
        assert_eq!(driver.explain_statement("GET key"), None);

        let roots = driver.children(None).expect("roots load");
        assert!(!roots.is_empty(), "a key/value store still has a tree");
        assert!(driver.ping().is_ok());
    }

    #[test]
    fn connecting_to_a_missing_sqlite_file_is_unreachable_rather_than_a_panic() {
        let mut profile = ConnectionProfile::new(1, Engine::Sqlite);
        profile.file = "/definitely/not/a/directory/that/exists/db.sqlite".into();
        assert!(matches!(connect(&profile), Err(DbError::Unreachable(_))));
        assert!(test_connection(&profile).is_err());
    }
}
