//! The Containers page store: the rows, where the load is, the search query, the
//! filters, the group expansion state and the selection over them. Plain data,
//! no GPUI — the view owns this plus the search input and the engine.

use std::collections::HashSet;

use crate::docker::models::container::Container;
use crate::docker::state::filters::ContainerFilters;
use crate::docker::state::grouping::{ContainerGroup, GroupKey, group_containers};
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
    /// The active filters. Composes with `query` by `&&` in [`Self::visible`].
    filters: ContainerFilters,
    /// The compose groups the user has collapsed. Default-expanded, so absence
    /// means expanded. Kept keyed by [`GroupKey`] and never pruned on refresh, so
    /// expansion survives reloads and searches — a project that momentarily has
    /// no matching rows keeps its state for when it returns.
    collapsed: HashSet<GroupKey>,
    pub selection: SelectionState,
    /// A transient error from a per-row or bulk action, shown as a banner above
    /// the table without discarding the rows. Cleared on the next action or
    /// refresh.
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
    /// seam live per-row polling plugs into. Returns whether the value actually
    /// changed, so a poll can skip a re-render when the reading is identical. A
    /// no-op (returning `false`) if the row is gone (a refresh raced the stats
    /// fetch).
    pub fn set_cpu(&mut self, id: &str, percent: Option<f64>) -> bool {
        if let Some(row) = self.rows.iter_mut().find(|row| row.id == id) {
            if row.cpu_percent != percent {
                row.cpu_percent = percent;
                return true;
            }
        }
        false
    }

    /// Merges a freshly-listed set over the current rows without a wholesale
    /// replace — the incremental path background polling takes each tick. Carries
    /// each surviving row's last CPU reading forward (a fresh list reports none),
    /// re-sorts, and only when the visible data actually differs prunes the
    /// selection and swaps the rows in. Returns whether anything changed — the
    /// rows differed, or a prior error/loading status has cleared — so the view
    /// re-renders (and re-sweeps CPU) only when it is worth it. Preserves the
    /// query, filters, group-expansion and selection across the merge.
    pub fn merge_rows(&mut self, mut incoming: Vec<Container>) -> bool {
        crate::docker::state::diff::carry_cpu(&self.rows, &mut incoming);
        sort_for_display(&mut incoming);
        let rows_differ = crate::docker::state::diff::rows_changed(&self.rows, &incoming);
        let was_not_ready = !matches!(self.status, LoadStatus::Ready);
        if rows_differ {
            self.selection
                .retain(incoming.iter().map(|row| row.id.as_str()));
            self.rows = incoming;
        }
        self.status = LoadStatus::Ready;
        rows_differ || was_not_ready
    }

    /// Degrades to the error state from a background poll that could not reach the
    /// engine, keeping the rows in memory so they return on the next good poll.
    /// Returns whether this is a *transition* into the error state, so repeated
    /// failed polls re-render at most once rather than spamming a re-render per
    /// tick.
    pub fn set_poll_error(&mut self, message: Str) -> bool {
        let was_failed = matches!(self.status, LoadStatus::Failed(_));
        self.status = LoadStatus::Failed(message);
        !was_failed
    }

    /// The rows matching the current search **and** every active filter, in
    /// display order. Search and filters narrow together.
    pub fn visible(&self) -> Vec<&Container> {
        self.rows
            .iter()
            .filter(|row| row.matches(&self.query) && self.filters.matches(row))
            .collect()
    }

    /// The visible rows partitioned into compose groups (Ungrouped last). Empty
    /// groups do not appear because filtering happens before grouping.
    pub fn visible_groups(&self) -> Vec<ContainerGroup> {
        group_containers(self.visible().into_iter().cloned().collect())
    }

    /// The filters, for the popover to read their checked state.
    pub fn filters(&self) -> &ContainerFilters {
        &self.filters
    }

    /// The filters, for the popover to toggle. The caller notifies afterwards.
    pub fn filters_mut(&mut self) -> &mut ContainerFilters {
        &mut self.filters
    }

    /// Whether a group is collapsed (default expanded).
    pub fn is_collapsed(&self, key: &GroupKey) -> bool {
        self.collapsed.contains(key)
    }

    /// Flips one group between collapsed and expanded.
    pub fn toggle_group(&mut self, key: GroupKey) {
        if !self.collapsed.remove(&key) {
            self.collapsed.insert(key);
        }
    }

    /// The distinct compose-project names across *all* rows (not just visible),
    /// sorted case-insensitively — the options the project filter offers.
    pub fn available_projects(&self) -> Vec<String> {
        let mut projects: Vec<String> = self
            .rows
            .iter()
            .filter_map(|row| row.compose_project.clone())
            .collect();
        projects.sort_by_key(|name| name.to_lowercase());
        projects.dedup();
        projects
    }

    /// The distinct image references across all rows, sorted — the options the
    /// image filter offers.
    pub fn available_images(&self) -> Vec<String> {
        let mut images: Vec<String> = self.rows.iter().map(|row| row.image.clone()).collect();
        images.sort();
        images.dedup();
        images
    }

    /// One loaded row by id, for the context menu's Delete to name it in the
    /// confirmation. `None` if a refresh removed it between the right-click and
    /// the menu action.
    pub fn row(&self, id: &str) -> Option<&Container> {
        self.rows.iter().find(|row| row.id == id)
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

    /// The loaded rows that are currently selected. Selection is over the whole
    /// list, not just the visible slice, so a row selected before a filter was
    /// applied is still acted on by a bulk action.
    fn selected_rows(&self) -> impl Iterator<Item = &Container> {
        self.rows
            .iter()
            .filter(|row| self.selection.is_selected(&row.id))
    }

    /// Every selected id, for bulk delete (valid on anything).
    pub fn selected_ids(&self) -> Vec<String> {
        self.selected_rows().map(|row| row.id.clone()).collect()
    }

    /// The selected ids a bulk **Start** applies to — the ones a start is valid
    /// for. The rest are ignored, so a mixed selection starts what it can.
    pub fn bulk_startable_ids(&self) -> Vec<String> {
        self.selected_rows()
            .filter(|row| row.status.can_start())
            .map(|row| row.id.clone())
            .collect()
    }

    /// The selected ids a bulk **Stop** applies to.
    pub fn bulk_stoppable_ids(&self) -> Vec<String> {
        self.selected_rows()
            .filter(|row| row.status.can_stop())
            .map(|row| row.id.clone())
            .collect()
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
    use crate::i18n::Str;

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

    /// A container with a compose project and image, for the grouping/filter tests.
    fn full(
        id: &str,
        name: &str,
        status: ContainerStatus,
        image: &str,
        project: Option<&str>,
    ) -> Container {
        Container {
            id: id.into(),
            name: name.into(),
            image: image.into(),
            status,
            ports: Vec::new(),
            compose_project: project.map(Into::into),
            started_at: None,
            cpu_percent: None,
        }
    }

    fn seeded() -> ContainersState {
        let mut state = ContainersState::default();
        state.set_rows(vec![
            full("1", "web", ContainerStatus::Running, "nginx", Some("app")),
            full("2", "db", ContainerStatus::Exited, "postgres", Some("app")),
            full(
                "3",
                "cache",
                ContainerStatus::Running,
                "redis",
                Some("infra"),
            ),
            full("4", "lonely", ContainerStatus::Exited, "alpine", None),
        ]);
        state
    }

    #[test]
    fn filters_and_search_narrow_together() {
        use crate::docker::state::grouping::GroupKey;
        let mut state = seeded();
        // Filter to Running only: db and lonely drop out.
        state
            .filters_mut()
            .toggle_status(ContainerStatus::Running, true);
        let visible: Vec<&str> = state.visible().iter().map(|r| r.name.as_str()).collect();
        assert_eq!(visible, ["cache", "web"]); // running-first sort, then name

        // Add a search: only "web" survives Running + "web".
        state.set_query("web".into());
        let visible: Vec<&str> = state.visible().iter().map(|r| r.name.as_str()).collect();
        assert_eq!(visible, ["web"]);

        // The groups reflect the same narrowing — only the "app" group remains,
        // and the now-empty "infra" group is gone.
        let groups = state.visible_groups();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].key, GroupKey::Project("app".into()));
    }

    #[test]
    fn select_all_visible_ignores_filtered_rows() {
        let mut state = seeded();
        state.set_query("web".into());
        // Only "web" is visible; select-all selects exactly it.
        state.selection.set_all(state.visible_ids());
        assert!(state.selection.is_selected("1"));
        assert!(!state.selection.is_selected("2"));
        assert_eq!(state.selection.count(), 1);
    }

    #[test]
    fn bulk_id_partitions_follow_action_validity() {
        let mut state = seeded();
        // Select the running web (1) and the exited db (2).
        state.selection.set_all(["1".to_string(), "2".to_string()]);
        // Start applies only to the exited one.
        assert_eq!(state.bulk_startable_ids(), vec!["2".to_string()]);
        // Stop applies only to the running one.
        assert_eq!(state.bulk_stoppable_ids(), vec!["1".to_string()]);
        // Delete applies to both.
        let mut all = state.selected_ids();
        all.sort();
        assert_eq!(all, vec!["1".to_string(), "2".to_string()]);
    }

    #[test]
    fn group_expansion_toggles_and_survives_refresh() {
        use crate::docker::state::grouping::GroupKey;
        let mut state = seeded();
        let key = GroupKey::Project("app".into());
        assert!(!state.is_collapsed(&key));
        state.toggle_group(key.clone());
        assert!(state.is_collapsed(&key));
        // A refresh (set_rows) must not reset expansion.
        state.set_rows(vec![full(
            "1",
            "web",
            ContainerStatus::Running,
            "nginx",
            Some("app"),
        )]);
        assert!(state.is_collapsed(&key));
        state.toggle_group(key.clone());
        assert!(!state.is_collapsed(&key));
    }

    #[test]
    fn merge_rows_carries_cpu_and_reports_change() {
        let mut state = ContainersState::default();
        state.set_rows(vec![
            full("1", "web", ContainerStatus::Running, "nginx", Some("app")),
            full("2", "db", ContainerStatus::Exited, "postgres", Some("app")),
        ]);
        state.set_cpu("1", Some(42.0));

        // Re-listing the identical set (CPU always None off the wire) is not a
        // change, and the carried-forward CPU survives.
        let same = vec![
            full("1", "web", ContainerStatus::Running, "nginx", Some("app")),
            full("2", "db", ContainerStatus::Exited, "postgres", Some("app")),
        ];
        assert!(!state.merge_rows(same));
        assert_eq!(
            state
                .visible()
                .iter()
                .find(|r| r.id == "1")
                .unwrap()
                .cpu_percent,
            Some(42.0)
        );

        // A status flip is a change.
        let changed = vec![
            full("1", "web", ContainerStatus::Exited, "nginx", Some("app")),
            full("2", "db", ContainerStatus::Exited, "postgres", Some("app")),
        ];
        assert!(state.merge_rows(changed));
    }

    #[test]
    fn merge_rows_preserves_selection_of_surviving_rows() {
        let mut state = seeded();
        state.selection.set_all(["1".to_string(), "2".to_string()]);
        // Re-list without row "2": its selection is pruned, "1" survives.
        let incoming = vec![
            full("1", "web", ContainerStatus::Running, "nginx", Some("app")),
            full(
                "3",
                "cache",
                ContainerStatus::Running,
                "redis",
                Some("infra"),
            ),
            full("4", "lonely", ContainerStatus::Exited, "alpine", None),
        ];
        assert!(state.merge_rows(incoming));
        assert!(state.selection.is_selected("1"));
        assert!(!state.selection.is_selected("2"));
    }

    #[test]
    fn merge_after_error_re_renders_even_when_rows_match() {
        let mut state = ContainersState::default();
        let rows = vec![full("1", "web", ContainerStatus::Running, "nginx", None)];
        state.set_rows(rows.clone());
        // A poll fails: transition into error is a change; a second failure is not.
        assert!(state.set_poll_error(Str::DockerConnectionError("down".into())));
        assert!(!state.set_poll_error(Str::DockerConnectionError("still down".into())));
        // Recovery re-lists the same rows — still a change, because the error is
        // clearing and the table must come back.
        assert!(state.merge_rows(rows));
    }

    #[test]
    fn set_cpu_reports_whether_it_changed() {
        let mut state = ContainersState::default();
        state.set_rows(vec![full(
            "1",
            "web",
            ContainerStatus::Running,
            "nginx",
            None,
        )]);
        assert!(state.set_cpu("1", Some(10.0)));
        assert!(!state.set_cpu("1", Some(10.0)));
        assert!(state.set_cpu("1", Some(11.0)));
        // A missing row is a no-op.
        assert!(!state.set_cpu("nope", Some(1.0)));
    }

    #[test]
    fn available_options_are_distinct_and_sorted() {
        let state = seeded();
        assert_eq!(state.available_projects(), vec!["app", "infra"]);
        assert_eq!(
            state.available_images(),
            vec!["alpine", "nginx", "postgres", "redis"]
        );
    }
}
