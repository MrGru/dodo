//! [`SelectionSet`] — §28's selection, as compact runtime ids and nothing else.
//!
//! §28 is one sentence and one prohibition: *"Selection should use compact
//! runtime state such as bitsets/sets of runtime IDs"*, and *"avoid cloning
//! complete element objects into selection state"*. This holds both, and the
//! second is worth spelling out — a box selection over the dense scene selects
//! about 2,000 nodes, and a selection that cloned each one would copy their
//! kinds, labels, styles and handle lists on every rubber-band frame.
//!
//! # Both a bitset and a list, because both questions are hot
//!
//! ```text
//! bits   Vec<u64>       "is this element selected?"  — asked once per painted element
//! order  Vec<Index>     "what is selected?"          — asked once per command
//! ```
//!
//! The painter asks the first for every element it draws, so it must be O(1)
//! and cache-friendly rather than a hash lookup. A command — move the
//! selection, delete it, restyle it — asks the second, and iterating a bitset
//! to answer it would be proportional to the *document* rather than to the
//! selection. Keeping both costs one `u32` per selected element and a bit per
//! element in the document.
//!
//! **The list is unordered.** Deselecting swaps the last entry into the hole,
//! because a selection is a set and paying O(n) per deselect to preserve an
//! order nobody reads would be the wrong trade.
//!
//! # The flags in the stores are still the truth for painting
//!
//! `NodeFlags::SELECTED` already exists and the painter already reads it.
//! [`GraphWorld`](crate::runtime::GraphWorld) writes both — the flag and this
//! set — from one place, so they cannot drift; nothing outside it should write
//! either.
//!
//! **This file names no UI framework.**

use crate::{
    geometry::Rect,
    models::{EdgeIndex, NodeIndex},
};

/// A dense bitset over runtime indices. Grows to fit; never shrinks, because a
/// selection that has been large once will be again.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct BitSet {
    words: Vec<u64>,
}

impl BitSet {
    fn contains(&self, index: u32) -> bool {
        let word = index as usize / 64;
        self.words
            .get(word)
            .is_some_and(|bits| bits & (1 << (index % 64)) != 0)
    }

    /// Sets the bit and reports whether it was previously clear.
    fn insert(&mut self, index: u32) -> bool {
        let word = index as usize / 64;
        if word >= self.words.len() {
            self.words.resize(word + 1, 0);
        }
        let mask = 1u64 << (index % 64);
        let was_clear = self.words[word] & mask == 0;
        self.words[word] |= mask;
        was_clear
    }

    /// Clears the bit and reports whether it was previously set.
    fn remove(&mut self, index: u32) -> bool {
        let word = index as usize / 64;
        let Some(bits) = self.words.get_mut(word) else {
            return false;
        };
        let mask = 1u64 << (index % 64);
        let was_set = *bits & mask != 0;
        *bits &= !mask;
        was_set
    }

    /// Empties the set **keeping the words allocated**, which is what makes a
    /// rubber band that repeatedly replaces its selection allocation-free
    /// (§40 rule 13).
    fn clear(&mut self) {
        self.words.iter_mut().for_each(|word| *word = 0);
    }

    fn memory_bytes(&self) -> usize {
        self.words.capacity() * size_of::<u64>()
    }
}

/// What is selected, as node and edge indices.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SelectionSet {
    node_bits: BitSet,
    nodes: Vec<NodeIndex>,
    edge_bits: BitSet,
    edges: Vec<EdgeIndex>,
}

impl SelectionSet {
    pub fn new() -> SelectionSet {
        SelectionSet::default()
    }

    /// **O(1)**, and the reason the bitset exists — the painter asks this for
    /// every element it draws.
    pub fn contains_node(&self, node: NodeIndex) -> bool {
        self.node_bits.contains(node.raw())
    }

    pub fn contains_edge(&self, edge: EdgeIndex) -> bool {
        self.edge_bits.contains(edge.raw())
    }

    /// The selected nodes, **in no particular order** — see the module doc.
    pub fn nodes(&self) -> &[NodeIndex] {
        &self.nodes
    }

    pub fn edges(&self) -> &[EdgeIndex] {
        &self.edges
    }

    pub fn len(&self) -> usize {
        self.nodes.len() + self.edges.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty() && self.edges.is_empty()
    }

    /// The single selected node, or `None` if nothing or several are selected.
    /// The question a single-selection inspector asks.
    pub fn single_node(&self) -> Option<NodeIndex> {
        match self.nodes.as_slice() {
            [node] if self.edges.is_empty() => Some(*node),
            _ => None,
        }
    }

    /// Adds a node, reporting whether it was not already selected.
    pub fn insert_node(&mut self, node: NodeIndex) -> bool {
        let added = self.node_bits.insert(node.raw());
        if added {
            self.nodes.push(node);
        }
        added
    }

    pub fn insert_edge(&mut self, edge: EdgeIndex) -> bool {
        let added = self.edge_bits.insert(edge.raw());
        if added {
            self.edges.push(edge);
        }
        added
    }

    /// Removes a node, reporting whether it had been selected.
    pub fn remove_node(&mut self, node: NodeIndex) -> bool {
        if !self.node_bits.remove(node.raw()) {
            return false;
        }
        if let Some(at) = self.nodes.iter().position(|other| *other == node) {
            self.nodes.swap_remove(at);
        }
        true
    }

    pub fn remove_edge(&mut self, edge: EdgeIndex) -> bool {
        if !self.edge_bits.remove(edge.raw()) {
            return false;
        }
        if let Some(at) = self.edges.iter().position(|other| *other == edge) {
            self.edges.swap_remove(at);
        }
        true
    }

    /// Empties the set, keeping every allocation.
    pub fn clear(&mut self) {
        self.node_bits.clear();
        self.nodes.clear();
        self.edge_bits.clear();
        self.edges.clear();
    }

    /// The set's own heap footprint, in bytes (§41).
    pub fn memory_bytes(&self) -> usize {
        self.node_bits.memory_bytes()
            + self.edge_bits.memory_bytes()
            + self.nodes.capacity() * size_of::<NodeIndex>()
            + self.edges.capacity() * size_of::<EdgeIndex>()
    }
}

/// What "inside the rubber band" means (§28).
///
/// Both modes exist in every editor that has a rubber band, and which one is
/// right is a product decision rather than a geometric one — so it is a
/// parameter of the resolve call rather than a constant here. [`Touch`] is the
/// default because it is what React Flow and Excalidraw both do: a band that
/// only selects fully enclosed elements makes selecting a long edge nearly
/// impossible.
///
/// [`Touch`]: BoxSelectMode::Touch
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BoxSelectMode {
    /// Any overlap selects. An edge crossing the band is selected even though
    /// both its ends are outside.
    #[default]
    Touch,
    /// Only elements entirely inside the band are selected.
    Enclose,
}

/// One rubber band, fully described.
///
/// A struct rather than four parameters because
/// [`GraphWorld::apply_box_selection`](crate::runtime::GraphWorld::apply_box_selection)
/// already takes two candidate iterators, and a call with six positional
/// arguments is a call whose `bool` nobody can read.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoxQuery {
    /// The band, in **world** units. Normalised on use, so either drag
    /// direction behaves the same.
    pub rect: Rect,
    pub mode: BoxSelectMode,
    /// Shift-drag: add to the selection instead of replacing it.
    pub additive: bool,
    /// How finely an edge's curve is flattened before it is tested, in
    /// **world** units.
    ///
    /// World rather than screen, because that is the space the test happens
    /// in — the caller converts, because the caller is the one that knows the
    /// zoom. [`BoxQuery::at_zoom`] is that conversion.
    pub tolerance: f32,
}

impl BoxQuery {
    /// A band whose edge tolerance is one screen pixel at this zoom.
    ///
    /// A rubber band is aimed by eye, so its precision is a screen-space
    /// question: a pixel of flattening error is exactly at the limit of what
    /// the person dragging it could have meant.
    pub fn at_zoom(rect: Rect, zoom: f32) -> BoxQuery {
        BoxQuery {
            rect,
            mode: BoxSelectMode::default(),
            additive: false,
            tolerance: if zoom.is_finite() && zoom > 0.0 {
                (1.0 / zoom).clamp(1e-3, 1e4)
            } else {
                1.0
            },
        }
    }

    pub fn additive(mut self, additive: bool) -> BoxQuery {
        self.additive = additive;
        self
    }

    pub fn with_mode(mut self, mode: BoxSelectMode) -> BoxQuery {
        self.mode = mode;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_set_is_empty() {
        let selection = SelectionSet::new();

        assert!(selection.is_empty());
        assert_eq!(selection.len(), 0);
        assert!(!selection.contains_node(NodeIndex::new(0)));
        assert_eq!(selection.single_node(), None);
    }

    #[test]
    fn inserting_twice_selects_once() {
        let mut selection = SelectionSet::new();
        let node = NodeIndex::new(7);

        assert!(selection.insert_node(node));
        assert!(!selection.insert_node(node));
        assert_eq!(selection.nodes(), &[node]);
        assert_eq!(selection.len(), 1);
    }

    #[test]
    fn removal_keeps_the_bitset_and_the_list_agreeing() {
        let mut selection = SelectionSet::new();
        for index in 0..8u32 {
            selection.insert_node(NodeIndex::new(index));
        }

        assert!(selection.remove_node(NodeIndex::new(3)));
        assert!(!selection.remove_node(NodeIndex::new(3)));

        assert_eq!(selection.nodes().len(), 7);
        assert!(!selection.contains_node(NodeIndex::new(3)));
        for index in [0, 1, 2, 4, 5, 6, 7] {
            assert!(selection.contains_node(NodeIndex::new(index)));
            assert!(selection.nodes().contains(&NodeIndex::new(index)));
        }
    }

    #[test]
    fn nodes_and_edges_are_independent() {
        let mut selection = SelectionSet::new();
        selection.insert_node(NodeIndex::new(2));
        selection.insert_edge(EdgeIndex::new(2));

        assert!(selection.contains_node(NodeIndex::new(2)));
        assert!(selection.contains_edge(EdgeIndex::new(2)));
        assert_eq!(selection.len(), 2);
        assert_eq!(
            selection.single_node(),
            None,
            "a node plus an edge is not a single-node selection"
        );

        selection.remove_edge(EdgeIndex::new(2));
        assert_eq!(selection.single_node(), Some(NodeIndex::new(2)));
    }

    #[test]
    fn a_far_index_does_not_grow_the_set_past_its_word() {
        let mut selection = SelectionSet::new();
        selection.insert_node(NodeIndex::new(1_000_000));

        assert!(selection.contains_node(NodeIndex::new(1_000_000)));
        assert!(!selection.contains_node(NodeIndex::new(999_999)));
        assert_eq!(selection.nodes().len(), 1);
    }

    /// The property a rubber band depends on: replacing the selection sixty
    /// times a second must not reallocate.
    #[test]
    fn clearing_keeps_the_allocations() {
        let mut selection = SelectionSet::new();
        for index in 0..5_000u32 {
            selection.insert_node(NodeIndex::new(index));
        }
        let bytes = selection.memory_bytes();

        selection.clear();
        assert!(selection.is_empty());
        assert!(!selection.contains_node(NodeIndex::new(4_999)));
        assert_eq!(selection.memory_bytes(), bytes);
    }

    #[test]
    fn removing_something_never_selected_is_a_no_op() {
        let mut selection = SelectionSet::new();
        assert!(!selection.remove_node(NodeIndex::new(9)));
        assert!(!selection.remove_edge(EdgeIndex::new(9)));
        assert!(selection.is_empty());
    }

    #[test]
    fn the_default_box_mode_is_touch() {
        assert_eq!(BoxSelectMode::default(), BoxSelectMode::Touch);
    }

    #[test]
    fn a_band_at_a_deeper_zoom_is_flattened_more_finely() {
        let rect = Rect::new(
            crate::geometry::Vec2::ZERO,
            crate::geometry::Vec2::splat(10.0),
        );

        let far = BoxQuery::at_zoom(rect, 0.1);
        let close = BoxQuery::at_zoom(rect, 10.0);
        assert!(close.tolerance < far.tolerance);

        // A degenerate zoom must not produce an infinite or zero tolerance,
        // which would flatten forever or not at all.
        let broken = BoxQuery::at_zoom(rect, 0.0);
        assert!(broken.tolerance.is_finite() && broken.tolerance > 0.0);
        assert!(BoxQuery::at_zoom(rect, f32::NAN).tolerance.is_finite());
    }

    #[test]
    fn the_builders_do_what_they_say() {
        let query = BoxQuery::at_zoom(Rect::ZERO, 1.0)
            .additive(true)
            .with_mode(BoxSelectMode::Enclose);

        assert!(query.additive);
        assert_eq!(query.mode, BoxSelectMode::Enclose);
    }
}
