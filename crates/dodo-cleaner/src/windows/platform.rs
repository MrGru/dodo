//! Windows Cleaner OS integrations: moving a path to the Recycle Bin,
//! revealing one in Explorer, and opening the native Installed Apps surface.
//! Unlike `cleaner::macos::platform`, which calls `NSFileManager` directly, trash
//! goes through the `trash` crate (see the dependency comment in
//! `Cargo.toml`) rather than hand-written `IFileOperation` bindings this
//! build has no way to check against a real Windows host.

use std::path::{Path, PathBuf};

use crate::core::ai_app_provider::AiAppActivity;

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

/// Opens Windows' own Installed Apps surface. This deliberately receives no
/// registry or vendor command text: Windows and the application's registered
/// installer retain ownership of the actual uninstall.
pub fn open_installed_apps_settings() -> Result<(), String> {
    let command = crate::windows::scanners::installed_apps::installed_apps_settings_command();
    std::process::Command::new(command.program)
        .args(command.args)
        .spawn()
        .map_err(|error| error.to_string())?;
    Ok(())
}

/// Read-only process-name probe for AI apps. Snapshot failure is unknown,
/// never "not running", so cleanable results are not selected by default.
pub fn ai_app_activity(process_names: &[&str]) -> AiAppActivity {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };

    struct Snapshot(HANDLE);
    impl Drop for Snapshot {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }

    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return AiAppActivity::Unknown;
    }
    let snapshot = Snapshot(snapshot);
    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    if unsafe { Process32FirstW(snapshot.0, &mut entry) } == 0 {
        return AiAppActivity::Unknown;
    }

    loop {
        let end = entry
            .szExeFile
            .iter()
            .position(|character| *character == 0)
            .unwrap_or(entry.szExeFile.len());
        let name = String::from_utf16_lossy(&entry.szExeFile[..end]);
        if process_names
            .iter()
            .any(|candidate| name.eq_ignore_ascii_case(candidate))
        {
            return AiAppActivity::Running;
        }
        if unsafe { Process32NextW(snapshot.0, &mut entry) } == 0 {
            return AiAppActivity::NotRunning;
        }
    }
}
