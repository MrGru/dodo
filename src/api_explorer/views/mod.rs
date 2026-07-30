//! The API Explorer's rendering.
//!
//! [`explorer::ApiExplorer`] is the entity and owns the page's structure; the
//! other modules add `impl ApiExplorer` blocks for one region each, so no
//! single file renders the whole page.

pub mod collections_panel;
pub mod environment_picker;
pub mod environments_editor;
pub mod explorer;
pub mod generate_code;
pub mod history_panel;
pub mod request_auth;
pub mod request_body;
pub mod request_editor;
pub mod request_scripts;
pub mod request_tabs;
pub mod response_console;
pub mod response_tests;
pub mod response_viewer;
pub mod script_consent;

pub use explorer::ApiExplorer;
