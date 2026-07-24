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
//!   `LoadingSkeleton`, `EmptyState` and `ErrorState`, generic so every page
//!   reuses them.
//! - [`views`] — [`DockerView`](views::DockerView), the four-page container the
//!   sidebar drives, and the four pages themselves.
//!
//! # What each round shipped, and where later ones plug in
//!
//! Rounds 1–2 ship Containers in full: the table, then compose grouping
//! ([`state::grouping`]), the multi-filter popover ([`state::filters`]) and bulk
//! actions over the selection. Round 3 replaces the Images, Volumes and Networks
//! placeholder pages with real list pages: `models::{image,volume,network}` rows
//! (plus the shared `models::size` formatting and the `models::usage` "containers
//! using" derivation), one generic store [`state::resource`], and the
//! [`views`](views) siblings, all switched to by the same
//! [`DockerPage`](views::DockerPage) wired into the sidebar. Each page's Inspect
//! action, and a Create/Build/Pull flow, are the placeholders a later round
//! fills in; its context menus and live auto-polling attach to the stores and
//! views that exist here — the selection model and the per-row CPU seam are
//! already in place for them.

pub mod components;
pub mod models;
pub mod services;
pub mod state;
pub mod views;

pub use views::{DockerPage, DockerView};
