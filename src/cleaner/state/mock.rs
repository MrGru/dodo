use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use crate::cleaner::core::cancellation::CancellationToken;
use crate::cleaner::core::category::CleanerCategory;
use crate::cleaner::core::errors::ScanError;
use crate::cleaner::core::item::{CleanableItem, CleanableItemId, ItemMetadata};
use crate::cleaner::core::permissions::MacPermission;
use crate::cleaner::core::progress::{ProgressSink, ScanPhase, ScanProgress};
use crate::cleaner::core::report::{CategoryScanResult, ScanCompleteness};
use crate::cleaner::core::risk::{ItemCapability, RiskLevel, SelectionPolicy};
use crate::cleaner::core::scan_context::ScanContext;
use crate::cleaner::core::scanner::CleanerScanner;

pub struct MockScanner {
    category: CleanerCategory,
}

impl MockScanner {
    pub fn new(category: CleanerCategory) -> Self {
        Self { category }
    }
}

impl CleanerScanner for MockScanner {
    fn category(&self) -> CleanerCategory {
        self.category
    }

    fn required_permissions(&self) -> &[MacPermission] {
        const NONE: &[MacPermission] = &[];
        NONE
    }

    fn scan(
        &self,
        _context: &ScanContext,
        progress: &dyn ProgressSink,
        cancellation: &CancellationToken,
    ) -> Result<CategoryScanResult, ScanError> {
        progress.report(ScanProgress {
            category: self.category,
            phase: ScanPhase::Preparing,
            current_path: None,
            scanned_entries: 0,
            discovered_items: 0,
            discovered_bytes: 0,
        });
        thread::sleep(Duration::from_millis(60));

        for step in 1..=5u64 {
            if cancellation.is_cancelled() {
                return Err(ScanError::Cancelled);
            }
            progress.report(ScanProgress {
                category: self.category,
                phase: ScanPhase::Traversing,
                current_path: Some(PathBuf::from(format!(
                    "~/Library/Mock/{:?}/{step}",
                    self.category
                ))),
                scanned_entries: step * 100,
                discovered_items: step,
                discovered_bytes: step * 2 * 1024 * 1024,
            });
            thread::sleep(Duration::from_millis(80));
        }

        let base = self.category as u64 + 1;
        let items = vec![CleanableItem {
            id: CleanableItemId(base),
            category: self.category,
            group: Some("Mock".to_string()),
            display_name: format!("{:?} sample", self.category),
            path: PathBuf::from(format!("/tmp/dodo-cleaner-mock/{base}")),
            logical_size: base * 1024 * 1024,
            allocated_size: None,
            modified_at: None,
            last_accessed_at: None,
            risk: RiskLevel::SafeRecreatable,
            selection_policy: SelectionPolicy::SelectedByDefault,
            capabilities: vec![ItemCapability::RevealInFinder, ItemCapability::CopyPath],
            explanation: "Mock result for phase-1 UI workflow.".to_string(),
            warnings: Vec::new(),
            metadata: ItemMetadata::Generic,
        }];

        progress.report(ScanProgress {
            category: self.category,
            phase: ScanPhase::Completed,
            current_path: None,
            scanned_entries: 500,
            discovered_items: items.len() as u64,
            discovered_bytes: items.iter().map(|item| item.logical_size).sum(),
        });

        Ok(CategoryScanResult {
            category: self.category,
            items,
            scanned_entries: 500,
            estimated_reclaimable_bytes: base * 1024 * 1024,
            warnings: Vec::new(),
            completeness: ScanCompleteness::Complete,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use crate::cleaner::core::cancellation::CancellationToken;
    use crate::cleaner::core::category::CleanerCategory;
    use crate::cleaner::core::errors::ScanError;
    use crate::cleaner::core::progress::{ProgressSink, ScanProgress};
    use crate::cleaner::core::scan_context::ScanContext;
    use crate::cleaner::core::scanner::CleanerScanner;
    use crate::cleaner::state::MockScanner;

    struct RecordingSink(Arc<Mutex<Vec<ScanProgress>>>);

    impl ProgressSink for RecordingSink {
        fn report(&self, progress: ScanProgress) {
            self.0
                .lock()
                .expect("recording sink lock poisoned")
                .push(progress);
        }
    }

    #[test]
    fn mock_scanner_reports_incremental_progress() {
        let scanner = MockScanner::new(CleanerCategory::SystemJunk);
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink = RecordingSink(events.clone());

        let result = scanner.scan(&ScanContext::new(), &sink, &CancellationToken::new());
        assert!(result.is_ok());

        let count = events.lock().expect("recording sink lock poisoned").len();
        assert!(count >= 3, "expected multiple progress events, got {count}");
    }

    #[test]
    fn cancellation_stops_scan() {
        let scanner = MockScanner::new(CleanerCategory::SystemJunk);
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink = RecordingSink(events);
        let cancellation = CancellationToken::new();
        cancellation.cancel();

        let result = scanner.scan(&ScanContext::new(), &sink, &cancellation);
        assert!(matches!(result, Err(ScanError::Cancelled)));
    }
}
