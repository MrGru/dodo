//! The API Explorer's rendering.
//!
//! [`explorer::ApiExplorer`] is the entity and owns the page's structure; the
//! other modules add `impl ApiExplorer` blocks for one region each, so no
//! single file renders the whole page.

pub mod collections_panel;
pub mod explorer;
pub mod request_editor;
pub mod request_tabs;
pub mod response_viewer;

pub use explorer::ApiExplorer;
