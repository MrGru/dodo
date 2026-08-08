//! Where the two files live, and where the bundle installs to.
//!
//! # This duplicates one line of dodo's `src/paths.rs`, on purpose
//!
//! `~/Library/Application Support/dodo` is dodo's data directory and is
//! **frozen** — changing it orphans every existing installation's saved
//! collections. dodo resolves it in `crate::paths`, which classifies the
//! platform from the target triple `build.rs` embedded and answers for Windows
//! and Linux too. None of that can be reached from here: this crate is linked by
//! a process that has no `build.rs`, no `build_info`, and no reason to know what
//! `%APPDATA%` is.
//!
//! So the macOS branch is spelled out again, and dodo's own test suite compares
//! the two spellings — see `paths::tests::the_input_method_agrees_about_the_data_directory`
//! in the `dodo` crate. Two implementations with a test between them is the
//! trade this crate makes everywhere: the alternative was for the bundle to link
//! dodo.
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

/// dodo's data directory for the current user, or `None` when the environment
/// names no home at all.
///
/// dodo's own resolver has a last-resort `.dodo` fallback for that case; this
/// one deliberately does not. A relative directory would put the input method's
/// settings wherever the *bundle* was launched from, which is nowhere the user
/// can find and not the same place dodo would look. Answering `None` means the
/// bundle types with its compiled-in defaults, which is the honest outcome.
pub fn support_dir_from_env() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|home| home.is_absolute())
        .map(|home| support_dir(&home))
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
    use super::{input_methods_dir, installed_bundle, support_dir, support_dir_from_env};
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
