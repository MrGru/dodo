//! The Collections panel's data.
//!
//! Phase 1 ships this empty and the panel renders its empty state. It exists
//! now — rather than the panel hard-coding "there is nothing here" — so that
//! phase 3 fills the tree and the view is unchanged.

/// A saved collection of requests.
///
/// Phase 3 gives this children and an import path; phase 1 only ever holds
/// zero of them, which is what the panel branches on.
pub struct Collection {
    pub name: String,
}

#[derive(Default)]
pub struct CollectionState {
    collections: Vec<Collection>,
}

impl CollectionState {
    /// Whether the panel shows its empty state or a tree.
    pub fn is_empty(&self) -> bool {
        self.collections.is_empty()
    }

    /// The collections to list. Empty in phase 1.
    pub fn all(&self) -> &[Collection] {
        &self.collections
    }
}
