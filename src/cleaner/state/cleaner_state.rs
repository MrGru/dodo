use std::collections::{BTreeMap, HashSet};

use crate::cleaner::core::category::{CleanerCategory, CleanerSection};
use crate::cleaner::core::item::CleanableItemId;
use crate::cleaner::core::progress::ScanProgress;
use crate::cleaner::core::report::{CategoryScanResult, CleanupReport, ScanCompleteness};
use crate::cleaner::core::selection::selected_by_default_ids;

/// Every state the panel can display. All nine already have a label in
/// `views::CleanerView`; three of them cannot be reached yet, and each carries
/// its own reason for that.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CleanerStatus {
    Idle,
    /// Entered once a scan gates on Full Disk Access — pending with
    /// `core::permissions`, which has no implementation in round 1.
    #[allow(dead_code)]
    CheckingPermissions,
    Scanning,
    Cancelling,
    PartiallyCompleted,
    Completed,
    /// Entered by the destructive cleanup path, which round 1 does not have.
    #[allow(dead_code)]
    Cleaning,
    CompletedWithFailures,
    /// Set by [`CleanerState::mark_failed`], for a scan that fails as a whole.
    /// Round 1 cannot fail as a whole: a scanner error is per-category and
    /// lands in `CompletedWithFailures` instead.
    #[allow(dead_code)]
    Failed,
}

pub struct CleanerState {
    section: CleanerSection,
    category: CleanerCategory,
    status: CleanerStatus,
    progress: Option<ScanProgress>,
    results: BTreeMap<CleanerCategory, CategoryScanResult>,
    selected_items: HashSet<CleanableItemId>,
    total_scanned_entries: u64,
    estimated_reclaimable_bytes: u64,
    cleanup_report: Option<CleanupReport>,
}

impl Default for CleanerState {
    fn default() -> Self {
        Self {
            section: CleanerSection::SmartCare,
            category: CleanerCategory::SystemJunk,
            status: CleanerStatus::Idle,
            progress: None,
            results: BTreeMap::new(),
            selected_items: HashSet::new(),
            total_scanned_entries: 0,
            estimated_reclaimable_bytes: 0,
            cleanup_report: None,
        }
    }
}

impl CleanerState {
    pub fn section(&self) -> CleanerSection {
        self.section
    }

    pub fn category(&self) -> CleanerCategory {
        self.category
    }

    pub fn status(&self) -> CleanerStatus {
        self.status
    }

    pub fn progress(&self) -> Option<&ScanProgress> {
        self.progress.as_ref()
    }

    pub fn set_section(&mut self, section: CleanerSection) {
        self.section = section;
        if !CleanerCategory::categories_for(section).any(|category| category == self.category) {
            self.category = CleanerCategory::categories_for(section)
                .next()
                .unwrap_or(CleanerCategory::SystemJunk);
        }
    }

    pub fn set_category(&mut self, category: CleanerCategory) {
        self.section = category.section();
        self.category = category;
    }

    pub fn begin_scan(&mut self) {
        self.status = CleanerStatus::Scanning;
        self.progress = None;
        self.results.clear();
        self.selected_items.clear();
        self.total_scanned_entries = 0;
        self.estimated_reclaimable_bytes = 0;
        self.cleanup_report = None;
    }

    pub fn begin_cancelling(&mut self) {
        if self.status == CleanerStatus::Scanning {
            self.status = CleanerStatus::Cancelling;
        }
    }

    pub fn update_progress(&mut self, progress: ScanProgress) {
        self.progress = Some(progress);
    }

    pub fn push_result(&mut self, result: CategoryScanResult) {
        self.total_scanned_entries += result.scanned_entries;
        self.estimated_reclaimable_bytes += result.estimated_reclaimable_bytes;
        self.selected_items
            .extend(selected_by_default_ids(&result.items));
        self.results.insert(result.category, result);
    }

    pub fn finish_scan(&mut self, cancelled: bool, had_failures: bool) {
        self.progress = None;
        self.status = if cancelled {
            CleanerStatus::PartiallyCompleted
        } else if had_failures {
            CleanerStatus::CompletedWithFailures
        } else if self
            .results
            .values()
            .any(|result| !matches!(result.completeness, ScanCompleteness::Complete))
        {
            CleanerStatus::PartiallyCompleted
        } else {
            CleanerStatus::Completed
        };
    }

    /// Abandon the run entirely. Uncalled in round 1 for the reason given on
    /// [`CleanerStatus::Failed`]: the only failures that exist are per-category
    /// and are reported through `finish_scan(_, had_failures)`.
    #[allow(dead_code)]
    pub fn mark_failed(&mut self) {
        self.progress = None;
        self.status = CleanerStatus::Failed;
    }

    pub fn result_for(&self, category: CleanerCategory) -> Option<&CategoryScanResult> {
        self.results.get(&category)
    }

    pub fn is_selected(&self, id: CleanableItemId) -> bool {
        self.selected_items.contains(&id)
    }

    pub fn toggle_selected(&mut self, id: CleanableItemId) {
        if !self.selected_items.remove(&id) {
            self.selected_items.insert(id);
        }
    }

    pub fn clear_selection_for(&mut self, category: CleanerCategory) {
        if let Some(result) = self.results.get(&category) {
            for item in &result.items {
                self.selected_items.remove(&item.id);
            }
        }
    }

    pub fn select_safe_items_for(&mut self, category: CleanerCategory) {
        if let Some(result) = self.results.get(&category) {
            for item in &result.items {
                if selected_by_default_ids(std::slice::from_ref(item)).contains(&item.id) {
                    self.selected_items.insert(item.id);
                }
            }
        }
    }

    pub fn selected_count_for(&self, category: CleanerCategory) -> usize {
        self.results
            .get(&category)
            .map(|result| {
                result
                    .items
                    .iter()
                    .filter(|item| self.selected_items.contains(&item.id))
                    .count()
            })
            .unwrap_or(0)
    }

    pub fn selected_ids_for(&self, category: CleanerCategory) -> Vec<CleanableItemId> {
        self.results
            .get(&category)
            .map(|result| {
                result
                    .items
                    .iter()
                    .filter(|item| self.selected_items.contains(&item.id))
                    .map(|item| item.id)
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn begin_cleaning(&mut self) {
        self.status = CleanerStatus::Cleaning;
        self.cleanup_report = None;
    }

    pub fn finish_cleaning(&mut self, report: CleanupReport) {
        for success in &report.successes {
            self.selected_items.remove(&success.id);
            for result in self.results.values_mut() {
                result.items.retain(|item| item.id != success.id);
            }
        }
        self.estimated_reclaimable_bytes = self
            .results
            .values()
            .map(|result| {
                result
                    .items
                    .iter()
                    .map(|item| item.logical_size)
                    .sum::<u64>()
            })
            .sum();
        self.cleanup_report = Some(report.clone());
        self.status = if report.failures.is_empty() {
            CleanerStatus::Completed
        } else {
            CleanerStatus::CompletedWithFailures
        };
    }

    /// Removes one item from a category's results and from the current
    /// selection, without a rescan and without going through the cleanup
    /// pipeline. Used by "Keep" (Phase 10): once the ignore list has been
    /// told to remember the path (`views::cleaner_view::CleanerView::mark_kept`),
    /// the item disappears from view immediately rather than waiting for the
    /// next scan to leave it out.
    pub fn remove_item(&mut self, category: CleanerCategory, id: CleanableItemId) {
        if let Some(result) = self.results.get_mut(&category) {
            result.items.retain(|item| item.id != id);
        }
        self.selected_items.remove(&id);
    }

    pub fn cleanup_report(&self) -> Option<&CleanupReport> {
        self.cleanup_report.as_ref()
    }

    pub fn total_scanned_entries(&self) -> u64 {
        self.total_scanned_entries
    }

    pub fn estimated_reclaimable_bytes(&self) -> u64 {
        self.estimated_reclaimable_bytes
    }
}

#[cfg(test)]
mod tests {
    use crate::cleaner::core::category::{CleanerCategory, CleanerSection};
    use crate::cleaner::core::progress::{ScanPhase, ScanProgress};
    use crate::cleaner::core::report::{CategoryScanResult, ScanCompleteness};
    use crate::cleaner::state::{CleanerState, CleanerStatus};

    #[test]
    fn section_switch_moves_to_the_first_category_in_that_section() {
        let mut state = CleanerState::default();
        state.set_category(CleanerCategory::SystemJunk);
        state.set_section(CleanerSection::Applications);

        assert_eq!(state.section(), CleanerSection::Applications);
        assert_eq!(state.category(), CleanerCategory::InstalledApps);
    }

    #[test]
    fn scan_lifecycle_transitions_and_accumulates_totals() {
        let mut state = CleanerState::default();
        state.begin_scan();
        assert_eq!(state.status(), CleanerStatus::Scanning);

        state.update_progress(ScanProgress {
            category: CleanerCategory::SystemJunk,
            phase: ScanPhase::Traversing,
            current_path: None,
            scanned_entries: 10,
            discovered_items: 2,
            discovered_bytes: 2048,
        });
        assert!(state.progress().is_some());

        state.push_result(CategoryScanResult {
            category: CleanerCategory::SystemJunk,
            items: Vec::new(),
            scanned_entries: 10,
            estimated_reclaimable_bytes: 2048,
            warnings: Vec::new(),
            completeness: ScanCompleteness::Complete,
        });
        state.finish_scan(false, false);

        assert_eq!(state.status(), CleanerStatus::Completed);
        assert!(state.progress().is_none());
        assert_eq!(state.total_scanned_entries(), 10);
        assert_eq!(state.estimated_reclaimable_bytes(), 2048);
    }

    #[test]
    fn cancellation_sets_partially_completed() {
        let mut state = CleanerState::default();
        state.begin_scan();
        state.begin_cancelling();
        assert_eq!(state.status(), CleanerStatus::Cancelling);

        state.finish_scan(true, false);
        assert_eq!(state.status(), CleanerStatus::PartiallyCompleted);
    }

    #[test]
    fn partial_results_set_partially_completed_without_runtime_failures() {
        let mut state = CleanerState::default();
        state.begin_scan();
        state.push_result(CategoryScanResult {
            category: CleanerCategory::UserCache,
            items: Vec::new(),
            scanned_entries: 0,
            estimated_reclaimable_bytes: 0,
            warnings: Vec::new(),
            completeness: ScanCompleteness::Partial {
                skipped_roots: vec!["/tmp/missing".into()],
                reason: crate::cleaner::core::report::PartialScanReason::RootUnavailable,
            },
        });

        state.finish_scan(false, false);
        assert_eq!(state.status(), CleanerStatus::PartiallyCompleted);
    }

    #[test]
    fn selected_by_default_items_are_tracked_and_can_be_cleared() {
        let mut state = CleanerState::default();
        state.push_result(CategoryScanResult {
            category: CleanerCategory::UserCache,
            items: vec![crate::cleaner::core::item::CleanableItem {
                id: crate::cleaner::core::item::CleanableItemId(1),
                category: CleanerCategory::UserCache,
                group: None,
                display_name: "Cache".into(),
                path: "/tmp/cache".into(),
                logical_size: 1,
                allocated_size: None,
                modified_at: None,
                last_accessed_at: None,
                risk: crate::cleaner::core::risk::RiskLevel::SafeRecreatable,
                selection_policy: crate::cleaner::core::risk::SelectionPolicy::SelectedByDefault,
                capabilities: Vec::new(),
                explanation: String::new(),
                warnings: Vec::new(),
                metadata: crate::cleaner::core::item::ItemMetadata::Generic,
            }],
            scanned_entries: 1,
            estimated_reclaimable_bytes: 1,
            warnings: Vec::new(),
            completeness: ScanCompleteness::Complete,
        });

        assert_eq!(state.selected_count_for(CleanerCategory::UserCache), 1);
        state.clear_selection_for(CleanerCategory::UserCache);
        assert_eq!(state.selected_count_for(CleanerCategory::UserCache), 0);
    }

    #[test]
    fn remove_item_hides_it_immediately_without_a_rescan() {
        use crate::cleaner::core::item::{CleanableItem, CleanableItemId, ItemMetadata};
        use crate::cleaner::core::risk::{RiskLevel, SelectionPolicy};

        let mut state = CleanerState::default();
        let item = CleanableItem {
            id: CleanableItemId(7),
            category: CleanerCategory::OrphanedFiles,
            group: None,
            display_name: "Orphan".into(),
            path: "/tmp/orphan".into(),
            logical_size: 10,
            allocated_size: None,
            modified_at: None,
            last_accessed_at: None,
            risk: RiskLevel::ReviewRecommended,
            selection_policy: SelectionPolicy::SelectedByDefault,
            capabilities: Vec::new(),
            explanation: String::new(),
            warnings: Vec::new(),
            metadata: ItemMetadata::Generic,
        };
        state.push_result(CategoryScanResult {
            category: CleanerCategory::OrphanedFiles,
            items: vec![item],
            scanned_entries: 1,
            estimated_reclaimable_bytes: 10,
            warnings: Vec::new(),
            completeness: ScanCompleteness::Complete,
        });
        assert_eq!(state.selected_count_for(CleanerCategory::OrphanedFiles), 1);

        state.remove_item(CleanerCategory::OrphanedFiles, CleanableItemId(7));

        assert_eq!(state.selected_count_for(CleanerCategory::OrphanedFiles), 0);
        assert!(
            state
                .result_for(CleanerCategory::OrphanedFiles)
                .expect("category still has a result")
                .items
                .is_empty(),
            "the kept item must be gone from the visible results"
        );
    }
}
