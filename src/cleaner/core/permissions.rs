use crate::cleaner::core::category::CleanerCategory;

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
