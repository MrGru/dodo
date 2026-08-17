//! [`DirtyState`] — **the mechanism the whole architecture exists to protect**
//! (§19).
//!
//! §19 states the target as a worked example rather than as a principle, and it
//! is worth restating here because every decision in this file follows from it:
//!
//! ```text
//! 100,000 nodes, 500,000 edges — move one node with four connected edges:
//!   one node updated, four edge geometry rebuilds, one spatial update,
//!   minimal render invalidation.
//! ```
//!
//! Nothing in that list is proportional to the size of the graph, so nothing
//! here may be either. Two consequences shape the type:
//!
//! - **A flag array is not enough.** Per-element flags answer "is this dirty?"
//!   in O(1), but finding *which* elements are dirty from flags alone is a scan
//!   over all of them — the thing §40 rule 1 forbids, arrived at from the other
//!   direction. So every flag array is paired with a **queue** of the indices
//!   that were touched, and consumers iterate the queue.
//! - **The queue must not grow with repeated marking.** Dragging a node emits a
//!   move per mouse event, and each one marks the same four edges. An index is
//!   pushed only on the transition from clean to dirty, which is why
//!   [`DirtyState::mark_edge`] reads the old flags before writing the new ones.
//!
//! # Bitflags without `bitflags`
//!
//! §19's example uses the `bitflags` crate. This phase's brief pins
//! `Cargo.lock`, and the whole of what dodo needs from it is a `u16` newtype
//! with `|`, `&` and a `contains` — written out below for both element kinds.
//! The crate can be adopted later without changing a call site.
//!
//! # The spatial queue is a seam, not an implementation
//!
//! [`DirtyState::spatial`] collects the nodes whose spatial-index entry is
//! stale. Phase 4's uniform grid drains it; until then it is the honest record
//! of "one spatial update" from §19's example, and the property test counts it.
//! Writing a spatial index here to have something to drain it with would be
//! exactly the placeholder Phase 2 declined to write for culling.
//!
//! **This file names no UI framework.**

use crate::models::{EdgeIndex, NodeIndex};

/// Declares one bitflag newtype over a `u16`. Two of these differ only in their
/// constants, and writing the operators out twice invites the drift a macro
/// cannot have.
macro_rules! dirty_flags {
    (
        $(#[$meta:meta])*
        $name:ident { $($(#[$flag_meta:meta])* $flag:ident = $bit:expr;)* }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
        pub struct $name(u16);

        impl $name {
            /// Nothing to do.
            pub const NONE: $name = $name(0);

            $($(#[$flag_meta])* pub const $flag: $name = $name(1 << $bit);)*

            pub const fn bits(self) -> u16 {
                self.0
            }

            pub const fn is_empty(self) -> bool {
                self.0 == 0
            }

            /// Every bit of `other` is set here. `NONE` is contained by
            /// everything, which is the usual bitset convention.
            pub const fn contains(self, other: $name) -> bool {
                self.0 & other.0 == other.0
            }

            /// Any bit of `other` is set here — the question a consumer that
            /// handles several flags the same way actually asks.
            pub const fn intersects(self, other: $name) -> bool {
                self.0 & other.0 != 0
            }

            /// Every flag this one does not hold. `flags & x.complement()`
            /// clears exactly `x`.
            pub const fn complement(self) -> $name {
                $name(!self.0)
            }
        }

        impl std::ops::BitOr for $name {
            type Output = $name;

            fn bitor(self, rhs: $name) -> $name {
                $name(self.0 | rhs.0)
            }
        }

        impl std::ops::BitOrAssign for $name {
            fn bitor_assign(&mut self, rhs: $name) {
                self.0 |= rhs.0;
            }
        }

        impl std::ops::BitAnd for $name {
            type Output = $name;

            fn bitand(self, rhs: $name) -> $name {
                $name(self.0 & rhs.0)
            }
        }
    };
}

dirty_flags!(
    /// What changed about a node since the last time anything looked (§19).
    NodeDirty {
        /// The node moved. Its own render transform and every incident edge's
        /// geometry follow from this.
        POSITION = 0;
        SIZE = 1;
        STYLE = 2;
        /// A handle was added, removed or moved along its edge.
        HANDLES = 3;
        TEXT = 4;
        /// The node's spatial-index entry is stale. Set alongside `POSITION`
        /// and `SIZE`, and queued separately — see the module doc.
        SPATIAL = 5;
    }
);

dirty_flags!(
    /// What changed about an edge (§19's "equivalent dirty state ... for other
    /// geometry-bearing elements").
    EdgeDirty {
        /// The derived route has to be rebuilt: an endpoint moved, a handle
        /// moved, or the routing style changed. **The flag the §19 property
        /// test counts.**
        GEOMETRY = 0;
        STYLE = 1;
        LABEL = 2;
        /// The edge was reconnected to a different node or handle. Implies
        /// `GEOMETRY`, but says why, which the adjacency index needs and the
        /// geometry does not.
        ENDPOINTS = 3;
    }
);

/// Which elements changed, and what changed about them.
///
/// One instance lives on the [`GraphWorld`](crate::runtime::GraphWorld). It is
/// sized with the stores and grows with them; the queues are cleared by
/// draining and **keep their capacity**, because a drag drains them sixty times
/// a second (§40 rule 14).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DirtyState {
    node_flags: Vec<NodeDirty>,
    edge_flags: Vec<EdgeDirty>,
    dirty_nodes: Vec<NodeIndex>,
    dirty_edges: Vec<EdgeIndex>,
    spatial: Vec<NodeIndex>,
}

impl DirtyState {
    pub fn new() -> DirtyState {
        DirtyState::default()
    }

    /// Adds a clean slot for a new node. Called by the store, so the flag array
    /// and the store cannot get out of step.
    pub fn push_node(&mut self) {
        self.node_flags.push(NodeDirty::NONE);
    }

    pub fn push_edge(&mut self) {
        self.edge_flags.push(EdgeDirty::NONE);
    }

    pub fn node_flags(&self, node: NodeIndex) -> NodeDirty {
        self.node_flags
            .get(node.index())
            .copied()
            .unwrap_or(NodeDirty::NONE)
    }

    pub fn edge_flags(&self, edge: EdgeIndex) -> EdgeDirty {
        self.edge_flags
            .get(edge.index())
            .copied()
            .unwrap_or(EdgeDirty::NONE)
    }

    /// **Marks a node, queueing it exactly once.** See the module doc for why
    /// the transition rather than the write is what queues.
    pub fn mark_node(&mut self, node: NodeIndex, flags: NodeDirty) {
        let Some(slot) = self.node_flags.get_mut(node.index()) else {
            return;
        };

        let before = *slot;
        *slot = before | flags;

        if before.is_empty() && !slot.is_empty() {
            self.dirty_nodes.push(node);
        }
        if !before.contains(NodeDirty::SPATIAL) && flags.contains(NodeDirty::SPATIAL) {
            self.spatial.push(node);
        }
    }

    /// Marks an edge, queueing it exactly once.
    pub fn mark_edge(&mut self, edge: EdgeIndex, flags: EdgeDirty) {
        let Some(slot) = self.edge_flags.get_mut(edge.index()) else {
            return;
        };

        let before = *slot;
        *slot = before | flags;

        if before.is_empty() && !slot.is_empty() {
            self.dirty_edges.push(edge);
        }
    }

    pub fn dirty_nodes(&self) -> &[NodeIndex] {
        &self.dirty_nodes
    }

    pub fn dirty_edges(&self) -> &[EdgeIndex] {
        &self.dirty_edges
    }

    /// The nodes whose spatial entry Phase 4's index will have to reinsert.
    pub fn spatial_updates(&self) -> &[NodeIndex] {
        &self.spatial
    }

    pub fn is_clean(&self) -> bool {
        self.dirty_nodes.is_empty() && self.dirty_edges.is_empty() && self.spatial.is_empty()
    }

    /// Hands the dirty-edge queue to a caller that needs `&mut` on the rest of
    /// the world while draining it, leaving an empty queue behind.
    ///
    /// The awkwardness is deliberate and is paid back by
    /// [`restore_edge_queue`](DirtyState::restore_edge_queue): the buffer goes
    /// out and comes back, so a drag that drains this on every mouse move
    /// allocates nothing after the first one.
    pub fn take_edge_queue(&mut self) -> Vec<EdgeIndex> {
        std::mem::take(&mut self.dirty_edges)
    }

    /// Returns the buffer [`take_edge_queue`](DirtyState::take_edge_queue)
    /// handed out, cleared and with its capacity intact.
    pub fn restore_edge_queue(&mut self, mut queue: Vec<EdgeIndex>) {
        queue.clear();
        // Anything marked while the queue was out went into the fresh `Vec`
        // that `take` left behind; keeping whichever buffer is larger means the
        // capacity survives either way.
        if queue.capacity() >= self.dirty_edges.capacity() && self.dirty_edges.is_empty() {
            self.dirty_edges = queue;
        }
    }

    /// Clears one edge's flags and returns what they were.
    pub fn clear_edge(&mut self, edge: EdgeIndex) -> EdgeDirty {
        match self.edge_flags.get_mut(edge.index()) {
            Some(slot) => std::mem::replace(slot, EdgeDirty::NONE),
            None => EdgeDirty::NONE,
        }
    }

    /// Clears one node's flags and returns what they were.
    pub fn clear_node(&mut self, node: NodeIndex) -> NodeDirty {
        match self.node_flags.get_mut(node.index()) {
            Some(slot) => std::mem::replace(slot, NodeDirty::NONE),
            None => NodeDirty::NONE,
        }
    }

    /// Drops the node queue, clearing every flag it named.
    ///
    /// The render layer calls this once it has consumed the invalidations; the
    /// spatial queue is **not** touched, because Phase 4's index drains that on
    /// its own schedule.
    pub fn clear_dirty_nodes(&mut self) {
        let mut queue = std::mem::take(&mut self.dirty_nodes);
        for node in &queue {
            if let Some(slot) = self.node_flags.get_mut(node.index()) {
                // `SPATIAL` outlives the render invalidation: it is owned by the
                // spatial queue, which has its own drain.
                *slot = *slot & NodeDirty::SPATIAL;
            }
        }
        queue.clear();
        self.dirty_nodes = queue;
    }

    /// Drops the spatial queue, clearing the flag on every node in it.
    pub fn clear_spatial_updates(&mut self) {
        for node in std::mem::take(&mut self.spatial) {
            if let Some(slot) = self.node_flags.get_mut(node.index()) {
                *slot = *slot & NodeDirty::SPATIAL.complement();
            }
        }
    }

    /// Everything clean, queues emptied, capacities kept.
    pub fn clear_all(&mut self) {
        for slot in &mut self.node_flags {
            *slot = NodeDirty::NONE;
        }
        for slot in &mut self.edge_flags {
            *slot = EdgeDirty::NONE;
        }
        self.dirty_nodes.clear();
        self.dirty_edges.clear();
        self.spatial.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::{DirtyState, EdgeDirty, NodeDirty};
    use crate::models::{EdgeIndex, NodeIndex};

    fn state(nodes: usize, edges: usize) -> DirtyState {
        let mut dirty = DirtyState::new();
        for _ in 0..nodes {
            dirty.push_node();
        }
        for _ in 0..edges {
            dirty.push_edge();
        }
        dirty
    }

    #[test]
    fn a_fresh_state_is_clean() {
        let dirty = state(4, 4);

        assert!(dirty.is_clean());
        assert!(dirty.node_flags(NodeIndex::new(0)).is_empty());
        assert!(dirty.edge_flags(EdgeIndex::new(3)).is_empty());
    }

    #[test]
    fn flags_accumulate_rather_than_replace() {
        let mut dirty = state(1, 0);
        let node = NodeIndex::new(0);

        dirty.mark_node(node, NodeDirty::POSITION);
        dirty.mark_node(node, NodeDirty::STYLE);

        let flags = dirty.node_flags(node);
        assert!(flags.contains(NodeDirty::POSITION));
        assert!(flags.contains(NodeDirty::STYLE));
        assert!(!flags.contains(NodeDirty::SIZE));
        assert!(flags.intersects(NodeDirty::SIZE | NodeDirty::STYLE));
    }

    /// **The property a drag depends on.** Sixty marks of the same node are one
    /// queue entry, or the queue would grow without bound over one gesture.
    #[test]
    fn marking_the_same_element_repeatedly_queues_it_once() {
        let mut dirty = state(1, 1);
        let node = NodeIndex::new(0);
        let edge = EdgeIndex::new(0);

        for _ in 0..60 {
            dirty.mark_node(node, NodeDirty::POSITION | NodeDirty::SPATIAL);
            dirty.mark_edge(edge, EdgeDirty::GEOMETRY);
        }

        assert_eq!(dirty.dirty_nodes(), &[node]);
        assert_eq!(dirty.dirty_edges(), &[edge]);
        assert_eq!(dirty.spatial_updates(), &[node]);
    }

    #[test]
    fn the_spatial_queue_only_collects_nodes_that_asked_for_it() {
        let mut dirty = state(3, 0);

        dirty.mark_node(NodeIndex::new(0), NodeDirty::POSITION | NodeDirty::SPATIAL);
        dirty.mark_node(NodeIndex::new(1), NodeDirty::STYLE);
        dirty.mark_node(NodeIndex::new(2), NodeDirty::SPATIAL);

        assert_eq!(
            dirty.spatial_updates(),
            &[NodeIndex::new(0), NodeIndex::new(2)]
        );
    }

    #[test]
    fn clearing_render_invalidation_leaves_the_spatial_queue_alone() {
        let mut dirty = state(1, 0);
        let node = NodeIndex::new(0);
        dirty.mark_node(node, NodeDirty::POSITION | NodeDirty::SPATIAL);

        dirty.clear_dirty_nodes();

        assert!(dirty.dirty_nodes().is_empty());
        assert_eq!(dirty.spatial_updates(), &[node]);
        assert!(dirty.node_flags(node).contains(NodeDirty::SPATIAL));
        assert!(!dirty.node_flags(node).contains(NodeDirty::POSITION));

        dirty.clear_spatial_updates();
        assert!(dirty.is_clean());
        assert!(dirty.node_flags(node).is_empty());
    }

    #[test]
    fn the_edge_queue_comes_back_with_its_capacity() {
        let mut dirty = state(0, 8);
        for edge in 0..8u32 {
            dirty.mark_edge(EdgeIndex::new(edge), EdgeDirty::GEOMETRY);
        }

        let queue = dirty.take_edge_queue();
        let capacity = queue.capacity();
        assert_eq!(queue.len(), 8);
        assert!(dirty.dirty_edges().is_empty());

        dirty.restore_edge_queue(queue);

        assert!(dirty.dirty_edges().is_empty());
        assert!(dirty.take_edge_queue().capacity() >= capacity);
    }

    #[test]
    fn clearing_an_edge_reports_what_it_was() {
        let mut dirty = state(0, 1);
        let edge = EdgeIndex::new(0);
        dirty.mark_edge(edge, EdgeDirty::GEOMETRY | EdgeDirty::ENDPOINTS);

        let flags = dirty.clear_edge(edge);

        assert!(flags.contains(EdgeDirty::GEOMETRY));
        assert!(flags.contains(EdgeDirty::ENDPOINTS));
        assert!(dirty.edge_flags(edge).is_empty());
    }

    #[test]
    fn marking_an_index_past_the_end_is_ignored_rather_than_a_panic() {
        let mut dirty = state(1, 1);

        dirty.mark_node(NodeIndex::new(99), NodeDirty::POSITION);
        dirty.mark_edge(EdgeIndex::new(99), EdgeDirty::GEOMETRY);

        assert!(dirty.is_clean());
    }
}
