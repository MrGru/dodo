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
//!   seams. Docker Cache, Node Tooling Cache and AI Apps are shared by all
//!   three: their OS differences are resolved inputs or thin activity probes.
//!   Installed Apps stays parallel by platform: Windows owns registry/MSIX
//!   discovery, Linux owns package-manager/desktop metadata, and neither runs
//!   vendor command text or deletes package-managed locations.
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
//! - **Not every [`core::category::CleanerCategory`] is on screen, and which
//!   ones are depends on the platform.** macOS lists all fourteen, while
//!   Windows and Linux each list eight — exactly what each scanner registry
//!   implements today. [`core::category::CleanerCategory::hidden_for`] is the
//!   entire switch and is a **pure function of a [`crate::paths::HostOs`]**,
//!   not a `cfg` split, so all three answers are unit tested from any host;
//!   `HostOs::current` is the one place the compiled-for platform enters it.
//!   Because a scan starts only from a category's own pane, a hidden category
//!   is never scanned. Tests pin both directions of the contract: no scanner
//!   may be hidden and no listed row may lack a scanner.
//! - **What a scan looks like is a tested pure function**, not a `match` in a
//!   `render`. [`core::scan_state::ScanState::indicator`] is the one mapping
//!   the sidebar row and the results pane both read. It exists because the
//!   sidebar's glyph builder documented a spinner and drew an empty `div()`
//!   for both in-flight states, which no test could have caught where it was.
//!
//! The results grid deliberately uses fixed columns and horizontal scrolling;
//! `views::results_table` records why feeding prepaint measurements back into
//! this view is unsafe.

pub(crate) mod ai_apps;
pub mod core;
pub(crate) mod docker_cache;
// Linux Installed Apps keeps its package parsers and inventory policy
// host-independent so their fixtures run on this Mac; process/filesystem I/O
// remains Linux-only in shipping builds.
#[cfg(any(target_os = "linux", test))]
pub mod linux;
#[cfg(target_os = "macos")]
pub mod macos;
pub(crate) mod node_tooling;
pub(crate) mod node_tooling_cache;
pub mod services;
pub mod state;
pub mod views;
// Windows' pure Installed Apps inventory policy is also compiled by this
// host's tests; OS integrations inside the module remain target-gated.
#[cfg(any(target_os = "windows", test))]
pub mod windows;

pub use views::CleanerView;
