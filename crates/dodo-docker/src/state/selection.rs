//! The row-selection set behind the table's checkboxes.
//!
//! A plain set of container ids with the operations the checkboxes and the
//! (round-2) bulk toolbar need. Selection is keyed by id, not row index, so it
//! survives a refresh that reorders or drops rows — [`SelectionState::retain`]
//! prunes ids that no longer exist.

use std::collections::HashSet;

#[derive(Default)]
pub struct SelectionState {
    selected: HashSet<String>,
}

impl SelectionState {
    pub fn is_selected(&self, id: &str) -> bool {
        self.selected.contains(id)
    }

    /// The size of the selection. Consumed by round 2's bulk toolbar (which
    /// shows "N selected"); part of the model that ships now.
    #[allow(dead_code)]
    pub fn count(&self) -> usize {
        self.selected.len()
    }

    /// Whether nothing is selected — round 2's bulk toolbar enables its actions
    /// on the inverse. Part of the model that ships now.
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.selected.is_empty()
    }

    /// Adds or removes one id.
    pub fn toggle(&mut self, id: &str, selected: bool) {
        if selected {
            self.selected.insert(id.to_string());
        } else {
            self.selected.remove(id);
        }
    }

    /// Selects exactly `ids` (used by the header checkbox's "select all"), or
    /// clears everything when they are all already selected — the header
    /// checkbox toggles.
    pub fn set_all(&mut self, ids: impl IntoIterator<Item = String>) {
        self.selected = ids.into_iter().collect();
    }

    pub fn clear(&mut self) {
        self.selected.clear();
    }

    /// Whether every id in `ids` is selected (and there is at least one), which
    /// is what the header checkbox shows as fully checked.
    pub fn all_selected<'a>(&self, mut ids: impl Iterator<Item = &'a str>) -> bool {
        let mut any = false;
        let all = ids.all(|id| {
            any = true;
            self.selected.contains(id)
        });
        any && all
    }

    /// Drops any selected id not present in `existing`, so a refresh that removes
    /// a container also unselects it.
    pub fn retain<'a>(&mut self, existing: impl Iterator<Item = &'a str>) {
        let keep: HashSet<&str> = existing.collect();
        self.selected.retain(|id| keep.contains(id.as_str()));
    }
}

#[cfg(test)]
mod tests {
    use super::SelectionState;

    #[test]
    fn toggling_tracks_membership_and_count() {
        let mut selection = SelectionState::default();
        assert!(selection.is_empty());
        selection.toggle("a", true);
        selection.toggle("b", true);
        assert!(selection.is_selected("a"));
        assert_eq!(selection.count(), 2);
        selection.toggle("a", false);
        assert!(!selection.is_selected("a"));
        assert_eq!(selection.count(), 1);
    }

    #[test]
    fn all_selected_needs_every_id_and_at_least_one() {
        let mut selection = SelectionState::default();
        assert!(!selection.all_selected(["a", "b"].into_iter()));
        selection.set_all(["a".to_string(), "b".to_string()]);
        assert!(selection.all_selected(["a", "b"].into_iter()));
        // An empty row set is not "all selected".
        assert!(!selection.all_selected(std::iter::empty()));
    }

    #[test]
    fn retain_prunes_ids_that_no_longer_exist() {
        let mut selection = SelectionState::default();
        selection.set_all(["a".to_string(), "b".to_string(), "c".to_string()]);
        selection.retain(["a", "c"].into_iter());
        assert!(selection.is_selected("a"));
        assert!(!selection.is_selected("b"));
        assert!(selection.is_selected("c"));
        assert_eq!(selection.count(), 2);
    }
}
