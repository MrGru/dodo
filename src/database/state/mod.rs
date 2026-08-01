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
//!
//! [`run`]: query::run

pub mod connections;
pub mod query;
pub mod tree;
