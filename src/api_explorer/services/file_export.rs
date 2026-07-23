//! Writing a chosen response body out to a file the user picked.
//!
//! Kept in `services` so that no view does raw disk IO itself. The path comes
//! from the platform save dialog (`App::prompt_for_new_path`); this only writes
//! the bytes, on the background executor, never on the UI thread.

use std::path::Path;

/// Writes `bytes` to `path`, returning the platform error text on failure so the
/// caller can surface it rather than unwrapping.
pub fn write_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    std::fs::write(path, bytes).map_err(|err| format!("{}: {err}", path.display()))
}
