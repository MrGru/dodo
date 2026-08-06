use std::path::PathBuf;

use crate::cleaner::core::category::CleanerCategory;
use crate::cleaner::core::item::CleanableItem;
use crate::cleaner::core::permissions::PermissionRequirement;

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CategoryScanResult {
    pub category: CleanerCategory,
    pub items: Vec<CleanableItem>,
    pub scanned_entries: u64,
    pub estimated_reclaimable_bytes: u64,
    pub warnings: Vec<ScanWarning>,
    pub completeness: ScanCompleteness,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ScanWarning {
    pub message: String,
}

/// Whether a category scan saw everything it was meant to. The mock scanners
/// always report `Complete`; `Partial` is produced once a real scanner can be
/// turned away from a root. `#[allow(dead_code)]` comes off then.
#[derive(Clone, PartialEq, Eq, Debug)]
#[allow(dead_code)]
pub enum ScanCompleteness {
    Complete,
    Partial {
        skipped_roots: Vec<PathBuf>,
        reason: PartialScanReason,
    },
}

/// Why a scan came back partial. Pending with [`ScanCompleteness::Partial`].
#[derive(Clone, PartialEq, Eq, Debug)]
#[allow(dead_code)]
pub enum PartialScanReason {
    PermissionDenied,
    RootUnavailable,
    Cancelled,
    UnsupportedEnvironment,
}

/// What a Smart Care run would scan, and how much of it at once. Round 1's
/// Scan button runs every scanner sequentially with no plan in between, so
/// nothing builds one yet; the allow comes off with the Smart Care run.
#[derive(Clone, PartialEq, Eq, Debug)]
#[allow(dead_code)]
pub struct SmartCarePlan {
    pub categories: Vec<CleanerCategory>,
    pub max_concurrent_categories: usize,
}

/// One category that failed inside a Smart Care run. Round 1 collapses every
/// scanner error into a single `had_failures` flag (see `views::CleanerView`),
/// so nothing names the failing category yet. Pending with [`SmartCareResult`].
#[derive(Clone, PartialEq, Eq, Debug)]
#[allow(dead_code)]
pub struct CategoryFailure {
    pub category: CleanerCategory,
    pub reason: String,
}

/// The aggregate a Smart Care run reports. Pending with [`SmartCarePlan`].
#[derive(Clone, PartialEq, Eq, Debug)]
#[allow(dead_code)]
pub struct SmartCareResult {
    pub category_results: Vec<CategoryScanResult>,
    pub total_scanned_entries: u64,
    pub estimated_reclaimable_bytes: u64,
    pub permission_warnings: Vec<PermissionRequirement>,
    pub failures: Vec<CategoryFailure>,
}
