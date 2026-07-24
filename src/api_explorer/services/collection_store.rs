//! Where the Collections tree lives between sessions.
//!
//! # Why a trait
//!
//! The same reason [`Transport`](crate::api_explorer::services::Transport) is a
//! trait: the state layer holds an `Arc<dyn CollectionStore>` and never learns
//! whether its collections are on disk, in memory, or somewhere else later. The
//! app runs on [`DiskCollectionStore`]; a unit test runs on
//! [`InMemoryCollectionStore`] with the exact same calls.
//!
//! # Threading
//!
//! Both methods perform blocking file IO and are **blocking by contract**, the
//! same as `Transport::execute`. Every caller runs them on GPUI's background
//! executor, never on the UI thread.

use std::path::PathBuf;
use std::sync::Mutex;

use crate::api_explorer::models::collection::Node;
use crate::i18n::Str;

/// A persistence failure, in terms the UI can show.
///
/// The underlying `std::io` / `serde_json` message is third-party English and
/// is kept verbatim inside a translated frame — the same convention the
/// transport errors follow.
#[derive(Debug)]
pub struct StoreError {
    detail: String,
}

impl StoreError {
    fn new(detail: String) -> Self {
        Self { detail }
    }

    /// The message shown when saving or loading collections fails.
    pub fn message(&self) -> Str {
        Str::CollectionStoreError(self.detail.clone())
    }
}

/// A place the Collections tree is loaded from and saved to.
pub trait CollectionStore: Send + Sync + 'static {
    /// The saved tree, or an empty forest when nothing has been saved yet.
    fn load(&self) -> Result<Vec<Node>, StoreError>;

    /// Replaces the saved tree with `roots`.
    fn persist(&self, roots: &[Node]) -> Result<(), StoreError>;
}

/// The app's config directory for user data, created on first save.
///
/// macOS keeps per-app data under `~/Library/Application Support`; this is the
/// first location dodo persists anything, so the directory is dodo's to make.
/// If `$HOME` is somehow unset, a relative fallback keeps the app working
/// rather than panicking.
pub fn data_dir() -> PathBuf {
    match std::env::var_os("HOME") {
        Some(home) => PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("dodo"),
        None => PathBuf::from(".dodo"),
    }
}

/// The Collections tree, stored as one JSON file under [`data_dir`].
pub struct DiskCollectionStore {
    path: PathBuf,
}

impl Default for DiskCollectionStore {
    fn default() -> Self {
        Self {
            path: data_dir().join("collections.json"),
        }
    }
}

impl DiskCollectionStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// A store backed by a specific file, for tests.
    #[cfg(test)]
    pub fn at(path: PathBuf) -> Self {
        Self { path }
    }
}

impl CollectionStore for DiskCollectionStore {
    fn load(&self) -> Result<Vec<Node>, StoreError> {
        match std::fs::read(&self.path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|err| StoreError::new(format!("{}: {err}", self.path.display()))),
            // A missing file is the ordinary first-run state, not an error.
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(err) => Err(StoreError::new(format!("{}: {err}", self.path.display()))),
        }
    }

    fn persist(&self, roots: &[Node]) -> Result<(), StoreError> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)
                .map_err(|err| StoreError::new(format!("{}: {err}", dir.display())))?;
        }
        let json =
            serde_json::to_vec_pretty(roots).map_err(|err| StoreError::new(err.to_string()))?;

        // Write to a sibling temp file and rename over the target, so a crash
        // mid-write leaves the previous save intact rather than a half file.
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, &json)
            .map_err(|err| StoreError::new(format!("{}: {err}", tmp.display())))?;
        std::fs::rename(&tmp, &self.path)
            .map_err(|err| StoreError::new(format!("{}: {err}", self.path.display())))?;
        Ok(())
    }
}

/// A store that keeps the tree in memory only. Used by tests, and available as
/// the session-only fallback behind the same trait — the app wires up the
/// disk-backed store, so this is not constructed in the shipping path.
#[derive(Default)]
#[allow(dead_code)]
pub struct InMemoryCollectionStore {
    roots: Mutex<Vec<Node>>,
}

impl CollectionStore for InMemoryCollectionStore {
    fn load(&self) -> Result<Vec<Node>, StoreError> {
        Ok(self
            .roots
            .lock()
            .map(|roots| roots.clone())
            .unwrap_or_default())
    }

    fn persist(&self, roots: &[Node]) -> Result<(), StoreError> {
        if let Ok(mut held) = self.roots.lock() {
            *held = roots.to_vec();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{CollectionStore, DiskCollectionStore, InMemoryCollectionStore};
    use crate::api_explorer::models::collection::CollectionTree;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn temp_path() -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        std::env::temp_dir().join(format!("dodo-store-test-{pid}-{n}/collections.json"))
    }

    fn tree_with_a_collection() -> CollectionTree {
        let mut tree = CollectionTree::default();
        let c = tree.add_collection("APIs".into());
        tree.add_request(c, "Ping".into(), Default::default())
            .expect("request");
        tree
    }

    #[test]
    fn loading_a_missing_file_is_an_empty_forest_not_an_error() {
        let store = DiskCollectionStore::at(temp_path());
        assert!(store.load().expect("no error on first run").is_empty());
    }

    #[test]
    fn what_is_persisted_to_disk_is_loaded_back() {
        let path = temp_path();
        let tree = tree_with_a_collection();

        let store = DiskCollectionStore::at(path.clone());
        store.persist(tree.roots()).expect("persists");

        // A brand new store at the same path — i.e. the next app launch.
        let reopened = DiskCollectionStore::at(path.clone());
        let loaded = reopened.load().expect("loads");
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name, "APIs");
        assert_eq!(loaded[0].children.len(), 1);

        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn the_in_memory_store_round_trips_too() {
        let store = InMemoryCollectionStore::default();
        let tree = tree_with_a_collection();
        store.persist(tree.roots()).expect("persists");
        assert_eq!(store.load().expect("loads")[0].name, "APIs");
    }
}
