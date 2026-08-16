//! Identity, in two forms that must never be confused.
//!
//! Requirements §4 asks for compact runtime IDs in hot paths and allows
//! external serialized IDs to stay strings or UUIDs, resolved to runtime IDs on
//! load. §31 forbids serializing runtime indices. This file is where both rules
//! are made mechanical rather than remembered:
//!
//! | | serialized | stable across a load | what it is |
//! |---|---|---|---|
//! | [`ElementId`] | yes | yes | the document's name for an element |
//! | [`NodeIndex`], [`EdgeIndex`], [`HandleIndex`] | **no** | **no** | a slot in a runtime store |
//!
//! **The indices deliberately do not derive `Serialize` or `Deserialize`.** A
//! persisted struct that reaches for one therefore fails to compile, which is a
//! better guard than a code review: an index is a position in a `Vec` that
//! the runtime stores are free to compact, so writing one to disk records a fact
//! that stops being true the moment an element is deleted.
//!
//! **`ElementId` is a `u64`, not a UUID.** dodo is deliberate about every
//! package in its graph (`deny.toml`, `THIRD-PARTY-NOTICES.md`), and a `uuid`
//! dependency buys collision-freedom across documents that nothing in this
//! design needs — ids are unique within one document, and
//! [`IdAllocator`] is serialized alongside them so a reopened document never
//! reissues one. Merging two documents is a remap, which a UUID would not have
//! saved us from anyway, because the *positions* still collide.

use std::fmt;

use serde::{Deserialize, Serialize};

/// The document's stable name for one element — a node, an edge, a shape, a
/// frame. Unique within one [`FlowDocument`](crate::models::FlowDocument) and
/// meaningless outside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ElementId(u64);

impl ElementId {
    /// The id no allocator ever hands out, for a placeholder or a sentinel.
    pub const NONE: ElementId = ElementId(0);

    pub const fn new(raw: u64) -> ElementId {
        ElementId(raw)
    }

    pub const fn raw(self) -> u64 {
        self.0
    }
}

impl fmt::Display for ElementId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Hands out fresh [`ElementId`]s, monotonically and never reusing one.
///
/// Serialized with the document precisely so that ids are not reused across a
/// save/load cycle: an undo history, a clipboard or another window may still be
/// holding the id of a deleted element, and reissuing it would silently make
/// that stale reference point at a different element.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct IdAllocator {
    next: u64,
}

impl Default for IdAllocator {
    fn default() -> IdAllocator {
        // Starts past `ElementId::NONE` so the sentinel is never issued.
        IdAllocator { next: 1 }
    }
}

impl IdAllocator {
    pub fn next_id(&mut self) -> ElementId {
        let id = ElementId(self.next);
        self.next += 1;
        id
    }

    /// Guarantees the next id issued is greater than `id`.
    ///
    /// Called after loading or after pasting elements that carry their own ids:
    /// a document written by a future version, or a hand-edited one, may hold
    /// ids above the watermark, and without this the allocator would hand out
    /// duplicates.
    pub fn observe(&mut self, id: ElementId) {
        self.next = self.next.max(id.0 + 1);
    }
}

/// A handle's name *within its node*. Requirements §4 asks for unique handle
/// ids and for hidden handles that stay geometrically connectable, so an edge
/// endpoint names a node plus, optionally, one of its handles.
///
/// A `String` rather than an interned symbol: §41 warns about duplicate strings
/// and this is the obvious candidate, but interning is only worth it once
/// profiling says so, and the handle names in a real document ("in", "out",
/// "error") are short enough that the `String` header dominates either way.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct HandleId(String);

impl HandleId {
    pub fn new(name: impl Into<String>) -> HandleId {
        HandleId(name.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for HandleId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Declares the compact runtime index newtypes. They are identical apart from
/// the doc comment, and writing them out three times invites the drift that a
/// macro cannot have.
macro_rules! runtime_index {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        ///
        /// A `u32` slot number in a runtime store, per requirements §41.
        /// **Not serializable, by design** — see this module's doc.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(u32);

        impl $name {
            pub const fn new(raw: u32) -> $name {
                $name(raw)
            }

            pub const fn raw(self) -> u32 {
                self.0
            }

            /// As a `usize`, for indexing the store it names.
            pub const fn index(self) -> usize {
                self.0 as usize
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }
    };
}

runtime_index!(
    /// A node's slot in the runtime `NodeStore`.
    NodeIndex
);
runtime_index!(
    /// An edge's slot in the runtime `EdgeStore`.
    EdgeIndex
);
runtime_index!(
    /// A handle's slot in the runtime handle table. Flat across the whole world
    /// rather than per node, so an edge endpoint resolves to one `u32` in the
    /// hot path instead of a node index plus a string lookup.
    HandleIndex
);

#[cfg(test)]
mod tests {
    use super::{EdgeIndex, ElementId, HandleId, HandleIndex, IdAllocator, NodeIndex};

    #[test]
    fn ids_are_issued_monotonically_and_never_reused() {
        let mut alloc = IdAllocator::default();

        let a = alloc.next_id();
        let b = alloc.next_id();
        let c = alloc.next_id();

        assert!(a < b && b < c);
        assert_ne!(a, ElementId::NONE);
    }

    #[test]
    fn observe_lifts_the_watermark_past_a_pasted_or_loaded_id() {
        let mut alloc = IdAllocator::default();

        alloc.observe(ElementId::new(500));
        assert_eq!(alloc.next_id(), ElementId::new(501));

        // A lower id must not lower the watermark.
        alloc.observe(ElementId::new(3));
        assert_eq!(alloc.next_id(), ElementId::new(502));
    }

    #[test]
    fn runtime_indices_convert_without_surprise() {
        assert_eq!(NodeIndex::new(7).index(), 7);
        assert_eq!(EdgeIndex::new(0).raw(), 0);
        assert_eq!(HandleIndex::new(u32::MAX).index(), u32::MAX as usize);
    }

    #[test]
    fn ids_and_handle_ids_display_as_their_content() {
        assert_eq!(ElementId::new(42).to_string(), "42");
        assert_eq!(HandleId::new("out").to_string(), "out");
        assert_eq!(HandleId::new("out").as_str(), "out");
    }
}
