//! Cleaner tool: a safety-first cleanup workflow with macOS-first support.
//!
//! Round 1 (this module's first landing) intentionally ships the shared domain
//! model, state machine, scanner traits and a mock incremental scan UI only.
//! There is no destructive cleanup path yet.
//!
//! Boundaries:
//!
//! - [`core`] is plain domain/state contracts with no GPUI dependency.
//! - [`state`] orchestrates scans and UI-facing state transitions.
//! - [`views`] renders the Cleaner panel.
//! - [`macos`] holds macOS-only implementation seams.

pub mod core;
#[cfg(target_os = "macos")]
pub mod macos;
pub mod services;
pub mod state;
pub mod views;

pub use views::CleanerView;
