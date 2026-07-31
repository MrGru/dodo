//! What a running dodo *is*, on disk, and therefore what an installer must
//! replace.
//!
//! This is pure path arithmetic over `std::env::current_exe()`'s answer, which
//! is why it lives in `models/` rather than inside an installer: deciding
//! whether `/Applications/dodo.app/Contents/MacOS/dodo` means "swap the bundle"
//! is a rule with edge cases, and a rule with edge cases wants a table of tests
//! rather than a filesystem.
//!
//! Nothing here touches the disk — not even to check that a path exists. The
//! installers do that, with the answer this gives them.

use std::path::{Path, PathBuf};

/// What the running binary is, and what replacing it means.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InstallTarget {
    /// A macOS `.app`. `bundle` is the `…/dodo.app` directory: the thing an
    /// archive replaces wholesale, because that is what the release's
    /// `-app.tar.gz` contains.
    MacosBundle {
        bundle: PathBuf,
        executable: PathBuf,
    },
    /// A loose executable — a `cargo run` build, an extracted bare archive, a
    /// binary somebody put on `$PATH`. Replaceable on Linux and Windows, and
    /// **not** on macOS, where the manifest offers only the bundle.
    BareExecutable { executable: PathBuf },
}

impl InstallTarget {
    /// The path that has to be writable for an in-place install: the *parent*
    /// of the thing being replaced, since replacing means creating a sibling
    /// and renaming over it.
    pub fn writable_parent(&self) -> Option<&Path> {
        match self {
            InstallTarget::MacosBundle { bundle, .. } => bundle.parent(),
            InstallTarget::BareExecutable { executable } => executable.parent(),
        }
    }

    pub fn executable(&self) -> &Path {
        match self {
            InstallTarget::MacosBundle { executable, .. }
            | InstallTarget::BareExecutable { executable } => executable,
        }
    }
}

/// Classifies the running executable.
///
/// A macOS bundle has a fixed interior layout — `Foo.app/Contents/MacOS/foo` —
/// so the test is exactly that: two levels up is `Contents`, three is something
/// ending in `.app`. Anything else is a bare executable, including a binary
/// that merely happens to live under a directory called `MacOS`.
///
/// The check is done on every platform, not just macOS, because it is pure and
/// because keeping it platform-free is what lets it be tested here. On Windows
/// and Linux no path can satisfy it in practice, and the answer is the same
/// `BareExecutable` either way.
pub fn classify(executable: &Path) -> InstallTarget {
    let bare = || InstallTarget::BareExecutable {
        executable: executable.to_path_buf(),
    };

    let Some(macos_dir) = executable.parent() else {
        return bare();
    };
    if macos_dir.file_name().and_then(|n| n.to_str()) != Some("MacOS") {
        return bare();
    }
    let Some(contents_dir) = macos_dir.parent() else {
        return bare();
    };
    if contents_dir.file_name().and_then(|n| n.to_str()) != Some("Contents") {
        return bare();
    }
    let Some(bundle) = contents_dir.parent() else {
        return bare();
    };
    let is_app = bundle
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|name| name.ends_with(".app"));
    if !is_app {
        return bare();
    }

    InstallTarget::MacosBundle {
        bundle: bundle.to_path_buf(),
        executable: executable.to_path_buf(),
    }
}

/// The staging directory an install extracts into: a hidden sibling of what is
/// being replaced, so the extraction and the final rename are on the **same
/// volume**. A staging directory in `/tmp` would make the swap a copy across
/// filesystems, which is neither atomic nor cheap for a 12 MB bundle.
///
/// The pid keeps two dodos updating at once from colliding.
pub fn staging_dir(parent: &Path, pid: u32) -> PathBuf {
    parent.join(format!(".dodo-update-{pid}"))
}

/// Where the running file is moved aside to. Windows cannot delete a running
/// executable but *can* rename it, and the same trick is the rollback path
/// everywhere else.
///
/// The suffix is what [`is_stale`] sweeps on the next launch.
pub const STALE_SUFFIX: &str = ".dodo-old";

/// The name a file is renamed to when it is moved out of the way.
pub fn stale_path(original: &Path) -> PathBuf {
    let mut name = original
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "dodo".to_owned());
    name.push_str(STALE_SUFFIX);
    original.with_file_name(name)
}

/// Whether a path is one of the moved-aside files a later launch should delete.
pub fn is_stale(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|name| name.ends_with(STALE_SUFFIX))
}

#[cfg(test)]
mod tests {
    use super::{InstallTarget, classify, is_stale, staging_dir, stale_path};
    use std::path::{Path, PathBuf};

    #[test]
    fn a_bundled_app_is_recognised_and_names_the_bundle_not_the_binary() {
        let exe = Path::new("/Applications/dodo.app/Contents/MacOS/dodo");
        assert_eq!(
            classify(exe),
            InstallTarget::MacosBundle {
                bundle: PathBuf::from("/Applications/dodo.app"),
                executable: exe.to_path_buf(),
            }
        );
    }

    #[test]
    fn the_writable_parent_is_where_the_sibling_gets_created() {
        let target = classify(Path::new("/Applications/dodo.app/Contents/MacOS/dodo"));
        assert_eq!(
            target.writable_parent(),
            Some(Path::new("/Applications")),
            "an in-place swap creates a sibling of the bundle, so it is /Applications \
             that has to be writable, not the bundle"
        );

        let bare = classify(Path::new("/usr/local/bin/dodo"));
        assert_eq!(bare.writable_parent(), Some(Path::new("/usr/local/bin")));
    }

    #[test]
    fn anything_that_is_not_the_bundle_layout_is_a_bare_executable() {
        for path in [
            "/usr/local/bin/dodo",
            "/home/someone/dodo",
            // The right leaf directories in the wrong shape.
            "/some/MacOS/dodo",
            "/some/Contents/MacOS/dodo",
            // A parent that is not a `.app`.
            "/some/dodo.zip/Contents/MacOS/dodo",
            // Windows.
            r"C:\Program Files\dodo\dodo.exe",
            "dodo",
        ] {
            assert!(
                matches!(
                    classify(Path::new(path)),
                    InstallTarget::BareExecutable { .. }
                ),
                "{path} must not be mistaken for a bundle"
            );
        }
    }

    /// The one that decides whether a `cargo run` build tries to swap something.
    #[test]
    fn a_development_build_is_a_bare_executable() {
        let exe = Path::new("/Users/someone/dodo/target/debug/dodo");
        assert!(matches!(
            classify(exe),
            InstallTarget::BareExecutable { .. }
        ));
    }

    #[test]
    fn a_bundle_anywhere_is_still_a_bundle() {
        for dir in ["/Applications", "/Users/someone/Applications", "/Volumes/x"] {
            let exe = PathBuf::from(dir).join("dodo.app/Contents/MacOS/dodo");
            assert!(
                matches!(classify(&exe), InstallTarget::MacosBundle { .. }),
                "{}",
                exe.display()
            );
        }
    }

    #[test]
    fn staging_is_a_sibling_so_the_rename_stays_on_one_volume() {
        let staging = staging_dir(Path::new("/Applications"), 4242);
        assert_eq!(staging, PathBuf::from("/Applications/.dodo-update-4242"));
        assert_eq!(
            staging.parent(),
            Some(Path::new("/Applications")),
            "extraction has to land on the same filesystem as the swap"
        );
    }

    #[test]
    fn two_processes_updating_at_once_do_not_share_a_staging_directory() {
        assert_ne!(
            staging_dir(Path::new("/Applications"), 1),
            staging_dir(Path::new("/Applications"), 2)
        );
    }

    #[test]
    fn a_moved_aside_file_is_recognisable_on_the_next_launch() {
        let old = stale_path(Path::new(r"C:\Program Files\dodo\dodo.exe"));
        assert!(
            old.to_string_lossy().ends_with("dodo.exe.dodo-old"),
            "{}",
            old.display()
        );
        assert!(is_stale(&old));
        assert!(!is_stale(Path::new(r"C:\Program Files\dodo\dodo.exe")));

        let bundle = stale_path(Path::new("/Applications/dodo.app"));
        assert_eq!(bundle, PathBuf::from("/Applications/dodo.app.dodo-old"));
        assert!(is_stale(&bundle));
    }

    #[test]
    fn moving_aside_keeps_the_file_in_its_own_directory() {
        let old = stale_path(Path::new("/Applications/dodo.app"));
        assert_eq!(
            old.parent(),
            Some(Path::new("/Applications")),
            "a cross-directory rename could cross a volume and stop being atomic"
        );
    }
}
