//! [`EdgeStore`] — the **logical** connections, and nothing about their shape
//! (§8).
//!
//! §8's last line is the one this file is built around:
//!
//! > The data model must separate the logical edge from derived path geometry.
//!
//! So an edge here is two resolved endpoints, a routing style and its paint
//! properties — sixteen bytes of hot data. Where it actually runs lives in
//! [`EdgeGeometry`](crate::runtime::EdgeGeometry), is derived from the endpoint
//! nodes' current positions, and is rebuilt when
//! [`EdgeDirty::GEOMETRY`](crate::runtime::EdgeDirty::GEOMETRY) says so. That
//! separation is what makes §19's target reachable at all: moving a node
//! invalidates four *derived* routes, and touches no edge record.
//!
//! # An endpoint is two `u32`s
//!
//! [`Endpoint`](crate::models::Endpoint) — the document's form — is an
//! [`ElementId`] plus an `Option<HandleId>`, and a `HandleId` is a `String`.
//! Resolving that per frame would be §40 rule 9 in the hot path, so
//! [`EdgeEnd`] holds the runtime indices and [`OptionalHandle`] packs the
//! "attached to the node itself" case into the same four bytes rather than
//! doubling the width with an `Option`.
//!
//! **This file names no UI framework.**

use std::sync::Arc;

use crate::models::{EdgeIndex, EdgeRouting, ElementId, ElementStyle, HandleIndex, NodeIndex};

/// A [`HandleIndex`] or nothing, in four bytes.
///
/// `Option<HandleIndex>` would be eight — `HandleIndex` is a plain `u32` with
/// no niche — and this sits in an array with one entry per edge endpoint, which
/// is two per edge. At §19's 500,000 edges that is 4 MB of padding avoided for
/// a type with one method.
///
/// [`OptionalHandle::NONE`] is §4's whole-node connection mode: the edge
/// attaches to the node itself and the router picks a point on its border.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OptionalHandle(u32);

impl OptionalHandle {
    pub const NONE: OptionalHandle = OptionalHandle(u32::MAX);

    pub const fn some(handle: HandleIndex) -> OptionalHandle {
        OptionalHandle(handle.raw())
    }

    pub const fn get(self) -> Option<HandleIndex> {
        if self.0 == u32::MAX {
            None
        } else {
            Some(HandleIndex::new(self.0))
        }
    }

    pub const fn is_none(self) -> bool {
        self.0 == u32::MAX
    }
}

impl From<Option<HandleIndex>> for OptionalHandle {
    fn from(handle: Option<HandleIndex>) -> OptionalHandle {
        match handle {
            Some(handle) => OptionalHandle::some(handle),
            None => OptionalHandle::NONE,
        }
    }
}

/// One end of an edge, resolved to runtime indices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EdgeEnd {
    pub node: NodeIndex,
    pub handle: OptionalHandle,
}

impl EdgeEnd {
    /// Attached to the node itself — §4's whole-node connection mode.
    pub const fn node(node: NodeIndex) -> EdgeEnd {
        EdgeEnd {
            node,
            handle: OptionalHandle::NONE,
        }
    }

    pub const fn handle(node: NodeIndex, handle: HandleIndex) -> EdgeEnd {
        EdgeEnd {
            node,
            handle: OptionalHandle::some(handle),
        }
    }
}

/// Boolean properties of an edge, packed (§41).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct EdgeFlags(u8);

impl EdgeFlags {
    pub const NONE: EdgeFlags = EdgeFlags(0);
    pub const HIDDEN: EdgeFlags = EdgeFlags(1 << 0);
    pub const SELECTED: EdgeFlags = EdgeFlags(1 << 1);
    /// Deleted, as a tombstone — the same decision and the same reasons as
    /// [`NodeFlags::REMOVED`](crate::runtime::NodeFlags::REMOVED), whose module
    /// doc records them. §30's *disconnect* is this flag on an edge.
    pub const REMOVED: EdgeFlags = EdgeFlags(1 << 2);

    pub const fn contains(self, other: EdgeFlags) -> bool {
        self.0 & other.0 == other.0
    }

    pub const fn complement(self) -> EdgeFlags {
        EdgeFlags(!self.0)
    }
}

impl std::ops::BitOr for EdgeFlags {
    type Output = EdgeFlags;

    fn bitor(self, rhs: EdgeFlags) -> EdgeFlags {
        EdgeFlags(self.0 | rhs.0)
    }
}

impl std::ops::BitAnd for EdgeFlags {
    type Output = EdgeFlags;

    fn bitand(self, rhs: EdgeFlags) -> EdgeFlags {
        EdgeFlags(self.0 & rhs.0)
    }
}

/// What an edge needs to exist.
#[derive(Debug, Clone, PartialEq)]
pub struct EdgeSpec {
    pub id: ElementId,
    pub source: EdgeEnd,
    pub target: EdgeEnd,
    pub routing: EdgeRouting,
    pub style: ElementStyle,
    pub label: Option<String>,
    pub link: Option<String>,
    pub z: i32,
    pub hidden: bool,
}

impl EdgeSpec {
    pub fn new(id: ElementId, source: EdgeEnd, target: EdgeEnd) -> EdgeSpec {
        EdgeSpec {
            id,
            source,
            target,
            routing: EdgeRouting::default(),
            style: ElementStyle::default(),
            label: None,
            link: None,
            z: 0,
            hidden: false,
        }
    }

    pub fn with_routing(mut self, routing: EdgeRouting) -> EdgeSpec {
        self.routing = routing;
        self
    }

    pub fn with_style(mut self, style: ElementStyle) -> EdgeSpec {
        self.style = style;
        self
    }
}

/// Every edge in the world.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EdgeStore {
    // ---- hot: read while rebuilding routes and while painting ----
    sources: Vec<EdgeEnd>,
    targets: Vec<EdgeEnd>,
    routings: Vec<EdgeRouting>,
    flags: Vec<EdgeFlags>,

    // ---- warm ----
    ids: Vec<ElementId>,
    z: Vec<i32>,
    styles: Vec<ElementStyle>,

    // ---- cold ----
    /// `Arc<str>` rather than `String`, for the same per-frame reason
    /// [`NodeCold::label`](crate::runtime::NodeCold::label) is: a
    /// [`TextPrimitive`](crate::render::plan::TextPrimitive) carries the text it
    /// draws, and §9's edge labels are built once per visible labelled edge per
    /// frame. An `Arc` clone is a refcount bump; a `String` clone is an
    /// allocation, which is what §40 rule 10 is about.
    labels: Vec<Option<Arc<str>>>,
    /// The edge's hyperlink. A `String` rather than an `Arc<str>` for the
    /// reason [`NodeCold::link`](crate::runtime::NodeCold::link) gives: nothing
    /// per frame carries a clone of it.
    links: Vec<Option<String>>,
}

impl EdgeStore {
    pub fn new() -> EdgeStore {
        EdgeStore::default()
    }

    pub fn len(&self) -> usize {
        self.sources.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sources.is_empty()
    }

    pub fn contains(&self, edge: EdgeIndex) -> bool {
        edge.index() < self.sources.len()
    }

    /// Every edge index, in insertion order. **Not a visibility query** — see
    /// [`NodeStore::indices`](crate::runtime::NodeStore::indices), and §40
    /// rule 2 for why finding one node's edges must never come through here.
    pub fn indices(&self) -> impl ExactSizeIterator<Item = EdgeIndex> + use<> {
        (0..self.sources.len() as u32).map(EdgeIndex::new)
    }

    pub fn reserve(&mut self, additional: usize) {
        self.sources.reserve(additional);
        self.targets.reserve(additional);
        self.routings.reserve(additional);
        self.flags.reserve(additional);
        self.ids.reserve(additional);
        self.z.reserve(additional);
        self.styles.reserve(additional);
        self.labels.reserve(additional);
        self.links.reserve(additional);
    }

    pub fn push(&mut self, spec: EdgeSpec) -> EdgeIndex {
        let index = EdgeIndex::new(self.sources.len() as u32);

        self.sources.push(spec.source);
        self.targets.push(spec.target);
        self.routings.push(spec.routing);
        self.flags.push(if spec.hidden {
            EdgeFlags::HIDDEN
        } else {
            EdgeFlags::NONE
        });

        self.ids.push(spec.id);
        self.z.push(spec.z);
        self.styles.push(spec.style);
        self.labels.push(spec.label.map(Arc::from));
        self.links.push(spec.link);

        index
    }

    // ---- reads ----------------------------------------------------------

    pub fn source(&self, edge: EdgeIndex) -> EdgeEnd {
        self.sources[edge.index()]
    }

    pub fn target(&self, edge: EdgeIndex) -> EdgeEnd {
        self.targets[edge.index()]
    }

    pub fn routing(&self, edge: EdgeIndex) -> EdgeRouting {
        self.routings[edge.index()]
    }

    pub fn flags(&self, edge: EdgeIndex) -> EdgeFlags {
        self.flags[edge.index()]
    }

    pub fn is_hidden(&self, edge: EdgeIndex) -> bool {
        self.flags[edge.index()].contains(EdgeFlags::HIDDEN)
    }

    pub fn is_selected(&self, edge: EdgeIndex) -> bool {
        self.flags[edge.index()].contains(EdgeFlags::SELECTED)
    }

    /// Whether this edge has been deleted. See [`EdgeFlags::REMOVED`].
    pub fn is_removed(&self, edge: EdgeIndex) -> bool {
        self.flags[edge.index()].contains(EdgeFlags::REMOVED)
    }

    /// Whether this slot holds an edge that is really there.
    pub fn is_live(&self, edge: EdgeIndex) -> bool {
        self.contains(edge) && !self.is_removed(edge)
    }

    /// Every edge that is really there, in insertion order.
    pub fn live_indices(&self) -> impl Iterator<Item = EdgeIndex> + '_ {
        self.indices().filter(|edge| !self.is_removed(*edge))
    }

    pub fn id(&self, edge: EdgeIndex) -> ElementId {
        self.ids[edge.index()]
    }

    pub fn z(&self, edge: EdgeIndex) -> i32 {
        self.z[edge.index()]
    }

    pub fn style(&self, edge: EdgeIndex) -> &ElementStyle {
        &self.styles[edge.index()]
    }

    /// The edge's label (§9), or `None`.
    ///
    /// Returns the `Arc` rather than a `&str` because the paint loop clones it
    /// into a primitive; a caller that only wants to read the text writes
    /// `.map(Arc::as_ref)` and pays nothing.
    pub fn label(&self, edge: EdgeIndex) -> Option<&Arc<str>> {
        self.labels[edge.index()].as_ref()
    }

    /// Whether this edge touches `node` at either end. Used by the duplicate
    /// check, over the **incident** edges of one node rather than over all of
    /// them.
    pub fn touches(&self, edge: EdgeIndex, node: NodeIndex) -> bool {
        self.sources[edge.index()].node == node || self.targets[edge.index()].node == node
    }

    // ---- writes ---------------------------------------------------------
    //
    // As in `NodeStore`: no dirty marking here. `GraphWorld` owns it, because
    // reconnecting an edge has to update the adjacency index too and one
    // half-done path through the store would be worse than none.

    pub fn set_routing(&mut self, edge: EdgeIndex, routing: EdgeRouting) {
        self.routings[edge.index()] = routing;
    }

    pub fn set_source(&mut self, edge: EdgeIndex, end: EdgeEnd) {
        self.sources[edge.index()] = end;
    }

    pub fn set_target(&mut self, edge: EdgeIndex, end: EdgeEnd) {
        self.targets[edge.index()] = end;
    }

    pub fn set_style(&mut self, edge: EdgeIndex, style: ElementStyle) {
        self.styles[edge.index()] = style;
    }

    /// **The edge's place in the paint order.** See
    /// [`NodeStore::set_z`](crate::runtime::NodeStore::set_z).
    pub fn set_z(&mut self, edge: EdgeIndex, z: i32) {
        self.z[edge.index()] = z;
    }

    pub fn link(&self, edge: EdgeIndex) -> Option<&str> {
        self.links[edge.index()].as_deref()
    }

    pub fn set_link(&mut self, edge: EdgeIndex, link: Option<String>) {
        self.links[edge.index()] = link;
    }

    pub fn set_label(&mut self, edge: EdgeIndex, label: Option<String>) {
        self.labels[edge.index()] = label.map(Arc::from);
    }

    pub fn style_mut(&mut self, edge: EdgeIndex) -> &mut ElementStyle {
        &mut self.styles[edge.index()]
    }

    pub fn set_flag(&mut self, edge: EdgeIndex, flag: EdgeFlags, on: bool) {
        let slot = &mut self.flags[edge.index()];
        *slot = if on {
            *slot | flag
        } else {
            *slot & flag.complement()
        };
    }
}

#[cfg(test)]
mod tests {
    use super::{EdgeEnd, EdgeFlags, EdgeSpec, EdgeStore, OptionalHandle};
    use crate::models::{EdgeIndex, EdgeRouting, ElementId, HandleIndex, NodeIndex};

    fn store() -> (EdgeStore, EdgeIndex) {
        let mut store = EdgeStore::new();
        let edge = store.push(
            EdgeSpec::new(
                ElementId::new(9),
                EdgeEnd::handle(NodeIndex::new(1), HandleIndex::new(4)),
                EdgeEnd::node(NodeIndex::new(2)),
            )
            .with_routing(EdgeRouting::Step),
        );
        (store, edge)
    }

    /// The packing that saves four bytes per endpoint. A sentinel is only
    /// acceptable if it round-trips exactly, so this is the test that matters.
    #[test]
    fn an_optional_handle_round_trips_through_four_bytes() {
        assert_eq!(OptionalHandle::NONE.get(), None);
        assert!(OptionalHandle::NONE.is_none());

        let handle = HandleIndex::new(1234);
        assert_eq!(OptionalHandle::some(handle).get(), Some(handle));
        assert!(!OptionalHandle::some(handle).is_none());

        assert_eq!(
            OptionalHandle::from(Some(handle)),
            OptionalHandle::some(handle)
        );
        assert_eq!(OptionalHandle::from(None), OptionalHandle::NONE);
        assert_eq!(std::mem::size_of::<OptionalHandle>(), 4);
        assert_eq!(std::mem::size_of::<EdgeEnd>(), 8);
    }

    #[test]
    fn a_pushed_edge_reads_back_as_it_went_in() {
        let (store, edge) = store();

        assert_eq!(store.len(), 1);
        assert_eq!(store.id(edge), ElementId::new(9));
        assert_eq!(store.source(edge).node, NodeIndex::new(1));
        assert_eq!(store.source(edge).handle.get(), Some(HandleIndex::new(4)));
        assert_eq!(store.target(edge).handle.get(), None);
        assert_eq!(store.routing(edge), EdgeRouting::Step);
        assert!(!store.is_hidden(edge));
    }

    #[test]
    fn touches_answers_for_both_ends_and_nothing_else() {
        let (store, edge) = store();

        assert!(store.touches(edge, NodeIndex::new(1)));
        assert!(store.touches(edge, NodeIndex::new(2)));
        assert!(!store.touches(edge, NodeIndex::new(3)));
    }

    #[test]
    fn flags_set_and_clear_independently() {
        let (mut store, edge) = store();

        store.set_flag(edge, EdgeFlags::SELECTED, true);
        store.set_flag(edge, EdgeFlags::HIDDEN, true);
        assert!(store.is_selected(edge));
        assert!(store.is_hidden(edge));

        store.set_flag(edge, EdgeFlags::HIDDEN, false);
        assert!(store.is_selected(edge));
        assert!(!store.is_hidden(edge));
    }

    #[test]
    fn reconnecting_an_end_replaces_only_that_end() {
        let (mut store, edge) = store();

        store.set_target(
            edge,
            EdgeEnd::handle(NodeIndex::new(5), HandleIndex::new(6)),
        );

        assert_eq!(store.source(edge).node, NodeIndex::new(1));
        assert_eq!(store.target(edge).node, NodeIndex::new(5));
        assert_eq!(store.target(edge).handle.get(), Some(HandleIndex::new(6)));
    }
}
