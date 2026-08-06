use std::path::PathBuf;

use crate::cleaner::core::permissions::MacPermission;

#[derive(Debug)]
pub enum ScanError {
    RootUnavailable(PathBuf),
    PermissionDenied(PathBuf),
    UnsupportedMacOsVersion,
    ExternalToolUnavailable(String),
    InvalidMetadata(PathBuf),
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    Cancelled,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum SafetyError {
    OutsideAllowedRoot(PathBuf),
    ProtectedPath(PathBuf),
    RootDeletionRejected(PathBuf),
    SymlinkRejected(PathBuf),
    EntryChanged(PathBuf),
    FileTypeChanged(PathBuf),
    SharedApplicationData(PathBuf),
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum CleanupError {
    Safety(SafetyError),
    PermissionRequired(MacPermission),
    ExternalOperationFailed { operation: String, message: String },
}
