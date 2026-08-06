use std::collections::{BTreeMap, HashSet};

use crate::cleaner::core::category::{CleanerCategory, CleanerSection};
use crate::cleaner::core::item::CleanableItemId;
use crate::cleaner::core::progress::ScanProgress;
use crate::cleaner::core::report::{CategoryScanResult, CleanupReport, ScanCompleteness};
use crate::cleaner::core::selection::selected_by_default_ids;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CleanerStatus {
    Idle,
    CheckingPermissions,
    Scanning,
    Cancelling,
    PartiallyCompleted,
    Completed,
    Cleaning,
    CompletedWithFailures,
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
}
