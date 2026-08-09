//! Where the two files live, and where the bundle installs to.
//!
//! # This duplicates dodo's platform paths, on purpose
//!
//! Native hosts cannot link dodo just to find `input-method.json`. macOS keeps
//! that file under `~/Library/Application Support/dodo`; Windows uses
//! `%APPDATA%\\dodo` (or `$HOME\\AppData\\Roaming\\dodo`). The matching pure
//! resolver in `src/paths.rs` is tested against the Windows helper below so the
//! processes do not silently read different settings files.
//!
//! # `~/Library/Input Methods` is not under `support_dir`
//!
//! It is macOS's directory, not dodo's: `~/Library/Input Methods` is where the
//! system looks for input-method bundles, it needs no admin rights, and its
//! *contents* are the only thing dodo may ever touch there — one bundle, by
//! name. [`installed_bundle`] is the only path in this crate dodo writes outside
//! its own data directory.

use std::path::{Path, PathBuf};

use crate::bundle::BUNDLE_NAME;

/// dodo's data directory on macOS, under a given home. Frozen; see the module
/// docs.
pub fn support_dir(home: &Path) -> PathBuf {
    home.join("Library")
        .join("Application Support")
        .join("dodo")
}

/// dodo's Windows data directory, from the environment values a TSF host gets.
///
/// Relative paths are rejected rather than using a directory chosen by the host
/// process's current working directory.
pub fn windows_support_dir(appdata: Option<&Path>, home: Option<&Path>) -> Option<PathBuf> {
    appdata
        .filter(|path| is_windows_absolute(path))
        .map(|path| path.join("dodo"))
        .or_else(|| {
            home.filter(|path| is_windows_absolute(path))
                .map(|path| path.join("AppData").join("Roaming").join("dodo"))
        })
}

/// A Windows path is not `Path::is_absolute()` when this pure function is
/// tested on Unix, so recognise the two Windows absolute forms explicitly.
fn is_windows_absolute(path: &Path) -> bool {
    let path = path.to_string_lossy();
    let bytes = path.as_bytes();
    path.starts_with("\\\\")
        || (bytes.len() >= 3
            && bytes[1] == b':'
            && bytes[0].is_ascii_alphabetic()
            && matches!(bytes[2], b'\\' | b'/'))
}

/// dodo's data directory for the current user's native host.
///
/// dodo's own resolver has a last-resort `.dodo` fallback for a missing home;
/// native hosts deliberately do not. A relative directory would put settings
/// wherever a host was launched from, so `None` means pass-through defaults.
pub fn support_dir_from_env() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        let appdata = std::env::var_os("APPDATA").map(PathBuf::from);
        let home = std::env::var_os("HOME").map(PathBuf::from);
        return windows_support_dir(appdata.as_deref(), home.as_deref());
    }

    #[cfg(target_os = "macos")]
    {
        return std::env::var_os("HOME")
            .map(PathBuf::from)
            .filter(|home| home.is_absolute())
            .map(|home| support_dir(&home));
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    None
}

/// The system directory input-method bundles are installed into.
///
/// No admin rights, no `sudo`, no privileged helper: this is the whole reason
/// dodo can install an input method with a button rather than an installer.
pub fn input_methods_dir(home: &Path) -> PathBuf {
    home.join("Library").join("Input Methods")
}

/// Where dodo's input method lives once installed.
pub fn installed_bundle(home: &Path) -> PathBuf {
    input_methods_dir(home).join(BUNDLE_NAME)
}

#[cfg(test)]
mod tests {
    use super::{
        input_methods_dir, installed_bundle, support_dir, support_dir_from_env, windows_support_dir,
    };
    use crate::bundle::BUNDLE_NAME;
    use std::path::{Path, PathBuf};

    /// The string that may not change. dodo's own `paths.rs` carries the same
    /// test against the same literal, and its suite compares the two functions.
    #[test]
    fn the_support_directory_is_the_frozen_macos_path() {
        assert_eq!(
            support_dir(Path::new("/Users/someone")),
            PathBuf::from("/Users/someone/Library/Application Support/dodo")
        );
    }

    #[test]
    fn the_install_destination_is_the_per_user_input_methods_directory() {
        let home = Path::new("/Users/someone");
        assert_eq!(
            input_methods_dir(home),
            PathBuf::from("/Users/someone/Library/Input Methods")
        );
        assert_eq!(
            installed_bundle(home),
            input_methods_dir(home).join(BUNDLE_NAME)
        );
    }

    /// Every test process has a `HOME`, and it is absolute. This asserts the
    /// shape rather than the value, which is all that can be said about the
    /// machine it runs on.
    #[test]
    fn windows_and_dodo_agree_about_the_windows_data_directory() {
        assert_eq!(
            windows_support_dir(Some(Path::new("C:/Users/someone/AppData/Roaming")), None),
            Some(PathBuf::from("C:/Users/someone/AppData/Roaming/dodo"))
        );
        assert_eq!(
            windows_support_dir(None, Some(Path::new("C:/Users/someone"))),
            Some(PathBuf::from("C:/Users/someone/AppData/Roaming/dodo"))
        );
        assert_eq!(windows_support_dir(Some(Path::new("relative")), None), None);
    }

    #[test]
    fn the_environment_resolves_to_an_absolute_directory_named_dodo() {
        let Some(dir) = support_dir_from_env() else {
            // A build environment with no HOME at all. The `None` branch is the
            // documented answer, so there is nothing to assert.
            return;
        };
        assert!(dir.is_absolute());
        assert_eq!(dir.file_name().unwrap(), "dodo");
    }
}
