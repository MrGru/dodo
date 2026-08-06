use std::path::PathBuf;

use crate::cleaner::core::category::CleanerCategory;
use crate::cleaner::core::errors::CleanupError;
use crate::cleaner::core::item::{CleanableItem, CleanableItemId};
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

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ScanCompleteness {
    Complete,
    Partial {
        skipped_roots: Vec<PathBuf>,
        reason: PartialScanReason,
    },
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum PartialScanReason {
    PermissionDenied,
    RootUnavailable,
    Cancelled,
    UnsupportedEnvironment,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SmartCarePlan {
    pub categories: Vec<CleanerCategory>,
    pub max_concurrent_categories: usize,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CategoryFailure {
    pub category: CleanerCategory,
    pub reason: String,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SmartCareResult {
    pub category_results: Vec<CategoryScanResult>,
    pub total_scanned_entries: u64,
    pub estimated_reclaimable_bytes: u64,
    pub permission_warnings: Vec<PermissionRequirement>,
    pub failures: Vec<CategoryFailure>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CleanupItemSuccess {
    pub id: CleanableItemId,
    pub path: PathBuf,
    pub trashed_path: Option<PathBuf>,
    pub logical_size: u64,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CleanupItemFailure {
    pub id: CleanableItemId,
    pub path: PathBuf,
    pub error: CleanupError,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CleanupReport {
    pub successes: Vec<CleanupItemSuccess>,
    pub failures: Vec<CleanupItemFailure>,
    pub estimated_reclaimed_bytes: u64,
}
