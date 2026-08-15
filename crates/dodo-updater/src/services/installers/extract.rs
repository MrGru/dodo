//! Unpacking a verified archive, with the system's own tools.
//!
//! # Why `tar`, and not a crate
//!
//! `flate2` + `tar` + `zip` is three dependencies and a few hundred kilobytes on
//! a binary whose size is measured per round
//! (`docs/build-optimization.md`), to do something every one of dodo's three
//! platforms already ships a tool for. `tar` reads both formats dodo publishes:
//! it auto-detects gzip, and the `bsdtar` that is `tar.exe` on Windows 10 1803+
//! reads `.zip` as well. PowerShell's `Expand-Archive` is the fallback for an
//! older Windows.
//!
//! # This is not "executing the download"
//!
//! The rule is that **a downloaded file is never executed**, and it is not: the
//! program that runs is the operating system's `tar`, and the archive is its
//! *input*. Nothing extracted is run until the user presses Restart, and then
//! only what the swap put in place.

use std::path::Path;
use std::process::Command;

use crate::models::state::UpdateError;

/// Extracts `archive` into `into`, creating it.
///
/// The directory is created empty: a stale extraction from an abandoned attempt
/// must not be mistaken for this one's output.
pub fn extract(archive: &Path, into: &Path) -> Result<(), UpdateError> {
    let _ = std::fs::remove_dir_all(into);
    std::fs::create_dir_all(into)
        .map_err(|err| UpdateError::Io(format!("{}: {err}", into.display())))?;

    match run_tar(archive, into) {
        Ok(()) => Ok(()),
        Err(tar_error) => {
            // Only a zip has a second reader worth trying; a `.tar.gz` that tar
            // could not read is not going to fare better in PowerShell.
            if !is_zip(archive) {
                return Err(tar_error);
            }
            run_expand_archive(archive, into).map_err(|expand_error| {
                UpdateError::Install(format!(
                    "{}; {}",
                    detail_of(&tar_error),
                    detail_of(&expand_error)
                ))
            })
        }
    }
}

fn is_zip(archive: &Path) -> bool {
    archive
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("zip"))
}

/// `tar -xf <archive> -C <dir>`. `-x` extracts, `-f` names the file, and the
/// compression is auto-detected — stating `-z` would break the `.zip` case.
fn run_tar(archive: &Path, into: &Path) -> Result<(), UpdateError> {
    let output = Command::new("tar")
        .arg("-xf")
        .arg(archive)
        .arg("-C")
        .arg(into)
        .output()
        .map_err(|err| UpdateError::Install(format!("tar: {err}")))?;

    if output.status.success() {
        return Ok(());
    }
    Err(UpdateError::Install(format!(
        "tar: {}",
        first_line(&output.stderr)
    )))
}

/// The Windows fallback, for a build old enough not to ship `tar.exe`.
fn run_expand_archive(archive: &Path, into: &Path) -> Result<(), UpdateError> {
    let output = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command"])
        .arg(format!(
            "Expand-Archive -LiteralPath '{}' -DestinationPath '{}' -Force",
            archive.display(),
            into.display()
        ))
        .output()
        .map_err(|err| UpdateError::Install(format!("Expand-Archive: {err}")))?;

    if output.status.success() {
        return Ok(());
    }
    Err(UpdateError::Install(format!(
        "Expand-Archive: {}",
        first_line(&output.stderr)
    )))
}

/// A subprocess's first line of stderr. One line, because these end up in a
/// dialog: `tar`'s multi-line complaints say the same thing five times.
fn first_line(stderr: &[u8]) -> String {
    String::from_utf8_lossy(stderr)
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("no output")
        .to_owned()
}

fn detail_of(error: &UpdateError) -> String {
    match error {
        UpdateError::Install(detail) | UpdateError::Io(detail) => detail.clone(),
        other => format!("{other:?}"),
    }
}

/// The single directory entry an extraction produced whose name ends with
/// `suffix`, or `None` when there is not exactly one.
///
/// "Exactly one" is the point: an archive that unpacked into two `.app`s, or
/// none, is not one this build understands, and picking the first would be
/// picking at random.
pub fn sole_entry_ending_with(dir: &Path, suffix: &str) -> Option<std::path::PathBuf> {
    let mut found = None;
    for entry in std::fs::read_dir(dir).ok()? {
        let path = entry.ok()?.path();
        let matches = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with(suffix));
        if !matches {
            continue;
        }
        if found.is_some() {
            return None;
        }
        found = Some(path);
    }
    found
}

#[cfg(test)]
mod tests {
    use super::{extract, first_line, is_zip, sole_entry_ending_with};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    fn scratch() -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("dodo-extract-test-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("creates");
        dir
    }

    /// A real round trip through the system's `tar`, in the same shape a
    /// release archive has: one top-level directory holding the payload.
    #[test]
    fn a_tar_gz_round_trips_through_the_system_tar() {
        let dir = scratch();
        let source = dir.join("src");
        std::fs::create_dir_all(source.join("dodo.app/Contents/MacOS")).expect("creates");
        std::fs::write(source.join("dodo.app/Contents/MacOS/dodo"), b"binary").expect("writes");

        let archive = dir.join("dodo.tar.gz");
        let packed = std::process::Command::new("tar")
            .arg("-czf")
            .arg(&archive)
            .arg("-C")
            .arg(&source)
            .arg("dodo.app")
            .status()
            .expect("tar runs on every platform dodo builds for");
        assert!(packed.success(), "could not build the fixture archive");

        let into = dir.join("staging");
        extract(&archive, &into).expect("extracts");
        assert_eq!(
            std::fs::read(into.join("dodo.app/Contents/MacOS/dodo")).expect("extracted"),
            b"binary"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn extracting_clears_whatever_was_staged_before() {
        let dir = scratch();
        let source = dir.join("src");
        std::fs::create_dir_all(&source).expect("creates");
        std::fs::write(source.join("new"), b"new").expect("writes");

        let archive = dir.join("a.tar.gz");
        assert!(
            std::process::Command::new("tar")
                .arg("-czf")
                .arg(&archive)
                .arg("-C")
                .arg(&source)
                .arg("new")
                .status()
                .expect("tar")
                .success()
        );

        let into = dir.join("staging");
        std::fs::create_dir_all(&into).expect("creates");
        std::fs::write(into.join("leftover"), b"stale").expect("writes");

        extract(&archive, &into).expect("extracts");
        assert!(into.join("new").exists());
        assert!(
            !into.join("leftover").exists(),
            "a stale extraction must not be mistaken for this one's output"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_archive_tar_cannot_read_is_an_error_not_a_panic() {
        let dir = scratch();
        let archive = dir.join("broken.tar.gz");
        std::fs::write(&archive, b"this is not an archive").expect("writes");

        let error = extract(&archive, &dir.join("staging")).expect_err("not an archive");
        assert!(
            format!("{error:?}").contains("tar"),
            "the error should name the tool that refused it: {error:?}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn exactly_one_matching_entry_is_required() {
        let dir = scratch();
        assert_eq!(sole_entry_ending_with(&dir, ".app"), None, "none");

        std::fs::create_dir(dir.join("dodo.app")).expect("creates");
        assert_eq!(
            sole_entry_ending_with(&dir, ".app"),
            Some(dir.join("dodo.app"))
        );

        std::fs::create_dir(dir.join("other.app")).expect("creates");
        assert_eq!(
            sole_entry_ending_with(&dir, ".app"),
            None,
            "two candidates means picking one at random, which is not a choice \
             an installer gets to make"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_directory_yields_nothing_rather_than_panicking() {
        assert_eq!(
            sole_entry_ending_with(Path::new("/nonexistent-dodo-test-dir"), ".app"),
            None
        );
    }

    #[test]
    fn only_a_zip_gets_the_powershell_fallback() {
        assert!(is_zip(Path::new("dodo-v0.1.6-windows-x64.zip")));
        assert!(is_zip(Path::new("DODO.ZIP")));
        assert!(!is_zip(Path::new("dodo-v0.1.6-macos-arm64-app.tar.gz")));
        assert!(!is_zip(Path::new("dodo")));
    }

    #[test]
    fn subprocess_errors_are_reduced_to_one_line() {
        assert_eq!(
            first_line(b"\n  first thing\nsecond thing\n"),
            "first thing"
        );
        assert_eq!(first_line(b""), "no output");
        assert_eq!(first_line(b"   \n\n"), "no output");
    }
}
