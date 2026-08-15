use std::path::{Path, PathBuf};

use objc2_app_kit::NSWorkspace;
use objc2_foundation::NSString;

/// Resolves an existing path before asking Finder to select it. The old
/// `activateFileViewerSelectingURLs:` call returns no status, so failures were
/// reported as success; `selectFile:inFileViewerRootedAtPath:` returns one.
pub fn reveal_in_finder(path: &Path) -> Result<(), String> {
    let (path, root) = finder_selection(path)?;
    let selected = NSString::from_str(&path.to_string_lossy());
    let root = NSString::from_str(&root.to_string_lossy());
    if NSWorkspace::sharedWorkspace().selectFile_inFileViewerRootedAtPath(Some(&selected), &root) {
        Ok(())
    } else {
        Err(format!("Finder could not reveal {}", path.display()))
    }
}

fn finder_selection(path: &Path) -> Result<(PathBuf, PathBuf), String> {
    let path = path
        .canonicalize()
        .map_err(|error| format!("{}: {error}", path.display()))?;
    let root = path
        .parent()
        .ok_or_else(|| format!("{} has no containing folder", path.display()))?
        .to_path_buf();
    Ok((path, root))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::finder_selection;

    #[test]
    fn reveal_selection_canonicalizes_an_existing_child() {
        let temp = std::env::temp_dir().join(format!("dodo-cleaner-finder-{}", std::process::id()));
        let nested = temp.join("parent").join("child");
        fs::create_dir_all(&nested).expect("creates target");

        let (path, root) = finder_selection(&temp.join("parent").join(".").join("child"))
            .expect("resolves target");
        assert_eq!(path, nested.canonicalize().expect("canonical target"));
        assert_eq!(
            root,
            temp.join("parent")
                .canonicalize()
                .expect("canonical parent")
        );

        fs::remove_dir_all(temp).expect("removes temp tree");
    }

    #[test]
    fn reveal_selection_rejects_a_missing_path() {
        let missing = std::env::temp_dir().join(format!(
            "dodo-cleaner-finder-missing-{}",
            std::process::id()
        ));
        assert!(finder_selection(&missing).is_err());
    }
}
