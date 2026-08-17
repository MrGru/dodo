//! [`CompactList`] — a short list of `u32` runtime indices that does not
//! allocate for the common case.
//!
//! Requirements §20's own sketch of the adjacency index is
//! `Vec<SmallVec<[EdgeIndex; 4]>>`, and this is that shape without the
//! dependency. dodo is deliberate about every package in its graph
//! (`deny.toml`, `THIRD-PARTY-NOTICES.md`) and this phase's brief pins
//! `Cargo.lock`, so the four inline slots are written out here instead —
//! forty lines against a new crate in the tree.
//!
//! # Why four, and why inline at all
//!
//! §20 also says the representation must be **benchmarked rather than
//! asserted**, and `examples/flow_graph_bench.rs` is where that happens.
//! [`crate::runtime::adjacency`]'s module doc holds the measured table and what
//! it changed; the short version is that the inline slots are worth 2.7× on
//! build time and 200,000 fewer allocations **on a realistically sparse graph**,
//! are worth nothing at all at §19's stress density where every list spills
//! anyway, and make no difference to the walk in either case.
//!
//! **This file names no UI framework.**

/// How many indices fit before the list spills to the heap. §20's own example
/// says four; the benchmark says four is enough for the shapes real graphs
/// have (a node's degree is small even when the graph is enormous).
pub const INLINE_CAPACITY: usize = 4;

/// A list of `u32` runtime indices, inline up to [`INLINE_CAPACITY`].
///
/// Deliberately not generic over the index newtype. `NodeIndex`, `EdgeIndex`
/// and `HandleIndex` are all `u32` underneath, and one concrete type means one
/// copy of the code and one set of tests; the newtype is restored at the API
/// boundary by whoever owns the list.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CompactList {
    inline: [u32; INLINE_CAPACITY],
    /// Total length, inline and spilled together.
    len: u32,
    /// Empty until the list outgrows [`INLINE_CAPACITY`], at which point it
    /// holds **every** element — the inline array is abandoned rather than
    /// used as a prefix, so `as_slice` is one branch and never a chain.
    spill: Vec<u32>,
}

impl CompactList {
    pub fn new() -> CompactList {
        CompactList::default()
    }

    pub fn len(&self) -> usize {
        self.len as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn as_slice(&self) -> &[u32] {
        if self.spill.is_empty() {
            &self.inline[..self.len as usize]
        } else {
            &self.spill
        }
    }

    pub fn iter(&self) -> std::slice::Iter<'_, u32> {
        self.as_slice().iter()
    }

    pub fn contains(&self, value: u32) -> bool {
        self.as_slice().contains(&value)
    }

    pub fn push(&mut self, value: u32) {
        let len = self.len as usize;

        if !self.spill.is_empty() {
            self.spill.push(value);
        } else if len < INLINE_CAPACITY {
            self.inline[len] = value;
        } else {
            // The one allocation this type exists to postpone. Reserving the
            // doubled capacity here rather than letting `Vec` grow from one
            // keeps the spill path from re-allocating immediately after.
            self.spill.reserve(INLINE_CAPACITY * 2);
            self.spill.extend_from_slice(&self.inline);
            self.spill.push(value);
        }

        self.len += 1;
    }

    /// Removes the first occurrence of `value`, preserving order, and says
    /// whether it was there.
    ///
    /// Order is preserved rather than swap-removed because these lists are what
    /// an edge's paint order and a node's handle order come from, and a
    /// swap-remove would reshuffle a node's handles when an unrelated one was
    /// deleted.
    pub fn remove(&mut self, value: u32) -> bool {
        let Some(at) = self.as_slice().iter().position(|&v| v == value) else {
            return false;
        };

        if self.spill.is_empty() {
            let len = self.len as usize;
            self.inline.copy_within(at + 1..len, at);
            self.inline[len - 1] = 0;
        } else {
            self.spill.remove(at);
        }

        self.len -= 1;
        true
    }

    pub fn clear(&mut self) {
        self.inline = [0; INLINE_CAPACITY];
        self.spill.clear();
        self.len = 0;
    }
}

impl<'a> IntoIterator for &'a CompactList {
    type Item = &'a u32;
    type IntoIter = std::slice::Iter<'a, u32>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl FromIterator<u32> for CompactList {
    fn from_iter<I: IntoIterator<Item = u32>>(iter: I) -> CompactList {
        let mut list = CompactList::new();
        for value in iter {
            list.push(value);
        }
        list
    }
}

#[cfg(test)]
mod tests {
    use super::{CompactList, INLINE_CAPACITY};

    #[test]
    fn an_empty_list_allocates_nothing_and_reads_as_empty() {
        let list = CompactList::new();

        assert!(list.is_empty());
        assert_eq!(list.len(), 0);
        assert_eq!(list.as_slice(), &[] as &[u32]);
        assert_eq!(
            list.spill.capacity(),
            0,
            "the inline case must not allocate"
        );
    }

    #[test]
    fn the_inline_capacity_is_reached_before_anything_is_allocated() {
        let mut list = CompactList::new();
        for value in 0..INLINE_CAPACITY as u32 {
            list.push(value);
        }

        assert_eq!(list.as_slice(), &[0, 1, 2, 3]);
        assert_eq!(list.spill.capacity(), 0);
    }

    /// The spill takes the whole list rather than only the overflow — the
    /// property `as_slice` depends on for being one branch.
    #[test]
    fn spilling_carries_every_earlier_element_across() {
        let mut list = CompactList::new();
        for value in 0..9u32 {
            list.push(value);
        }

        assert_eq!(list.len(), 9);
        assert_eq!(list.as_slice(), &[0, 1, 2, 3, 4, 5, 6, 7, 8]);
    }

    #[test]
    fn remove_preserves_order_inline_and_spilled() {
        let mut inline: CompactList = (0..4u32).collect();
        assert!(inline.remove(1));
        assert_eq!(inline.as_slice(), &[0, 2, 3]);
        assert!(!inline.remove(1));

        let mut spilled: CompactList = (0..7u32).collect();
        assert!(spilled.remove(0));
        assert_eq!(spilled.as_slice(), &[1, 2, 3, 4, 5, 6]);
    }

    /// Removing back down below the inline capacity leaves the list on the
    /// spill. That is deliberate — a list that shrank once usually grows again,
    /// and shrinking back would trade a real allocation for a saved one.
    #[test]
    fn a_shrunk_list_stays_correct_however_it_is_stored() {
        let mut list: CompactList = (0..6u32).collect();
        for value in 0..5u32 {
            assert!(list.remove(value));
        }

        assert_eq!(list.len(), 1);
        assert_eq!(list.as_slice(), &[5]);
        assert!(list.contains(5));
    }

    #[test]
    fn clear_empties_both_representations() {
        let mut list: CompactList = (0..9u32).collect();
        list.clear();

        assert!(list.is_empty());
        assert_eq!(list.as_slice(), &[] as &[u32]);
    }
}
