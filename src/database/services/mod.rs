//! The driver layer the state stores talk to.
//!
//! # Why a trait
//!
//! The same discipline as `api_explorer::services::Transport` and
//! `docker::services::DockerEngine`: the state holds an `Arc<dyn Driver>` and
//! never learns which crate is behind it. It hands over a statement and gets
//! back [`models`](crate::database::models) types. A second backend is another
//! [`Driver`] and nothing above this module changes. **This module and its
//! children are the only place that may name `postgres`, `rusqlite`, `rustls`
//! or `tokio-postgres-rustls`.**
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
//! [`Capabilities`] carries what *this round's UI reads*, and nothing else.
//! The design report proposes fields for transactions, Explain, cancellation,
//! column provenance and `LIMIT`/`OFFSET` support — every one of which
//! describes a feature that is not built, so every one would be a value no code
//! reads. That is the same reasoning `Cargo.toml` records for the marker
//! features this crate declined to add: a flag nothing reads is decoration, and
//! decoration has to be maintained, tested and kept honest.
//!
//! The extension point is the struct and [`Driver::capabilities`], not a
//! pre-filled list of guesses. A field arrives with the control that reads it.
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
pub mod postgres;
pub mod sqlite;

#[cfg(test)]
pub mod fake;

use std::sync::Arc;

use crate::database::models::catalog::{CatalogNode, NodeId};
use crate::database::models::connection::ConnectionProfile;
use crate::database::models::engine::Engine;
use crate::database::models::error::DbError;
use crate::database::models::page::RowSink;
use crate::database::models::query::{Execution, QueryRequest};

/// What a driver can do, as far as this round's UI needs to know.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Capabilities {
    /// The language id handed to the query editor's `code_editor`, so a backend
    /// whose console is not SQL is never coloured as though it were.
    pub editor_language: &'static str,
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
}

/// Opens a connection for `profile`.
///
/// Blocking, like everything else here. One arm per engine: adding a backend is
/// a line here and a file beside this one.
pub fn connect(profile: &ConnectionProfile) -> Result<Arc<dyn Driver>, DbError> {
    match profile.engine {
        Engine::PostgreSql => postgres::connect(profile).map(|driver| driver as Arc<dyn Driver>),
        Engine::Sqlite => sqlite::connect(profile).map(|driver| driver as Arc<dyn Driver>),
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
