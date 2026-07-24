//! The Containers filter set: the multiple, simultaneous filters the Filter
//! popover drives, and the one predicate that decides whether a row survives all
//! of them.
//!
//! Plain data, no GPUI — the view toggles these and asks [`ContainerFilters::matches`]
//! per row, exactly as the search box asks [`Container::matches`]. The two compose
//! by `&&` in the store, so search and filters narrow together.
//!
//! # How the filters combine
//!
//! **AND across filter *types*, OR within a multi-select type.** A row must clear
//! every active type; within Status (or Project, or Image) it clears the type by
//! matching any one of the selected values. An empty set for a type means "that
//! type is not filtering" and passes everything — so no filters at all passes
//! every row.
//!
//! Favorites is deliberately absent: there is no favorites backend yet, so the
//! popover renders a disabled, clearly-labelled placeholder rather than a toggle
//! that would do nothing here. It becomes a field the day favorites ship.

use std::collections::HashSet;

use crate::docker::models::container::Container;
use crate::docker::models::status::ContainerStatus;

/// The statuses the Status filter offers, in the order the popover lists them.
/// A deliberately curated subset of [`ContainerStatus`] — the lifecycle states a
/// person actually filters by — not every engine state.
pub const FILTERABLE_STATUSES: [ContainerStatus; 5] = [
    ContainerStatus::Running,
    ContainerStatus::Exited,
    ContainerStatus::Created,
    ContainerStatus::Restarting,
    ContainerStatus::Paused,
];

/// Every active filter over the container list. All-empty is the default and
/// matches everything.
#[derive(Default, Clone)]
pub struct ContainerFilters {
    /// Selected statuses. Empty means "any status".
    statuses: HashSet<ContainerStatus>,
    /// Selected compose-project names. Empty means "any project". A row with no
    /// compose project never matches a non-empty project filter — asking for
    /// "project X" hides standalone containers, which is the expected reading.
    projects: HashSet<String>,
    /// Selected image references. Empty means "any image".
    images: HashSet<String>,
    /// When set, only containers that publish a port to the host pass.
    published_ports_only: bool,
}

impl ContainerFilters {
    /// Whether `container` clears every active filter. AND across types, OR
    /// within each multi-select type; see the module doc.
    pub fn matches(&self, container: &Container) -> bool {
        if !self.statuses.is_empty() && !self.statuses.contains(&container.status) {
            return false;
        }
        if !self.projects.is_empty() {
            match container.compose_project.as_deref() {
                Some(project) if self.projects.contains(project) => {}
                _ => return false,
            }
        }
        if !self.images.is_empty() && !self.images.contains(&container.image) {
            return false;
        }
        if self.published_ports_only && !container.has_published_ports() {
            return false;
        }
        true
    }

    /// Whether any filter is narrowing the list. Drives the toolbar's active
    /// indication and whether "Clear filters" does anything.
    pub fn is_active(&self) -> bool {
        !self.statuses.is_empty()
            || !self.projects.is_empty()
            || !self.images.is_empty()
            || self.published_ports_only
    }

    /// How many filter *types* are active — the number shown on the Filter
    /// button, so "3" means Status, Project and Image (say) are each constraining,
    /// regardless of how many values are ticked within them.
    pub fn active_count(&self) -> usize {
        usize::from(!self.statuses.is_empty())
            + usize::from(!self.projects.is_empty())
            + usize::from(!self.images.is_empty())
            + usize::from(self.published_ports_only)
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }

    // ---- Status -------------------------------------------------------------

    pub fn is_status_selected(&self, status: ContainerStatus) -> bool {
        self.statuses.contains(&status)
    }

    pub fn toggle_status(&mut self, status: ContainerStatus, selected: bool) {
        toggle(&mut self.statuses, status, selected);
    }

    // ---- Compose project ----------------------------------------------------

    pub fn is_project_selected(&self, project: &str) -> bool {
        self.projects.contains(project)
    }

    pub fn toggle_project(&mut self, project: String, selected: bool) {
        toggle(&mut self.projects, project, selected);
    }

    // ---- Image --------------------------------------------------------------

    pub fn is_image_selected(&self, image: &str) -> bool {
        self.images.contains(image)
    }

    pub fn toggle_image(&mut self, image: String, selected: bool) {
        toggle(&mut self.images, image, selected);
    }

    // ---- Has published ports ------------------------------------------------

    pub fn published_ports_only(&self) -> bool {
        self.published_ports_only
    }

    pub fn set_published_ports_only(&mut self, only: bool) {
        self.published_ports_only = only;
    }
}

/// Inserts or removes `value`, the shared body of every `toggle_*`.
fn toggle<T: std::hash::Hash + Eq>(set: &mut HashSet<T>, value: T, selected: bool) {
    if selected {
        set.insert(value);
    } else {
        set.remove(&value);
    }
}

#[cfg(test)]
mod tests {
    use super::ContainerFilters;
    use crate::docker::models::container::Container;
    use crate::docker::models::port::PortMapping;
    use crate::docker::models::status::ContainerStatus;

    fn container(image: &str, status: ContainerStatus, project: Option<&str>) -> Container {
        Container {
            id: "id".into(),
            name: "name".into(),
            image: image.into(),
            status,
            ports: Vec::new(),
            compose_project: project.map(Into::into),
            started_at: None,
            cpu_percent: None,
        }
    }

    #[test]
    fn no_filters_matches_everything() {
        let filters = ContainerFilters::default();
        assert!(!filters.is_active());
        assert_eq!(filters.active_count(), 0);
        assert!(filters.matches(&container("nginx", ContainerStatus::Running, None)));
        assert!(filters.matches(&container("redis", ContainerStatus::Exited, Some("app"))));
    }

    #[test]
    fn a_status_filter_is_or_within_the_type() {
        let mut filters = ContainerFilters::default();
        filters.toggle_status(ContainerStatus::Running, true);
        filters.toggle_status(ContainerStatus::Paused, true);
        assert!(filters.is_active());
        assert_eq!(filters.active_count(), 1);
        assert!(filters.matches(&container("a", ContainerStatus::Running, None)));
        assert!(filters.matches(&container("a", ContainerStatus::Paused, None)));
        assert!(!filters.matches(&container("a", ContainerStatus::Exited, None)));
    }

    #[test]
    fn filter_types_combine_with_and() {
        let mut filters = ContainerFilters::default();
        filters.toggle_status(ContainerStatus::Running, true);
        filters.toggle_project("app".into(), true);
        assert_eq!(filters.active_count(), 2);
        // Running AND in project "app".
        assert!(filters.matches(&container("x", ContainerStatus::Running, Some("app"))));
        // Right project, wrong status.
        assert!(!filters.matches(&container("x", ContainerStatus::Exited, Some("app"))));
        // Right status, wrong project.
        assert!(!filters.matches(&container("x", ContainerStatus::Running, Some("other"))));
        // Right status, but standalone — a project filter excludes the ungrouped.
        assert!(!filters.matches(&container("x", ContainerStatus::Running, None)));
    }

    #[test]
    fn image_filter_matches_exact_reference() {
        let mut filters = ContainerFilters::default();
        filters.toggle_image("nginx:latest".into(), true);
        assert!(filters.matches(&container("nginx:latest", ContainerStatus::Running, None)));
        assert!(!filters.matches(&container("nginx:1.25", ContainerStatus::Running, None)));
    }

    #[test]
    fn published_ports_filter_needs_a_host_port() {
        let mut filters = ContainerFilters::default();
        filters.set_published_ports_only(true);
        assert!(filters.is_active());

        let mut published = container("web", ContainerStatus::Running, None);
        published.ports = vec![PortMapping {
            host: Some(8080),
            container: 80,
            protocol: "tcp".into(),
        }];
        assert!(filters.matches(&published));
        assert!(!filters.matches(&container("web", ContainerStatus::Running, None)));
    }

    #[test]
    fn toggling_off_and_clear_reset_the_type() {
        let mut filters = ContainerFilters::default();
        filters.toggle_status(ContainerStatus::Running, true);
        filters.toggle_status(ContainerStatus::Running, false);
        assert!(!filters.is_active());

        filters.toggle_image("nginx".into(), true);
        filters.set_published_ports_only(true);
        filters.clear();
        assert!(!filters.is_active());
        assert_eq!(filters.active_count(), 0);
    }
}
