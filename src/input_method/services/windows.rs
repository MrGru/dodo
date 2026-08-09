//! Windows TSF install/status driver.
//!
//! The native server exposes `DllRegisterServer` and `DllUnregisterServer`; this
//! service only copies the packaged DLL and asks Windows' own `regsvr32` to call
//! those standard exports. The COM key lives in HKCU, so no administrator or
//! permanent helper is needed.

use std::path::{Path, PathBuf};
use std::process::Command;

use windows_sys::Win32::System::Registry::{
    HKEY_CURRENT_USER, KEY_READ, RegCloseKey, RegOpenKeyExW,
};

use crate::input_method::models::windows::{
    WindowsInstallFailure, WindowsInstallOutcome, WindowsInstallPlan, installed_dll,
    registration_key, source_candidates,
};

/// Installs or replaces the current user's TSF server.
pub fn install(
    executable: &Path,
    working_directory: &Path,
    data_dir: &Path,
) -> WindowsInstallOutcome {
    let Some(source) = source_candidates(executable, working_directory)
        .into_iter()
        .find(|candidate| candidate.is_file())
    else {
        return WindowsInstallOutcome::Failed(WindowsInstallFailure::NoSourceDll);
    };
    let plan = WindowsInstallPlan {
        source,
        destination: installed_dll(data_dir),
    };
    if let Some(directory) = plan.destination.parent()
        && let Err(error) = std::fs::create_dir_all(directory)
    {
        return WindowsInstallOutcome::Failed(WindowsInstallFailure::Copy {
            detail: error.to_string(),
        });
    }
    // A loaded old DLL can keep its profile registered. Ask it to unregister
    // first, but a missing/broken prior registration must not block install.
    let _ = run_regsvr(&plan.destination, true);
    if let Err(error) = std::fs::copy(&plan.source, &plan.destination) {
        return WindowsInstallOutcome::Failed(WindowsInstallFailure::Copy {
            detail: error.to_string(),
        });
    }
    if let Err(detail) = run_regsvr(&plan.destination, false) {
        return WindowsInstallOutcome::Failed(WindowsInstallFailure::Register { detail });
    }
    if is_registered() {
        WindowsInstallOutcome::Ready
    } else {
        WindowsInstallOutcome::Failed(WindowsInstallFailure::Register {
            detail: "Windows did not expose the registered text service".into(),
        })
    }
}

/// Removes the current user's profile and installed DLL.
pub fn uninstall(data_dir: &Path) -> WindowsInstallOutcome {
    let destination = installed_dll(data_dir);
    if destination.exists()
        && let Err(detail) = run_regsvr(&destination, true)
    {
        return WindowsInstallOutcome::Failed(WindowsInstallFailure::Unregister { detail });
    }
    if let Err(error) = std::fs::remove_file(&destination)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        return WindowsInstallOutcome::Failed(WindowsInstallFailure::Unregister {
            detail: error.to_string(),
        });
    }
    if is_registered() {
        WindowsInstallOutcome::Failed(WindowsInstallFailure::Unregister {
            detail: "Windows still reports the text service as registered".into(),
        })
    } else {
        WindowsInstallOutcome::Removed
    }
}

/// Whether this account has the exact per-user COM registration.
pub fn is_registered() -> bool {
    let key = wide(&registration_key());
    let mut opened = std::ptr::null_mut();
    let result =
        unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, key.as_ptr(), 0, KEY_READ, &mut opened) };
    if result != 0 {
        return false;
    }
    unsafe {
        let _ = RegCloseKey(opened);
    }
    true
}

fn run_regsvr(dll: &Path, unregister: bool) -> Result<(), String> {
    let program = std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .map(|root| root.join("System32").join("regsvr32.exe"))
        .unwrap_or_else(|| PathBuf::from("regsvr32.exe"));
    let mut command = Command::new(program);
    command.arg("/s");
    if unregister {
        command.arg("/u");
    }
    let status = command
        .arg(dll)
        .status()
        .map_err(|error| error.to_string())?;
    status
        .success()
        .then_some(())
        .ok_or_else(|| format!("regsvr32 exited with {status}"))
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}
