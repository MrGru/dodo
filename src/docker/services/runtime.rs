//! Round 7: automatic detection of the container runtimes/daemons on this
//! machine (Docker, Podman Machine, Kubernetes, containerd) plus Start/Stop,
//! behind a trait for the same reason `services::DockerEngine` is one — the
//! Runtimes page never learns it is shelling out to `docker`, `podman`,
//! `kubectl` and `systemctl`.
//!
//! Unlike the Docker engine there is exactly one implementation and no
//! third-party client crate to isolate, so the trait, its error type and the
//! implementation live together in this one file rather than split across
//! `mod.rs` and a sibling.
//!
//! # Never a shell string
//!
//! Every command is run as an argument vector
//! (`Command::new(program).args(args)`), never `sh -c` / `cmd /C` with an
//! interpolated string. [`crate::docker::models::runtime::command_for`] is the
//! only place a command is assembled, and it hands back a program name plus
//! an already-split `Vec<String>`; this file only ever executes what that
//! table produced.
//!
//! # What is, and is not, unit tested
//!
//! `models::runtime` is exhaustively tested: which command runs for which
//! (kind, action, platform), and how to read a captured stdout/exit code back
//! into a status. Actually spawning a process is not — like
//! `services::engine::connect`, that needs a live daemon or an installed
//! binary this test suite cannot assume exists on the machine running it.

use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use crate::docker::models::runtime::{
    self, CommandOutput, RuntimeAction, RuntimeCommand, RuntimeInfo, RuntimeKind, RuntimeStatus,
};
use crate::i18n::{Str, docker};
use crate::paths::HostOs;

/// A runtime Start/Stop that did not complete, in terms the UI can act on.
#[derive(Debug, Clone)]
pub enum RuntimeError {
    /// No command exists for this (kind, action, platform) — the UI is not
    /// supposed to reach this, since the button is disabled whenever
    /// [`RuntimeInfo::can_start`]/`can_stop` says so, but a stale click
    /// (raced by a background poll) is still handled rather than panicking.
    Unsupported,
    /// Neither a known install location nor `PATH` had the required
    /// command-line tool.
    NotFound,
    /// The command ran and reported failure.
    Operation(String),
}

impl RuntimeError {
    pub fn message(&self) -> Str {
        match self {
            RuntimeError::Unsupported => docker::Text::RuntimeActionUnsupported.into(),
            RuntimeError::NotFound => docker::Text::RuntimeBinaryNotFound.into(),
            RuntimeError::Operation(detail) => docker::Text::OperationError(detail.clone()).into(),
        }
    }
}

/// A container-runtime detection/control backend.
///
/// Every method performs blocking IO and is always invoked from a background
/// task, the same contract `DockerEngine` follows. `Send + Sync + 'static` is
/// what lets one be shared as an `Arc` across those tasks.
pub trait RuntimeService: Send + Sync + 'static {
    /// Detects every kind in [`RuntimeKind::ALL`], in that order. Never fails
    /// as a whole: a kind whose own command could not run becomes
    /// [`RuntimeStatus::Unknown`] on its row rather than losing the page.
    fn detect_all(&self) -> Vec<RuntimeInfo>;
    fn start(&self, kind: RuntimeKind) -> Result<(), RuntimeError>;
    fn stop(&self, kind: RuntimeKind) -> Result<(), RuntimeError>;
}

/// The real backend: shells out to the platform's own tools.
pub struct SystemRuntimeService {
    os: HostOs,
}

impl SystemRuntimeService {
    pub fn new() -> Self {
        Self {
            os: HostOs::current(),
        }
    }

    fn detect_one(&self, kind: RuntimeKind) -> RuntimeInfo {
        let Some(command) = runtime::command_for(kind, RuntimeAction::Detect, self.os) else {
            return RuntimeInfo::new(kind, RuntimeStatus::Unsupported, None, self.os);
        };
        match run(&command, self.os) {
            Ok(output) => {
                let (status, detail) = runtime::classify(kind, &output);
                RuntimeInfo::new(kind, status, detail, self.os)
            }
            Err(RunError::NotFound) => {
                RuntimeInfo::new(kind, RuntimeStatus::NotInstalled, None, self.os)
            }
            Err(RunError::Io(_)) => RuntimeInfo::new(kind, RuntimeStatus::Unknown, None, self.os),
        }
    }

    fn act(&self, kind: RuntimeKind, action: RuntimeAction) -> Result<(), RuntimeError> {
        let command =
            runtime::command_for(kind, action, self.os).ok_or(RuntimeError::Unsupported)?;
        match run(&command, self.os) {
            Ok(output) if output.success => Ok(()),
            Ok(output) => Err(RuntimeError::Operation(first_line(
                &output.stderr,
                &output.stdout,
            ))),
            Err(RunError::NotFound) => Err(RuntimeError::NotFound),
            Err(RunError::Io(message)) => Err(RuntimeError::Operation(message)),
        }
    }
}

impl Default for SystemRuntimeService {
    fn default() -> Self {
        Self::new()
    }
}

impl RuntimeService for SystemRuntimeService {
    fn detect_all(&self) -> Vec<RuntimeInfo> {
        RuntimeKind::ALL
            .into_iter()
            .map(|kind| self.detect_one(kind))
            .collect()
    }

    fn start(&self, kind: RuntimeKind) -> Result<(), RuntimeError> {
        self.act(kind, RuntimeAction::Start)
    }

    fn stop(&self, kind: RuntimeKind) -> Result<(), RuntimeError> {
        self.act(kind, RuntimeAction::Stop)
    }
}

enum RunError {
    /// The program itself could not be found — distinguished so a missing
    /// tool becomes [`RuntimeStatus::NotInstalled`] / [`RuntimeError::NotFound`]
    /// rather than the catch-all `Unknown`/`Operation`.
    NotFound,
    Io(String),
}

/// Runs one [`RuntimeCommand`], resolving its program through
/// [`resolve_program`] first.
fn run(command: &RuntimeCommand, os: HostOs) -> Result<CommandOutput, RunError> {
    let program = resolve_program(command.program, os);
    match Command::new(&program).args(&command.args).output() {
        Ok(output) => Ok(CommandOutput {
            success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Err(RunError::NotFound),
        Err(error) => Err(RunError::Io(error.to_string())),
    }
}

/// Resolves `program` to a known install location when one exists on disk
/// (see [`runtime::candidate_paths`]), falling back to the bare name so
/// `Command::new` still tries `PATH` — which works for a terminal-launched
/// dev build even when it will not for one launched from Finder/Explorer.
fn resolve_program(program: &str, os: HostOs) -> String {
    runtime::candidate_paths(program, os)
        .into_iter()
        .find(|candidate| Path::new(candidate).exists())
        .map(str::to_string)
        .unwrap_or_else(|| program.to_string())
}

/// The first non-empty line of stderr, falling back to stdout — the short,
/// human-readable reason a failed action carries into
/// [`RuntimeError::Operation`].
fn first_line(stderr: &str, stdout: &str) -> String {
    let text = if stderr.trim().is_empty() {
        stdout
    } else {
        stderr
    };
    text.lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("")
        .trim()
        .to_string()
}

/// The runtime-detection backend the app runs with.
pub fn default_runtime_service() -> Arc<dyn RuntimeService> {
    Arc::new(SystemRuntimeService::new())
}
