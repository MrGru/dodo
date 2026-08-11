//! Per-user login registration for the tray-only launch.
//!
//! macOS uses `SMAppService.mainApp`, Apple's current Login Items API. It only
//! works for a bundled `Dodo.app` on macOS 13 or later; older systems and a
//! directly run development binary leave the setting off. Windows uses the
//! per-user `HKCU\…\Run` key Microsoft documents for programs that run at
//! sign-in. Neither path starts a helper: each launches dodo itself.
//!
//! The platform registration is the setting's durable state. Keeping a second
//! boolean in `session.json` would let the UI claim startup was enabled after
//! the OS rejected or removed its registration.

#[cfg(any(target_os = "windows", test))]
use std::path::Path;

#[cfg(any(target_os = "windows", test))]
const STARTUP_FLAG: &str = "--startup";
const MACOS_LOGIN_ARGUMENT: &str = "-NSApplicationLaunchIsHidden";
#[cfg(any(target_os = "windows", test))]
const MAX_WINDOWS_RUN_COMMAND_CHARS: usize = 260;

/// Whether this process was launched by the OS login registration.
pub fn launched_at_login() -> bool {
    #[cfg(target_os = "macos")]
    {
        contains_argument(std::env::args(), MACOS_LOGIN_ARGUMENT)
    }

    #[cfg(target_os = "windows")]
    {
        contains_argument(std::env::args(), STARTUP_FLAG)
    }
}

fn contains_argument<I, S>(args: I, wanted: &str) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    args.into_iter().any(|arg| arg.as_ref() == wanted)
}

/// Whether this user's OS login registration is active.
pub fn enabled() -> bool {
    #[cfg(target_os = "macos")]
    {
        macos::enabled()
    }

    #[cfg(target_os = "windows")]
    {
        windows::enabled()
    }
}

/// Enables or removes this user's OS login registration.
pub fn set_enabled(enabled: bool) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        macos::set_enabled(enabled)
    }

    #[cfg(target_os = "windows")]
    {
        windows::set_enabled(enabled)
    }
}

/// The Windows Run value. Paths cannot contain a quote, so quoting the executable
/// is sufficient for Windows' command-line parser and preserves spaces.
#[cfg(any(target_os = "windows", test))]
fn windows_command(path: &Path) -> Result<String, String> {
    let command = format!("\"{}\" {STARTUP_FLAG}", path.display());
    if command.encode_utf16().count() > MAX_WINDOWS_RUN_COMMAND_CHARS {
        return Err(format!(
            "Windows Run commands are limited to {MAX_WINDOWS_RUN_COMMAND_CHARS} characters"
        ));
    }
    Ok(command)
}

#[cfg(target_os = "macos")]
mod macos {
    use std::ptr;

    use objc2::msg_send;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject, Bool};

    const ENABLED: isize = 1;

    // `SMAppService` is available only on macOS 13. The dynamic class lookup
    // below makes older systems a normal, reported unsupported case instead of
    // sending an unavailable selector.
    #[link(name = "ServiceManagement", kind = "framework")]
    unsafe extern "C" {}

    fn service() -> Result<Retained<AnyObject>, String> {
        let class = AnyClass::get(c"SMAppService")
            .ok_or_else(|| "Start with OS requires macOS 13 or later".to_owned())?;
        // SAFETY: `mainAppService` is the documented class method on
        // `SMAppService`; the dynamic lookup above establishes its availability.
        let service: Retained<AnyObject> = unsafe { msg_send![class, mainAppService] };
        Ok(service)
    }

    pub(super) fn enabled() -> bool {
        let Ok(service) = service() else {
            return false;
        };
        // SAFETY: `status` returns `SMAppServiceStatus` (`NSInteger`) for this
        // documented service instance.
        let status: isize = unsafe { msg_send![&*service, status] };
        status == ENABLED
    }

    pub(super) fn set_enabled(enabled: bool) -> Result<(), String> {
        let service = service()?;
        let mut error = ptr::null_mut::<AnyObject>();
        // SAFETY: both documented selectors return `BOOL` and take an optional
        // `NSError **`; the service is kept alive across the message send.
        let succeeded: Bool = unsafe {
            if enabled {
                msg_send![&*service, registerAndReturnError: &mut error]
            } else {
                msg_send![&*service, unregisterAndReturnError: &mut error]
            }
        };
        if succeeded.as_bool() {
            Ok(())
        } else {
            Err("macOS rejected the Login Items change".to_owned())
        }
    }
}

#[cfg(target_os = "windows")]
mod windows {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt as _;
    use std::ptr;

    use windows_sys::Win32::Foundation::ERROR_FILE_NOT_FOUND;
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE, REG_SZ, RegCloseKey, RegCreateKeyW,
        RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
    };

    use super::windows_command;

    const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const VALUE_NAME: &str = "dodo";

    fn wide(value: &str) -> Vec<u16> {
        OsStr::new(value).encode_wide().chain(Some(0)).collect()
    }

    fn command() -> Result<String, String> {
        windows_command(&std::env::current_exe().map_err(|error| error.to_string())?)
    }

    pub(super) fn enabled() -> bool {
        let Ok(expected) = command() else {
            return false;
        };
        let mut key: HKEY = ptr::null_mut();
        let run_key = wide(RUN_KEY);
        // SAFETY: the path is NUL-terminated and `key` receives an owned handle
        // which is closed on every successful open below.
        if unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, run_key.as_ptr(), 0, KEY_READ, &mut key) } != 0
        {
            return false;
        }

        let value = wide(VALUE_NAME);
        let mut kind = 0;
        let mut bytes = 0;
        // SAFETY: a null data pointer asks Windows for the required size only.
        let result = unsafe {
            RegQueryValueExW(
                key,
                value.as_ptr(),
                ptr::null(),
                &mut kind,
                ptr::null_mut(),
                &mut bytes,
            )
        };
        if result != 0 || kind != REG_SZ || bytes % 2 != 0 {
            // SAFETY: `key` came from a successful `RegOpenKeyExW`.
            unsafe { RegCloseKey(key) };
            return false;
        }

        let mut buffer = vec![0_u16; bytes as usize / 2];
        // SAFETY: the buffer is exactly the size Windows just reported.
        let result = unsafe {
            RegQueryValueExW(
                key,
                value.as_ptr(),
                ptr::null(),
                &mut kind,
                buffer.as_mut_ptr().cast(),
                &mut bytes,
            )
        };
        // SAFETY: `key` came from a successful `RegOpenKeyExW`.
        unsafe { RegCloseKey(key) };
        result == 0
            && kind == REG_SZ
            && String::from_utf16_lossy(&buffer).trim_end_matches('\0') == expected
    }

    pub(super) fn set_enabled(enabled: bool) -> Result<(), String> {
        let run_key = wide(RUN_KEY);
        let value = wide(VALUE_NAME);
        let mut key: HKEY = ptr::null_mut();

        if enabled {
            let command = command()?;
            let command = wide(&command);
            // SAFETY: the paths are NUL-terminated and `key` is closed below.
            let created = unsafe { RegCreateKeyW(HKEY_CURRENT_USER, run_key.as_ptr(), &mut key) };
            if created != 0 {
                return Err(format!("could not open Windows Run key ({created})"));
            }
            // SAFETY: `command` is UTF-16 with its trailing NUL included.
            let result = unsafe {
                RegSetValueExW(
                    key,
                    value.as_ptr(),
                    0,
                    REG_SZ,
                    command.as_ptr().cast(),
                    (command.len() * size_of::<u16>()) as u32,
                )
            };
            // SAFETY: `key` came from a successful `RegCreateKeyW`.
            unsafe { RegCloseKey(key) };
            return (result == 0)
                .then_some(())
                .ok_or_else(|| format!("could not update Windows Run key ({result})"));
        }

        // SAFETY: the path is NUL-terminated and a successful open is closed.
        let opened = unsafe {
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                run_key.as_ptr(),
                0,
                KEY_SET_VALUE,
                &mut key,
            )
        };
        if opened == ERROR_FILE_NOT_FOUND {
            return Ok(());
        }
        if opened != 0 {
            return Err(format!("could not open Windows Run key ({opened})"));
        }
        // SAFETY: `value` is NUL-terminated and `key` is valid.
        let result = unsafe { RegDeleteValueW(key, value.as_ptr()) };
        // SAFETY: `key` came from a successful `RegOpenKeyExW`.
        unsafe { RegCloseKey(key) };
        (result == 0 || result == ERROR_FILE_NOT_FOUND)
            .then_some(())
            .ok_or_else(|| format!("could not remove Windows Run value ({result})"))
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{MACOS_LOGIN_ARGUMENT, STARTUP_FLAG, contains_argument, windows_command};

    #[test]
    fn a_macos_login_launch_is_the_hidden_one() {
        assert!(contains_argument(
            ["dodo", MACOS_LOGIN_ARGUMENT],
            MACOS_LOGIN_ARGUMENT
        ));
        assert!(!contains_argument(["dodo"], MACOS_LOGIN_ARGUMENT));
    }

    #[test]
    fn windows_run_command_quotes_the_executable_and_requests_tray_startup() {
        assert_eq!(
            windows_command(Path::new(r"C:\Program Files\Dodo\dodo.exe")),
            Ok(format!(
                r#""C:\Program Files\Dodo\dodo.exe" {STARTUP_FLAG}"#
            )),
        );
    }

    #[test]
    fn windows_run_command_refuses_the_os_limit() {
        let path = format!("C:\\{}\\dodo.exe", "a".repeat(300));
        assert!(windows_command(Path::new(&path)).is_err());
    }
}
