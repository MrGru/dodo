use std::path::PathBuf;

use crate::cleaner::core::category::CleanerCategory;

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
