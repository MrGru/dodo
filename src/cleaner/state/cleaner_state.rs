use std::collections::BTreeMap;

use crate::cleaner::core::category::{CleanerCategory, CleanerSection};
use crate::cleaner::core::progress::ScanProgress;
use crate::cleaner::core::report::CategoryScanResult;

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
    total_scanned_entries: u64,
    estimated_reclaimable_bytes: u64,
}

impl Default for CleanerState {
    fn default() -> Self {
        Self {
            section: CleanerSection::SmartCare,
            category: CleanerCategory::SystemJunk,
            status: CleanerStatus::Idle,
            progress: None,
            results: BTreeMap::new(),
            total_scanned_entries: 0,
            estimated_reclaimable_bytes: 0,
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
        self.total_scanned_entries = 0;
        self.estimated_reclaimable_bytes = 0;
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
        self.results.insert(result.category, result);
    }

    pub fn finish_scan(&mut self, cancelled: bool, had_failures: bool) {
        self.progress = None;
        self.status = if cancelled {
            CleanerStatus::PartiallyCompleted
        } else if had_failures {
            CleanerStatus::CompletedWithFailures
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
}
