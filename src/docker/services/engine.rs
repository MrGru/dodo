//! The `bollard` implementation of [`DockerEngine`]. The one file in the crate
//! that names the Docker API client.
//!
//! Every public method is blocking: it drives an async `bollard` call to
//! completion on the owned tokio runtime and returns a [`models`] type, so the
//! layers above never see a future, a socket, or a `bollard` struct. See the
//! module doc in `services/mod.rs` for why the runtime lives here.

use std::future::Future;
use std::path::Path;

use bollard::container::LogOutput;
use bollard::models::{ContainerStatsResponse, ContainerSummary, ImageSummary};
use bollard::query_parameters::{
    ListContainersOptionsBuilder, ListImagesOptionsBuilder, LogsOptionsBuilder,
    RemoveContainerOptionsBuilder, RemoveImageOptionsBuilder, RemoveVolumeOptionsBuilder,
    RestartContainerOptionsBuilder, StatsOptionsBuilder, StopContainerOptionsBuilder,
};
use bollard::{API_DEFAULT_VERSION, Docker};
use futures_util::StreamExt as _;
use serde::Serialize;
use tokio::runtime::Runtime;

use crate::docker::models::container::{Container, clean_name, compose_project};
use crate::docker::models::image::{Image, split_repo_tag};
use crate::docker::models::inspect::{InspectDetail, InspectKind};
use crate::docker::models::logs::{LogLine, LogStream, lines_from_frames, tail};
use crate::docker::models::network::Network;
use crate::docker::models::port::PortMapping;
use crate::docker::models::stats::{CpuSample, cpu_percent};
use crate::docker::models::status::ContainerStatus;
use crate::docker::models::time::parse_rfc3339_to_unix;
use crate::docker::models::usage::{ContainerUsage, ContainerUsageEntry};
use crate::docker::models::volume::Volume;
use crate::docker::services::{DockerEngine, DockerError};

/// The standard Docker socket, tried when `DOCKER_HOST` is not set.
const DOCKER_SOCKET: &str = "/var/run/docker.sock";
/// The read/write timeout, in seconds, for a single connection.
const CONNECT_TIMEOUT: u64 = 120;

pub struct BollardEngine {
    /// One multi-threaded tokio runtime for the whole app's Docker IO. `None`
    /// only if the runtime itself failed to build (effectively OOM), in which
    /// case every call reports the engine as unreachable rather than panicking.
    runtime: Option<Runtime>,
}

impl BollardEngine {
    pub fn new() -> Self {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .ok();
        Self { runtime }
    }

    /// Drives one already-connected interaction to completion on the runtime.
    /// A missing runtime reports the engine as unreachable rather than panicking.
    fn block_on<T>(
        &self,
        future: impl Future<Output = Result<T, DockerError>>,
    ) -> Result<T, DockerError> {
        match self.runtime.as_ref() {
            Some(runtime) => runtime.block_on(future),
            None => Err(DockerError::Unreachable(
                "the async runtime for Docker could not be started".to_string(),
            )),
        }
    }
}

impl Default for BollardEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// Resolves a daemon the way the Docker CLI does, in the order the round-1 brief
/// specifies: honour `DOCKER_HOST`, then the standard Docker socket, then the
/// Podman default socket. Building the client does not contact the daemon, so a
/// down engine is not caught here — the first call reports it.
fn connect() -> Result<Docker, DockerError> {
    // 1. An explicit DOCKER_HOST wins; `connect_with_defaults` reads it and picks
    //    the right connector (unix socket / named pipe / http) itself.
    if std::env::var_os("DOCKER_HOST").is_some_and(|value| !value.is_empty()) {
        return Docker::connect_with_defaults().map_err(unreachable);
    }
    // 2. The standard Docker socket, if it is there.
    if Path::new(DOCKER_SOCKET).exists() {
        return Docker::connect_with_unix(DOCKER_SOCKET, CONNECT_TIMEOUT, API_DEFAULT_VERSION)
            .map_err(unreachable);
    }
    // 3. Otherwise fall back to Podman's default socket probing.
    Docker::connect_with_podman_defaults().map_err(unreachable)
}

fn unreachable(error: bollard::errors::Error) -> DockerError {
    DockerError::Unreachable(error.to_string())
}

fn operation(error: bollard::errors::Error) -> DockerError {
    DockerError::Operation(error.to_string())
}

impl DockerEngine for BollardEngine {
    fn list_containers(&self) -> Result<Vec<Container>, DockerError> {
        self.block_on(async {
            let docker = connect()?;
            let options = ListContainersOptionsBuilder::new().all(true).build();
            let summaries = docker
                .list_containers(Some(options))
                .await
                .map_err(unreachable)?;

            let mut rows = Vec::with_capacity(summaries.len());
            for summary in &summaries {
                let mut row = row_from_summary(summary);
                // The Last Started time is not in the list summary; it comes from
                // a per-container inspect. A failed inspect leaves the row with no
                // start time (rendered "Never") rather than failing the whole page.
                if !row.id.is_empty()
                    && let Ok(details) = docker.inspect_container(&row.id, None).await
                {
                    row.started_at = details
                        .state
                        .and_then(|state| state.started_at)
                        .as_deref()
                        .and_then(parse_rfc3339_to_unix);
                }
                rows.push(row);
            }
            Ok(rows)
        })
    }

    fn cpu_percent(&self, id: &str) -> Result<Option<f64>, DockerError> {
        self.block_on(async {
            let docker = connect()?;
            // Cumulative counters make a percentage a delta between two frames.
            // The streaming endpoint gives frames ~1s apart; two is enough and
            // works whether or not the daemon fills its own `precpu` block.
            let options = StatsOptionsBuilder::new().stream(true).build();
            let mut stream = Box::pin(docker.stats(id, Some(options)));

            let Some(first) = next_frame(&mut stream).await? else {
                return Ok(None);
            };
            let Some(second) = next_frame(&mut stream).await? else {
                return Ok(None);
            };

            match (cpu_sample(&first), cpu_sample(&second)) {
                (Some(earlier), Some(later)) => Ok(cpu_percent(earlier, later)),
                _ => Ok(None),
            }
        })
    }

    fn start(&self, id: &str) -> Result<(), DockerError> {
        self.block_on(async {
            let docker = connect()?;
            docker
                .start_container(id, None::<bollard::query_parameters::StartContainerOptions>)
                .await
                .map_err(operation)
        })
    }

    fn stop(&self, id: &str) -> Result<(), DockerError> {
        self.block_on(async {
            let docker = connect()?;
            let options = StopContainerOptionsBuilder::new().build();
            docker
                .stop_container(id, Some(options))
                .await
                .map_err(operation)
        })
    }

    fn restart(&self, id: &str) -> Result<(), DockerError> {
        self.block_on(async {
            let docker = connect()?;
            let options = RestartContainerOptionsBuilder::new().build();
            docker
                .restart_container(id, Some(options))
                .await
                .map_err(operation)
        })
    }

    fn remove(&self, id: &str) -> Result<(), DockerError> {
        self.block_on(async {
            let docker = connect()?;
            // `force` so a running container can be removed after the user
            // confirms, matching the Delete action's intent.
            let options = RemoveContainerOptionsBuilder::new().force(true).build();
            docker
                .remove_container(id, Some(options))
                .await
                .map_err(operation)
        })
    }

    fn container_usage(&self) -> Result<ContainerUsage, DockerError> {
        self.block_on(async {
            let docker = connect()?;
            // No per-container inspect: the summary already carries the image id,
            // the mounts and the network attachments the usage columns need.
            let options = ListContainersOptionsBuilder::new().all(true).build();
            let summaries = docker
                .list_containers(Some(options))
                .await
                .map_err(unreachable)?;
            let entries = summaries.iter().map(usage_entry).collect();
            Ok(ContainerUsage::new(entries))
        })
    }

    fn list_images(&self) -> Result<Vec<Image>, DockerError> {
        self.block_on(async {
            let docker = connect()?;
            // Default (`all(false)`): the top-level images `docker images` shows,
            // including dangling `<none>` ones, but not intermediate layers.
            let options = ListImagesOptionsBuilder::new().build();
            let summaries = docker
                .list_images(Some(options))
                .await
                .map_err(unreachable)?;
            Ok(summaries.iter().map(image_from_summary).collect())
        })
    }

    fn remove_image(&self, id: &str) -> Result<(), DockerError> {
        self.block_on(async {
            let docker = connect()?;
            // No `force`: an image still used by a container must be refused so
            // the page can surface the "image in use" message rather than
            // silently deleting a tag out from under a running container.
            let options = RemoveImageOptionsBuilder::new().build();
            docker
                .remove_image(id, Some(options), None)
                .await
                .map(|_| ())
                .map_err(operation)
        })
    }

    fn list_volumes(&self) -> Result<Vec<Volume>, DockerError> {
        self.block_on(async {
            let docker = connect()?;
            let response = docker
                .list_volumes(None::<bollard::query_parameters::ListVolumesOptions>)
                .await
                .map_err(unreachable)?;
            Ok(response
                .volumes
                .into_iter()
                .flatten()
                .map(volume_from_engine)
                .collect())
        })
    }

    fn remove_volume(&self, name: &str) -> Result<(), DockerError> {
        self.block_on(async {
            let docker = connect()?;
            // No `force`: a volume still mounted by a container is refused, and
            // the refusal becomes the page's inline error.
            let options = RemoveVolumeOptionsBuilder::new().build();
            docker
                .remove_volume(name, Some(options))
                .await
                .map_err(operation)
        })
    }

    fn list_networks(&self) -> Result<Vec<Network>, DockerError> {
        self.block_on(async {
            let docker = connect()?;
            let networks = docker
                .list_networks(None::<bollard::query_parameters::ListNetworksOptions>)
                .await
                .map_err(unreachable)?;
            Ok(networks.iter().map(network_from_engine).collect())
        })
    }

    fn remove_network(&self, id: &str) -> Result<(), DockerError> {
        self.block_on(async {
            let docker = connect()?;
            docker.remove_network(id).await.map_err(operation)
        })
    }

    fn inspect_container(&self, id: &str) -> Result<InspectDetail, DockerError> {
        self.block_on(async {
            let docker = connect()?;
            let response = docker
                .inspect_container(id, None)
                .await
                .map_err(operation)?;
            detail(InspectKind::Container, &response)
        })
    }

    fn inspect_image(&self, id: &str) -> Result<InspectDetail, DockerError> {
        self.block_on(async {
            let docker = connect()?;
            let response = docker.inspect_image(id).await.map_err(operation)?;
            detail(InspectKind::Image, &response)
        })
    }

    fn inspect_volume(&self, name: &str) -> Result<InspectDetail, DockerError> {
        self.block_on(async {
            let docker = connect()?;
            let response = docker.inspect_volume(name).await.map_err(operation)?;
            detail(InspectKind::Volume, &response)
        })
    }

    fn inspect_network(&self, id: &str) -> Result<InspectDetail, DockerError> {
        self.block_on(async {
            let docker = connect()?;
            let response = docker.inspect_network(id, None).await.map_err(operation)?;
            detail(InspectKind::Network, &response)
        })
    }

    fn container_logs(&self, id: &str, tail_lines: usize) -> Result<Vec<LogLine>, DockerError> {
        self.block_on(async move {
            let docker = connect()?;
            // `follow(false)`: the engine replays the requested window and closes
            // the stream, so the call terminates on its own. Live following is
            // deliberately out of scope — see `models::logs`.
            let options = LogsOptionsBuilder::new()
                .stdout(true)
                .stderr(true)
                .follow(false)
                .tail(&tail_lines.to_string())
                .build();
            let mut stream = Box::pin(docker.logs(id, Some(options)));

            let mut frames = Vec::new();
            while let Some(frame) = stream.next().await {
                frames.push(log_frame(frame.map_err(operation)?));
            }
            // The engine already bounded the window; bounding again keeps the
            // promise even if a daemon ignores `tail`.
            Ok(tail(lines_from_frames(frames), tail_lines))
        })
    }
}

/// Reduces one inspect response to the panel's detail. The response is turned
/// into plain JSON here — the last point where a `bollard` type is named — and
/// every field rule then lives in the unit-tested
/// [`models::inspect`](crate::docker::models::inspect).
fn detail(kind: InspectKind, response: &impl Serialize) -> Result<InspectDetail, DockerError> {
    let value = serde_json::to_value(response)
        .map_err(|error| DockerError::Operation(error.to_string()))?;
    Ok(InspectDetail::from_value(kind, &value))
}

/// One log frame as the model wants it: which stream it came from and its bytes
/// as lossy UTF-8 (a container may write anything at all).
fn log_frame(output: LogOutput) -> (LogStream, String) {
    let (stream, message) = match output {
        LogOutput::StdErr { message } => (LogStream::Stderr, message),
        // StdIn and Console are what a TTY-attached container reports; both are
        // what the user sees as normal output.
        LogOutput::StdOut { message }
        | LogOutput::StdIn { message }
        | LogOutput::Console { message } => (LogStream::Stdout, message),
    };
    (stream, String::from_utf8_lossy(&message).into_owned())
}

/// Reduces one container summary to its resource references for the usage
/// columns: the resolved image id, its named-volume mounts, and its network
/// attachments. Bind mounts (no name) are skipped so they do not inflate a
/// volume's count.
fn usage_entry(summary: &ContainerSummary) -> ContainerUsageEntry {
    let volume_names = summary
        .mounts
        .iter()
        .flatten()
        .filter_map(|mount| mount.name.clone())
        .filter(|name| !name.is_empty())
        .collect();

    let network_names = summary
        .network_settings
        .as_ref()
        .and_then(|settings| settings.networks.as_ref())
        .map(|networks| networks.keys().cloned().collect())
        .unwrap_or_default();

    ContainerUsageEntry {
        image_id: summary.image_id.clone().unwrap_or_default(),
        volume_names,
        network_names,
    }
}

/// Translates one engine image summary into a table row.
fn image_from_summary(summary: &ImageSummary) -> Image {
    let (repository, tag) = split_repo_tag(&summary.repo_tags);
    Image {
        id: summary.id.clone(),
        repository,
        tag,
        size: summary.size,
        created: summary.created,
    }
}

/// Translates one engine volume into a table row. A size of `-1` (or a driver
/// that reports none) becomes `None`, rendered as `N/A`.
fn volume_from_engine(volume: bollard::models::Volume) -> Volume {
    let size = volume
        .usage_data
        .map(|usage| usage.size)
        .filter(|&size| size >= 0);
    Volume {
        name: volume.name,
        driver: volume.driver,
        mountpoint: volume.mountpoint,
        size,
    }
}

/// Translates one engine network into a table row.
fn network_from_engine(network: &bollard::models::Network) -> Network {
    Network {
        id: network.id.clone().unwrap_or_default(),
        name: network.name.clone().unwrap_or_default(),
        driver: network.driver.clone().unwrap_or_default(),
        scope: network.scope.clone().unwrap_or_default(),
        created: network.created.as_deref().and_then(parse_rfc3339_to_unix),
    }
}

/// Pulls the next stats frame, mapping a stream error to an operation failure and
/// the end of the stream (a container that stopped mid-read) to `None`.
async fn next_frame(
    stream: &mut (
             impl futures_util::Stream<Item = Result<ContainerStatsResponse, bollard::errors::Error>>
             + Unpin
         ),
) -> Result<Option<ContainerStatsResponse>, DockerError> {
    match stream.next().await {
        Some(Ok(frame)) => Ok(Some(frame)),
        Some(Err(error)) => Err(operation(error)),
        None => Ok(None),
    }
}

/// The CPU sample a percentage needs, or `None` if the frame is missing any of
/// the three counters.
fn cpu_sample(frame: &ContainerStatsResponse) -> Option<CpuSample> {
    let cpu = frame.cpu_stats.as_ref()?;
    Some(CpuSample {
        container_usage: cpu.cpu_usage.as_ref()?.total_usage?,
        system_usage: cpu.system_cpu_usage?,
        online_cpus: cpu.online_cpus.unwrap_or(1).max(1) as u64,
    })
}

/// Translates one engine container summary into a table row (everything except
/// the separately-measured CPU and the inspect-sourced start time).
fn row_from_summary(summary: &ContainerSummary) -> Container {
    let name = summary
        .names
        .as_ref()
        .and_then(|names| names.first())
        .map(|name| clean_name(name))
        .unwrap_or_default();

    let status = ContainerStatus::from_engine_state(
        &summary
            .state
            .as_ref()
            .map(|state| state.to_string())
            .unwrap_or_default(),
    );

    let ports = summary
        .ports
        .iter()
        .flatten()
        .map(|port| PortMapping {
            host: port.public_port,
            container: port.private_port,
            protocol: port
                .typ
                .as_ref()
                .map(|typ| typ.to_string())
                .unwrap_or_else(|| "tcp".to_string()),
        })
        .collect();

    let compose_project = summary.labels.as_ref().and_then(compose_project);

    Container {
        id: summary.id.clone().unwrap_or_default(),
        name,
        image: summary.image.clone().unwrap_or_default(),
        status,
        ports,
        compose_project,
        started_at: None,
        cpu_percent: None,
    }
}
