//! [`HandleStore`] — every connection point in the world, in one flat arena
//! (§4).
//!
//! # Flat, not per node
//!
//! A handle could have lived inside its node. It does not, for the reason §4
//! gives when it says to use compact runtime ids in hot paths: an edge endpoint
//! resolves to **one `u32`**, and reading where that endpoint is costs one
//! indexed load. Per-node storage would make an endpoint a node index plus a
//! position within that node — or worse, a `HandleId` string lookup, which is
//! §40 rule 9 exactly.
//!
//! # Indices are stable, so a node's handles are a list rather than a range
//!
//! The obvious layout is a contiguous range of handles per node, and it is
//! wrong here. §4 asks for **dynamic handles**, so a node gains one after its
//! neighbours already occupy the slots after it; a range would have to relocate
//! that node's handles to the end of the arena, and relocation moves handle
//! indices that live edges are holding. Every edge in the graph would have to
//! be patched to add one handle to one node.
//!
//! So the arena is **append-only and indices never move**, and a node holds a
//! [`CompactList`](crate::runtime::CompactList) of its handles — four inline,
//! which covers a node with one handle per side.
//!
//! # `hidden` is a paint flag and nothing else
//!
//! §4 asks for hidden handles that remain geometrically connectable, so nothing
//! in routing, validation or hit-testing may read [`HandleFlags::HIDDEN`]. It
//! is read by the painter, and by Phase 5's element extraction.
//!
//! **This file names no UI framework.**

use crate::{
    geometry::{Rect, Vec2},
    models::{
        HandleDirection, HandleId, HandleIndex, HandlePlacement, NodeIndex, handle_world_position,
    },
};

/// Boolean properties of a handle, packed. §41 asks for `bitflags` over `bool`
/// fields; there is one flag today and the shape is what matters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct HandleFlags(u8);

impl HandleFlags {
    pub const NONE: HandleFlags = HandleFlags(0);
    /// Not painted. **Still connectable** — see the module doc.
    pub const HIDDEN: HandleFlags = HandleFlags(1);

    pub const fn contains(self, other: HandleFlags) -> bool {
        self.0 & other.0 == other.0
    }
}

impl std::ops::BitOr for HandleFlags {
    type Output = HandleFlags;

    fn bitor(self, rhs: HandleFlags) -> HandleFlags {
        HandleFlags(self.0 | rhs.0)
    }
}

/// What a handle needs to exist. The store's arrays are private; this is how
/// one is described on the way in.
#[derive(Debug, Clone, PartialEq)]
pub struct HandleSpec {
    pub id: HandleId,
    pub placement: HandlePlacement,
    pub direction: HandleDirection,
    /// Fraction along the placement edge. Outside `0.0..=1.0` is §4's arbitrary
    /// placement and is deliberately not clamped.
    pub offset: f32,
    /// `None` is unlimited (§4's connection limits).
    pub max_connections: Option<u32>,
    pub hidden: bool,
}

impl Default for HandleSpec {
    fn default() -> HandleSpec {
        HandleSpec {
            id: HandleId::new(""),
            placement: HandlePlacement::default(),
            direction: HandleDirection::default(),
            offset: 0.5,
            max_connections: None,
            hidden: false,
        }
    }
}

impl HandleSpec {
    pub fn new(
        id: impl Into<String>,
        placement: HandlePlacement,
        direction: HandleDirection,
    ) -> HandleSpec {
        HandleSpec {
            id: HandleId::new(id),
            placement,
            direction,
            ..HandleSpec::default()
        }
    }

    pub fn with_offset(mut self, offset: f32) -> HandleSpec {
        self.offset = offset;
        self
    }

    pub fn with_limit(mut self, max_connections: u32) -> HandleSpec {
        self.max_connections = Some(max_connections);
        self
    }

    pub fn hidden(mut self) -> HandleSpec {
        self.hidden = true;
        self
    }
}

/// The sentinel for "no limit", so the hot array is a plain `Vec<u32>` rather
/// than a `Vec<Option<u32>>` — which would be twice the width for a value that
/// is almost always absent.
const UNLIMITED: u32 = u32::MAX;

/// Every handle in the world.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct HandleStore {
    // ---- hot: read while routing every edge and hit-testing every press ----
    owners: Vec<NodeIndex>,
    placements: Vec<HandlePlacement>,
    directions: Vec<HandleDirection>,
    offsets: Vec<f32>,
    limits: Vec<u32>,
    flags: Vec<HandleFlags>,

    // ---- cold: the author's name for the handle, touched on load and save ----
    ids: Vec<HandleId>,
}

impl HandleStore {
    pub fn new() -> HandleStore {
        HandleStore::default()
    }

    pub fn len(&self) -> usize {
        self.owners.len()
    }

    pub fn is_empty(&self) -> bool {
        self.owners.is_empty()
    }

    pub fn reserve(&mut self, additional: usize) {
        self.owners.reserve(additional);
        self.placements.reserve(additional);
        self.directions.reserve(additional);
        self.offsets.reserve(additional);
        self.limits.reserve(additional);
        self.flags.reserve(additional);
        self.ids.reserve(additional);
    }

    /// Appends a handle owned by `node`. **The index it returns is permanent.**
    pub fn push(&mut self, node: NodeIndex, spec: HandleSpec) -> HandleIndex {
        let index = HandleIndex::new(self.owners.len() as u32);

        self.owners.push(node);
        self.placements.push(spec.placement);
        self.directions.push(spec.direction);
        self.offsets.push(spec.offset);
        self.limits.push(spec.max_connections.unwrap_or(UNLIMITED));
        self.flags.push(if spec.hidden {
            HandleFlags::HIDDEN
        } else {
            HandleFlags::NONE
        });
        self.ids.push(spec.id);

        index
    }

    pub fn owner(&self, handle: HandleIndex) -> NodeIndex {
        self.owners[handle.index()]
    }

    pub fn placement(&self, handle: HandleIndex) -> HandlePlacement {
        self.placements[handle.index()]
    }

    pub fn direction(&self, handle: HandleIndex) -> HandleDirection {
        self.directions[handle.index()]
    }

    pub fn offset(&self, handle: HandleIndex) -> f32 {
        self.offsets[handle.index()]
    }

    /// `None` is unlimited.
    pub fn limit(&self, handle: HandleIndex) -> Option<u32> {
        match self.limits[handle.index()] {
            UNLIMITED => None,
            limit => Some(limit),
        }
    }

    pub fn is_hidden(&self, handle: HandleIndex) -> bool {
        self.flags[handle.index()].contains(HandleFlags::HIDDEN)
    }

    pub fn id(&self, handle: HandleIndex) -> &HandleId {
        &self.ids[handle.index()]
    }

    /// **One handle as the spec that would recreate it**, for a duplicate.
    ///
    /// Here rather than assembled at the call site because the store holds a
    /// handle in six parallel arrays and a caller that read five of them would
    /// produce a copy that differs from its original in the one it forgot.
    pub fn spec(&self, handle: HandleIndex) -> HandleSpec {
        HandleSpec {
            id: self.id(handle).clone(),
            placement: self.placement(handle),
            direction: self.direction(handle),
            offset: self.offset(handle),
            max_connections: self.limit(handle),
            hidden: self.is_hidden(handle),
        }
    }

    pub fn contains(&self, handle: HandleIndex) -> bool {
        handle.index() < self.owners.len()
    }

    pub fn set_offset(&mut self, handle: HandleIndex, offset: f32) {
        self.offsets[handle.index()] = offset;
    }

    pub fn set_placement(&mut self, handle: HandleIndex, placement: HandlePlacement) {
        self.placements[handle.index()] = placement;
    }

    /// Where this handle sits in world space, given its node's rectangle.
    ///
    /// Delegates to
    /// [`handle_world_position`](crate::models::handle_world_position) so the
    /// document model and the runtime cannot drift — a handle that painted in one place and routed
    /// from another would be a maddening bug, and it is prevented by there
    /// being one formula.
    pub fn world_position(&self, handle: HandleIndex, node_bounds: Rect) -> Vec2 {
        handle_world_position(
            self.placements[handle.index()],
            self.offsets[handle.index()],
            node_bounds,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{HandleSpec, HandleStore};
    use crate::{
        geometry::{Rect, Vec2},
        models::{Handle, HandleDirection, HandlePlacement, NodeIndex, handle_world_position},
    };

    fn node() -> Rect {
        Rect::new(Vec2::new(100.0, 200.0), Vec2::new(160.0, 80.0))
    }

    #[test]
    fn a_pushed_handle_keeps_everything_it_was_given() {
        let mut store = HandleStore::new();
        let spec = HandleSpec::new("out", HandlePlacement::Right, HandleDirection::Source)
            .with_offset(0.25)
            .with_limit(2)
            .hidden();

        let handle = store.push(NodeIndex::new(7), spec);

        assert_eq!(store.owner(handle), NodeIndex::new(7));
        assert_eq!(store.placement(handle), HandlePlacement::Right);
        assert_eq!(store.direction(handle), HandleDirection::Source);
        assert_eq!(store.offset(handle), 0.25);
        assert_eq!(store.limit(handle), Some(2));
        assert!(store.is_hidden(handle));
        assert_eq!(store.id(handle).as_str(), "out");
    }

    #[test]
    fn an_unspecified_limit_reads_as_unlimited_rather_than_as_a_number() {
        let mut store = HandleStore::new();
        let handle = store.push(
            NodeIndex::new(0),
            HandleSpec::new("in", HandlePlacement::Left, HandleDirection::Target),
        );

        assert_eq!(store.limit(handle), None);
    }

    /// Indices are permanent, which is what lets an edge hold one. Adding a
    /// handle to an earlier node must not disturb a later node's.
    #[test]
    fn indices_are_stable_as_the_arena_grows() {
        let mut store = HandleStore::new();
        let first = store.push(
            NodeIndex::new(0),
            HandleSpec::new("a", HandlePlacement::Top, HandleDirection::Loose),
        );
        let second = store.push(
            NodeIndex::new(1),
            HandleSpec::new("b", HandlePlacement::Bottom, HandleDirection::Loose),
        );
        let third = store.push(
            NodeIndex::new(0),
            HandleSpec::new("c", HandlePlacement::Left, HandleDirection::Loose),
        );

        assert_eq!(store.id(first).as_str(), "a");
        assert_eq!(store.id(second).as_str(), "b");
        assert_eq!(store.id(third).as_str(), "c");
        assert_eq!(store.owner(third), NodeIndex::new(0));
    }

    #[test]
    fn the_four_placements_land_on_the_four_edges() {
        let bounds = node();

        assert_eq!(
            handle_world_position(HandlePlacement::Top, 0.5, bounds),
            Vec2::new(180.0, 200.0)
        );
        assert_eq!(
            handle_world_position(HandlePlacement::Bottom, 0.5, bounds),
            Vec2::new(180.0, 280.0)
        );
        assert_eq!(
            handle_world_position(HandlePlacement::Left, 0.5, bounds),
            Vec2::new(100.0, 240.0)
        );
        assert_eq!(
            handle_world_position(HandlePlacement::Right, 0.5, bounds),
            Vec2::new(260.0, 240.0)
        );
    }

    /// §4's arbitrary placement: an offset outside the edge is honoured rather
    /// than clamped back onto it.
    #[test]
    fn an_offset_outside_the_edge_is_not_clamped() {
        let bounds = node();

        assert_eq!(
            handle_world_position(HandlePlacement::Top, 1.5, bounds),
            Vec2::new(340.0, 200.0)
        );
    }

    /// The document model and the runtime store must agree, because one paints
    /// the handle and the other routes the edge to it.
    #[test]
    fn the_store_and_the_document_model_place_a_handle_identically() {
        let bounds = node();
        let mut store = HandleStore::new();
        let handle = store.push(
            NodeIndex::new(0),
            HandleSpec::new("out", HandlePlacement::Right, HandleDirection::Source)
                .with_offset(0.3),
        );

        let mut document_handle =
            Handle::new("out", HandlePlacement::Right, HandleDirection::Source);
        document_handle.offset = 0.3;

        assert_eq!(
            store.world_position(handle, bounds),
            document_handle.world_position(bounds)
        );
    }
}
