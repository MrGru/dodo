use std::path::{Path, PathBuf};

use objc2_foundation::{NSError, NSFileManager, NSURL};

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TrashReceipt {
    pub original_path: PathBuf,
    pub trashed_path: Option<PathBuf>,
}

/// A failed move-to-Trash, classified so the cleanup loop can tell a Full Disk
/// Access / TCC denial — which the user can fix and retry — apart from any
/// other failure. The report shows `message` regardless; `permission_denied`
/// is what raises the Full Disk Access prompt.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TrashError {
    pub message: String,
    pub permission_denied: bool,
}

pub fn move_to_trash(path: &Path) -> Result<TrashReceipt, TrashError> {
    let Some(url) = NSURL::from_path(path, path.is_dir(), None) else {
        return Err(TrashError {
            message: format!("could not convert {} to file URL", path.display()),
            permission_denied: false,
        });
    };
    let mut resulting_url = None;
    NSFileManager::defaultManager()
        .trashItemAtURL_resultingItemURL_error(&url, Some(&mut resulting_url))
        .map_err(|err| TrashError {
            message: err.to_string(),
            permission_denied: is_permission_denied(&err),
        })?;
    Ok(TrashReceipt {
        original_path: path.to_path_buf(),
        trashed_path: resulting_url.and_then(|url| url.to_file_path()),
    })
}

fn is_permission_denied(error: &NSError) -> bool {
    permission_denied_code(&error.domain().to_string(), error.code())
}

/// Whether an `NSFileManager` error code means "the OS refused access", which
/// for a `~/Library/Containers` sandbox container is a missing Full Disk
/// Access grant. Kept pure — no `NSError` — so the mapping stays testable.
///
/// `NSCocoaErrorDomain`: `NSFileReadNoPermissionError` (257, the "you don't
/// have permission to access it" description) and `NSFileWriteNoPermissionError`
/// (513). `NSPOSIXErrorDomain`: `EPERM` (1) / `EACCES` (13), the underlying
/// refusal when it surfaces directly.
fn permission_denied_code(domain: &str, code: isize) -> bool {
    match domain {
        "NSCocoaErrorDomain" => code == 257 || code == 513,
        "NSPOSIXErrorDomain" => code == 1 || code == 13,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::permission_denied_code;

    #[test]
    fn permission_codes_are_recognised_only_in_their_domains() {
        assert!(permission_denied_code("NSCocoaErrorDomain", 257));
        assert!(permission_denied_code("NSCocoaErrorDomain", 513));
        assert!(permission_denied_code("NSPOSIXErrorDomain", 1));
        assert!(permission_denied_code("NSPOSIXErrorDomain", 13));
        // Not-a-permission codes, and a permission code in the wrong domain.
        assert!(!permission_denied_code("NSCocoaErrorDomain", 4)); // NSFileNoSuchFileError
        assert!(!permission_denied_code("NSPOSIXErrorDomain", 2)); // ENOENT
        assert!(!permission_denied_code("NSCocoaErrorDomain", 1));
        assert!(!permission_denied_code("SomeOtherDomain", 257));
    }
}
