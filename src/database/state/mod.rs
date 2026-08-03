//! What the Database Explorer knows, between the driver and the screen.
//!
//! - [`connections`] — the saved connections, which one is selected, and each
//!   one's live status. The `Arc<dyn Driver>` handles themselves are held by
//!   the view, so everything here stays testable with no server.
//! - [`tree`] — one load state per object-tree node, and the outline the view
//!   turns into `TreeItem`s. Owns expansion, for two reasons its module doc
//!   spells out.
//! - [`query`] — running the editor's buffer, and what the footer says. [`run`]
//!   is one blocking function over a `&dyn Driver`, which is what makes the
//!   ordering and the reporting testable with a fake.
//! - [`editor`] — which grammar the query editor is pointed at. One field, and
//!   its module doc is the record of why the SQL highlighting did not work.
//! - [`tabs`] — the open query tabs: each one's text, run in flight and
//!   result, and the index arithmetic that decides which tab shows after a
//!   close.
//! - [`history`] and [`saved_queries`] — the bounded persisted execution list
//!   and saved-query CRUD; their JSON boundary remains in `services`.
//! - [`catalog_search`] — one bounded walk over `Driver::children`, cached in
//!   memory and filtered locally after the remote work finishes.
//! - [`edit`] — original/displayed rows together, so edits, inserts and deletes
//!   remain local until Commit and Rollback is one exact reset.
//!
//! [`run`]: query::run

pub mod catalog_search;
pub mod connections;
pub mod detail;
pub mod edit;
pub mod editor;
pub mod history;
pub mod query;
pub mod saved_queries;
pub mod tabs;
pub mod tree;
