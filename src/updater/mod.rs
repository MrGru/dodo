//! The in-app updater: check, ask, download, verify, install, restart.
//!
//! Layered exactly like `api_explorer` and `docker`, for the same reason —
//! everything above [`services`] is testable without a network, a daemon or a
//! `Window`:
//!
//! - [`models`] — the `update.json` document and its strict parse, the platform
//!   key, SemVer and channels, an incremental SHA-256, `updater.json`, and the
//!   state and event enums. No GPUI, no `reqwest`, no filesystem.
//! - [`services`] — the only new place that may name `reqwest`. The
//!   [`ManifestSource`](services::ManifestSource),
//!   [`Downloader`](services::Downloader), [`Verifier`](services::Verifier) and
//!   [`PlatformInstaller`](services::PlatformInstaller) traits, their real
//!   implementations, an in-memory twin of each, and the blocking pipeline that
//!   sequences them.
//! - [`state`] — the state machine. GPUI-free, so every transition is a unit
//!   test.
//! - [`views`] — the dialog, and nothing else.
//!
//! # The one rule
//!
//! **The state machine is the only thing that mutates state.** The pipeline
//! runs on the background executor and emits [`UpdateEvent`]s; the dialog
//! applies them and renders. Nothing in `services` or `state` can reach the UI,
//! and the UI never performs IO — it does not so much as `stat` a file.

pub mod models;
pub mod services;
pub mod state;

use gpui::{App, Global};

use crate::updater::models::config::UpdaterConfig;

/// The updater's process-wide settings.
///
/// A global for the same reason [`ScriptPolicy`](crate::api_explorer::ScriptPolicy)
/// and [`Language`](crate::i18n::Language) are: it is read from anywhere and
/// written from one place. Unlike those two it is **persisted** — see
/// [`models::config`] for why "skip this version" makes that unavoidable.
#[derive(Clone, Default)]
pub struct Updater {
    config: UpdaterConfig,
}

impl Global for Updater {}

impl Updater {
    /// The active configuration, or the defaults before the file has loaded.
    pub fn config(cx: &App) -> UpdaterConfig {
        cx.try_global::<Updater>()
            .map_or_else(UpdaterConfig::default, |updater| updater.config.clone())
    }

    /// Replaces the configuration in memory. Persisting it is the caller's job,
    /// on the background executor.
    pub fn set_config(config: UpdaterConfig, cx: &mut App) {
        cx.set_global(Updater { config });
    }
}
