//! The permission contract the macOS scanners will be gated on.
//!
//! Every item here is pending: round 1 ships mock scanners that touch nothing,
//! so nothing checks Full Disk Access yet and no implementation of
//! [`PermissionService`] exists. The allow is module-wide rather than per item
//! because the whole module is the unit that is waiting — it comes off with the
//! first real permission check, and until then nothing in here should be
//! deleted to quieten the lint.
#![allow(dead_code)]

use crate::core::category::CleanerCategory;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MacPermission {
    FullDiskAccess,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PermissionState {
    Unknown,
    Checking,
    Granted,
    Denied,
    Restricted,
    RequiresRestart,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PermissionRequirement {
    pub permission: MacPermission,
    pub reason: String,
    pub affected_categories: Vec<CleanerCategory>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum PermissionError {
    UnsupportedPlatform,
    CheckFailed(String),
}

pub trait PermissionService: Send + Sync {
    fn check_full_disk_access(&self) -> Result<PermissionState, PermissionError>;
    fn trigger_tcc_registration(&self) -> Result<(), PermissionError>;
    fn open_full_disk_access_settings(&self) -> Result<(), PermissionError>;
    fn reveal_application_bundle(&self) -> Result<(), PermissionError>;
}
