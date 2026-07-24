//! The Docker/Podman management module: Docker Desktop's container workflows in
//! dodo's own design system.
//!
//! It talks to the Docker Engine API (equally, Podman's Docker-compatible
//! socket) and is the second tool to outgrow one file, so it copies the
//! `api_explorer` layout exactly:
//!
//! - [`models`] — plain data, no GPUI, no `bollard`, unit tested: the container
//!   row, its status, port and relative-time formatting, and the CPU-percent
//!   maths.
//! - [`services`] — the [`DockerEngine`](services::DockerEngine) trait and its
//!   `bollard` implementation. The one place that may name `bollard`, mirroring
//!   how `api_explorer::services::http` is the only place that names `reqwest`.
//!   Also the one place a tokio runtime lives; every call is blocking-by-contract
//!   and runs on GPUI's background executor.
//! - [`state`] — the Containers store and the selection model, plain data.
//! - [`components`] — the reusable `StatusBadge`, `SearchBar`, `Toolbar`,
//!   `LoadingSkeleton`, `EmptyState` and `ErrorState`, generic so round 2's pages
//!   reuse them.
//! - [`views`] — [`DockerView`](views::DockerView), the four-page container the
//!   sidebar drives, and the Containers page itself.
//!
//! # Where later rounds plug in
//!
//! Rounds 1–2 ship Containers in full: the table, then compose grouping
//! ([`state::grouping`]), the multi-filter popover ([`state::filters`]) and bulk
//! actions over the selection. Images, Volumes and Networks are still placeholder
//! pages so the nav shape and state preservation are correct now; their real
//! pages are `models::{image,volume,network}` + `state::*` siblings + `views::*`
//! pages, switched to by the same [`DockerPage`](views::DockerPage) already wired
//! into the sidebar. Round 4's context menus and live auto-polling attach to the
//! Containers store and view that exist here — the selection model and the per-row
//! CPU seam are already in place for them.

pub mod components;
pub mod models;
pub mod services;
pub mod state;
pub mod views;

pub use views::{DockerPage, DockerView};
