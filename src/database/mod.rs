//! The Database Explorer: connect to a database, browse its objects, run a
//! query, read the result.
//!
//! The third tool to outgrow one file, and it follows the same five layers as
//! `api_explorer` and `docker` — read those two first if this is unfamiliar,
//! because the split is identical and only the contents differ:
//!
//! - [`models`] — plain data. No GPUI, no driver crate, no other dodo module,
//!   unit tested with no server.
//! - [`services`] — the [`Driver`](services::Driver) trait and its
//!   implementations. **The only layer that may name `postgres`, `rusqlite`,
//!   `mysql`, `redis`, `rustls` or `tokio-postgres-rustls`**, mirroring how
//!   `api_explorer::services::http` is the only place that names `reqwest` and
//!   `docker::services` the only place that names `bollard`. Also where
//!   `connections.json` is read and written.
//! - [`state`] — the saved connections and their live status, the object
//!   tree's per-node load state, open query tabs, saved queries, persisted
//!   history and the bounded catalog-search index. Plain data over an
//!   `Arc<dyn Driver>`.
//! - [`components`] — the small elements the widget library does not have.
//! - [`views`] — the page itself.
//!
//! # What rounds 1–6 ship, and what they deliberately do not
//!
//! Round 1 built the foundation against **PostgreSQL and SQLite only**: saved
//! connections, the lazy object tree, one editor and one bounded result grid.
//! Round 2 added session-only query tabs, protocol-level cancellation,
//! PostgreSQL's non-executing `EXPLAIN`, cell/row copy, streamed CSV/JSON export
//! and searchable in-session history. Round 3 adds one table/view detail
//! surface: Data, Columns, Indexes, Constraints and DDL. Table data alone is
//! server-paged with a statement generated from the driver's opaque catalog id;
//! editor text is still never rewritten. PostgreSQL DDL is explicitly labelled
//! reconstructed, while SQLite shows the statement stored in `sqlite_master`.
//! Round 4 adds MySQL and MariaDB through one shared-protocol driver, plus
//! Redis as the trait's first non-SQL store. Redis's tree is numbered logical
//! databases → type groups → cursor-paged keys; the console runs one command
//! per line, and only an opened key fetches its value. Round 5 adds pending
//! table-data edits, add/delete/duplicate row, and explicit Commit/Rollback for
//! PostgreSQL, SQLite and MySQL/MariaDB; Redis remains read-only. Round 6 adds
//! connection-scoped saved queries, bounded persisted query history and one
//! bounded in-memory catalog search over connected databases. Search walks the
//! existing `Driver::children` seam off-thread; Redis therefore keeps using
//! cursor-paged `SCAN … TYPE`, never `KEYS` or one query per key.
//!
//! The connection form can also be **filled from a pasted URI** — all four
//! engines, both PostgreSQL schemes, `rediss://`, the `sqlite:` forms. The
//! parse is [`models::uri`], a pure function unit-tested against every shape a
//! real URI takes; the form only moves its answer into the inputs and never
//! saves, connects or tests as a side effect.
//!
//! Still not built, and nothing is reserved for them: favourites, saved-query
//! folders/tags/sharing/import/export, tab restore, autocomplete, or a
//! background catalog daemon. **Column sorting is not built either**: the
//! result grid's headers carry the column name and its type and no sort
//! affordance at all, because sorting one bounded page would be dishonest and
//! server-side sorting was not part of round 3's accepted scope.
//!
//! # Safe table-data editing
//!
//! [`models::identity`] is the safety boundary: wire column origins plus
//! catalog primary/unique-key metadata are the only route to an
//! `EditableSource`; SQL text is never parsed for identity. A primary key wins,
//! otherwise a unique index is accepted only when all key columns are NOT NULL,
//! and every key value must be present and untruncated in the result. SQLite's
//! rowid is accepted only when the result actually contains it. Joins, unions,
//! aggregates, computed columns, missing keys and Redis stay read-only with a
//! localized reason. dodo never falls back to old-value predicates, `LIMIT 1`,
//! guessed keys, or every displayed column.
//!
//! [`models::statement`] alone quotes mutation identifiers and builds bound
//! parameters. [`state::edit::PendingGrid`] holds edits locally and derives the
//! batch shown in [`views::commit_dialog`] before execution. Each SQL driver
//! runs that exact batch in one transaction and rolls it all back unless every
//! statement reports exactly one matched row. MySQL requests
//! `CLIENT_FOUND_ROWS`, so assigning an unchanged value still reports the one
//! safely matched row. Round 5 deliberately does not detect concurrent lost
//! updates; the confirmation says so rather than inventing old-value or version
//! predicates.
//!
//! # Threading
//!
//! Every [`Driver`](services::Driver) method performs blocking IO and is
//! **blocking by contract**, exactly like `Transport::execute` and
//! `DockerEngine`: callers run them on GPUI's background executor, never on the
//! UI thread. Nothing in this module — including `connections.json` and
//! `query-data.json` — is read or written on the UI thread.
//!
//! MySQL and Redis use their crates' blocking clients directly; neither builds
//! or requires an async runtime. One honest wrinkle worth knowing before
//! reading `services/postgres.rs`: the
//! `postgres` crate is a synchronous façade over `tokio-postgres` and builds a
//! private **current-thread** tokio runtime per client. That adds no threads
//! (the calling background-executor thread drives it) and this module never
//! names `tokio`, but it is why `AGENTS.md` now says `docker::services` owns
//! the only tokio runtime dodo *constructs* rather than the only one that
//! exists.
//!
//! # This module is self-contained
//!
//! It reads no other dodo module's state and no other module reads it. In
//! particular it does **not** name `crate::docker`: the design report proposed
//! a read-only "detect running database containers" prefill on the connection
//! form, and that feature was dropped in every round, precisely so this module
//! keeps no compile-time edge onto another tool.
//!
//! The invariant, stated so it can be checked: **no `use crate::` line here
//! names another tool.** `grep -rn '^use crate::' src/database/ | grep -vE
//! 'crate::(database|i18n|app_icon|paths)'` returns nothing — the anchor keeps
//! the check from matching its own description. Other modules are
//! mentioned in prose all over these docs — that is a pointer, not an edge, and
//! the two are worth telling apart.
//!
//! # The left panel is one tree, and the connections are its roots
//!
//! Round 1 shipped a connection list stacked above a separate object tree, and
//! the captain asked for one tree instead: a connection *is* a root node, its
//! databases and tables hang under it, and several connections are several
//! roots that are all browsable at once. [`state::tree::Forest`] is that
//! arrangement and `views::connections_panel` draws it.
//!
//! Three consequences worth knowing before changing any of it:
//!
//! - **Selecting a connection no longer clears anything.** It only says which
//!   one the query editor runs against. Every connection keeps its own loaded,
//!   expanded tree.
//! - **Opening a connection root connects it**, the way every database client
//!   does. Connect stays in the context menu for the explicit path, and a root
//!   that is not connected says so in a placeholder child rather than opening
//!   onto nothing.
//! - **Element ids are qualified by connection** ([`state::tree::RowRef`]), and
//!   that is not decoration: two connections routinely produce the same node id,
//!   and two rows sharing an element id is a gpui bug that is miserable to find.
//!
//! The per-connection actions are a right-click context menu on the root, built
//! with the tree widget's own `context_menu`; the disclosure chevron is
//! **dodo's**, because the widget draws none.
//!
//! # Sidebar registration
//!
//! **One flat top-level row.** Not a group with children: an icon-collapsed
//! sidebar renders no children at all, which is what made Docker's four pages
//! unreachable and moved them onto that module's own rail. The tree of
//! connections is this module's own navigation, inside the page.

pub mod components;
pub mod models;
pub mod services;
pub mod state;
pub mod views;

use gpui::{App, KeyBinding, actions};

pub use views::DatabaseView;

pub(crate) const KEY_CONTEXT: &str = "DatabaseResult";

actions!(database, [DatabaseCopyCell, DatabaseCopyRow]);

/// Registers result-grid copy shortcuts after `gpui_component::init`, so these
/// bindings win the tie with the component library's own contexts.
pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new(
            "cmd-c",
            DatabaseCopyCell,
            Some("DatabaseResult > DataTable"),
        ),
        KeyBinding::new(
            "ctrl-c",
            DatabaseCopyCell,
            Some("DatabaseResult > DataTable"),
        ),
        KeyBinding::new(
            "cmd-shift-c",
            DatabaseCopyRow,
            Some("DatabaseResult > DataTable"),
        ),
        KeyBinding::new(
            "ctrl-shift-c",
            DatabaseCopyRow,
            Some("DatabaseResult > DataTable"),
        ),
    ]);
}
