//! The "keep" / ignore list for orphan detection (Phase 10): paths the user
//! has told dodo to stop treating as orphan candidates.
//!
//! Pure data, no filesystem access — [`crate::services::ignore_store`]
//! is where this gets read from and written to disk. Kept in `core` rather
//! than beside the macOS-only orphan detector because nothing here needs GPUI
//! or macOS: the document is just a set of path strings the user marked
//! "Keep", and `core` is where every other GPUI-free, platform-independent
//! piece of Cleaner's domain model already lives (`item.rs`, `report.rs`,
//! `safety.rs`).
//!
//! # Why a path string, not a `CleanableItemId`
//!
//! [`crate::core::item::CleanableItemId`] is a hash computed fresh
//! every scan from a path seen in *that* scan. Nothing about it promises to
//! be stable across a restart, a different scan order, or a future hashing
//! change — it was never designed for that. A kept item has to survive a
//! restart and a rescan, so the ignore list keys on the one thing that
//! actually is stable: the item's own absolute path, as a string.
//!
//! # Versioning
//!
//! Same discipline as `script-consent.json`, `quick-nav.json` and
//! `updater.json`: an explicit `version` from the very first write, refused
//! if it is higher than this build understands — see
//! `crate::services::ignore_store::parse_document` for the parser
//! that enforces it. Deliberately not `collections.json`'s
//! `#[serde(default)]`-only style: a kept item is a decision the user made
//! about one specific path, and half-reading a newer file's shape could
//! silently un-ignore it.

use std::collections::BTreeSet;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// The schema version written into every `cleaner-ignored-items.json`.
pub const SCHEMA_VERSION: u32 = 1;

/// The persisted "keep" list: absolute paths the user has excluded from
/// future orphan review.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IgnoredItemsDocument {
    /// Written first and read first. See the module doc.
    pub version: u32,
    #[serde(default)]
    pub ignored_paths: BTreeSet<String>,
}

impl Default for IgnoredItemsDocument {
    fn default() -> Self {
        Self {
            version: SCHEMA_VERSION,
            ignored_paths: BTreeSet::new(),
        }
    }
}

impl IgnoredItemsDocument {
    /// Whether `path` has been marked "Keep".
    pub fn is_ignored(&self, path: &Path) -> bool {
        self.ignored_paths.contains(&path_signature(path))
    }

    /// Adds `path` to the keep list. A no-op if it is already there.
    pub fn keep(&mut self, path: &Path) {
        self.ignored_paths.insert(path_signature(path));
    }
}

/// The stable signature one path is keyed by: its string form, as
/// [`Path::to_string_lossy`] renders it. A free function — rather than
/// inlining `.to_string_lossy()` at every call site — so [`IgnoredItemsDocument::is_ignored`]
/// and [`IgnoredItemsDocument::keep`] are provably using the same key.
pub fn path_signature(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{IgnoredItemsDocument, SCHEMA_VERSION, path_signature};

    #[test]
    fn a_fresh_document_has_the_current_version_and_nothing_ignored() {
        let document = IgnoredItemsDocument::default();
        assert_eq!(document.version, SCHEMA_VERSION);
        assert!(document.ignored_paths.is_empty());
    }

    #[test]
    fn keeping_a_path_makes_it_ignored_and_is_idempotent() {
        let mut document = IgnoredItemsDocument::default();
        let path = Path::new("/Users/someone/Library/Caches/Orphan");
        assert!(!document.is_ignored(path));

        document.keep(path);
        assert!(document.is_ignored(path));
        assert_eq!(document.ignored_paths.len(), 1);

        document.keep(path);
        assert_eq!(
            document.ignored_paths.len(),
            1,
            "keeping the same path twice must not duplicate the entry"
        );
    }

    #[test]
    fn a_different_path_is_unaffected() {
        let mut document = IgnoredItemsDocument::default();
        document.keep(Path::new("/Users/someone/Library/Caches/A"));
        assert!(!document.is_ignored(Path::new("/Users/someone/Library/Caches/B")));
    }

    #[test]
    fn the_signature_is_just_the_path_string() {
        let path = Path::new("/tmp/example");
        assert_eq!(path_signature(path), "/tmp/example");
    }
}
