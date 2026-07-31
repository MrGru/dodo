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
//! runs on the background executor and emits
//! [`UpdateEvent`](models::state::UpdateEvent)s; the dialog applies them and
//! renders. Nothing in `services` or `state` can reach the UI, and the UI never
//! performs IO — it does not so much as `stat` a file.
//!
//! # What happens at startup
//!
//! [`init`] loads `updater.json`, sweeps the file a previous install renamed
//! aside, and — if the settings allow it — schedules a check. **The check is
//! silent**: it opens nothing, says nothing and logs one line unless it finds
//! something, in which case the dialog opens by itself. Nothing is ever
//! downloaded without the user pressing a button; that was decided with the
//! captain and is enforced structurally in [`services::pipeline`].

pub mod models;
pub mod services;
pub mod state;
pub mod views;

use std::sync::Arc;
use std::time::Duration;

use gpui::{App, Global, Task};

use crate::updater::models::config::UpdaterConfig;
use crate::updater::services::config_store::UpdaterConfigStore;
use crate::updater::services::{
    Downloader, ManifestSource, PlatformInstaller, Verifier, log, pipeline,
};

// Deliberately no `pub use` of the dialog type. `docker` re-exports
// `DockerView` because `layout.rs` holds one; nothing outside this module ever
// names `UpdateDialog`, because the updater's whole surface is two functions —
// `init` and `open` — and re-exporting a type no caller mentions would be a
// name that exists only to look symmetrical.

/// How long after launch the first check runs.
///
/// Not zero: startup is the busiest moment a desktop app has, and a check that
/// nobody asked to watch has no business competing with the first frame for the
/// background executor. Ten seconds is long after the window is interactive and
/// long before anyone would call it slow.
const STARTUP_DELAY: Duration = Duration::from_secs(10);

/// The service bundle, built once so there is one HTTP client rather than one
/// per dialog.
#[derive(Clone)]
pub struct UpdaterServices {
    pub source: Arc<dyn ManifestSource>,
    pub downloader: Arc<dyn Downloader>,
    pub verifier: Arc<dyn Verifier>,
    pub installer: Arc<dyn PlatformInstaller>,
    pub store: Arc<dyn UpdaterConfigStore>,
}

/// The updater's process-wide state.
///
/// A global for the same reason
/// [`ScriptPolicy`](crate::api_explorer::ScriptPolicy) and
/// [`Language`](crate::i18n::Language) are: it is read from anywhere and
/// written from one place. Unlike those two the configuration is **persisted** —
/// see [`models::config`] for why "skip this version" makes that unavoidable.
#[derive(Clone)]
pub struct Updater {
    config: UpdaterConfig,
    services: UpdaterServices,
    /// The periodic check. Held so that dropping the global cancels it; the
    /// task is replaced, never accumulated, so exactly one loop can be running.
    schedule: Arc<Option<Task<()>>>,
}

impl Global for Updater {}

impl Updater {
    /// The active configuration, or the defaults before `updater.json` has
    /// loaded.
    pub fn config(cx: &App) -> UpdaterConfig {
        cx.try_global::<Updater>()
            .map_or_else(UpdaterConfig::default, |updater| updater.config.clone())
    }

    /// The shared services. Built on demand for the (impossible in the app, but
    /// cheap to allow) case of a dialog opened before [`init`] ran.
    pub fn services(cx: &mut App) -> UpdaterServices {
        if let Some(updater) = cx.try_global::<Updater>() {
            return updater.services.clone();
        }
        let services = views::dialog::default_services();
        cx.set_global(Updater {
            config: UpdaterConfig::default(),
            services: services.clone(),
            schedule: Arc::new(None),
        });
        services
    }

    /// Replaces the configuration in memory, keeping the services and the
    /// running schedule. Persisting is the caller's job, on the background
    /// executor.
    pub fn set_config(config: UpdaterConfig, cx: &mut App) {
        let services = Self::services(cx);
        let schedule = cx
            .try_global::<Updater>()
            .map_or_else(|| Arc::new(None), |updater| updater.schedule.clone());
        cx.set_global(Updater {
            config,
            services,
            schedule,
        });
    }

    fn set_schedule(task: Task<()>, cx: &mut App) {
        let config = Self::config(cx);
        let services = Self::services(cx);
        cx.set_global(Updater {
            config,
            services,
            schedule: Arc::new(Some(task)),
        });
    }
}

/// Starts the updater.
///
/// Must run from `main` after `gpui_component::init`, the same ordering rule
/// `settings::init`, `api_explorer::init` and `docker::init` depend on — this
/// one registers no key bindings today, and keeping the ordering means adding
/// one later is not a debugging session.
///
/// Everything it does is asynchronous. The first thing on the UI thread is
/// reading a global; the file read, the sweep and the check all run on the
/// background executor.
pub fn init(cx: &mut App) {
    let services = Updater::services(cx);

    let load = services.clone();
    cx.spawn(async move |cx| {
        // The settings, then the sweep, then the schedule — in that order,
        // because the settings decide whether there is a schedule at all.
        let loaded = cx
            .background_executor()
            .spawn({
                let store = load.store.clone();
                async move { store.load() }
            })
            .await;

        let config = match loaded {
            Ok(config) => config,
            Err(error) => {
                // A settings file this build cannot read leaves the defaults in
                // place — stable channel, official URL — rather than turning
                // updates off. See `config_store`'s doc.
                log::problem(&format!("could not read updater.json: {error:?}"));
                UpdaterConfig::default()
            }
        };

        // Delete whatever a previous install renamed aside. On Windows the file
        // could not have been deleted by the process that replaced it.
        let sweep = load.installer.clone();
        cx.background_executor()
            .spawn(async move { sweep.sweep_stale() })
            .detach();

        let checks = config.checks_on_startup();
        let interval = Duration::from_secs(u64::from(config.effective_interval_hours()) * 3600);

        cx.update(|cx| Updater::set_config(config, cx));
        if !checks {
            return;
        }

        cx.update(|cx| {
            let task = cx.spawn(async move |cx| check_loop(STARTUP_DELAY, interval, cx).await);
            Updater::set_schedule(task, cx);
        });
    })
    .detach();
}

/// Opens the update dialog and starts a check — the sidebar's **Check for
/// updates**.
pub fn open(window: &mut gpui::Window, cx: &mut App) {
    views::dialog::open(window, cx);
}

/// The silent background check, forever.
///
/// Shaped after the Docker pages' poll loop (`docker::views::containers`): one
/// task, sequential, so a slow network slows the cadence instead of piling
/// tasks up, and dropping the task handle ends it.
///
/// It differs from that loop in one way worth stating: it keeps running while
/// the user is in another tool, because an update is not a property of the
/// visible page. The cadence is hours rather than seconds, so there is no
/// equivalent of Docker's "only the visible page polls" rule to make.
async fn check_loop(first: Duration, interval: Duration, cx: &mut gpui::AsyncApp) {
    let mut delay = first;
    loop {
        cx.background_executor().timer(delay).await;
        delay = interval;

        // Re-read every tick: the checkbox in the dialog can turn this off
        // without a restart, and a loop that ignored it would be a bug the user
        // cannot see.
        let config = cx.update(|cx| Updater::config(cx));
        if !config.checks_automatically() {
            continue;
        }

        let services = cx.update(Updater::services);
        let Ok(current) = pipeline::current_version() else {
            return;
        };

        let outcome = cx
            .background_executor()
            .spawn({
                let source = services.source.clone();
                async move { pipeline::check(source.as_ref(), &config, &current, &|_| {}) }
            })
            .await;

        match outcome {
            Ok(pipeline::CheckOutcome::Found(info)) => {
                let info = *info;
                // The one moment the check stops being silent. `windows()` is
                // one window in dodo; a launch that has not opened it yet gets
                // nothing, and the next tick tries again.
                cx.update(|cx| {
                    if let Some(window) = cx.windows().first().cloned() {
                        let _ = window.update(cx, |_, window, cx| {
                            views::dialog::open_with(info.clone(), window, cx);
                        });
                    }
                });
            }
            // Silent by design: neither "you are up to date" nor "the network is
            // down" is news the user asked for. A failure is one stderr line and
            // the next tick tries again.
            Ok(pipeline::CheckOutcome::UpToDate) => {}
            Err(error) => log::problem(&format!("background check failed: {error:?}")),
        }
    }
}
