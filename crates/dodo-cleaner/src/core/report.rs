use std::path::PathBuf;

use crate::core::category::CleanerCategory;
use crate::core::errors::CleanupError;
use crate::core::item::{CleanableItem, CleanableItemId};

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
