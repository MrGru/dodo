//! What one category's scan is doing, as an explicit enum rather than
//! something inferred from a warning string or an `Option` combination —
//! see [`ScanState::from_outcome`].

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

impl ScanState {
    pub fn is_active(self) -> bool {
        matches!(self, ScanState::Scanning | ScanState::Cancelling)
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
    use super::ScanState;

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
}
