use std::path::{Path, PathBuf};

use objc2_app_kit::NSWorkspace;
use objc2_foundation::{NSArray, NSURL};

/// Reveals `path` in Finder with the item selected in its containing folder.
///
/// Uses `activateFileViewerSelectingURLs:`, the canonical reveal-and-select
/// API, rather than `selectFile:inFileViewerRootedAtPath:` — the latter, given
/// a non-empty root, opened a window rooted there instead of reliably showing
/// the enclosing folder, which is the "does not open the containing folder"
/// report. `activateFileViewerSelectingURLs:` reports no status, so the only
/// guard is that the item is not positively gone; a path the process cannot
/// stat (a sandbox container without Full Disk Access) is still handed to
/// Finder, which has its own access — "cannot stat" is not "gone".
pub fn reveal_in_finder(path: &Path) -> Result<(), String> {
    let path = revealable_path(path)?;
    let Some(url) = NSURL::from_path(&path, path.is_dir(), None) else {
        return Err(format!("could not convert {} to file URL", path.display()));
    };
    let urls = NSArray::from_retained_slice(&[url]);
    NSWorkspace::sharedWorkspace().activateFileViewerSelectingURLs(&urls);
    Ok(())
}

/// Confirms an item is worth revealing. Deliberately no `canonicalize()`: that
/// needs read access to every path component and so fails for the very
/// sandbox-container paths this has to reveal. Only a positive "not found"
/// refuses; a permission error falls through to Finder.
fn revealable_path(path: &Path) -> Result<PathBuf, String> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(path.to_path_buf()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Err(format!("{} no longer exists", path.display()))
        }
        Err(_) => Ok(path.to_path_buf()),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::revealable_path;

    #[test]
    fn revealable_path_accepts_an_existing_item() {
        let temp = std::env::temp_dir().join(format!("dodo-cleaner-finder-{}", std::process::id()));
        let nested = temp.join("parent").join("child");
        fs::create_dir_all(&nested).expect("creates target");

        assert_eq!(revealable_path(&nested).expect("resolves target"), nested);

        fs::remove_dir_all(temp).expect("removes temp tree");
    }

    #[test]
    fn revealable_path_rejects_a_missing_item() {
        let missing = std::env::temp_dir().join(format!(
            "dodo-cleaner-finder-missing-{}",
            std::process::id()
        ));
        assert!(revealable_path(&missing).is_err());
    }
}
