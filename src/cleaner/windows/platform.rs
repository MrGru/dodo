//! The two OS integrations every Windows scanner needs: moving a path to the
//! Recycle Bin, and revealing one in Explorer. Unlike
//! `cleaner::macos::platform`, which calls `NSFileManager` directly, trash
//! goes through the `trash` crate (see the dependency comment in
//! `Cargo.toml`) rather than hand-written `IFileOperation` bindings this
//! build has no way to check against a real Windows host.

use std::path::{Path, PathBuf};

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TrashReceipt {
    pub original_path: PathBuf,
    /// Always `None`: the `trash` crate's basic API reports success or
    /// failure only, not where the item landed. Callers already treat this
    /// as optional — `cleaner::macos::platform::trash` is the only backend
    /// that can fill it in.
    pub trashed_path: Option<PathBuf>,
}

pub fn move_to_trash(path: &Path) -> Result<TrashReceipt, String> {
    trash::delete(path).map_err(|error| error.to_string())?;
    Ok(TrashReceipt {
        original_path: path.to_path_buf(),
        trashed_path: None,
    })
}

/// Opens Explorer with `path` pre-selected. Fire-and-forget like
/// `cleaner::macos::platform::finder::reveal_in_finder`: `explorer.exe`
/// itself is well known to exit non-zero even on success, so this only
/// reports whether the process could be *started*, never whether the
/// selection actually happened.
pub fn reveal_in_explorer(path: &Path) -> Result<(), String> {
    std::process::Command::new("explorer")
        .arg(format!("/select,{}", path.display()))
        .spawn()
        .map_err(|error| error.to_string())?;
    Ok(())
}
