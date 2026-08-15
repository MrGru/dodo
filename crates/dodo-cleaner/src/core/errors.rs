use std::path::PathBuf;

use crate::core::permissions::MacPermission;

/// Everything a scanner can fail with. Round 1 ships mock scanners, which only
/// ever return `Cancelled`; the remaining variants are constructed by the real
/// macOS scanners (see `cleaner::macos`). `#[allow(dead_code)]` comes off with
/// the first of those.
#[derive(Debug)]
#[allow(dead_code)]
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

/// Why a deletion was refused. Nothing produces this yet: round 1 has no
/// destructive cleanup path at all (see `cleaner`'s module docs), so the safety
/// checks that would return it do not exist. `#[allow(dead_code)]` comes off
/// when the cleanup path lands.
#[derive(Clone, PartialEq, Eq, Debug)]
#[allow(dead_code)]
pub enum SafetyError {
    OutsideAllowedRoot(PathBuf),
    ProtectedPath(PathBuf),
    RootDeletionRejected(PathBuf),
    SymlinkRejected(PathBuf),
    EntryChanged(PathBuf),
    FileTypeChanged(PathBuf),
    SharedApplicationData(PathBuf),
}

/// Why a cleanup run failed. Pending for the same reason as [`SafetyError`].
#[derive(Clone, PartialEq, Eq, Debug)]
#[allow(dead_code)]
pub enum CleanupError {
    Safety(SafetyError),
    Trash(String),
    PermissionRequired(MacPermission),
    ExternalOperationFailed { operation: String, message: String },
}
