//! `CleanerCategory::DockerCache` (Phase 13): dangling/unused images, stopped
//! containers, and unused volumes/networks — via the `docker` CLI, never the
//! `bollard`/tokio integration `crates/dodo-docker/` already owns.
//!
//! # Why this does not reuse `crate::docker::services::DockerEngine`
//!
//! It would be the obvious reuse — `crates/dodo-docker/` already resolves a daemon
//! connection (Docker Desktop, then Podman) and lists containers/images/
//! volumes/networks. But `docs/cleaner/...` aside, dodo's own convention
//! (see `dodo-database-internals`'s "self-contained-module invariant": *no*
//! `use crate::` line in `src/database/` names another tool, and a
//! "detect running database containers" feature was dropped in every design
//! round specifically to avoid that compile-time edge) applies here with
//! equal force: `src/cleaner/` must not gain a `use crate::docker` edge
//! either. So this scanner is a second, independent, much smaller Docker
//! client — the `docker` CLI, invoked with argument vectors and parsed as
//! line-delimited JSON (`--format '{{json .}}'`), which needs neither
//! `bollard` nor a second tokio runtime. This is also a reasonable reading of
//! the ticket's own "Pass CLI arguments safely" and "Parse structured output
//! where available" — the ticket did not actually require the bollard route.
//!
//! # Detecting daemon status
//!
//! Every one of the four list commands can fail for the same two reasons: the
//! `docker` binary is not on `PATH` (`io::Error::NotFound`) or it runs but the
//! daemon is unreachable (non-zero exit, stderr naming the daemon). Both are
//! folded into one [`ScanCompleteness::Partial`] with
//! [`PartialScanReason::UnsupportedEnvironment`] and a [`ScanWarning`] —
//! **never** a [`ScanError`] that would fail the whole category, matching the
//! ticket's "Handle daemon unavailable cleanly" and Smart Care's "Continue
//! when one category fails".
//!
//! # No filesystem, no VM disk
//!
//! This scanner performs no filesystem traversal at all and never touches
//! Docker Desktop's VM disk file — every byte of information comes from
//! parsing `docker ... ls --format '{{json .}}'` output. "Filesystem caches"
//! (the ticket's own phrase, distinct from "Docker-engine objects") would be
//! Docker Desktop's own log files under `~/Library/Containers/
//! com.docker.docker/...`; this phase does not add that as a second root
//! here, since none of the six current filesystem scanners' conservative bar
//! has been applied to it yet — see `docs/cleaner/known-limitations.md`.
//!
//! # Reference-checking is the daemon's job, not this scanner's
//!
//! [`prune_items`] calls `docker rmi`/`rm`/`volume rm`/`network rm` with no
//! `--force`. The engine itself refuses (a normal non-zero exit, reported as
//! a per-item [`CleanupItemFailure`]) when something still references the
//! object — image still used by a container, container still running, volume
//! still mounted, network still attached or predefined. This satisfies the
//! ticket's "Check references before cleanup" more robustly than
//! re-deriving the same check from a possibly-stale scan result: the daemon's
//! answer is always current, this scan result might not be.
//!
//! # Sizes are estimates, and volumes/containers/networks have none at all
//!
//! `docker image ls` reports a human-formatted size string ("128MB", "1.2GB")
//! — [`parse_human_size`] converts it back to an approximate byte count
//! (decimal units, matching `go-units.HumanSize`, the library the `docker`
//! CLI itself uses to produce that string). `docker volume ls`/`ps -a`/
//! `network ls` report no size at all (getting one needs `docker system df
//! -v`, out of scope this phase) — those items carry `logical_size: 0` and
//! their `explanation` says so explicitly rather than implying an empty
//! reclaim.

use std::io;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;

use serde_json::Value;

use crate::core::cancellation::CancellationToken;
use crate::core::category::CleanerCategory;
use crate::core::errors::{CleanupError, ScanError};
use crate::core::item::{
    CleanableItem, CleanableItemId, DockerItemMetadata, DockerObjectKind, ItemMetadata,
};
use crate::core::permissions::MacPermission;
use crate::core::progress::{ProgressSink, ScanPhase, ScanProgress};
use crate::core::report::{
    CategoryScanResult, CleanupItemFailure, CleanupItemSuccess, CleanupReport, PartialScanReason,
    ScanCompleteness, ScanWarning,
};
use crate::core::risk::{ItemCapability, RiskLevel, SelectionPolicy};
use crate::core::scan_context::ScanContext;
use crate::core::scanner::CleanerScanner;

/// The engine's own networks, created for the daemon's lifetime and never
/// removable. Duplicated (not imported) from `docker::models::network`'s
/// identical constant — see this module's doc comment on why `src/cleaner/`
/// cannot depend on `crates/dodo-docker/` at all, not even for three string
/// literals.
const PREDEFINED_NETWORKS: [&str; 3] = ["bridge", "host", "none"];

struct DockerCommandOutput {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    exit_code: Option<i32>,
}

impl DockerCommandOutput {
    fn succeeded(&self) -> bool {
        self.exit_code == Some(0)
    }
}

trait DockerCommandRunner: Send + Sync {
    fn run(&self, args: &[&str]) -> io::Result<DockerCommandOutput>;
}

struct ProcessDockerCommandRunner;

impl DockerCommandRunner for ProcessDockerCommandRunner {
    fn run(&self, args: &[&str]) -> io::Result<DockerCommandOutput> {
        let output = Command::new("docker").args(args).output()?;
        Ok(DockerCommandOutput {
            stdout: output.stdout,
            stderr: output.stderr,
            exit_code: output.status.code(),
        })
    }
}

pub struct DockerCacheScanner {
    runner: Arc<dyn DockerCommandRunner>,
}

impl DockerCacheScanner {
    pub fn new() -> Self {
        Self {
            runner: Arc::new(ProcessDockerCommandRunner),
        }
    }

    #[cfg(test)]
    fn with_runner(runner: Arc<dyn DockerCommandRunner>) -> Self {
        Self { runner }
    }
}

impl CleanerScanner for DockerCacheScanner {
    fn category(&self) -> CleanerCategory {
        CleanerCategory::DockerCache
    }

    fn required_permissions(&self) -> &[MacPermission] {
        const NONE: &[MacPermission] = &[];
        NONE
    }

    fn scan(
        &self,
        _context: &ScanContext,
        progress: &dyn ProgressSink,
        cancellation: &CancellationToken,
    ) -> Result<CategoryScanResult, ScanError> {
        progress.report(ScanProgress {
            category: CleanerCategory::DockerCache,
            phase: ScanPhase::Preparing,
            current_path: None,
            scanned_entries: 0,
            discovered_items: 0,
            discovered_bytes: 0,
        });

        if cancellation.is_cancelled() {
            return Err(ScanError::Cancelled);
        }

        let containers = match run_json_lines(
            self.runner.as_ref(),
            &["ps", "-a", "--format", "{{json .}}"],
        ) {
            Ok(rows) => rows,
            Err(message) => return Ok(unavailable_result(message)),
        };
        if cancellation.is_cancelled() {
            return Err(ScanError::Cancelled);
        }
        let images = match run_json_lines(
            self.runner.as_ref(),
            &["image", "ls", "--format", "{{json .}}"],
        ) {
            Ok(rows) => rows,
            Err(message) => return Ok(unavailable_result(message)),
        };
        let volumes = match run_json_lines(
            self.runner.as_ref(),
            &["volume", "ls", "--format", "{{json .}}"],
        ) {
            Ok(rows) => rows,
            Err(message) => return Ok(unavailable_result(message)),
        };
        let networks = match run_json_lines(
            self.runner.as_ref(),
            &["network", "ls", "--format", "{{json .}}"],
        ) {
            Ok(rows) => rows,
            Err(message) => return Ok(unavailable_result(message)),
        };

        let used_images: Vec<String> = containers
            .iter()
            .filter_map(|row| field(row, "Image"))
            .collect();
        let used_volumes: Vec<String> = containers
            .iter()
            .flat_map(|row| split_csv(field(row, "Mounts").as_deref().unwrap_or("")))
            .collect();
        let used_networks: Vec<String> = containers
            .iter()
            .flat_map(|row| split_csv(field(row, "Networks").as_deref().unwrap_or("")))
            .collect();

        let mut items = Vec::new();

        for row in &containers {
            let Some(id) = field(row, "ID") else {
                continue;
            };
            let state = field(row, "State").unwrap_or_default();
            if !matches!(state.as_str(), "exited" | "dead") {
                continue;
            }
            let name = field(row, "Names").unwrap_or_else(|| id.clone());
            items.push(container_item(&id, &name, &state));
        }

        for row in &images {
            let Some(id) = field(row, "ID") else {
                continue;
            };
            let repository = field(row, "Repository").unwrap_or_else(|| "<none>".to_string());
            let tag = field(row, "Tag").unwrap_or_else(|| "<none>".to_string());
            let dangling = repository == "<none>" && tag == "<none>";
            let reference = format!("{repository}:{tag}");
            let short_id: String = id.chars().take(12).collect();
            let in_use = used_images
                .iter()
                .any(|used| *used == reference || used.starts_with(&short_id));
            if in_use {
                continue;
            }
            let size = field(row, "Size")
                .as_deref()
                .and_then(parse_human_size)
                .unwrap_or(0);
            items.push(image_item(&id, &repository, &tag, dangling, size));
        }

        for row in &volumes {
            let Some(name) = field(row, "Name") else {
                continue;
            };
            if used_volumes.contains(&name) {
                continue;
            }
            items.push(volume_item(&name));
        }

        for row in &networks {
            let Some(id) = field(row, "ID") else {
                continue;
            };
            let name = field(row, "Name").unwrap_or_else(|| id.clone());
            if PREDEFINED_NETWORKS.contains(&name.as_str()) {
                continue;
            }
            if used_networks.contains(&name) {
                continue;
            }
            items.push(network_item(&id, &name));
        }

        items.sort_by_key(|item| std::cmp::Reverse(item.logical_size));
        let estimated_reclaimable_bytes = items.iter().map(|item| item.logical_size).sum();
        Ok(CategoryScanResult {
            category: CleanerCategory::DockerCache,
            items,
            scanned_entries: (containers.len() + images.len() + volumes.len() + networks.len())
                as u64,
            estimated_reclaimable_bytes,
            warnings: Vec::new(),
            completeness: ScanCompleteness::Complete,
        })
    }
}

fn unavailable_result(message: String) -> CategoryScanResult {
    CategoryScanResult {
        category: CleanerCategory::DockerCache,
        items: Vec::new(),
        scanned_entries: 0,
        estimated_reclaimable_bytes: 0,
        warnings: vec![ScanWarning { message }],
        completeness: ScanCompleteness::Partial {
            skipped_roots: Vec::new(),
            reason: PartialScanReason::UnsupportedEnvironment,
        },
    }
}

fn container_item(id: &str, name: &str, state: &str) -> CleanableItem {
    CleanableItem {
        id: item_id(DockerObjectKind::Container, id),
        category: CleanerCategory::DockerCache,
        group: Some("Stopped containers".to_string()),
        display_name: name.trim_start_matches('/').to_string(),
        path: docker_path(DockerObjectKind::Container, id),
        logical_size: 0,
        allocated_size: None,
        modified_at: None,
        last_accessed_at: None,
        risk: RiskLevel::ReviewRecommended,
        selection_policy: SelectionPolicy::NotSelectedByDefault,
        capabilities: vec![ItemCapability::RunExternalCleanup, ItemCapability::CopyPath],
        explanation: format!(
            "Stopped container ({state}). Size not shown — `docker ps` does not report it; \
             removing it is safe once you no longer need its logs or filesystem changes."
        ),
        warnings: Vec::new(),
        metadata: ItemMetadata::Docker(DockerItemMetadata {
            kind: DockerObjectKind::Container,
            engine_id: id.to_string(),
        }),
    }
}

fn image_item(id: &str, repository: &str, tag: &str, dangling: bool, size: u64) -> CleanableItem {
    let display_name = if dangling {
        format!("<none> ({})", short_id(id))
    } else {
        format!("{repository}:{tag}")
    };
    CleanableItem {
        id: item_id(DockerObjectKind::Image, id),
        category: CleanerCategory::DockerCache,
        group: Some(if dangling {
            "Dangling images".to_string()
        } else {
            "Unused images".to_string()
        }),
        display_name,
        path: docker_path(DockerObjectKind::Image, id),
        logical_size: size,
        allocated_size: None,
        modified_at: None,
        last_accessed_at: None,
        risk: if dangling {
            RiskLevel::SafeRecreatable
        } else {
            RiskLevel::ReviewRecommended
        },
        selection_policy: if dangling {
            SelectionPolicy::SelectedByDefault
        } else {
            SelectionPolicy::NotSelectedByDefault
        },
        capabilities: vec![ItemCapability::RunExternalCleanup, ItemCapability::CopyPath],
        explanation: if dangling {
            "Untagged image layer no container references. Docker re-creates these during a \
             normal build; safe to remove."
                .to_string()
        } else {
            "Tagged image no running or stopped container currently uses. Removing it means \
             pulling or rebuilding it again if you need it later."
                .to_string()
        },
        warnings: Vec::new(),
        metadata: ItemMetadata::Docker(DockerItemMetadata {
            kind: DockerObjectKind::Image,
            engine_id: id.to_string(),
        }),
    }
}

fn volume_item(name: &str) -> CleanableItem {
    CleanableItem {
        id: item_id(DockerObjectKind::Volume, name),
        category: CleanerCategory::DockerCache,
        group: Some("Unused volumes".to_string()),
        display_name: name.to_string(),
        path: docker_path(DockerObjectKind::Volume, name),
        logical_size: 0,
        allocated_size: None,
        modified_at: None,
        last_accessed_at: None,
        risk: RiskLevel::UserData,
        selection_policy: SelectionPolicy::NeverBulkSelect,
        capabilities: vec![ItemCapability::RunExternalCleanup, ItemCapability::CopyPath],
        explanation: "No running or stopped container currently mounts this volume. Volumes \
             hold user data, so this is never selected automatically — review what it \
             contains before removing it."
            .to_string(),
        warnings: Vec::new(),
        metadata: ItemMetadata::Docker(DockerItemMetadata {
            kind: DockerObjectKind::Volume,
            engine_id: name.to_string(),
        }),
    }
}

fn network_item(id: &str, name: &str) -> CleanableItem {
    CleanableItem {
        id: item_id(DockerObjectKind::Network, id),
        category: CleanerCategory::DockerCache,
        group: Some("Unused networks".to_string()),
        display_name: name.to_string(),
        path: docker_path(DockerObjectKind::Network, id),
        logical_size: 0,
        allocated_size: None,
        modified_at: None,
        last_accessed_at: None,
        risk: RiskLevel::ReviewRecommended,
        selection_policy: SelectionPolicy::NotSelectedByDefault,
        capabilities: vec![ItemCapability::RunExternalCleanup, ItemCapability::CopyPath],
        explanation: "No container is currently attached to this network.".to_string(),
        warnings: Vec::new(),
        metadata: ItemMetadata::Docker(DockerItemMetadata {
            kind: DockerObjectKind::Network,
            engine_id: id.to_string(),
        }),
    }
}

/// Moves selected Docker-engine objects through the `docker` CLI —
/// `docker_cache`'s counterpart to `cleanup::cleanup_items`, never routed
/// through it: these items carry a synthetic `docker://` path, not a real
/// filesystem one, so `core::safety::validate_path` could never authorize
/// them (nor should it — there is nothing on the host filesystem to
/// contain).
pub fn prune_items(items: &[CleanableItem]) -> CleanupReport {
    prune_items_with_runner(items, &ProcessDockerCommandRunner)
}

fn prune_items_with_runner(
    items: &[CleanableItem],
    runner: &dyn DockerCommandRunner,
) -> CleanupReport {
    let mut successes = Vec::new();
    let mut failures = Vec::new();

    for item in items {
        let ItemMetadata::Docker(metadata) = &item.metadata else {
            failures.push(CleanupItemFailure {
                id: item.id,
                path: item.path.clone(),
                error: CleanupError::ExternalOperationFailed {
                    operation: "docker".to_string(),
                    message: "not a Docker-engine item".to_string(),
                },
            });
            continue;
        };
        let args = remove_args(metadata.kind, &metadata.engine_id);
        let argv: Vec<&str> = args.iter().map(String::as_str).collect();
        match runner.run(&argv) {
            Ok(output) if output.succeeded() => successes.push(CleanupItemSuccess {
                id: item.id,
                path: item.path.clone(),
                trashed_path: None,
                logical_size: item.logical_size,
            }),
            Ok(output) => failures.push(CleanupItemFailure {
                id: item.id,
                path: item.path.clone(),
                error: CleanupError::ExternalOperationFailed {
                    operation: format!("docker {}", args.join(" ")),
                    message: String::from_utf8_lossy(&output.stderr).trim().to_string(),
                },
            }),
            Err(error) => failures.push(CleanupItemFailure {
                id: item.id,
                path: item.path.clone(),
                error: CleanupError::ExternalOperationFailed {
                    operation: "docker".to_string(),
                    message: error.to_string(),
                },
            }),
        }
    }

    CleanupReport {
        estimated_reclaimed_bytes: successes.iter().map(|success| success.logical_size).sum(),
        successes,
        failures,
    }
}

fn remove_args(kind: DockerObjectKind, id: &str) -> Vec<String> {
    match kind {
        DockerObjectKind::Image => vec!["rmi".to_string(), id.to_string()],
        DockerObjectKind::Container => vec!["rm".to_string(), id.to_string()],
        DockerObjectKind::Volume => vec!["volume".to_string(), "rm".to_string(), id.to_string()],
        DockerObjectKind::Network => {
            vec!["network".to_string(), "rm".to_string(), id.to_string()]
        }
    }
}

/// Runs `docker <args>`, parsing stdout as one JSON object per line (the
/// shape `--format '{{json .}}'` produces). `Err` means "treat the whole
/// category as daemon-unavailable" — a missing binary, a non-zero exit, or
/// output that is not line-delimited JSON at all (untrusted external output,
/// per the ticket — a line that fails to parse is skipped, not fatal, but a
/// process that fails outright is).
fn run_json_lines(runner: &dyn DockerCommandRunner, args: &[&str]) -> Result<Vec<Value>, String> {
    let output = runner
        .run(args)
        .map_err(|error| format!("docker CLI not found: {error}"))?;
    if !output.succeeded() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .collect())
}

fn field(row: &Value, name: &str) -> Option<String> {
    row.get(name).and_then(Value::as_str).map(ToOwned::to_owned)
}

fn split_csv(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

/// Parses a `go-units.HumanSize`-formatted string (`"128MB"`, `"1.23GB"`,
/// `"512B"`) back to an approximate byte count, decimal units (1000-based),
/// matching the library the `docker` CLI itself uses to produce the string
/// in the first place. Returns `None` for anything that does not parse —
/// callers treat that as size `0`, never a scan failure.
fn parse_human_size(text: &str) -> Option<u64> {
    let text = text.trim();
    let split_at = text.find(|ch: char| !ch.is_ascii_digit() && ch != '.')?;
    let (number, unit) = text.split_at(split_at);
    let number: f64 = number.parse().ok()?;
    let multiplier: f64 = match unit.trim() {
        "B" => 1.0,
        "kB" => 1_000.0,
        "MB" => 1_000_000.0,
        "GB" => 1_000_000_000.0,
        "TB" => 1_000_000_000_000.0,
        _ => return None,
    };
    Some((number * multiplier).round() as u64)
}

fn short_id(id: &str) -> String {
    id.chars().take(12).collect()
}

fn docker_path(kind: DockerObjectKind, id: &str) -> PathBuf {
    let kind = match kind {
        DockerObjectKind::Image => "image",
        DockerObjectKind::Container => "container",
        DockerObjectKind::Volume => "volume",
        DockerObjectKind::Network => "network",
    };
    PathBuf::from(format!("docker://{kind}/{id}"))
}

fn item_id(kind: DockerObjectKind, id: &str) -> CleanableItemId {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    (kind as u8, id).hash(&mut hasher);
    CleanableItemId(hasher.finish())
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::io;
    use std::sync::{Arc, Mutex};

    use super::*;

    struct FakeRunner {
        responses: Mutex<VecDeque<io::Result<DockerCommandOutput>>>,
        commands: Mutex<Vec<Vec<String>>>,
    }

    impl FakeRunner {
        fn new(responses: Vec<io::Result<DockerCommandOutput>>) -> Self {
            Self {
                responses: Mutex::new(responses.into()),
                commands: Mutex::new(Vec::new()),
            }
        }

        fn commands(&self) -> Vec<Vec<String>> {
            self.commands.lock().expect("commands lock").clone()
        }
    }

    impl DockerCommandRunner for FakeRunner {
        fn run(&self, args: &[&str]) -> io::Result<DockerCommandOutput> {
            self.commands
                .lock()
                .expect("commands lock")
                .push(args.iter().map(|arg| (*arg).to_string()).collect());
            self.responses
                .lock()
                .expect("responses lock")
                .pop_front()
                .expect("a response for every command")
        }
    }

    struct IgnoreProgress;

    impl ProgressSink for IgnoreProgress {
        fn report(&self, _progress: ScanProgress) {}
    }

    fn output(exit_code: i32, stdout: &str, stderr: &str) -> io::Result<DockerCommandOutput> {
        Ok(DockerCommandOutput {
            stdout: stdout.as_bytes().to_vec(),
            stderr: stderr.as_bytes().to_vec(),
            exit_code: Some(exit_code),
        })
    }

    fn scan_with(runner: Arc<FakeRunner>) -> CategoryScanResult {
        DockerCacheScanner::with_runner(runner)
            .scan(
                &ScanContext::new(),
                &IgnoreProgress,
                &CancellationToken::new(),
            )
            .expect("scan result")
    }

    fn item_for(
        result: &CategoryScanResult,
        kind: DockerObjectKind,
        engine_id: &str,
    ) -> CleanableItem {
        result
            .items
            .iter()
            .find(|item| {
                matches!(
                    &item.metadata,
                    ItemMetadata::Docker(metadata)
                        if metadata.kind == kind && metadata.engine_id == engine_id
                )
            })
            .expect("classified item")
            .clone()
    }

    #[test]
    fn captured_json_is_classified_without_showing_referenced_objects() {
        let runner = Arc::new(FakeRunner::new(vec![
            output(
                0,
                r#"{"ID":"stopped-id","Image":"nginx:latest","Mounts":"used-vol","Networks":"used-net","State":"exited","Names":"/old-web"}
{"ID":"running-id","Image":"busy:latest","Mounts":"","Networks":"","State":"running","Names":"live"}"#,
                "",
            ),
            output(
                0,
                r#"{"ID":"sha256:nginx","Repository":"nginx","Tag":"latest","Size":"10MB"}
{"ID":"sha256:busy","Repository":"busy","Tag":"latest","Size":"20MB"}
{"ID":"sha256:dangling","Repository":"<none>","Tag":"<none>","Size":"1.5MB"}
{"ID":"sha256:alpine","Repository":"alpine","Tag":"3.20","Size":"2MB"}"#,
                "",
            ),
            output(0, "{\"Name\":\"used-vol\"}\n{\"Name\":\"free-vol\"}", ""),
            output(
                0,
                r#"{"ID":"default","Name":"bridge"}
{"ID":"used","Name":"used-net"}
{"ID":"free","Name":"free-net"}"#,
                "",
            ),
        ]));

        let result = scan_with(runner.clone());

        assert_eq!(result.scanned_entries, 11);
        assert_eq!(result.items.len(), 5);
        assert_eq!(result.estimated_reclaimable_bytes, 3_500_000);
        assert_eq!(result.completeness, ScanCompleteness::Complete);
        assert_eq!(
            runner.commands(),
            vec![
                vec!["ps", "-a", "--format", "{{json .}}"],
                vec!["image", "ls", "--format", "{{json .}}"],
                vec!["volume", "ls", "--format", "{{json .}}"],
                vec!["network", "ls", "--format", "{{json .}}"],
            ]
        );

        let container = item_for(&result, DockerObjectKind::Container, "stopped-id");
        assert_eq!(
            container.selection_policy,
            SelectionPolicy::NotSelectedByDefault
        );
        let dangling = item_for(&result, DockerObjectKind::Image, "sha256:dangling");
        assert_eq!(dangling.risk, RiskLevel::SafeRecreatable);
        assert_eq!(
            dangling.selection_policy,
            SelectionPolicy::SelectedByDefault
        );
        let tagged = item_for(&result, DockerObjectKind::Image, "sha256:alpine");
        assert_eq!(tagged.risk, RiskLevel::ReviewRecommended);
        let volume = item_for(&result, DockerObjectKind::Volume, "free-vol");
        assert_eq!(volume.risk, RiskLevel::UserData);
        assert_eq!(volume.selection_policy, SelectionPolicy::NeverBulkSelect);
        let network = item_for(&result, DockerObjectKind::Network, "free");
        assert_eq!(
            network.selection_policy,
            SelectionPolicy::NotSelectedByDefault
        );
    }

    #[test]
    fn missing_cli_and_daemon_failure_are_partial_without_items() {
        let missing = Arc::new(FakeRunner::new(vec![Err(io::Error::new(
            io::ErrorKind::NotFound,
            "docker missing",
        ))]));
        let missing_result = scan_with(missing);
        assert!(missing_result.items.is_empty());
        assert!(matches!(
            missing_result.completeness,
            ScanCompleteness::Partial {
                reason: PartialScanReason::UnsupportedEnvironment,
                ..
            }
        ));
        assert!(
            missing_result.warnings[0]
                .message
                .contains("docker CLI not found")
        );

        let daemon_down = Arc::new(FakeRunner::new(vec![
            output(0, "", ""),
            output(1, "", "Cannot connect to the Docker daemon"),
        ]));
        let daemon_result = scan_with(daemon_down.clone());
        assert!(daemon_result.items.is_empty());
        assert!(matches!(
            daemon_result.completeness,
            ScanCompleteness::Partial {
                reason: PartialScanReason::UnsupportedEnvironment,
                ..
            }
        ));
        assert_eq!(
            daemon_result.warnings[0].message,
            "Cannot connect to the Docker daemon"
        );
        assert_eq!(daemon_down.commands().len(), 2);
    }

    #[test]
    fn dangling_images_are_safe_and_selected_by_default() {
        let item = image_item(
            "sha256:abcdef1234567890",
            "<none>",
            "<none>",
            true,
            1_000_000,
        );
        assert_eq!(item.risk, RiskLevel::SafeRecreatable);
        assert_eq!(item.selection_policy, SelectionPolicy::SelectedByDefault);
        assert!(!item.capabilities.contains(&ItemCapability::MoveToTrash));
        assert!(
            item.capabilities
                .contains(&ItemCapability::RunExternalCleanup)
        );
    }

    #[test]
    fn unused_tagged_images_are_review_only() {
        let item = image_item("sha256:abcdef1234567890", "nginx", "1.27", false, 500);
        assert_eq!(item.risk, RiskLevel::ReviewRecommended);
        assert_eq!(item.selection_policy, SelectionPolicy::NotSelectedByDefault);
    }

    #[test]
    fn volumes_are_never_bulk_selected() {
        let item = volume_item("my-data");
        assert_eq!(item.risk, RiskLevel::UserData);
        assert_eq!(item.selection_policy, SelectionPolicy::NeverBulkSelect);
    }

    #[test]
    fn predefined_networks_are_recognized() {
        assert!(PREDEFINED_NETWORKS.contains(&"bridge"));
        assert!(!PREDEFINED_NETWORKS.contains(&"my-custom-net"));
    }

    #[test]
    fn human_size_parses_decimal_units() {
        assert_eq!(parse_human_size("512B"), Some(512));
        assert_eq!(parse_human_size("1kB"), Some(1_000));
        assert_eq!(parse_human_size("1.5MB"), Some(1_500_000));
        assert_eq!(parse_human_size("2GB"), Some(2_000_000_000));
        assert_eq!(parse_human_size("bogus"), None);
    }

    #[test]
    fn csv_splitting_trims_and_drops_empties() {
        assert_eq!(
            split_csv("vol-a, vol-b ,,vol-c"),
            vec![
                "vol-a".to_string(),
                "vol-b".to_string(),
                "vol-c".to_string()
            ]
        );
        assert_eq!(split_csv(""), Vec::<String>::new());
    }

    #[test]
    fn pruning_uses_the_exact_non_forcing_argv_for_every_object_kind() {
        let runner = FakeRunner::new(vec![
            output(0, "", ""),
            output(0, "", ""),
            output(0, "", ""),
            output(0, "", ""),
        ]);
        let items = vec![
            image_item("image-id", "repo", "tag", false, 10),
            container_item("container-id", "old", "exited"),
            volume_item("volume-name"),
            network_item("network-id", "unused"),
        ];

        let report = prune_items_with_runner(&items, &runner);

        assert_eq!(report.successes.len(), 4);
        assert!(report.failures.is_empty());
        assert_eq!(
            runner.commands(),
            vec![
                vec!["rmi", "image-id"],
                vec!["rm", "container-id"],
                vec!["volume", "rm", "volume-name"],
                vec!["network", "rm", "network-id"],
            ]
        );
    }

    #[test]
    fn synthetic_docker_paths_never_look_like_a_real_filesystem_root() {
        let path = docker_path(DockerObjectKind::Volume, "my-data");
        assert!(!path.exists());
        assert_eq!(path, PathBuf::from("docker://volume/my-data"));
    }
}
