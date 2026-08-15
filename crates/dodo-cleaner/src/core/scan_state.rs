//! What one category's scan is doing, as an explicit enum rather than
//! something inferred from a warning string or an `Option` combination —
//! see [`ScanState::from_outcome`].
//!
//! [`ScanState::indicator`] is the second half: the single mapping from that
//! state to what the category's own row and pane *show* about it. It lives
//! here, pure, rather than inside a `match` in a `render` body, because the
//! defect it fixes was invisible in exactly that position — the sidebar's
//! glyph builder claimed in its doc comment to draw "a `Spinner` while
//! scanning/cancelling" and returned an empty `div()` for both, so a
//! category being scanned looked identical to one that had never been
//! scanned. A `render` cannot be unit tested; this can.

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ScanState {
    #[default]
    NotScanned,
    Scanning,
    Cancelling,
    Completed,
    CompletedWithWarnings,
    PartiallyCompleted,
    Cancelled,
    Failed,
}

/// What a category's row and pane draw about its scan. One variant per
/// *visual*, not per [`ScanState`] — several states share a glyph, and that
/// collapsing is the thing worth pinning in a test.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScanIndicator {
    /// Nothing to say: never scanned, or scanned and then cancelled outright
    /// so there is no outcome to report.
    Idle,
    /// A scan (or its cancellation) is in flight — the spinner.
    InProgress,
    Success,
    Warning,
    Error,
}

impl ScanState {
    pub fn is_active(self) -> bool {
        matches!(self, ScanState::Scanning | ScanState::Cancelling)
    }

    /// The one place a scan state becomes something visible. Every surface
    /// that shows scan progress reads this rather than matching on
    /// [`ScanState`] itself, so a new state cannot be added without deciding
    /// what it looks like, and the sidebar tab and the pane cannot drift into
    /// disagreeing about whether a scan is running.
    pub fn indicator(self) -> ScanIndicator {
        match self {
            ScanState::Scanning | ScanState::Cancelling => ScanIndicator::InProgress,
            ScanState::Completed => ScanIndicator::Success,
            ScanState::CompletedWithWarnings | ScanState::PartiallyCompleted => {
                ScanIndicator::Warning
            }
            ScanState::Failed => ScanIndicator::Error,
            ScanState::NotScanned | ScanState::Cancelled => ScanIndicator::Idle,
        }
    }

    /// The single place a finished scan's outcome becomes one [`ScanState`].
    /// `cancelled` wins over everything else — a scan the user stopped is
    /// `Cancelled` even if every category-level scanner call it managed to
    /// finish came back clean. `had_error` is a scanner-level failure (the
    /// call to [`super::scanner::CleanerScanner::scan`] itself returned
    /// `Err`, not [`super::errors::ScanError::Cancelled`]) — distinct from
    /// [`super::report::ScanCompleteness::Partial`], which is the scanner
    /// succeeding while reporting that *part* of its own roots were
    /// unreachable.
    pub fn from_outcome(
        cancelled: bool,
        had_error: bool,
        partial: bool,
        has_warnings: bool,
    ) -> Self {
        if cancelled {
            ScanState::Cancelled
        } else if had_error {
            ScanState::Failed
        } else if partial {
            ScanState::PartiallyCompleted
        } else if has_warnings {
            ScanState::CompletedWithWarnings
        } else {
            ScanState::Completed
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ScanIndicator, ScanState};

    #[test]
    fn cancelled_wins_over_every_other_signal() {
        assert_eq!(
            ScanState::from_outcome(true, true, true, true),
            ScanState::Cancelled
        );
    }

    #[test]
    fn a_scanner_error_is_failed_even_without_partial_completeness() {
        assert_eq!(
            ScanState::from_outcome(false, true, false, false),
            ScanState::Failed
        );
    }

    #[test]
    fn partial_completeness_wins_over_warnings() {
        assert_eq!(
            ScanState::from_outcome(false, false, true, true),
            ScanState::PartiallyCompleted
        );
    }

    #[test]
    fn warnings_alone_is_completed_with_warnings() {
        assert_eq!(
            ScanState::from_outcome(false, false, false, true),
            ScanState::CompletedWithWarnings
        );
    }

    #[test]
    fn a_clean_run_is_plain_completed() {
        assert_eq!(
            ScanState::from_outcome(false, false, false, false),
            ScanState::Completed
        );
    }

    /// The captain's report: "no spinner on the tab being scanned". Both
    /// in-flight states must map to the in-progress indicator, and neither
    /// may fall through to `Idle` — which is exactly what the sidebar used to
    /// draw for them.
    #[test]
    fn a_scan_in_flight_is_always_the_in_progress_indicator() {
        assert_eq!(ScanState::Scanning.indicator(), ScanIndicator::InProgress);
        assert_eq!(ScanState::Cancelling.indicator(), ScanIndicator::InProgress);
    }

    /// Exhaustive on purpose: adding a `ScanState` without deciding what it
    /// looks like should fail here, not go unnoticed on screen.
    #[test]
    fn every_scan_state_maps_to_exactly_one_indicator() {
        let table = [
            (ScanState::NotScanned, ScanIndicator::Idle),
            (ScanState::Scanning, ScanIndicator::InProgress),
            (ScanState::Cancelling, ScanIndicator::InProgress),
            (ScanState::Completed, ScanIndicator::Success),
            (ScanState::CompletedWithWarnings, ScanIndicator::Warning),
            (ScanState::PartiallyCompleted, ScanIndicator::Warning),
            (ScanState::Cancelled, ScanIndicator::Idle),
            (ScanState::Failed, ScanIndicator::Error),
        ];
        for (state, expected) in table {
            assert_eq!(state.indicator(), expected, "{state:?}");
        }
    }

    /// The indicator and the state machine must agree about what "running"
    /// means — one of them alone deciding is how the tab and the pane came
    /// to disagree in the first place.
    #[test]
    fn in_progress_is_exactly_the_active_states() {
        for state in [
            ScanState::NotScanned,
            ScanState::Scanning,
            ScanState::Cancelling,
            ScanState::Completed,
            ScanState::CompletedWithWarnings,
            ScanState::PartiallyCompleted,
            ScanState::Cancelled,
            ScanState::Failed,
        ] {
            assert_eq!(
                state.indicator() == ScanIndicator::InProgress,
                state.is_active(),
                "{state:?}"
            );
        }
    }

    /// A finished scan always says *something*; only "never scanned" and
    /// "cancelled with nothing to report" are silent.
    #[test]
    fn only_the_two_outcome_free_states_are_idle() {
        let idle: Vec<ScanState> = [
            ScanState::NotScanned,
            ScanState::Scanning,
            ScanState::Cancelling,
            ScanState::Completed,
            ScanState::CompletedWithWarnings,
            ScanState::PartiallyCompleted,
            ScanState::Cancelled,
            ScanState::Failed,
        ]
        .into_iter()
        .filter(|state| state.indicator() == ScanIndicator::Idle)
        .collect();
        assert_eq!(idle, vec![ScanState::NotScanned, ScanState::Cancelled]);
    }
}
