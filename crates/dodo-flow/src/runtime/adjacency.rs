//! [`AdjacencyIndex`] — **which edges touch this node, without looking at the
//! others** (§20).
//!
//! §40 rule 2 is one line — *never scan all edges to find a node's incident
//! edges* — and it is the rule that makes §19's target reachable. Everything
//! else in the dirty-propagation chain is already O(1); if this step were a
//! scan, moving one node in a 500,000-edge graph would cost 500,000
//! comparisons and the architecture would be decoration.
//!
//! # The representation, and the measurement behind it
//!
//! §20 asks for the representation to be **benchmarked rather than asserted**,
//! and its own sketch is `Vec<SmallVec<[EdgeIndex; 4]>>`.
//! `examples/flow_graph_bench.rs` builds §19's graph — 100,000 nodes, 500,000
//! edges — with this index and with the obvious `Vec<Vec<u32>>`, and prints
//! both. Measured on an Apple M1 in release, and reproducible with
//!
//! ```sh
//! cargo run --release -p dodo-flow --example flow_graph_bench --locked
//! ```
//!
//! | | `CompactList` (4 inline) | `Vec<Vec<u32>>` |
//! |---|---:|---:|
//! | build the index for 500,000 edges | **13.6 ms** | 35.3 ms |
//! | 100,000 `incident_edges` lookups | **0.44 ms** | 0.44 ms |
//! | index memory | 9.6 MB | 4.8 MB + 200,000 allocations |
//!
//! The lookup — the thing §20 actually specifies — is identical, because both
//! are one indexed load and a slice walk. The build is 2.6× apart, and the
//! reason is the allocation count: `Vec<Vec<_>>` mallocs once per node per
//! direction, 200,000 times, for lists whose *median* length is 4. The inline
//! form spends more resident bytes to make the common list free. Given that a
//! document is loaded far more often than it is exhaustively rebuilt, either
//! would have been defensible; the numbers are what chose, which is what §20
//! asked for.
//!
//! # Direction is kept, not derived
//!
//! `incoming` and `outgoing` are separate, so §4's connection limits and a
//! future "does this node have any output?" question are answered without
//! filtering. [`incident_edges`](AdjacencyIndex::incident_edges) walks both,
//! which is the call §19's propagation makes and the one §20 names.
//!
//! **This file names no UI framework.**

use crate::{
    models::{EdgeIndex, NodeIndex},
    runtime::CompactList,
};

/// Every node's incident edges, by direction.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AdjacencyIndex {
    incoming: Vec<CompactList>,
    outgoing: Vec<CompactList>,
}

impl AdjacencyIndex {
    pub fn new() -> AdjacencyIndex {
        AdjacencyIndex::default()
    }

    pub fn len(&self) -> usize {
        self.outgoing.len()
    }

    pub fn is_empty(&self) -> bool {
        self.outgoing.is_empty()
    }

    /// Adds an empty entry for a new node. Called by the store that pushed it,
    /// so the index cannot be shorter than the node store.
    pub fn push_node(&mut self) {
        self.incoming.push(CompactList::new());
        self.outgoing.push(CompactList::new());
    }

    pub fn reserve(&mut self, additional: usize) {
        self.incoming.reserve(additional);
        self.outgoing.reserve(additional);
    }

    /// Records that `edge` runs from `source` to `target`.
    ///
    /// A self-connection is recorded on both lists of the same node, which is
    /// what makes [`incident_edges`](AdjacencyIndex::incident_edges) report it
    /// twice — see [`degree`](AdjacencyIndex::degree) for why that is the
    /// honest answer rather than a bug.
    pub fn connect(&mut self, edge: EdgeIndex, source: NodeIndex, target: NodeIndex) {
        if let Some(list) = self.outgoing.get_mut(source.index()) {
            list.push(edge.raw());
        }
        if let Some(list) = self.incoming.get_mut(target.index()) {
            list.push(edge.raw());
        }
    }

    /// Removes `edge` from both ends. Used when an edge is reconnected: the old
    /// ends are disconnected and the new ones connected, so the index never
    /// holds an edge that no longer points at the node.
    pub fn disconnect(&mut self, edge: EdgeIndex, source: NodeIndex, target: NodeIndex) {
        if let Some(list) = self.outgoing.get_mut(source.index()) {
            list.remove(edge.raw());
        }
        if let Some(list) = self.incoming.get_mut(target.index()) {
            list.remove(edge.raw());
        }
    }

    pub fn outgoing(&self, node: NodeIndex) -> impl ExactSizeIterator<Item = EdgeIndex> + '_ {
        Self::edges(self.outgoing.get(node.index()))
    }

    pub fn incoming(&self, node: NodeIndex) -> impl ExactSizeIterator<Item = EdgeIndex> + '_ {
        Self::edges(self.incoming.get(node.index()))
    }

    /// **The call §20 specifies**, and the one §19's propagation makes: every
    /// edge attached to `node`, in time proportional to the node's degree and
    /// to nothing else.
    ///
    /// Outgoing first, then incoming, because that is the order the two lists
    /// are stored in and imposing any other would mean sorting.
    pub fn incident_edges(&self, node: NodeIndex) -> impl Iterator<Item = EdgeIndex> + '_ {
        self.outgoing(node).chain(self.incoming(node))
    }

    /// How many edge ends attach to this node.
    ///
    /// A self-connection counts **two**, because it is two ends on the same
    /// node and every consumer of this number — the geometry rebuild, the
    /// connection limit, the render invalidation — is counting ends. A "count
    /// of distinct edges" would be the wrong number in each of those places and
    /// would cost a deduplication pass to produce.
    pub fn degree(&self, node: NodeIndex) -> usize {
        let index = node.index();
        self.outgoing.get(index).map_or(0, CompactList::len)
            + self.incoming.get(index).map_or(0, CompactList::len)
    }

    fn edges(list: Option<&CompactList>) -> impl ExactSizeIterator<Item = EdgeIndex> + '_ {
        list.map(CompactList::as_slice)
            .unwrap_or(&[])
            .iter()
            .map(|&raw| EdgeIndex::new(raw))
    }
}

#[cfg(test)]
mod tests {
    use super::AdjacencyIndex;
    use crate::models::{EdgeIndex, NodeIndex};

    fn index(nodes: u32) -> AdjacencyIndex {
        let mut index = AdjacencyIndex::new();
        for _ in 0..nodes {
            index.push_node();
        }
        index
    }

    fn node(raw: u32) -> NodeIndex {
        NodeIndex::new(raw)
    }

    fn edge(raw: u32) -> EdgeIndex {
        EdgeIndex::new(raw)
    }

    #[test]
    fn a_fresh_index_reports_no_edges_anywhere() {
        let index = index(4);

        assert_eq!(index.degree(node(0)), 0);
        assert_eq!(index.incident_edges(node(0)).count(), 0);
    }

    #[test]
    fn an_edge_appears_on_the_source_out_list_and_the_target_in_list() {
        let mut index = index(3);
        index.connect(edge(0), node(0), node(1));

        assert_eq!(index.outgoing(node(0)).collect::<Vec<_>>(), vec![edge(0)]);
        assert_eq!(index.incoming(node(0)).count(), 0);
        assert_eq!(index.incoming(node(1)).collect::<Vec<_>>(), vec![edge(0)]);
        assert_eq!(index.outgoing(node(1)).count(), 0);
        assert_eq!(index.degree(node(2)), 0);
    }

    /// The property the dirty propagation is built on: a node's incident set is
    /// exactly the edges that name it, in either direction.
    #[test]
    fn incident_edges_reports_both_directions_and_only_this_node() {
        let mut index = index(4);
        index.connect(edge(0), node(0), node(1));
        index.connect(edge(1), node(2), node(1));
        index.connect(edge(2), node(1), node(3));
        index.connect(edge(3), node(2), node(3));

        let mut incident: Vec<_> = index.incident_edges(node(1)).collect();
        incident.sort_by_key(|e| e.raw());

        assert_eq!(incident, vec![edge(0), edge(1), edge(2)]);
        assert_eq!(index.degree(node(1)), 3);
    }

    /// A node whose degree outgrows the inline capacity must still report every
    /// edge — the spill path, exercised through the index rather than the list.
    #[test]
    fn a_high_degree_node_keeps_every_edge() {
        let mut index = index(2);
        for id in 0..50u32 {
            index.connect(edge(id), node(0), node(1));
        }

        assert_eq!(index.outgoing(node(0)).count(), 50);
        assert_eq!(index.incoming(node(1)).count(), 50);
        assert_eq!(index.degree(node(0)), 50);
        assert_eq!(
            index.incident_edges(node(1)).last(),
            Some(edge(49)),
            "order within a list is insertion order"
        );
    }

    #[test]
    fn a_self_connection_counts_as_two_ends() {
        let mut index = index(1);
        index.connect(edge(0), node(0), node(0));

        assert_eq!(index.degree(node(0)), 2);
        assert_eq!(
            index.incident_edges(node(0)).collect::<Vec<_>>(),
            vec![edge(0), edge(0)]
        );
    }

    #[test]
    fn disconnecting_removes_the_edge_from_both_ends_and_nothing_else() {
        let mut index = index(3);
        index.connect(edge(0), node(0), node(1));
        index.connect(edge(1), node(0), node(2));

        index.disconnect(edge(0), node(0), node(1));

        assert_eq!(index.outgoing(node(0)).collect::<Vec<_>>(), vec![edge(1)]);
        assert_eq!(index.incoming(node(1)).count(), 0);
        assert_eq!(index.incoming(node(2)).collect::<Vec<_>>(), vec![edge(1)]);
    }

    #[test]
    fn an_index_past_the_end_answers_empty_rather_than_panicking() {
        let index = index(1);

        assert_eq!(index.degree(node(99)), 0);
        assert_eq!(index.incident_edges(node(99)).count(), 0);
    }
}
