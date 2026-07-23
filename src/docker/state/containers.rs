//! The Containers page store: the rows, where the load is, the search query and
//! the selection over them. Plain data, no GPUI — the view owns this plus the
//! search input and the engine.

use crate::docker::models::container::Container;
use crate::docker::state::selection::SelectionState;
use crate::i18n::Str;

/// Where a load of the container list currently is.
#[derive(Default)]
pub enum LoadStatus {
    /// A load is in flight and no rows have arrived yet — the skeleton state.
    #[default]
    Loading,
    /// A load completed. The rows may still be empty (the empty state); that is
    /// distinct from still loading.
    Ready,
    /// The engine could not be reached. Held as a [`Str`] so the banner
    /// re-translates when the language changes, the same as the API Explorer's
    /// failure outcome.
    Failed(Str),
}

#[derive(Default)]
pub struct ContainersState {
    status: LoadStatus,
    /// All rows from the last successful load, already sorted for display.
    rows: Vec<Container>,
    query: String,
    pub selection: SelectionState,
    /// A transient error from a per-row action (start/stop/…), shown as a banner
    /// above the table without discarding the rows. Cleared on the next action
    /// or refresh.
    action_error: Option<Str>,
}

impl ContainersState {
    pub fn status(&self) -> &LoadStatus {
        &self.status
    }

    pub fn action_error(&self) -> Option<&Str> {
        self.action_error.as_ref()
    }

    /// Marks a load as started. Keeps the existing rows on screen so a refresh
    /// does not blank the table — the toolbar shows the activity instead.
    pub fn begin_load(&mut self) {
        self.status = LoadStatus::Loading;
        self.action_error = None;
    }

    /// Installs the rows from a successful load: sorted, with the selection
    /// pruned to what still exists.
    pub fn set_rows(&mut self, mut rows: Vec<Container>) {
        sort_for_display(&mut rows);
        self.selection
            .retain(rows.iter().map(|row| row.id.as_str()));
        self.rows = rows;
        self.status = LoadStatus::Ready;
    }

    pub fn set_error(&mut self, message: Str) {
        self.status = LoadStatus::Failed(message);
    }

    pub fn set_action_error(&mut self, message: Str) {
        self.action_error = Some(message);
    }

    pub fn set_query(&mut self, query: String) {
        self.query = query;
    }

    /// Updates one row's CPU percent in place, without disturbing the rest — the
    /// seam live per-row polling plugs into in round 2. A no-op if the row is
    /// gone (a refresh raced the stats fetch).
    pub fn set_cpu(&mut self, id: &str, percent: Option<f64>) {
        if let Some(row) = self.rows.iter_mut().find(|row| row.id == id) {
            row.cpu_percent = percent;
        }
    }

    /// The rows matching the current search, in display order.
    pub fn visible(&self) -> Vec<&Container> {
        self.rows
            .iter()
            .filter(|row| row.matches(&self.query))
            .collect()
    }

    /// The ids of every running container, for the CPU-stats sweep after a load.
    pub fn running_ids(&self) -> Vec<String> {
        self.rows
            .iter()
            .filter(|row| row.status.is_running())
            .map(|row| row.id.clone())
            .collect()
    }

    /// The ids currently visible, for the header "select all" checkbox.
    pub fn visible_ids(&self) -> Vec<String> {
        self.visible().iter().map(|row| row.id.clone()).collect()
    }

    /// True when a completed load produced no rows at all (as opposed to rows
    /// that a search has filtered away).
    pub fn is_empty(&self) -> bool {
        matches!(self.status, LoadStatus::Ready) && self.rows.is_empty()
    }

    /// Whether any rows are loaded, regardless of the current search. Distinguishes
    /// a first load (show the skeleton) from a background refresh (keep the table).
    pub fn has_rows(&self) -> bool {
        !self.rows.is_empty()
    }
}

/// Running containers first, then by name case-insensitively — the order Docker
/// Desktop shows and the one a person scans for "what is up right now".
fn sort_for_display(rows: &mut [Container]) {
    rows.sort_by(|a, b| {
        b.status
            .is_running()
            .cmp(&a.status.is_running())
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
}

#[cfg(test)]
mod tests {
    use super::{ContainersState, sort_for_display};
    use crate::docker::models::container::Container;
    use crate::docker::models::status::ContainerStatus;

    fn container(id: &str, name: &str, status: ContainerStatus) -> Container {
        Container {
            id: id.into(),
            name: name.into(),
            image: "img".into(),
            status,
            ports: Vec::new(),
            compose_project: None,
            started_at: None,
            cpu_percent: None,
        }
    }

    #[test]
    fn display_order_is_running_first_then_name() {
        let mut rows = vec![
            container("1", "zulu", ContainerStatus::Exited),
            container("2", "alpha", ContainerStatus::Exited),
            container("3", "yankee", ContainerStatus::Running),
        ];
        sort_for_display(&mut rows);
        let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["yankee", "alpha", "zulu"]);
    }

    #[test]
    fn set_cpu_updates_one_row_only() {
        let mut state = ContainersState::default();
        state.set_rows(vec![
            container("1", "a", ContainerStatus::Running),
            container("2", "b", ContainerStatus::Running),
        ]);
        state.set_cpu("2", Some(12.5));
        let rows = state.visible();
        assert_eq!(rows.iter().find(|r| r.id == "1").unwrap().cpu_percent, None);
        assert_eq!(
            rows.iter().find(|r| r.id == "2").unwrap().cpu_percent,
            Some(12.5)
        );
    }

    #[test]
    fn search_filters_visible_rows() {
        let mut state = ContainersState::default();
        state.set_rows(vec![
            container("1", "mailcrab", ContainerStatus::Running),
            container("2", "postgres", ContainerStatus::Exited),
        ]);
        state.set_query("post".into());
        let visible = state.visible();
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].name, "postgres");
    }

    #[test]
    fn empty_is_only_after_a_ready_load_with_no_rows() {
        let mut state = ContainersState::default();
        // Still loading → not "empty".
        assert!(!state.is_empty());
        state.set_rows(vec![]);
        assert!(state.is_empty());
        state.set_rows(vec![container("1", "a", ContainerStatus::Running)]);
        assert!(!state.is_empty());
    }
}
