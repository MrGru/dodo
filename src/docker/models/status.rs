//! A container's lifecycle state: its badge colour, its label, and which of the
//! per-row actions it makes valid.
//!
//! The mapping from the engine's state string lives in [`ContainerStatus::from_engine_state`]
//! so the service layer has one tested place to translate `bollard`'s enum
//! through (via its `Display`, which is exactly these lowercase tokens).

use gpui::{App, Hsla};
use gpui_component::ActiveTheme as _;

use crate::i18n::Str;

/// The Docker container lifecycle states, as the Engine API reports them.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ContainerStatus {
    Created,
    Running,
    Paused,
    Restarting,
    Exited,
    Removing,
    Dead,
    Stopping,
    /// Anything the engine reports that is not one of the above (including the
    /// empty state string). Shown plainly rather than guessed at.
    #[default]
    Unknown,
}

impl ContainerStatus {
    /// Maps the engine's lowercase state token (`"running"`, `"exited"`, …) onto
    /// a status. Unrecognised input — including an empty string — is
    /// [`ContainerStatus::Unknown`] rather than an error: a new engine state
    /// should degrade to a plain badge, not break the page.
    pub fn from_engine_state(state: &str) -> Self {
        match state.trim().to_ascii_lowercase().as_str() {
            "created" => ContainerStatus::Created,
            "running" => ContainerStatus::Running,
            "paused" => ContainerStatus::Paused,
            "restarting" => ContainerStatus::Restarting,
            "exited" => ContainerStatus::Exited,
            "removing" => ContainerStatus::Removing,
            "dead" => ContainerStatus::Dead,
            "stopping" => ContainerStatus::Stopping,
            _ => ContainerStatus::Unknown,
        }
    }

    /// The badge caption, localized.
    pub fn label(self) -> Str {
        match self {
            ContainerStatus::Created => Str::DockerStatusCreated,
            ContainerStatus::Running => Str::DockerStatusRunning,
            ContainerStatus::Paused => Str::DockerStatusPaused,
            ContainerStatus::Restarting => Str::DockerStatusRestarting,
            ContainerStatus::Exited => Str::DockerStatusExited,
            ContainerStatus::Removing => Str::DockerStatusRemoving,
            ContainerStatus::Dead => Str::DockerStatusDead,
            ContainerStatus::Stopping => Str::DockerStatusStopping,
            ContainerStatus::Unknown => Str::DockerStatusUnknown,
        }
    }

    /// The badge colour, as a semantic theme field so every theme re-skins it:
    /// green for Running, gray for Exited/Created, yellow for Restarting/Paused
    /// (and the transient Removing/Stopping), red for Dead.
    pub fn color(self, cx: &App) -> Hsla {
        match self {
            ContainerStatus::Running => cx.theme().success,
            ContainerStatus::Exited | ContainerStatus::Created | ContainerStatus::Unknown => {
                cx.theme().muted_foreground
            }
            ContainerStatus::Restarting
            | ContainerStatus::Paused
            | ContainerStatus::Removing
            | ContainerStatus::Stopping => cx.theme().warning,
            ContainerStatus::Dead => cx.theme().danger,
        }
    }

    /// Whether the container is doing work, which is what the CPU column and the
    /// live-stats fetch key off.
    pub fn is_running(self) -> bool {
        matches!(self, ContainerStatus::Running)
    }

    /// Start is offered only when the container is not already up. A restarting
    /// or removing container is mid-transition and offers nothing.
    pub fn can_start(self) -> bool {
        matches!(
            self,
            ContainerStatus::Created | ContainerStatus::Exited | ContainerStatus::Dead
        )
    }

    /// Stop is offered for something that is up or paused.
    pub fn can_stop(self) -> bool {
        matches!(self, ContainerStatus::Running | ContainerStatus::Paused)
    }

    /// Restart is offered for anything currently running or paused.
    pub fn can_restart(self) -> bool {
        matches!(self, ContainerStatus::Running | ContainerStatus::Paused)
    }
}

#[cfg(test)]
mod tests {
    use super::ContainerStatus;

    #[test]
    fn engine_states_map_case_insensitively() {
        assert_eq!(
            ContainerStatus::from_engine_state("running"),
            ContainerStatus::Running
        );
        assert_eq!(
            ContainerStatus::from_engine_state("RUNNING"),
            ContainerStatus::Running
        );
        assert_eq!(
            ContainerStatus::from_engine_state(" exited "),
            ContainerStatus::Exited
        );
        assert_eq!(
            ContainerStatus::from_engine_state("created"),
            ContainerStatus::Created
        );
        assert_eq!(
            ContainerStatus::from_engine_state("dead"),
            ContainerStatus::Dead
        );
    }

    #[test]
    fn unknown_states_do_not_panic() {
        assert_eq!(
            ContainerStatus::from_engine_state(""),
            ContainerStatus::Unknown
        );
        assert_eq!(
            ContainerStatus::from_engine_state("frobnicating"),
            ContainerStatus::Unknown
        );
    }

    #[test]
    fn actions_are_gated_by_state() {
        // A stopped container can start but not stop or restart.
        assert!(ContainerStatus::Exited.can_start());
        assert!(!ContainerStatus::Exited.can_stop());
        assert!(!ContainerStatus::Exited.can_restart());

        // A running one can stop and restart but not start.
        assert!(!ContainerStatus::Running.can_start());
        assert!(ContainerStatus::Running.can_stop());
        assert!(ContainerStatus::Running.can_restart());

        // Mid-transition states offer nothing actionable.
        assert!(!ContainerStatus::Restarting.can_start());
        assert!(!ContainerStatus::Restarting.can_stop());
        assert!(!ContainerStatus::Removing.can_restart());
    }

    #[test]
    fn only_running_counts_as_running() {
        assert!(ContainerStatus::Running.is_running());
        assert!(!ContainerStatus::Paused.is_running());
        assert!(!ContainerStatus::Exited.is_running());
    }
}
