//! The Collections panel's runtime state.
//!
//! The tree itself is plain data ([`CollectionTree`], unit tested without
//! GPUI); this wraps it with the two things the panel needs at runtime but the
//! model has no business holding: the last persistence/import error to show,
//! and nothing else. The search text, the rename field and which row's action
//! menu is open are transient view state and live on the page.

use crate::api_explorer::models::collection::CollectionTree;
use crate::i18n::Str;

#[derive(Default)]
pub struct CollectionState {
    tree: CollectionTree,
    /// The last error from loading, saving or importing collections, shown as a
    /// calm line under the header until the next successful edit clears it.
    error: Option<Str>,
}

impl CollectionState {
    pub fn tree(&self) -> &CollectionTree {
        &self.tree
    }

    pub fn tree_mut(&mut self) -> &mut CollectionTree {
        &mut self.tree
    }

    /// Replaces the whole tree — used when the initial disk load completes.
    pub fn set_tree(&mut self, tree: CollectionTree) {
        self.tree = tree;
    }

    pub fn is_empty(&self) -> bool {
        self.tree.is_empty()
    }

    pub fn error(&self) -> Option<Str> {
        self.error.clone()
    }

    pub fn set_error(&mut self, error: Option<Str>) {
        self.error = error;
    }
}
