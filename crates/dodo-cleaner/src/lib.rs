//! dodo's Cleaner tool: a safety-first cleanup workflow with macOS-first
//! support.
//!
//! **This is a *feature* crate, and the first of them.** It was
//! `src/cleaner/` until 2026-08-15, when it moved out whole: the binary now
//! links it and names exactly one item from it — `layout.rs`'s
//! [`CleanerView`] — while `main.rs` aliases the crate back to
//! `crate::cleaner`, so no call site on either side of the seam changed. Its
//! outbound edges are only the three kernel crates (`dodo-app-icon`,
//! `dodo-i18n`, `dodo-paths`), which is what made it extractable at all;
//! `Cargo.toml` says why each remaining dependency is there, and
//! `docs/architecture/workspace-layout.md` is the authority on the shape. Two
//! seams exist only because of the move and are worth knowing about:
//! [`paths`] supplies the one impure input `dodo_paths`' pure rules take,
//! and the `app_icon` / `i18n` aliases below are what keep
//! `crate::app_icon::AppIcon` and `crate::i18n::t` spelled as they were.
//!
//! **The module declarations below say `pub(crate)`, and that is not a
//! narrowing.** Inside a binary crate `pub` already meant "reachable from
//! dodo and nowhere else"; spelling it `pub(crate)` here is what keeps that
//! true now that there is an outside. The crate's whole public surface is
//! [`CleanerView`] — the one item `layout.rs` names — and [`paths`], which
//! `main.rs` tests against its own copy of the same question. Widening any of
//! them would hand the binary a way into the Cleaner's internals that
//! `src/cleaner/` never had.
//!
//! **90 of the 93 files here name no UI framework**, and that is a contract
//! rather than an accident: only `views::cleaner_view`,
//! `views::results_table` and `views::uninstall_review_dialog` may `use
//! gpui`. Everything else — every scanner, the deletion-safety boundary, the
//! scan state machine — is tested with no `App` and no frame.
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
//!   entire switch and is a **pure function of a [`paths::HostOs`]**,
//!   not a `cfg` split, so all three answers are unit tested from any host;
//!   `paths::current` is the one place the compiled-for platform enters it.
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

// The icon set and the string catalogue are kernel crates, aliased here for
// exactly the reason `main.rs` aliases the first of them: every call site in
// this crate still reads `crate::app_icon::AppIcon` and
// `crate::i18n::{cleaner, t}`, unchanged by the move out of the binary.
use dodo_app_icon as app_icon;
use dodo_i18n as i18n;

pub(crate) mod ai_apps;
pub(crate) mod core;
pub(crate) mod docker_cache;
// Linux Installed Apps keeps its package parsers and inventory policy
// host-independent so their fixtures run on this Mac; process/filesystem I/O
// remains Linux-only in shipping builds.
#[cfg(any(target_os = "linux", test))]
pub(crate) mod linux;
#[cfg(target_os = "macos")]
pub(crate) mod macos;
pub(crate) mod node_tooling;
pub(crate) mod node_tooling_cache;
pub mod paths;
pub(crate) mod services;
pub(crate) mod state;
pub(crate) mod views;
// Windows' pure Installed Apps inventory policy is also compiled by this
// host's tests; OS integrations inside the module remain target-gated.
#[cfg(any(target_os = "windows", test))]
pub(crate) mod windows;

pub use views::CleanerView;
