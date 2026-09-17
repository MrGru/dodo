use std::path::{Path, PathBuf};

/// Reveals `path` in Finder with the item selected in its containing folder.
///
/// Shells out to `/usr/bin/open -R`, the documented reveal-and-select command,
/// rather than `NSWorkspace activateFileViewerSelectingURLs:` — that objc2 call
/// returns no status and cannot report why nothing appeared.
///
/// **The `__CFBundleIdentifier` strip is the actual fix, not hygiene.** A
/// terminal that runs `cargo run` (Ghostty, Terminal, iTerm…) exports
/// `__CFBundleIdentifier=<the terminal's id>`, which dodo and every child it
/// spawns inherit. LaunchServices reads that variable to decide *which app is
/// asking*, so the reveal was attributed to the terminal — a background app —
/// and macOS suppresses a background app's attempt to bring Finder forward.
/// From a shell the same `open -R` works only because the terminal really is
/// frontmost. Removing the variable makes the request come from `open` itself,
/// so Finder activates. (A properly bundled `.app` launched from Finder gets
/// dodo's own id here and never hit this; the bug is specific to the unbundled,
/// terminal-launched binary — which is exactly the `cargo run` dev build.) The
/// same stale id also broke the earlier in-process `NSWorkspace` reveal, for
/// the identical reason.
///
/// This mirrors how `windows::platform` (`explorer /select,`) and
/// `linux::platform` (`xdg-open`) reveal. The guard stays the same: only a path
/// that is positively gone refuses; a path the process cannot stat (a sandbox
/// container without Full Disk Access) is still handed to Finder, which has its
/// own access — "cannot stat" is not "gone".
pub fn reveal_in_finder(path: &Path) -> Result<(), String> {
    let path = revealable_path(path)?;
    let output = std::process::Command::new("/usr/bin/open")
        .arg("-R")
        .arg(&path)
        .env_remove("__CFBundleIdentifier")
        .output()
        .map_err(|error| format!("could not run /usr/bin/open -R: {error}"))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let detail = stderr.trim();
    let status = match output.status.code() {
        Some(code) => format!("exit code {code}"),
        None => "a signal".to_string(),
    };
    if detail.is_empty() {
        Err(format!("open -R {} failed with {status}", path.display()))
    } else {
        Err(format!(
            "open -R {} failed with {status}: {detail}",
            path.display()
        ))
    }
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
