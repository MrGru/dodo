//! Round 7's domain model: automatic detection of the container
//! runtimes/daemons on this machine — Docker, Podman Machine, Kubernetes,
//! containerd — plus Start/Stop.
//!
//! Plain data and pure functions, no GPUI beyond the same theme-colour lookup
//! [`status::ContainerStatus::color`](crate::models::status::ContainerStatus::color)
//! already does, and no process spawning:
//! [`services::runtime`](crate::services::runtime) is the only place
//! that runs a command, mirroring how `services::engine` is the only place
//! that names `bollard`. Everything here — which kind exists on which
//! platform, which command detects or controls it, and how to read that
//! command's captured output back into a [`RuntimeStatus`] — is a pure
//! function keyed on [`HostOs`], the same platform-as-data trick
//! `crate::paths` uses, so the whole detection and control policy is unit
//! tested for all three platforms without spawning a process or requiring any
//! of the four tools to be installed on the machine running the tests.
//!
//! # Why some actions are deliberately `None`
//!
//! `command_for` returns `None` when an action does not make sense for a
//! (kind, platform) pair, and that `None` is a design decision recorded once
//! here rather than a gap:
//!
//! - **Kubernetes never offers Start or Stop.** A local cluster's lifecycle
//!   belongs to whichever provider runs it — Docker Desktop's toggle,
//!   `minikube start`/`stop`, `kind create`/`delete`, a remote control plane —
//!   and there is no command that is correct for all of them. Detection stays
//!   read-only on purpose rather than guessing one provider and silently
//!   doing nothing for the rest.
//! - **Podman Machine has no meaning on Linux.** Podman there talks to a
//!   rootless daemon directly; "machine" is the macOS/Windows concept of the
//!   VM that hosts the daemon Podman has no native build for.
//! - **containerd has no standalone system service on macOS or Windows.**
//!   Where it exists on those platforms at all, it runs inside another
//!   runtime's own VM (Docker Desktop's, Lima's, …) with no independent
//!   lifecycle a user command could address.

use crate::i18n::{Str, docker};
use crate::paths::HostOs;

/// Which container runtime/daemon a row is about.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum RuntimeKind {
    Docker,
    PodmanMachine,
    Kubernetes,
    Containerd,
}

impl RuntimeKind {
    /// Every kind the Runtimes page lists, in display order.
    pub const ALL: [RuntimeKind; 4] = [
        RuntimeKind::Docker,
        RuntimeKind::PodmanMachine,
        RuntimeKind::Kubernetes,
        RuntimeKind::Containerd,
    ];

    /// The localized row name. Each is a term of art, identical in every
    /// language dodo ships — the same convention `DockerPage::title` follows
    /// for "Containers"/"Images"/"Volumes"/"Networks".
    pub fn title(self) -> Str {
        match self {
            RuntimeKind::Docker => docker::Text::Docker.into(),
            RuntimeKind::PodmanMachine => docker::Text::RuntimePodmanMachine.into(),
            RuntimeKind::Kubernetes => docker::Text::RuntimeKubernetes.into(),
            RuntimeKind::Containerd => docker::Text::RuntimeContainerd.into(),
        }
    }
}

/// Where a runtime stands, as detection last found it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum RuntimeStatus {
    Running,
    Stopped,
    /// Neither a known install location nor `PATH` had the tool this kind
    /// detects through.
    NotInstalled,
    /// This kind has no concept of its own on the host platform — see the
    /// module doc's "why some actions are deliberately `None`" section, which
    /// applies to detection exactly as it does to Start/Stop.
    Unsupported,
    /// The detection command ran but its output could not be classified.
    Unknown,
}

impl RuntimeStatus {
    /// The badge caption, localized.
    pub fn label(&self) -> Str {
        match self {
            RuntimeStatus::Running => docker::Text::RuntimeStatusRunning.into(),
            RuntimeStatus::Stopped => docker::Text::RuntimeStatusStopped.into(),
            RuntimeStatus::NotInstalled => docker::Text::RuntimeStatusNotInstalled.into(),
            RuntimeStatus::Unsupported => docker::Text::RuntimeStatusUnsupported.into(),
            RuntimeStatus::Unknown => docker::Text::RuntimeStatusUnknown.into(),
        }
    }

    /// The badge colour, the same semantic-theme-field treatment
    /// [`ContainerStatus::color`](crate::models::status::ContainerStatus::color)
    /// uses: green for Running, gray for a plain Stopped/NotInstalled/
    /// Unsupported, and the muted/warning split reserved for Unknown so a
    /// genuinely unreadable result still stands out from an ordinary "off".
    pub fn color(&self, cx: &gpui::App) -> gpui::Hsla {
        use gpui_component::ActiveTheme as _;
        match self {
            RuntimeStatus::Running => cx.theme().success,
            RuntimeStatus::Stopped | RuntimeStatus::NotInstalled | RuntimeStatus::Unsupported => {
                cx.theme().muted_foreground
            }
            RuntimeStatus::Unknown => cx.theme().warning,
        }
    }
}

/// One detected row: the kind, its status, any extra context the detection
/// command reported, and whether Start/Stop apply right now.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RuntimeInfo {
    pub kind: RuntimeKind,
    pub status: RuntimeStatus,
    /// Extra context from the detection command — a server version, the
    /// running machine's name, kubectl's own status line. Raw, untranslated
    /// data, like an image tag or a container name elsewhere in this module:
    /// not UI chrome, so it is never a [`Str`].
    pub detail: Option<String>,
    pub can_start: bool,
    pub can_stop: bool,
}

impl RuntimeInfo {
    /// Builds a row, deriving `can_start`/`can_stop` from whether a command
    /// exists for that action on `os` **and** the status makes it sensible —
    /// there is no Start button on something already running, and no Stop
    /// button on something already down.
    pub fn new(
        kind: RuntimeKind,
        status: RuntimeStatus,
        detail: Option<String>,
        os: HostOs,
    ) -> Self {
        let can_start = !matches!(status, RuntimeStatus::Running)
            && command_for(kind, RuntimeAction::Start, os).is_some();
        let can_stop = matches!(status, RuntimeStatus::Running)
            && command_for(kind, RuntimeAction::Stop, os).is_some();
        Self {
            kind,
            status,
            detail,
            can_start,
            can_stop,
        }
    }
}

/// Which operation a [`RuntimeCommand`] performs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RuntimeAction {
    Detect,
    Start,
    Stop,
}

/// A concrete command to run: a program name plus its argument vector,
/// already split — never a shell string. `services::runtime` runs this
/// exactly as `Command::new(program).args(args)`, so nothing downstream of
/// this table ever builds `sh -c "…"` out of interpolated text.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RuntimeCommand {
    pub program: &'static str,
    pub args: Vec<String>,
}

impl RuntimeCommand {
    fn new(program: &'static str, args: &[&str]) -> Self {
        Self {
            program,
            args: args.iter().map(|arg| arg.to_string()).collect(),
        }
    }
}

/// The command that performs `action` for `kind` on `os`, or `None` when the
/// action does not exist there — see the module doc.
pub fn command_for(kind: RuntimeKind, action: RuntimeAction, os: HostOs) -> Option<RuntimeCommand> {
    match (kind, action, os) {
        // Docker: `docker info` proves the daemon answers and hands back its
        // version; Start/Stop drive Docker Desktop on macOS/Windows and the
        // system daemon directly on Linux.
        (RuntimeKind::Docker, RuntimeAction::Detect, _) => Some(RuntimeCommand::new(
            "docker",
            &["info", "--format", "{{.ServerVersion}}"],
        )),
        (RuntimeKind::Docker, RuntimeAction::Start, HostOs::MacOs) => {
            Some(RuntimeCommand::new("open", &["-a", "Docker"]))
        }
        (RuntimeKind::Docker, RuntimeAction::Start, HostOs::Windows) => Some(RuntimeCommand::new(
            "powershell",
            &["-NoProfile", "-Command", "Start-Process 'Docker Desktop'"],
        )),
        (RuntimeKind::Docker, RuntimeAction::Start, HostOs::Unix) => {
            Some(RuntimeCommand::new("systemctl", &["start", "docker"]))
        }
        (RuntimeKind::Docker, RuntimeAction::Stop, HostOs::MacOs) => {
            // AppleScript quit, not a socket call: Docker Desktop has no CLI
            // stop, and this is the same clean shutdown the menu-bar Quit
            // performs. The script is a fixed literal, passed as one `-e`
            // argument — never interpolated into a shell string.
            Some(RuntimeCommand::new(
                "osascript",
                &["-e", "quit app \"Docker\""],
            ))
        }
        (RuntimeKind::Docker, RuntimeAction::Stop, HostOs::Windows) => Some(RuntimeCommand::new(
            "powershell",
            &[
                "-NoProfile",
                "-Command",
                "Stop-Process -Name 'Docker Desktop' -Force",
            ],
        )),
        (RuntimeKind::Docker, RuntimeAction::Stop, HostOs::Unix) => {
            Some(RuntimeCommand::new("systemctl", &["stop", "docker"]))
        }

        // Podman Machine: the VM concept exists only on macOS and Windows.
        (RuntimeKind::PodmanMachine, _, HostOs::Unix) => None,
        (RuntimeKind::PodmanMachine, RuntimeAction::Detect, _) => Some(RuntimeCommand::new(
            "podman",
            &["machine", "list", "--format", "json"],
        )),
        (RuntimeKind::PodmanMachine, RuntimeAction::Start, _) => {
            Some(RuntimeCommand::new("podman", &["machine", "start"]))
        }
        (RuntimeKind::PodmanMachine, RuntimeAction::Stop, _) => {
            Some(RuntimeCommand::new("podman", &["machine", "stop"]))
        }

        // Kubernetes: read-only everywhere, on every platform — see the
        // module doc for why Start/Stop are never offered.
        (RuntimeKind::Kubernetes, RuntimeAction::Detect, _) => Some(RuntimeCommand::new(
            "kubectl",
            &["cluster-info", "--request-timeout=2s"],
        )),
        (RuntimeKind::Kubernetes, RuntimeAction::Start | RuntimeAction::Stop, _) => None,

        // containerd: a standalone systemd unit only exists on Linux.
        (RuntimeKind::Containerd, RuntimeAction::Detect, HostOs::Unix) => Some(
            RuntimeCommand::new("systemctl", &["is-active", "containerd"]),
        ),
        (RuntimeKind::Containerd, RuntimeAction::Detect, _) => None,
        (RuntimeKind::Containerd, RuntimeAction::Start, HostOs::Unix) => {
            Some(RuntimeCommand::new("systemctl", &["start", "containerd"]))
        }
        (RuntimeKind::Containerd, RuntimeAction::Start, _) => None,
        (RuntimeKind::Containerd, RuntimeAction::Stop, HostOs::Unix) => {
            Some(RuntimeCommand::new("systemctl", &["stop", "containerd"]))
        }
        (RuntimeKind::Containerd, RuntimeAction::Stop, _) => None,
    }
}

/// Fixed install locations to check before falling back to `PATH`
/// resolution — the same reason `services::engine::podman_machine_socket`
/// never shells out to `podman` to find it: dodo launched from
/// Finder/Explorer does not inherit a shell's `PATH`, so Homebrew's and
/// Docker Desktop's own bundled binaries are often invisible to
/// `Command::new("docker")` even though a terminal finds them instantly.
/// `services::runtime::resolve_program` still falls back to the bare name
/// when none of these exist, which is what makes a terminal-launched dev
/// build (and a Linux install that used its package manager's own `/usr/bin`
/// path) keep working.
pub fn candidate_paths(program: &str, os: HostOs) -> Vec<&'static str> {
    match (program, os) {
        ("docker", HostOs::MacOs) => vec![
            "/usr/local/bin/docker",
            "/opt/homebrew/bin/docker",
            "/Applications/Docker.app/Contents/Resources/bin/docker",
        ],
        ("podman", HostOs::MacOs) => vec!["/opt/homebrew/bin/podman", "/usr/local/bin/podman"],
        ("kubectl", HostOs::MacOs) => vec!["/opt/homebrew/bin/kubectl", "/usr/local/bin/kubectl"],
        ("osascript", HostOs::MacOs) => vec!["/usr/bin/osascript"],
        ("open", HostOs::MacOs) => vec!["/usr/bin/open"],
        ("systemctl", HostOs::Unix) => vec!["/usr/bin/systemctl", "/bin/systemctl"],
        ("docker", HostOs::Unix) => vec!["/usr/bin/docker", "/usr/local/bin/docker"],
        ("kubectl", HostOs::Unix) => vec!["/usr/local/bin/kubectl", "/usr/bin/kubectl"],
        _ => Vec::new(),
    }
}

/// What a finished command reported, exactly the fields
/// `services::runtime` can hand back without this module ever touching a
/// process itself.
#[derive(Clone, Debug, Default)]
pub struct CommandOutput {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

/// Reads one finished [`CommandOutput`] into a status and optional detail, for
/// the detection command that ran for `kind`.
pub fn classify(kind: RuntimeKind, output: &CommandOutput) -> (RuntimeStatus, Option<String>) {
    match kind {
        RuntimeKind::Docker => classify_docker(output),
        RuntimeKind::PodmanMachine => classify_podman_machine(output),
        RuntimeKind::Kubernetes => classify_kubectl(output),
        RuntimeKind::Containerd => classify_systemctl_is_active(output),
    }
}

/// `docker info --format {{.ServerVersion}}`: success with a version string on
/// stdout means the daemon answered; any failure (no daemon, wrong context,
/// permission denied) is read as simply stopped rather than distinguished
/// further — the same "unreachable is unreachable" simplicity
/// `DockerError::Unreachable` uses elsewhere in this module.
fn classify_docker(output: &CommandOutput) -> (RuntimeStatus, Option<String>) {
    if !output.success {
        return (RuntimeStatus::Stopped, None);
    }
    let version = output.stdout.trim();
    (
        RuntimeStatus::Running,
        (!version.is_empty()).then(|| version.to_string()),
    )
}

/// `podman machine list --format json`: an array of machine objects, each
/// with (at least) `Name` and `Running`. Any machine running is enough to
/// call the kind Running, named after the first one found; a well-formed
/// empty or all-stopped list is Stopped; anything the JSON shape does not
/// match is Unknown rather than guessed at.
fn classify_podman_machine(output: &CommandOutput) -> (RuntimeStatus, Option<String>) {
    if !output.success {
        return (RuntimeStatus::Stopped, None);
    }
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&output.stdout) else {
        return (RuntimeStatus::Unknown, None);
    };
    let Some(machines) = value.as_array() else {
        return (RuntimeStatus::Unknown, None);
    };
    let running = machines.iter().find(|machine| {
        machine
            .get("Running")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
    });
    match running {
        Some(machine) => {
            let name = machine
                .get("Name")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string);
            (RuntimeStatus::Running, name)
        }
        None => (RuntimeStatus::Stopped, None),
    }
}

/// `kubectl cluster-info --request-timeout=2s`: success means a cluster
/// answered; its first non-empty line ("Kubernetes control plane is running
/// at …") becomes the detail. Any failure — no kubeconfig, an unreachable
/// context, the 2-second timeout tripping — is read as simply stopped.
fn classify_kubectl(output: &CommandOutput) -> (RuntimeStatus, Option<String>) {
    if !output.success {
        return (RuntimeStatus::Stopped, None);
    }
    let detail = output
        .stdout
        .lines()
        .find(|line| !line.trim().is_empty())
        .map(|line| line.trim().to_string());
    (RuntimeStatus::Running, detail)
}

/// `systemctl is-active containerd`: the state is on stdout regardless of the
/// exit code (a non-`active` state exits non-zero), so this reads stdout
/// directly rather than trusting `success`.
fn classify_systemctl_is_active(output: &CommandOutput) -> (RuntimeStatus, Option<String>) {
    match output.stdout.trim() {
        "active" => (RuntimeStatus::Running, None),
        "inactive" | "failed" | "unknown" => (RuntimeStatus::Stopped, None),
        "" => (RuntimeStatus::Unknown, None),
        other => (RuntimeStatus::Unknown, Some(other.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- command_for ---------------------------------------------------

    #[test]
    fn docker_detect_exists_on_every_platform() {
        for os in [HostOs::MacOs, HostOs::Windows, HostOs::Unix] {
            let command = command_for(RuntimeKind::Docker, RuntimeAction::Detect, os).unwrap();
            assert_eq!(command.program, "docker");
        }
    }

    #[test]
    fn docker_start_and_stop_differ_per_platform() {
        assert_eq!(
            command_for(RuntimeKind::Docker, RuntimeAction::Start, HostOs::MacOs)
                .unwrap()
                .program,
            "open"
        );
        assert_eq!(
            command_for(RuntimeKind::Docker, RuntimeAction::Start, HostOs::Unix)
                .unwrap()
                .program,
            "systemctl"
        );
        assert_eq!(
            command_for(RuntimeKind::Docker, RuntimeAction::Stop, HostOs::MacOs)
                .unwrap()
                .program,
            "osascript"
        );
    }

    #[test]
    fn podman_machine_is_unsupported_on_linux() {
        for action in [
            RuntimeAction::Detect,
            RuntimeAction::Start,
            RuntimeAction::Stop,
        ] {
            assert_eq!(
                command_for(RuntimeKind::PodmanMachine, action, HostOs::Unix),
                None
            );
        }
        assert!(
            command_for(
                RuntimeKind::PodmanMachine,
                RuntimeAction::Detect,
                HostOs::MacOs
            )
            .is_some()
        );
        assert!(
            command_for(
                RuntimeKind::PodmanMachine,
                RuntimeAction::Detect,
                HostOs::Windows
            )
            .is_some()
        );
    }

    #[test]
    fn kubernetes_never_offers_start_or_stop() {
        for os in [HostOs::MacOs, HostOs::Windows, HostOs::Unix] {
            assert!(command_for(RuntimeKind::Kubernetes, RuntimeAction::Detect, os).is_some());
            assert_eq!(
                command_for(RuntimeKind::Kubernetes, RuntimeAction::Start, os),
                None
            );
            assert_eq!(
                command_for(RuntimeKind::Kubernetes, RuntimeAction::Stop, os),
                None
            );
        }
    }

    #[test]
    fn containerd_is_linux_only() {
        for action in [
            RuntimeAction::Detect,
            RuntimeAction::Start,
            RuntimeAction::Stop,
        ] {
            assert!(command_for(RuntimeKind::Containerd, action, HostOs::Unix).is_some());
            assert_eq!(
                command_for(RuntimeKind::Containerd, action, HostOs::MacOs),
                None
            );
            assert_eq!(
                command_for(RuntimeKind::Containerd, action, HostOs::Windows),
                None
            );
        }
    }

    // ---- candidate_paths -------------------------------------------------

    #[test]
    fn candidate_paths_are_declared_for_known_tools() {
        assert!(!candidate_paths("docker", HostOs::MacOs).is_empty());
        assert!(!candidate_paths("systemctl", HostOs::Unix).is_empty());
    }

    #[test]
    fn candidate_paths_are_empty_for_unknown_combinations() {
        assert!(candidate_paths("nonexistent-tool", HostOs::MacOs).is_empty());
        assert!(candidate_paths("docker", HostOs::Windows).is_empty());
    }

    // ---- classify ----------------------------------------------------------

    fn output(success: bool, stdout: &str) -> CommandOutput {
        CommandOutput {
            success,
            stdout: stdout.to_string(),
            stderr: String::new(),
        }
    }

    #[test]
    fn docker_running_with_a_version_carries_it_as_detail() {
        let (status, detail) = classify_docker(&output(true, "24.0.7\n"));
        assert_eq!(status, RuntimeStatus::Running);
        assert_eq!(detail.as_deref(), Some("24.0.7"));
    }

    #[test]
    fn docker_failure_is_stopped_not_unknown() {
        let (status, detail) = classify_docker(&output(false, ""));
        assert_eq!(status, RuntimeStatus::Stopped);
        assert_eq!(detail, None);
    }

    #[test]
    fn podman_machine_running_names_the_running_machine() {
        let json = r#"[{"Name":"podman-machine-default","Running":true}]"#;
        let (status, detail) = classify_podman_machine(&output(true, json));
        assert_eq!(status, RuntimeStatus::Running);
        assert_eq!(detail.as_deref(), Some("podman-machine-default"));
    }

    #[test]
    fn podman_machine_all_stopped_is_stopped() {
        let json = r#"[{"Name":"podman-machine-default","Running":false}]"#;
        let (status, _) = classify_podman_machine(&output(true, json));
        assert_eq!(status, RuntimeStatus::Stopped);
    }

    #[test]
    fn podman_machine_empty_list_is_stopped() {
        let (status, _) = classify_podman_machine(&output(true, "[]"));
        assert_eq!(status, RuntimeStatus::Stopped);
    }

    #[test]
    fn podman_machine_malformed_json_is_unknown() {
        let (status, _) = classify_podman_machine(&output(true, "not json"));
        assert_eq!(status, RuntimeStatus::Unknown);
    }

    #[test]
    fn kubectl_running_carries_the_first_line_as_detail() {
        let stdout =
            "Kubernetes control plane is running at https://127.0.0.1:6443\n\nMore info...\n";
        let (status, detail) = classify_kubectl(&output(true, stdout));
        assert_eq!(status, RuntimeStatus::Running);
        assert_eq!(
            detail.as_deref(),
            Some("Kubernetes control plane is running at https://127.0.0.1:6443")
        );
    }

    #[test]
    fn kubectl_failure_is_stopped() {
        let (status, _) = classify_kubectl(&output(false, ""));
        assert_eq!(status, RuntimeStatus::Stopped);
    }

    #[test]
    fn systemctl_is_active_reads_stdout_over_exit_code() {
        // `is-active` exits non-zero for anything but "active", so `success`
        // must not be trusted here.
        assert_eq!(
            classify_systemctl_is_active(&output(true, "active\n")).0,
            RuntimeStatus::Running
        );
        assert_eq!(
            classify_systemctl_is_active(&output(false, "inactive\n")).0,
            RuntimeStatus::Stopped
        );
        assert_eq!(
            classify_systemctl_is_active(&output(false, "failed\n")).0,
            RuntimeStatus::Stopped
        );
        assert_eq!(
            classify_systemctl_is_active(&output(false, "")).0,
            RuntimeStatus::Unknown
        );
    }

    // ---- RuntimeInfo::new --------------------------------------------------

    #[test]
    fn running_offers_stop_but_not_start() {
        let row = RuntimeInfo::new(
            RuntimeKind::Docker,
            RuntimeStatus::Running,
            None,
            HostOs::MacOs,
        );
        assert!(!row.can_start);
        assert!(row.can_stop);
    }

    #[test]
    fn stopped_offers_start_but_not_stop() {
        let row = RuntimeInfo::new(
            RuntimeKind::Docker,
            RuntimeStatus::Stopped,
            None,
            HostOs::MacOs,
        );
        assert!(row.can_start);
        assert!(!row.can_stop);
    }

    #[test]
    fn kubernetes_never_offers_either_action_regardless_of_status() {
        for status in [RuntimeStatus::Running, RuntimeStatus::Stopped] {
            let row = RuntimeInfo::new(RuntimeKind::Kubernetes, status, None, HostOs::MacOs);
            assert!(!row.can_start);
            assert!(!row.can_stop);
        }
    }

    #[test]
    fn unsupported_offers_neither_action() {
        let row = RuntimeInfo::new(
            RuntimeKind::Containerd,
            RuntimeStatus::Unsupported,
            None,
            HostOs::MacOs,
        );
        assert!(!row.can_start);
        assert!(!row.can_stop);
    }
}
