//! The Database Explorer's page.
//!
//! [`DatabaseView`] is the whole tool: one entity, split across four files the
//! way the API Explorer splits its regions — [`database`] owns the behaviour
//! and the layout, [`connections_panel`] and [`query_pane`] draw the two
//! halves, and [`result_grid`] is the table delegate.
//!
//! [`connection_form`], [`row_editor`] and [`commit_dialog`] are separate
//! entities because a dialog body has to be one. The first two own inputs; the
//! last is the read-only exact-statement gate before any mutation executes.

pub mod commit_dialog;
pub mod connection_form;
pub mod connections_panel;
pub mod database;
pub mod history;
pub mod object_detail;
pub mod query_pane;
pub mod result_grid;
pub mod row_editor;

pub use database::DatabaseView;
