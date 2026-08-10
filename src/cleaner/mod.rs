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
//! - [`macos`], [`windows`] and [`linux`] hold platform-only implementation
//!   seams. macOS carries every category; Windows and Linux carry only the
//!   four generic ones (System Junk, User Cache, Trash Bins, Large & Old
//!   Files) — see each module's own doc comment for what that means and why.

pub mod core;
#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "macos")]
pub mod macos;
pub mod services;
pub mod state;
pub mod views;
#[cfg(target_os = "windows")]
pub mod windows;

pub use views::CleanerView;
