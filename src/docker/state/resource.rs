//! The store the Images, Volumes and Networks pages share.
//!
//! Round 1's [`ContainersState`](crate::docker::state::containers::ContainersState)
//! carries container-specific machinery — the CPU seam, compose grouping, the
//! filter set, the selection — that the three list pages do not need. What they
//! *do* share is exactly the container store's spine: a load status, the last
//! rows, an instant search query, the derived "containers using" usage and a
//! transient action-error banner. That spine is generic over the row type here,
//! so each page is a `ResourceState<Image>` / `<Volume>` / `<Network>` plus its
//! own columns. Plain data, no GPUI — the [`LoadStatus`] is reused from the
//! container store so the pages render the same skeleton/empty/error shapes.

use crate::docker::models::image::Image;
use crate::docker::models::network::Network;
use crate::docker::models::usage::ContainerUsage;
use crate::docker::models::volume::Volume;
use crate::docker::state::containers::LoadStatus;
use crate::i18n::Str;

/// A row that the shared search box can filter. Implemented by each list page's
/// model so [`ResourceState::visible`] is one generic method.
pub trait Searchable {
    /// Case-insensitive match against the page's search query; an empty query
    /// matches every row.
    fn matches(&self, query: &str) -> bool;
}

impl Searchable for Image {
    fn matches(&self, query: &str) -> bool {
        Image::matches(self, query)
    }
}

impl Searchable for Volume {
    fn matches(&self, query: &str) -> bool {
        Volume::matches(self, query)
    }
}

impl Searchable for Network {
    fn matches(&self, query: &str) -> bool {
        Network::matches(self, query)
    }
}

/// A list page's store: its rows, where their load is, the search query, the
/// derived container usage and a transient action error.
pub struct ResourceState<T> {
    status: LoadStatus,
    /// The rows from the last successful load, already in display order (the
    /// service sorts them).
    rows: Vec<T>,
    query: String,
    /// The container references the "containers using" column counts against,
    /// loaded alongside the rows. Empty until the first load completes.
    usage: ContainerUsage,
    /// A transient error from a delete, shown as a banner above the table
    /// without discarding the rows. Cleared on the next refresh.
    action_error: Option<Str>,
}

impl<T> Default for ResourceState<T> {
    fn default() -> Self {
        Self {
            status: LoadStatus::default(),
            rows: Vec::new(),
            query: String::new(),
            usage: ContainerUsage::default(),
            action_error: None,
        }
    }
}

impl<T: Searchable + Clone> ResourceState<T> {
    pub fn status(&self) -> &LoadStatus {
        &self.status
    }

    pub fn action_error(&self) -> Option<&Str> {
        self.action_error.as_ref()
    }

    pub fn usage(&self) -> &ContainerUsage {
        &self.usage
    }

    /// Marks a load as started, keeping the existing rows on screen so a refresh
    /// does not blank the table.
    pub fn begin_load(&mut self) {
        self.status = LoadStatus::Loading;
        self.action_error = None;
    }

    /// Installs the rows from a successful load (already display-sorted).
    pub fn set_rows(&mut self, rows: Vec<T>) {
        self.rows = rows;
        self.status = LoadStatus::Ready;
    }

    /// Installs the container usage the "containers using" column counts against.
    pub fn set_usage(&mut self, usage: ContainerUsage) {
        self.usage = usage;
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

    /// The rows matching the current search, in display order.
    pub fn visible(&self) -> Vec<&T> {
        self.rows
            .iter()
            .filter(|row| row.matches(&self.query))
            .collect()
    }

    /// True when a completed load produced no rows at all (as opposed to rows a
    /// search has filtered away) — the empty state.
    pub fn is_empty(&self) -> bool {
        matches!(self.status, LoadStatus::Ready) && self.rows.is_empty()
    }

    /// Whether any rows are loaded, regardless of the search. Distinguishes a
    /// first load (skeleton) from a background refresh (keep the table).
    pub fn has_rows(&self) -> bool {
        !self.rows.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::{ResourceState, Searchable};
    use crate::docker::models::volume::Volume;

    fn volume(name: &str) -> Volume {
        Volume {
            name: name.into(),
            driver: "local".into(),
            mountpoint: "/mnt".into(),
            size: None,
        }
    }

    #[test]
    fn search_filters_visible_rows() {
        let mut state: ResourceState<Volume> = ResourceState::default();
        state.set_rows(vec![volume("pgdata"), volume("cache")]);
        state.set_query("pg".into());
        let visible = state.visible();
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].name, "pgdata");
    }

    #[test]
    fn empty_is_only_after_a_ready_load_with_no_rows() {
        let mut state: ResourceState<Volume> = ResourceState::default();
        // Still loading → not "empty".
        assert!(!state.is_empty());
        state.set_rows(vec![]);
        assert!(state.is_empty());
        state.set_rows(vec![volume("data")]);
        assert!(!state.is_empty());
        assert!(state.has_rows());
    }

    #[test]
    fn searchable_delegates_to_the_row() {
        let volume = volume("pgdata");
        assert!(Searchable::matches(&volume, "PG"));
        assert!(!Searchable::matches(&volume, "redis"));
    }
}
