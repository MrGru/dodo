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
        // buttons and act on the right-clicked row; the last three are the
        // disabled "coming soon" placeholders a later round fills in.
        DockerContextStart,
        DockerContextStop,
        DockerContextRestart,
        DockerContextDelete,
        DockerContextInspect,
        DockerContextLogs,
        DockerContextTerminal,
    ]
);

/// Registers the Docker list pages' keyboard shortcuts, scoped to [`KEY_CONTEXT`]:
///
/// - `up` / `down` — move the highlighted row.
/// - `space` / `x` — toggle the highlighted row's selection (Containers only).
/// - `cmd-r` — refresh the active page (manual Refresh, from the keyboard).
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
    ]);
}
