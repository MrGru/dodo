//! The two OS integrations every Linux scanner needs: moving a path to the
//! Trash, and revealing one in whatever file manager is registered as the
//! default. Trash goes through the `trash` crate (see the dependency
//! comment in `Cargo.toml`), which implements the freedesktop.org Trash
//! specification itself rather than this build hand-rolling
//! `$XDG_DATA_HOME/Trash/files` + `.trashinfo` writing a second time.

use std::path::{Path, PathBuf};

use crate::core::ai_app_provider::AiAppActivity;

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TrashReceipt {
    pub original_path: PathBuf,
    /// Always `None`: the `trash` crate's basic API reports success or
    /// failure only, not where the item landed inside `Trash/files`.
    /// Callers already treat this as optional — `cleaner::macos::platform`
    /// is the only backend that can fill it in.
    pub trashed_path: Option<PathBuf>,
}

pub fn move_to_trash(path: &Path) -> Result<TrashReceipt, String> {
    trash::delete(path).map_err(|error| error.to_string())?;
    Ok(TrashReceipt {
        original_path: path.to_path_buf(),
        trashed_path: None,
    })
}

/// Opens the containing folder in the desktop's default file manager.
/// `xdg-open` has no "select this one file" equivalent portable across
/// desktop environments, unlike Finder or Explorer, so this reveals the
/// *parent* directory rather than the item itself.
pub fn reveal_in_file_manager(path: &Path) -> Result<(), String> {
    let target = path.parent().unwrap_or(path);
    std::process::Command::new("xdg-open")
        .arg(target)
        .spawn()
        .map_err(|error| error.to_string())?;
    Ok(())
}

/// Read `/proc/*/comm` without invoking a process-listing command. If procfs
/// cannot be enumerated or yields no readable process names, activity remains
/// unknown and the shared scanner suppresses default selection.
pub fn ai_app_activity(process_names: &[&str]) -> AiAppActivity {
    let Ok(processes) = std::fs::read_dir("/proc") else {
        return AiAppActivity::Unknown;
    };
    let mut inspected = false;
    for process in processes.flatten().filter(|entry| {
        entry
            .file_name()
            .to_string_lossy()
            .bytes()
            .all(|byte| byte.is_ascii_digit())
    }) {
        let Ok(name) = std::fs::read_to_string(process.path().join("comm")) else {
            continue;
        };
        inspected = true;
        if process_names
            .iter()
            .any(|candidate| name.trim().eq_ignore_ascii_case(candidate))
        {
            return AiAppActivity::Running;
        }
    }
    if inspected {
        AiAppActivity::NotRunning
    } else {
        AiAppActivity::Unknown
    }
}
