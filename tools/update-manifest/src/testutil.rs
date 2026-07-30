//! A throwaway directory for tests.
//!
//! `tempfile` would do this in one line, but the crate's dependency list is
//! deliberately four entries long and a dev-dependency is still a dependency to
//! audit and lock. This is the whole feature that was needed.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// A directory under the system temp dir, removed when it goes out of scope.
pub struct TempDir {
    path: PathBuf,
}

impl TempDir {
    pub fn new(label: &str) -> TempDir {
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "update-manifest-{label}-{}-{unique}",
            std::process::id()
        ));
        // A leftover from a killed run would make the test lie about what it
        // found, so start from empty.
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("create temp dir");
        TempDir { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Writes `contents` to `name` inside the directory and returns its path.
    pub fn write(&self, name: &str, contents: &[u8]) -> PathBuf {
        let path = self.path.join(name);
        std::fs::write(&path, contents).expect("write temp file");
        path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        // Best effort: a failure here must not mask the test's own result.
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
