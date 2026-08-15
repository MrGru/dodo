use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};

use crate::core::category::CleanerCategory;

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ScanProgress {
    pub category: CleanerCategory,
    pub phase: ScanPhase,
    pub current_path: Option<PathBuf>,
    pub scanned_entries: u64,
    pub discovered_items: u64,
    pub discovered_bytes: u64,
}

/// The phases a scan reports through. The mock scanners only ever report
/// `Preparing`, `Traversing` and `Completed`; the other four belong to the real
/// macOS scanners (permission checks, root discovery, aggregation,
/// classification). `#[allow(dead_code)]` comes off with them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[allow(dead_code)]
pub enum ScanPhase {
    Preparing,
    CheckingPermissions,
    DiscoveringRoots,
    Traversing,
    Aggregating,
    Classifying,
    Completed,
}

pub trait ProgressSink: Send + Sync {
    fn report(&self, progress: ScanProgress);
}

/// The bounded, latest-wins hand-off between one category's scanning
/// background thread and the UI's progress pump.
///
/// Progress is a *display* value: only the newest one is ever drawn, and
/// [`crate::state::CleanerState::update_progress`] does nothing but
/// overwrite the previous one. An unbounded queue therefore bought nothing
/// but work — every intermediate update the pump drained had already been
/// superseded by the time it was applied — while a UI thread busy elsewhere
/// let that queue grow without any limit behind a scanner reporting faster
/// than the pump's 120 ms tick.
///
/// So this slot holds **at most one** update per category: a producer that
/// reports while one is still pending overwrites it, and the pump takes
/// exactly one per category per tick, which is what bounds catch-up after a
/// stall to one apply per category rather than one per report.
///
/// Dropping an intermediate update is safe *only* because nothing a scan
/// finally decides travels this way: the result, the cancellation and the
/// error all come back as the background task's own return value in
/// `CleanerView::run_scan` and reach
/// [`crate::state::CleanerState::finish_scan`] directly. Never move
/// a terminal signal into this slot — coalescing would then be able to lose
/// it.
pub struct LatestProgress {
    slot: Mutex<Option<ScanProgress>>,
    coalesced: AtomicU64,
}

impl LatestProgress {
    pub fn new() -> Self {
        Self {
            slot: Mutex::new(None),
            coalesced: AtomicU64::new(0),
        }
    }

    /// Takes the pending update, if any, leaving the slot empty. The pump
    /// calls this once per category per tick.
    pub fn take(&self) -> Option<ScanProgress> {
        self.guard().take()
    }

    /// How many updates are waiting — 0 or 1, never more. That bound is the
    /// whole point of this type, so it is asserted rather than assumed in
    /// this module's tests.
    pub fn pending(&self) -> usize {
        usize::from(self.guard().is_some())
    }

    /// How many updates a later report overwrote before the pump could take
    /// them. Diagnostics only: a coalesced update is intentionally lost, so
    /// this exists to prove the flood was real in tests, never to reconstruct
    /// anything.
    pub fn coalesced(&self) -> u64 {
        self.coalesced.load(Ordering::Relaxed)
    }

    /// A poisoned lock here means a scanner panicked mid-report. Progress is
    /// a display value with no invariant to protect, so recovering the guard
    /// keeps the pump running instead of taking the UI down with it.
    fn guard(&self) -> MutexGuard<'_, Option<ScanProgress>> {
        self.slot
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl Default for LatestProgress {
    fn default() -> Self {
        Self::new()
    }
}

impl ProgressSink for LatestProgress {
    fn report(&self, progress: ScanProgress) {
        if self.guard().replace(progress).is_some() {
            self.coalesced.fetch_add(1, Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::{LatestProgress, ProgressSink, ScanPhase, ScanProgress};
    use crate::core::cancellation::CancellationToken;
    use crate::core::category::CleanerCategory;
    use crate::core::errors::ScanError;
    use crate::core::permissions::MacPermission;
    use crate::core::report::{CategoryScanResult, ScanCompleteness};
    use crate::core::scan_context::ScanContext;
    use crate::core::scanner::CleanerScanner;

    const CATEGORY: CleanerCategory = CleanerCategory::UserCache;

    fn progress(scanned_entries: u64) -> ScanProgress {
        ScanProgress {
            category: CATEGORY,
            phase: ScanPhase::Traversing,
            current_path: None,
            scanned_entries,
            discovered_items: 0,
            discovered_bytes: 0,
        }
    }

    /// A scanner that reports far faster than any pump could take, so the
    /// only thing standing between it and an unbounded backlog is the slot.
    struct FloodingScanner {
        reports: u64,
        cancel_after: Option<u64>,
        cancellation: Option<CancellationToken>,
        reported: AtomicU64,
    }

    impl FloodingScanner {
        fn new(reports: u64) -> Self {
            Self {
                reports,
                cancel_after: None,
                cancellation: None,
                reported: AtomicU64::new(0),
            }
        }

        fn cancelling_after(reports: u64, at: u64, cancellation: CancellationToken) -> Self {
            Self {
                reports,
                cancel_after: Some(at),
                cancellation: Some(cancellation),
                reported: AtomicU64::new(0),
            }
        }
    }

    impl CleanerScanner for FloodingScanner {
        fn category(&self) -> CleanerCategory {
            CATEGORY
        }

        fn required_permissions(&self) -> &[MacPermission] {
            const NONE: &[MacPermission] = &[];
            NONE
        }

        fn scan(
            &self,
            _context: &ScanContext,
            progress_sink: &dyn ProgressSink,
            cancellation: &CancellationToken,
        ) -> Result<CategoryScanResult, ScanError> {
            for step in 1..=self.reports {
                if cancellation.is_cancelled() {
                    return Err(ScanError::Cancelled);
                }
                progress_sink.report(progress(step));
                self.reported.fetch_add(1, Ordering::Relaxed);
                if self.cancel_after == Some(step)
                    && let Some(token) = &self.cancellation
                {
                    token.cancel();
                }
            }
            Ok(CategoryScanResult {
                category: CATEGORY,
                items: Vec::new(),
                scanned_entries: self.reports,
                estimated_reclaimable_bytes: 0,
                warnings: Vec::new(),
                completeness: ScanCompleteness::Complete,
            })
        }
    }

    #[test]
    fn a_flood_of_reports_leaves_exactly_one_pending_update() {
        let slot = LatestProgress::new();
        for step in 1..=10_000u64 {
            slot.report(progress(step));
            assert_eq!(slot.pending(), 1, "the slot is capacity one, always");
        }
        assert_eq!(slot.coalesced(), 9_999);
        assert_eq!(slot.take(), Some(progress(10_000)));
        assert_eq!(slot.pending(), 0);
        assert_eq!(slot.take(), None);
    }

    /// The point of the whole design: a scan may lose intermediate progress,
    /// but never its result. The result does not travel through the slot at
    /// all — it is the scan call's return value.
    #[test]
    fn the_final_result_arrives_whole_despite_a_flood() {
        let slot = Arc::new(LatestProgress::new());
        let scanner = FloodingScanner::new(5_000);
        let result = scanner
            .scan(
                &ScanContext::new(),
                slot.as_ref(),
                &CancellationToken::new(),
            )
            .expect("an uncancelled scan returns its result");

        assert_eq!(result.category, CATEGORY);
        assert_eq!(result.scanned_entries, 5_000);
        assert_eq!(result.completeness, ScanCompleteness::Complete);
        assert_eq!(slot.pending(), 1);
        assert_eq!(slot.take(), Some(progress(5_000)));
    }

    /// Cancellation is a terminal signal too, and takes the same independent
    /// path: the coalescing slot cannot swallow it.
    #[test]
    fn cancellation_is_reported_even_though_progress_was_coalesced() {
        let cancellation = CancellationToken::new();
        let slot = Arc::new(LatestProgress::new());
        let scanner = FloodingScanner::cancelling_after(5_000, 100, cancellation.clone());
        let outcome = scanner.scan(&ScanContext::new(), slot.as_ref(), &cancellation);

        assert!(matches!(outcome, Err(ScanError::Cancelled)));
        assert_eq!(slot.pending(), 1);
        assert_eq!(
            slot.take(),
            Some(progress(100)),
            "the last progress before cancelling is still the newest one"
        );
    }
}
