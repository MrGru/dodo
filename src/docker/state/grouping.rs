//! Grouping the container list by Docker Compose project.
//!
//! Plain data, no GPUI — [`group_containers`] is a pure partition the store calls
//! on its already-filtered, already-sorted rows, and the view renders the result.
//! Keeping it here lets the partition rule and the project-status summary be unit
//! tested without a window.
//!
//! # The rules, stated once
//!
//! - Every container carries an optional `compose_project` (round 1 extracts it).
//!   Rows sharing a project form a group; rows with none collect under
//!   [`GroupKey::Ungrouped`].
//! - Groups are ordered by project name, case-insensitively, with **Ungrouped
//!   always last** so the compose projects — the thing a user came to find — sit
//!   at the top.
//! - Row order *within* a group is preserved from the input, so the store's
//!   "running first, then name" ordering carries through untouched.
//! - The partition is over whatever rows it is handed. Because the store filters
//!   and searches first, a project whose every row was filtered away simply does
//!   not appear — empty groups never render.

use crate::docker::models::container::Container;

/// Identifies a group for expansion state and rendering. `Ungrouped` is a single
/// bucket, distinct from any real project name.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum GroupKey {
    Project(String),
    Ungrouped,
}

/// The rolled-up run state of a group, driving the header's status summary and
/// its colour.
///
/// The rule is deliberately simple and defensible: **all running**, **none
/// running**, or **partially running** in between. "Running" here means exactly
/// [`ContainerStatus::is_running`] — a paused or restarting container is not
/// counted as up, matching what the per-row badge shows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
// The shared `Running` postfix is the subject of the enum, not noise: dropping it
// as clippy suggests yields `GroupStatus::None`, which reads as "no status" and
// shadows `Option::None` at a glance. Quantifier + subject is the clearer name.
#[allow(clippy::enum_variant_names)]
pub enum GroupStatus {
    AllRunning,
    PartiallyRunning,
    NoneRunning,
}

/// One compose project's (or the ungrouped bucket's) containers, with the counts
/// the header needs. Holds owned rows because the view renders them directly.
pub struct ContainerGroup {
    pub key: GroupKey,
    pub containers: Vec<Container>,
}

impl ContainerGroup {
    pub fn total(&self) -> usize {
        self.containers.len()
    }

    pub fn running_count(&self) -> usize {
        self.containers
            .iter()
            .filter(|c| c.status.is_running())
            .count()
    }

    /// The rolled-up status; see [`GroupStatus`]. An empty group (which the store
    /// never produces) reads as `NoneRunning`.
    pub fn status(&self) -> GroupStatus {
        let running = self.running_count();
        if running == 0 {
            GroupStatus::NoneRunning
        } else if running == self.total() {
            GroupStatus::AllRunning
        } else {
            GroupStatus::PartiallyRunning
        }
    }
}

/// Partitions `rows` into compose-project groups plus the ungrouped bucket,
/// ordered per the module rules. Input order is preserved within each group.
pub fn group_containers(rows: Vec<Container>) -> Vec<ContainerGroup> {
    // A Vec of (key, rows) rather than a HashMap so first-seen order is kept for
    // the tie-break-free sort below and rows stay in input order.
    let mut groups: Vec<ContainerGroup> = Vec::new();
    let mut ungrouped: Vec<Container> = Vec::new();

    for row in rows {
        match row.compose_project.clone() {
            Some(project) => {
                if let Some(group) = groups
                    .iter_mut()
                    .find(|g| g.key == GroupKey::Project(project.clone()))
                {
                    group.containers.push(row);
                } else {
                    groups.push(ContainerGroup {
                        key: GroupKey::Project(project),
                        containers: vec![row],
                    });
                }
            }
            None => ungrouped.push(row),
        }
    }

    // Projects alphabetically (case-insensitive), Ungrouped appended last.
    groups.sort_by_key(|a| project_name(&a.key));
    if !ungrouped.is_empty() {
        groups.push(ContainerGroup {
            key: GroupKey::Ungrouped,
            containers: ungrouped,
        });
    }
    groups
}

/// The lowercase project name behind a key, for ordering. `Ungrouped` sorts after
/// every project, but it is appended explicitly so this only ever sees projects.
fn project_name(key: &GroupKey) -> String {
    match key {
        GroupKey::Project(name) => name.to_lowercase(),
        GroupKey::Ungrouped => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::{GroupKey, GroupStatus, group_containers};
    use crate::docker::models::container::Container;
    use crate::docker::models::status::ContainerStatus;

    fn container(name: &str, status: ContainerStatus, project: Option<&str>) -> Container {
        Container {
            id: name.into(),
            name: name.into(),
            image: "img".into(),
            status,
            ports: Vec::new(),
            compose_project: project.map(Into::into),
            started_at: None,
            cpu_percent: None,
        }
    }

    #[test]
    fn projects_sort_alphabetically_with_ungrouped_last() {
        let groups = group_containers(vec![
            container("a", ContainerStatus::Running, Some("zeta")),
            container("b", ContainerStatus::Running, None),
            container("c", ContainerStatus::Running, Some("Alpha")),
        ]);
        let keys: Vec<&GroupKey> = groups.iter().map(|g| &g.key).collect();
        assert_eq!(
            keys,
            [
                &GroupKey::Project("Alpha".into()),
                &GroupKey::Project("zeta".into()),
                &GroupKey::Ungrouped,
            ]
        );
    }

    #[test]
    fn rows_of_a_project_collect_and_keep_input_order() {
        let groups = group_containers(vec![
            container("web", ContainerStatus::Running, Some("app")),
            container("db", ContainerStatus::Exited, Some("app")),
        ]);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].total(), 2);
        let names: Vec<&str> = groups[0]
            .containers
            .iter()
            .map(|c| c.name.as_str())
            .collect();
        assert_eq!(names, ["web", "db"]);
    }

    #[test]
    fn ungrouped_only_appears_when_a_standalone_exists() {
        let grouped = group_containers(vec![container(
            "web",
            ContainerStatus::Running,
            Some("app"),
        )]);
        assert!(grouped.iter().all(|g| g.key != GroupKey::Ungrouped));
    }

    #[test]
    fn status_summary_reflects_the_group() {
        let all = group_containers(vec![
            container("a", ContainerStatus::Running, Some("p")),
            container("b", ContainerStatus::Running, Some("p")),
        ]);
        assert_eq!(all[0].status(), GroupStatus::AllRunning);
        assert_eq!(all[0].running_count(), 2);

        let some = group_containers(vec![
            container("a", ContainerStatus::Running, Some("p")),
            container("b", ContainerStatus::Exited, Some("p")),
        ]);
        assert_eq!(some[0].status(), GroupStatus::PartiallyRunning);
        assert_eq!(some[0].running_count(), 1);

        let none = group_containers(vec![
            container("a", ContainerStatus::Exited, Some("p")),
            // A paused container is not "running" for the summary.
            container("b", ContainerStatus::Paused, Some("p")),
        ]);
        assert_eq!(none[0].status(), GroupStatus::NoneRunning);
    }
}
