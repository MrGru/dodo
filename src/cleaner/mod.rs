//! Cleaner tool: a safety-first cleanup workflow with macOS-first support.
//!
//! Round 1 (this module's first landing) intentionally shipped the shared
//! domain model, state machine, scanner traits and a mock incremental scan UI
//! only, with no destructive cleanup path. Rounds have been landing since;
//! `macos::cleanup` is a real move-to-trash path today, so that sentence is
//! history rather than current state.
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
//!
//! Three things settled on 2026-08-13, each of which is counter-intuitive
//! enough that the module holding it is named here rather than left to be
//! found:
//!
//! - **An application icon is a bounded, shared payload, and the bound is a
//!   type.** [`core::icon::IconRaster`] refuses anything over 32 KiB and
//!   holds it behind an `Arc`. What it replaced —
//!   `-[NSImage TIFFRepresentation]` — measured **73,949,448 bytes per
//!   application** and could not be decoded by the `image` crate at all, so
//!   it cost gigabytes *and* drew nothing. `core::icon` and
//!   `macos::platform::icon` carry the measurements; do not put a raw
//!   `Vec<u8>` icon back on an item.
//! - **Not every [`core::category::CleanerCategory`] is on screen.**
//!   `CleanerCategory::HIDDEN` is the entire switch, and because a scan is
//!   only ever started from a category's own pane, a hidden category is not
//!   scanned at all. `ALL` still names all fourteen, so nothing about the
//!   scanners, their tests or their cleanup paths changes with it.
//! - **What a scan looks like is a tested pure function**, not a `match` in a
//!   `render`. [`core::scan_state::ScanState::indicator`] is the one mapping
//!   the sidebar row and the results pane both read. It exists because the
//!   sidebar's glyph builder documented a spinner and drew an empty `div()`
//!   for both in-flight states, which no test could have caught where it was.

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
