//! The Database Explorer's page.
//!
//! [`DatabaseView`] is the whole tool: one entity, split across four files the
//! way the API Explorer splits its regions — [`database`] owns the behaviour
//! and the layout, [`connections_panel`] and [`query_pane`] draw the two
//! halves, and [`result_grid`] is the table delegate.
//!
//! [`connection_form`] is the one separate entity, because a dialog body has to
//! be one — see its module doc for the two rules that come with that.

pub mod connection_form;
pub mod connections_panel;
pub mod database;
pub mod query_pane;
pub mod result_grid;

pub use database::DatabaseView;
