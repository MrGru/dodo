use std::path::{Path, PathBuf};

/// Reveals `path` in Finder with the item selected in its containing folder.
///
/// Shells out to `/usr/bin/open -R`, the documented reveal-and-select command,
/// rather than `NSWorkspace activateFileViewerSelectingURLs:`. The objc2 call
/// returns no status yet silently revealed nothing from inside dodo's GPUI
/// event loop (the click handler is reached and the path is valid — "Copy
/// path" on the same row works — but no Finder window ever comes forward).
/// `open -R` goes through LaunchServices in a separate process, so it is
/// independent of the caller's main-thread/autorelease-pool/run-loop context,
/// and it is exactly how `windows::platform` (`explorer /select,`) and
/// `linux::platform` (`xdg-open`) already reveal. The guard stays the same:
/// only a path that is positively gone refuses; a path the process cannot stat
/// (a sandbox container without Full Disk Access) is still handed to Finder,
/// which has its own access — "cannot stat" is not "gone".
pub fn reveal_in_finder(path: &Path) -> Result<(), String> {
    let path = revealable_path(path)?;
    std::process::Command::new("/usr/bin/open")
        .arg("-R")
        .arg(&path)
        .spawn()
        .map_err(|error| error.to_string())?;
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
