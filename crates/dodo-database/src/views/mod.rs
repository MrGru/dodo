//! The Database Explorer's page.
//!
//! [`DatabaseView`] is the whole tool: one entity, split across four files the
//! way the API Explorer splits its regions — [`database`] owns the behaviour
//! and the layout, [`connections_panel`] and [`query_pane`] draw the two
//! halves, and [`result_grid`] is the table delegate.
//!
//! [`connection_form`], [`row_editor`], [`saved_query_form`] and
//! [`commit_dialog`] are separate entities because a dialog body has to be one.
//! [`history`], [`saved_queries`] and [`catalog_search`] use the library's
//! searchable list; catalog filtering is entirely local after one bounded
//! background crawl.

pub mod catalog_search;
pub mod commit_dialog;
pub mod connection_form;
pub mod connections_panel;
pub mod database;
pub mod history;
pub mod object_detail;
pub mod query_pane;
pub mod result_grid;
pub mod row_editor;
pub mod saved_queries;
pub mod saved_query_form;

pub use database::DatabaseView;
