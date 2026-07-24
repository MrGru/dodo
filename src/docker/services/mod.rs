//! The engine layer the state stores talk to.
//!
//! # Why a trait
//!
//! The same discipline as `api_explorer::services::Transport`: the stores hold
//! an `Arc<dyn DockerEngine>` and never learn that the engine is `bollard`
//! talking to a unix socket. They pass ids and get back [`models`] types; a
//! `podman`-over-TCP or a fake in a test is another [`DockerEngine`] and nothing
//! above this module changes. **This module is the only place that may name
//! `bollard`**, exactly as `services::http` is the only place that names
//! `reqwest`.
//!
//! # Threading — a tokio runtime under a blocking contract
//!
//! `reqwest` is used in blocking mode; `bollard` has no blocking mode, it is
//! async on tokio. So [`BollardEngine`](engine::BollardEngine) owns one
//! multi-threaded tokio runtime, built once, and every method drives its async
//! `bollard` calls to completion with `Runtime::block_on`. The methods are
//! therefore **blocking by contract**, just like `Transport::execute`, and every
//! caller runs them on GPUI's background executor — never on the UI thread. The
//! runtime's own worker threads carry the socket IO; the executor thread that
//! called `block_on` simply parks until the call returns. No async runtime ever
//! touches the render path.
//!
//! # Where the remaining features plug in
//!
//! An Exec/terminal session and a Create/Build/Pull flow are the two things the
//! module still stubs, and both land here first: `bollard` already has
//! `create_exec`/`start_exec` (a bidirectional stream, so the trait would need a
//! non-blocking shape for that one — see `docker/mod.rs`) and
//! `create_container` / `build_image` / `create_image`, which are ordinary
//! blocking-by-contract additions like the methods below.
//!
//! # Connection resolution
//!
//! [`engine::BollardEngine`] resolves a daemon the way the Docker CLI does:
//! honour `DOCKER_HOST` if set, otherwise fall back per platform — on unix the
//! standard Docker socket at `/var/run/docker.sock` and then the Podman default
//! socket, on Windows the default `//./pipe/docker_engine` named pipe (bollard
//! has no Windows Podman probing; Podman there is reached via `DOCKER_HOST`).
//! `engine::connect` is `#[cfg]`-split accordingly. Building a
//! connection does not prove the daemon is up — that only shows on the first
//! call — so an unreachable daemon surfaces as a [`DockerError::Unreachable`]
//! from `list_containers`, which the page renders as its error state with a
//! Retry button rather than crashing.

pub mod engine;

use std::sync::Arc;

use crate::docker::models::container::Container;
use crate::docker::models::image::Image;
use crate::docker::models::inspect::InspectDetail;
use crate::docker::models::logs::LogLine;
use crate::docker::models::network::Network;
use crate::docker::models::usage::ContainerUsage;
use crate::docker::models::volume::Volume;
use crate::i18n::Str;

/// A Docker operation that did not complete, in terms the UI can act on.
///
/// The underlying `bollard` / IO message is third-party English kept verbatim
/// inside a translated frame — the same convention the transport and store
/// errors follow.
#[derive(Debug, Clone)]
pub enum DockerError {
    /// The engine could not be reached: no socket, daemon down, a bad
    /// `DOCKER_HOST`. Drives the page's error state and its Retry.
    Unreachable(String),
    /// A specific request failed against a reachable engine — a lifecycle action
    /// on one container, most often. Drives the inline action banner.
    Operation(String),
}

impl DockerError {
    /// The message shown for this failure.
    pub fn message(&self) -> Str {
        match self {
            DockerError::Unreachable(detail) => Str::DockerConnectionError(detail.clone()),
            DockerError::Operation(detail) => Str::DockerOperationError(detail.clone()),
        }
    }
}

/// A Docker engine backend.
///
/// Every method performs blocking IO (see the module threading note) and is
/// always invoked from a background task. `Send + Sync + 'static` is what lets
/// one be shared as an `Arc` across those tasks.
pub trait DockerEngine: Send + Sync + 'static {
    /// Every container, running or not, as table rows — including the
    /// `StartedAt` used for the Last Started column but *not* the CPU percent,
    /// which is measured separately and per row.
    fn list_containers(&self) -> Result<Vec<Container>, DockerError>;

    /// The current CPU busy-percent for one container, or `None` when it cannot
    /// be measured (the container stopped, or the engine gave an incomplete
    /// sample). Measured from two stats frames, so this call takes about a
    /// second.
    fn cpu_percent(&self, id: &str) -> Result<Option<f64>, DockerError>;

    fn start(&self, id: &str) -> Result<(), DockerError>;
    fn stop(&self, id: &str) -> Result<(), DockerError>;
    fn restart(&self, id: &str) -> Result<(), DockerError>;
    fn remove(&self, id: &str) -> Result<(), DockerError>;

    /// The container set reduced to its resource references — the image each
    /// runs, the volumes it mounts, the networks it is attached to. The
    /// Images/Volumes/Networks pages count their "containers using" column
    /// against this, deriving it from the live container list rather than
    /// trusting the engine's own per-resource counters. Cheaper than
    /// [`list_containers`](DockerEngine::list_containers): it needs no per-row
    /// inspect.
    fn container_usage(&self) -> Result<ContainerUsage, DockerError>;

    /// Every image, as table rows. "Containers using" is not included — it is
    /// derived from [`container_usage`](DockerEngine::container_usage).
    fn list_images(&self) -> Result<Vec<Image>, DockerError>;
    /// Removes an image by id. Refused by the engine (surfaced as a
    /// [`DockerError::Operation`]) when a container still references it.
    fn remove_image(&self, id: &str) -> Result<(), DockerError>;

    /// Every volume, as table rows.
    fn list_volumes(&self) -> Result<Vec<Volume>, DockerError>;
    /// Removes a volume by name. Refused while a container still mounts it.
    fn remove_volume(&self, name: &str) -> Result<(), DockerError>;

    /// Every network, as table rows.
    fn list_networks(&self) -> Result<Vec<Network>, DockerError>;
    /// Removes a network by id. Refused for the predefined networks and while a
    /// container is still attached.
    fn remove_network(&self, id: &str) -> Result<(), DockerError>;

    // ---- Read-only detail (round 5) -----------------------------------------
    //
    // One method per resource because the Engine API has one endpoint per
    // resource; each returns the same
    // [`InspectDetail`](crate::docker::models::inspect::InspectDetail), so the
    // panel above is written once. The engine's own response is what the model
    // reduces, and the pretty-printed JSON travels inside the detail.

    /// A container's full inspect: key fields plus the raw JSON.
    fn inspect_container(&self, id: &str) -> Result<InspectDetail, DockerError>;
    /// An image's full inspect.
    fn inspect_image(&self, id: &str) -> Result<InspectDetail, DockerError>;
    /// A volume's full inspect, by name.
    fn inspect_volume(&self, name: &str) -> Result<InspectDetail, DockerError>;
    /// A network's full inspect.
    fn inspect_network(&self, id: &str) -> Result<InspectDetail, DockerError>;

    /// The last `tail` lines a container wrote to stdout and stderr, oldest
    /// first. Not a follow: the call returns once the engine has replayed the
    /// requested window. A container that has never started, or has written
    /// nothing, is not an error — it yields no lines.
    fn container_logs(&self, id: &str, tail: usize) -> Result<Vec<LogLine>, DockerError>;
}

/// The engine the app runs with: `bollard` against the resolved local socket.
pub fn default_engine() -> Arc<dyn DockerEngine> {
    Arc::new(engine::BollardEngine::new())
}
