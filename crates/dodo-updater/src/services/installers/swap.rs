//! Putting the new thing where the old thing was, and taking it back if that
//! goes wrong.
//!
//! Shared by all three installers, because the sequence is the same everywhere
//! and only the *thing* differs — a `.app` bundle on macOS, an executable on
//! Windows and Linux.
//!
//! # How atomic this actually is
//!
//! Not entirely, and the honest description matters more than the word. The
//! swap is two renames:
//!
//! 1. `existing` → `existing.dodo-old`
//! 2. `replacement` → `existing`
//!
//! Each rename is atomic (both paths are on the same volume — that is what
//! [`staging_dir`](crate::models::install_target::staging_dir) is for),
//! but the *pair* is not: a crash between them leaves nothing at `existing`. So
//! step 2 rolls step 1 back on failure, and the window where neither exists is
//! the duration of one `rename(2)`.
//!
//! macOS does offer a genuinely atomic exchange — `renamex_np` with
//! `RENAME_SWAP` — and it is not used here because reaching it needs a direct
//! `libc` dependency for one call on one platform. That is the trade; it is
//! recorded rather than papered over with the word "atomic".
//!
//! # Why the old copy is renamed rather than deleted
//!
//! Windows cannot delete a running executable. It *can* rename one, which is
//! the whole trick: the running process keeps its handle to the same file under
//! its new name, the new binary takes the old path, and the stale file is swept
//! by a **later launch** ([`sweep`]). The same shape is used on macOS and Linux
//! even though they would tolerate a delete, because one code path is easier to
//! reason about than three.

use std::path::{Path, PathBuf};

use crate::models::install_target::{is_stale, stale_path};
use crate::models::state::{ManualReason, UpdateError};

/// Whether this process can create files in `dir`, and if not, why.
///
/// Probed by actually creating a file rather than by reading permission bits:
/// the bits do not account for ACLs, a read-only mount, macOS's
/// System Integrity Protection, or a sandbox, and the only question that
/// matters is whether the write will work.
pub fn probe_writable(dir: &Path) -> Result<(), ManualReason> {
    let probe = dir.join(format!(".dodo-write-probe-{}", std::process::id()));
    match std::fs::write(&probe, b"") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            Ok(())
        }
        Err(err) => Err(classify_probe_failure(err.kind())),
    }
}

/// Turns a failed write probe into the reason the user is shown.
///
/// Split out and tested as a pure function because the two cases lead to
/// different advice — "run the installer with permission" against "move dodo
/// off this volume" — and because a read-only filesystem does not reliably
/// report the same `ErrorKind` on every platform.
pub fn classify_probe_failure(kind: std::io::ErrorKind) -> ManualReason {
    match kind {
        std::io::ErrorKind::PermissionDenied => ManualReason::NotWritable,
        // EROFS, a missing directory, a sandbox denial: not a permissions
        // problem this user can fix by authenticating, so it is reported as a
        // location problem.
        _ => ManualReason::ReadOnlyLocation,
    }
}

/// Replaces `existing` with `replacement`, returning where the old copy went.
///
/// Both paths must be on the same volume. On failure the old copy is put back,
/// so a failed install leaves the installation exactly as it was.
pub fn swap(existing: &Path, replacement: &Path) -> Result<PathBuf, UpdateError> {
    let old = stale_path(existing);
    // A leftover from an earlier install would make the first rename fail on
    // Windows, where renaming onto an existing path is an error.
    remove_any(&old);

    let existed = existing.exists();
    if existed {
        std::fs::rename(existing, &old).map_err(|err| {
            UpdateError::Install(format!(
                "could not move the current version aside: {}: {err}",
                existing.display()
            ))
        })?;
    }

    if let Err(err) = std::fs::rename(replacement, existing) {
        // Put it back. If *this* fails there is nothing further to try, and the
        // error names the moved-aside copy so the user can restore it by hand.
        if existed && std::fs::rename(&old, existing).is_err() {
            return Err(UpdateError::Install(format!(
                "the update failed and the previous version could not be restored; \
                 it is at {}",
                old.display()
            )));
        }
        return Err(UpdateError::Install(format!(
            "could not move the new version into place: {}: {err}",
            existing.display()
        )));
    }

    Ok(old)
}

/// Deletes the copies previous installs renamed aside, in one directory.
///
/// Best effort throughout: a stale file that cannot be removed is not worth
/// reporting, and the next launch tries again.
pub fn sweep(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if is_stale(&path) {
            remove_any(&path);
        }
    }
}

/// Removes a path whether it is a file or a directory. A `.app` is a directory
/// and a `dodo.exe` is not, and the swap treats them identically.
fn remove_any(path: &Path) {
    if path.is_dir() {
        let _ = std::fs::remove_dir_all(path);
    } else {
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use super::{classify_probe_failure, probe_writable, swap, sweep};
    use crate::models::state::{ManualReason, UpdateError};
    use std::io::ErrorKind;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn scratch() -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("dodo-swap-test-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("creates");
        dir
    }

    #[test]
    fn a_swap_installs_the_new_copy_and_keeps_the_old_one_aside() {
        let dir = scratch();
        let installed = dir.join("dodo");
        let staged = dir.join("staging-dodo");
        std::fs::write(&installed, b"old").expect("writes");
        std::fs::write(&staged, b"new").expect("writes");

        let old = swap(&installed, &staged).expect("swaps");

        assert_eq!(std::fs::read(&installed).expect("installed"), b"new");
        assert_eq!(
            std::fs::read(&old).expect("kept aside"),
            b"old",
            "the previous version has to survive until a later launch sweeps it — \
             on Windows it cannot be deleted while it is running"
        );
        assert!(!staged.exists(), "the staged copy was moved, not copied");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_directory_swaps_the_same_way_a_file_does() {
        let dir = scratch();
        let installed = dir.join("dodo.app");
        let staged = dir.join("staging/dodo.app");
        std::fs::create_dir_all(installed.join("Contents")).expect("creates");
        std::fs::write(installed.join("Contents/marker"), b"old").expect("writes");
        std::fs::create_dir_all(staged.join("Contents")).expect("creates");
        std::fs::write(staged.join("Contents/marker"), b"new").expect("writes");

        swap(&installed, &staged).expect("swaps");
        assert_eq!(
            std::fs::read(installed.join("Contents/marker")).expect("installed"),
            b"new"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The failure that matters: if the new copy cannot be moved in, the old
    /// one has to come back. An install that half-happens is worse than one
    /// that does not happen.
    #[test]
    fn a_failed_second_rename_restores_the_previous_version() {
        let dir = scratch();
        let installed = dir.join("dodo");
        std::fs::write(&installed, b"old").expect("writes");
        let missing = dir.join("not-staged-at-all");

        let error = swap(&installed, &missing).expect_err("nothing to move in");
        assert!(matches!(error, UpdateError::Install(_)), "{error:?}");
        assert_eq!(
            std::fs::read(&installed).expect("restored"),
            b"old",
            "the installation must be exactly as it was"
        );
        assert!(
            !crate::models::install_target::stale_path(&installed).exists(),
            "and nothing left moved aside"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_leftover_from_an_earlier_install_does_not_block_a_new_one() {
        let dir = scratch();
        let installed = dir.join("dodo");
        let old = crate::models::install_target::stale_path(&installed);
        std::fs::write(&installed, b"current").expect("writes");
        std::fs::write(&old, b"from a previous install").expect("writes");
        let staged = dir.join("staged");
        std::fs::write(&staged, b"new").expect("writes");

        swap(&installed, &staged).expect("swaps over the leftover");
        assert_eq!(std::fs::read(&installed).expect("installed"), b"new");
        assert_eq!(std::fs::read(&old).expect("aside"), b"current");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sweeping_removes_only_the_moved_aside_copies() {
        let dir = scratch();
        std::fs::write(dir.join("dodo"), b"keep").expect("writes");
        std::fs::write(dir.join("dodo.dodo-old"), b"remove").expect("writes");
        std::fs::create_dir(dir.join("dodo.app.dodo-old")).expect("creates");
        std::fs::write(dir.join("dodo.app.dodo-old/x"), b"remove").expect("writes");

        sweep(&dir);

        assert!(dir.join("dodo").exists(), "the live copy stays");
        assert!(!dir.join("dodo.dodo-old").exists());
        assert!(
            !dir.join("dodo.app.dodo-old").exists(),
            "a bundle is a directory and has to be swept as one"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sweeping_a_directory_that_is_not_there_does_nothing() {
        sweep(std::path::Path::new("/nonexistent-dodo-sweep-test"));
    }

    #[test]
    fn a_writable_directory_probes_clean() {
        let dir = scratch();
        assert_eq!(probe_writable(&dir), Ok(()));
        assert_eq!(
            std::fs::read_dir(&dir).expect("readable").count(),
            0,
            "the probe must not leave its own file behind"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_directory_that_is_not_there_reports_a_location_problem() {
        assert_eq!(
            probe_writable(std::path::Path::new("/nonexistent-dodo-probe-test")),
            Err(ManualReason::ReadOnlyLocation)
        );
    }

    /// The two failures lead to different advice, so they must not collapse.
    #[test]
    fn a_permissions_failure_is_told_apart_from_a_read_only_location() {
        assert_eq!(
            classify_probe_failure(ErrorKind::PermissionDenied),
            ManualReason::NotWritable
        );
        for kind in [
            ErrorKind::NotFound,
            ErrorKind::Other,
            ErrorKind::InvalidInput,
        ] {
            assert_eq!(
                classify_probe_failure(kind),
                ManualReason::ReadOnlyLocation,
                "{kind:?}"
            );
        }
    }
}
