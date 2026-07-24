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
//! - [`state`] — the Containers store, the generic list store, the selection
//!   model and the detail surfaces' load status, all plain data.
//! - [`components`] — the reusable `StatusBadge`, `SearchBar`, `Toolbar`,
//!   `LoadingSkeleton`, `EmptyState` and `ErrorState`, generic so every page
//!   reuses them.
//! - [`views`] — [`DockerView`](views::DockerView), the four-page container the
//!   sidebar drives, the four pages themselves, and the read-only detail overlay
//!   they share.
//!
//! # What each round shipped
//!
//! Rounds 1–2 ship Containers in full: the table, then compose grouping
//! ([`state::grouping`]), the multi-filter popover ([`state::filters`]) and bulk
//! actions over the selection. Round 3 replaces the Images, Volumes and Networks
//! placeholder pages with real list pages: `models::{image,volume,network}` rows
//! (plus the shared `models::size` formatting and the `models::usage` "containers
//! using" derivation), one generic store [`state::resource`], and the
//! [`views`](views) siblings, all switched to by the same
//! [`DockerPage`](views::DockerPage) wired into the sidebar. Round 4 adds
//! background polling ([`POLL_INTERVAL`], the incremental merges in
//! [`state::diff`]), keyboard row navigation ([`state::focus`]) and the row
//! context menus, both routed through the actions below.
//!
//! Round 5 — the last — turns the two highest-value placeholders into real,
//! read-only surfaces:
//!
//! - **Inspect**, for all four resource types. Four engine endpoints
//!   ([`services::DockerEngine::inspect_container`] and its three siblings), one
//!   model ([`models::inspect`], which reduces the response *as JSON* so every
//!   field rule is testable without a daemon), one overlay
//!   ([`views::detail::DetailPanel`]) shared by the four pages: key fields plus
//!   the engine's own JSON in the highlighted code editor.
//! - **Container logs**, a bounded non-following tail
//!   ([`services::DockerEngine::container_logs`], reassembled by [`models::logs`])
//!   in the same overlay.
//!
//! # What is still a placeholder, and where it plugs in
//!
//! These are deliberately disabled controls with a "Coming soon" label, not
//! omissions — each one is a round of its own:
//!
//! - **Open Terminal / Exec** (container context menu). The largest: an
//!   interactive PTY needs `bollard`'s `create_exec`/`start_exec` and a
//!   *bidirectional* stream, so unlike every other call it does not fit
//!   [`services`]' blocking-by-contract shape. It would render in
//!   [`views::detail`] the way Logs does, over a writable stream.
//! - **Create container** (Containers toolbar and empty state) and **Pull** /
//!   **Build** (Images toolbar). Ordinary blocking additions to
//!   [`services::DockerEngine`] (`create_container`, `create_image`,
//!   `build_image`) plus a form; the progress stream a pull reports is the only
//!   novel part.
//! - **Stats beyond live CPU%** (container context menu). The per-row CPU sweep
//!   in [`views::containers`] already reads the full stats frame
//!   ([`models::stats`]); memory, network and block IO are more fields off that
//!   same frame, and a history would need a ring buffer in the store.
//! - **Favorites** (filter popover). A persisted set of container ids; the
//!   predicate seam is [`state::filters`], the persistence seam is the API
//!   Explorer's `DiskCollectionStore`.
//!
//! Log *following*, log filtering and ANSI colour parsing are noted as future
//! work in [`models::logs`], where the reassembly they would build on lives.

pub mod components;
pub mod models;
pub mod services;
pub mod state;
pub mod views;

use std::time::Duration;

use gpui::{App, KeyBinding, actions};

pub use views::{DockerPage, DockerView};

/// How often the active Docker page re-lists its data in the background, without
/// the user pressing Refresh. Five seconds is Docker Desktop's own list cadence:
/// brisk enough that a container starting or stopping shows up on its own within
/// a beat, slow enough that dozens of rows (each Containers tick also re-measures
/// live CPU) never saturate the background executor. It is a constant rather than
/// a setting on purpose — see `AGENTS.md` — polling pauses whenever the Docker
/// section is not the visible view, so an idle cadence never runs.
pub const POLL_INTERVAL: Duration = Duration::from_secs(5);

/// The key-binding context the Docker list pages establish on their root. Bindings
/// registered against it in [`init`] fire only while a Docker page holds focus, so
/// row navigation never leaks into another tool — the same scoping
/// `api_explorer`'s `ApiExplorer` context uses for its send shortcut.
pub const KEY_CONTEXT: &str = "DockerList";

actions!(
    dodo,
    [
        // Keyboard navigation on the list pages.
        DockerMoveUp,
        DockerMoveDown,
        DockerToggleSelect,
        DockerRefreshList,
        // Right-click context-menu actions. The lifecycle four mirror the row
        // buttons and act on the right-clicked row; Inspect and Logs open the
        // round-5 detail panel; Terminal and Stats are the disabled "coming
        // soon" placeholders (see the module doc).
        DockerContextStart,
        DockerContextStop,
        DockerContextRestart,
        DockerContextDelete,
        DockerContextInspect,
        DockerContextLogs,
        DockerContextTerminal,
        DockerContextStats,
        // Closes an open Inspect/Logs panel (escape).
        DockerCloseDetail,
    ]
);

/// Registers the Docker list pages' keyboard shortcuts, scoped to [`KEY_CONTEXT`]:
///
/// - `up` / `down` — move the highlighted row.
/// - `space` / `x` — toggle the highlighted row's selection (Containers only).
/// - `cmd-r` — refresh the active page (manual Refresh, from the keyboard).
/// - `escape` — close an open Inspect / Logs panel.
///
/// Must run after `gpui_component::init`, so a binding registered here wins the
/// tie at equal context depth — the same ordering rule `api_explorer::init` and
/// `settings::init` depend on. The arrow and space keys are only claimed by a
/// focused text input (the search box), whose deeper context takes them first, so
/// they drive row navigation everywhere else on the page.
pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("up", DockerMoveUp, Some(KEY_CONTEXT)),
        KeyBinding::new("down", DockerMoveDown, Some(KEY_CONTEXT)),
        KeyBinding::new("space", DockerToggleSelect, Some(KEY_CONTEXT)),
        KeyBinding::new("x", DockerToggleSelect, Some(KEY_CONTEXT)),
        KeyBinding::new("cmd-r", DockerRefreshList, Some(KEY_CONTEXT)),
        KeyBinding::new("escape", DockerCloseDetail, Some(KEY_CONTEXT)),
    ]);
}
