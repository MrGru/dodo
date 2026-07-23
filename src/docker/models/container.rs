//! One row of the Containers table: the engine's container reduced to exactly
//! what the seven columns and the search box need.
//!
//! The service builds these out of `bollard`'s wire types; nothing above the
//! service ever sees anything else. `cpu_percent` is filled in separately, after
//! the row first appears, which is why it is an `Option` the store can set on a
//! single row without rebuilding the table.

use std::collections::HashMap;

use crate::docker::models::port::PortMapping;
use crate::docker::models::status::ContainerStatus;

/// The compose-project label keys, in priority order. Docker Compose writes the
/// first; Podman's compose implementations write one of the others.
const COMPOSE_PROJECT_KEYS: [&str; 3] = [
    "com.docker.compose.project",
    "io.podman.compose.project",
    "com.docker.stack.namespace",
];

/// A container as the table renders it.
#[derive(Clone, PartialEq, Debug)]
pub struct Container {
    /// The full engine id. The stable key for row identity, selection and the
    /// per-row lifecycle calls.
    pub id: String,
    /// The display name, already stripped of the engine's leading `/`.
    pub name: String,
    pub image: String,
    pub status: ContainerStatus,
    pub ports: Vec<PortMapping>,
    /// The compose project this container belongs to, if any. Shown nowhere in
    /// round 1's columns but searchable now and the grouping key in round 2.
    pub compose_project: Option<String>,
    /// `State.StartedAt` as Unix seconds, or `None` if it never started.
    pub started_at: Option<i64>,
    /// Busy-percent, filled in after the row appears. `None` means "not measured
    /// yet" (or not running); the column shows a dash until it arrives.
    pub cpu_percent: Option<f64>,
}

impl Container {
    /// Whether this row matches a search query. Case-insensitive over the name,
    /// the image and the compose project — the three identifiers a user types to
    /// find a container. An empty (or whitespace-only) query matches everything.
    pub fn matches(&self, query: &str) -> bool {
        let query = query.trim().to_lowercase();
        if query.is_empty() {
            return true;
        }
        let haystacks = [
            Some(self.name.as_str()),
            Some(self.image.as_str()),
            self.compose_project.as_deref(),
        ];
        haystacks
            .into_iter()
            .flatten()
            .any(|field| field.to_lowercase().contains(&query))
    }
}

/// Strips the engine's leading `/` from a container name. The API returns names
/// as `/loving_hopper`; the slash is a wire artefact, not part of the name.
pub fn clean_name(raw: &str) -> String {
    raw.strip_prefix('/').unwrap_or(raw).to_string()
}

/// Extracts the compose project from a container's labels, honouring the Docker
/// and Podman label conventions in turn. `None` when the container is not part
/// of a compose project.
pub fn compose_project(labels: &HashMap<String, String>) -> Option<String> {
    COMPOSE_PROJECT_KEYS
        .iter()
        .find_map(|key| labels.get(*key))
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::{Container, clean_name, compose_project};
    use crate::docker::models::status::ContainerStatus;

    fn container(name: &str, image: &str, project: Option<&str>) -> Container {
        Container {
            id: "abc123".into(),
            name: name.into(),
            image: image.into(),
            status: ContainerStatus::Running,
            ports: Vec::new(),
            compose_project: project.map(Into::into),
            started_at: None,
            cpu_percent: None,
        }
    }

    #[test]
    fn names_lose_their_leading_slash() {
        assert_eq!(clean_name("/loving_hopper"), "loving_hopper");
        assert_eq!(clean_name("already-clean"), "already-clean");
    }

    #[test]
    fn compose_project_reads_the_docker_then_podman_labels() {
        let mut docker = HashMap::new();
        docker.insert(
            "com.docker.compose.project".to_string(),
            "ghs-be".to_string(),
        );
        assert_eq!(compose_project(&docker), Some("ghs-be".to_string()));

        let mut podman = HashMap::new();
        podman.insert("io.podman.compose.project".to_string(), "lab".to_string());
        assert_eq!(compose_project(&podman), Some("lab".to_string()));

        assert_eq!(compose_project(&HashMap::new()), None);
    }

    #[test]
    fn a_blank_project_label_is_treated_as_absent() {
        let mut labels = HashMap::new();
        labels.insert("com.docker.compose.project".to_string(), "  ".to_string());
        assert_eq!(compose_project(&labels), None);
    }

    #[test]
    fn search_is_case_insensitive_over_name_image_and_project() {
        let c = container("mailcrab-1", "marlonb/mailcrab:latest", Some("ghs-be"));
        assert!(c.matches("MAIL"));
        assert!(c.matches("marlonb"));
        assert!(c.matches("GHS"));
        assert!(!c.matches("nginx"));
    }

    #[test]
    fn an_empty_query_matches_every_row() {
        let c = container("anything", "image", None);
        assert!(c.matches(""));
        assert!(c.matches("   "));
    }
}
