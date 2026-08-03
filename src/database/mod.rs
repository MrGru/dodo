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
//!   `rustls` or `tokio-postgres-rustls`**, mirroring how
//!   `api_explorer::services::http` is the only place that names `reqwest` and
//!   `docker::services` the only place that names `bollard`. Also where
//!   `connections.json` is read and written.
//! - [`state`] — the saved connections and their live status, the object
//!   tree's per-node load state, open query tabs and in-session history. Plain
//!   data over an `Arc<dyn Driver>`.
//! - [`components`] — the small elements the widget library does not have.
//! - [`views`] — the page itself.
//!
//! # What rounds 1–2 ship, and what they deliberately do not
//!
//! Round 1 built the foundation against **PostgreSQL and SQLite only**: saved
//! connections, the lazy object tree, one editor and one bounded result grid.
//! Round 2 adds several session-only query tabs, protocol-level cancellation,
//! PostgreSQL's non-executing `EXPLAIN`, cell/row copy, streamed CSV/JSON export
//! and searchable in-session history. Export re-runs the statement into a file
//! sink; it never serialises the bounded page and never injects `LIMIT`.
//!
//! Still not built, and nothing is reserved for them: object detail tabs and
//! DDL, any editing or CRUD, favourites, pinned queries, persisted history or
//! tab restore, autocomplete, global search, MySQL and Redis. **Column sorting
//! is not built either**, and that goes further than the design report proposed:
//! the result grid's headers carry the column name and its type and no sort
//! affordance at all, because an absent control is honest where a disabled one
//! invites the question.
//!
//! # Threading
//!
//! Every [`Driver`](services::Driver) method performs blocking IO and is
//! **blocking by contract**, exactly like `Transport::execute` and
//! `DockerEngine`: callers run them on GPUI's background executor, never on the
//! UI thread. Nothing in this module — including `connections.json` — is read
//! or written on the UI thread.
//!
//! One honest wrinkle worth knowing before reading `services/postgres.rs`: the
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
