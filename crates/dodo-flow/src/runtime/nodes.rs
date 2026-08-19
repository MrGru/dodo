//! [`NodeStore`] — §17's SoA node storage, and the split between what is read
//! every frame and what is read twice in a document's life.
//!
//! # What "SoA-ish" bought, concretely
//!
//! §17 asks for a struct-of-arrays layout for frequently iterated hot data and
//! warns in the same breath against converting everything to strict SoA if it
//! costs clarity without a measured benefit. The line drawn here is **what the
//! paint loop touches**:
//!
//! ```text
//! hot   positions  sizes  shapes  flags      one indexed load each, per visible node
//! warm  ids  z  handles  styles              read per painted node, not per node
//! cold  kind  label  parent                  read on load, on save, by the registry
//! ```
//!
//! The interesting one is `shapes`. [`ElementKind`] carries a `String` in three
//! of its variants, so an array of them is an array of 32-byte enums with heap
//! pointers in it — and the paint loop's actual question is "quad, ellipse,
//! diamond or graph node?", which is one byte. [`NodeShape`] is that byte, kept
//! beside the geometry, and the full kind stays cold for §43's renderer
//! registry to read when it builds a rich element for the handful of nodes that
//! get one. That is the §17 rule and §40 rule 9 meeting in one field.
//!
//! # Append-only, on purpose, for now
//!
//! There is no `remove`. Every index in the world — an edge's endpoints, an
//! adjacency entry, a handle's owner — is a slot number, and removing a node
//! means either a tombstone (which every iteration then has to skip) or a
//! swap-remove (which moves another node's index out from under everything
//! holding it). Both are real designs; both belong with the command layer
//! (§30, Phase 7) that has to be able to *undo* a removal, because that is what
//! decides which one is right. Adding one now would be guessing.
//!
//! **This file names no UI framework.**

use std::sync::Arc;

use crate::{
    geometry::{Rect, Vec2},
    models::{
        ElementId, ElementKind, ElementStyle, GraphNodeKind, HandleIndex, NodeIndex, ShapeKind,
    },
    runtime::CompactList,
};

/// Boolean properties of a node, packed (§41).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct NodeFlags(u8);

impl NodeFlags {
    pub const NONE: NodeFlags = NodeFlags(0);
    /// Not painted and not hit-tested. Still part of the document, and still
    /// counted by [`content_bounds`](crate::models::FlowDocument::content_bounds).
    pub const HIDDEN: NodeFlags = NodeFlags(1 << 0);
    /// Cannot be dragged or edited.
    pub const LOCKED: NodeFlags = NodeFlags(1 << 1);
    /// In the current selection (§28).
    pub const SELECTED: NodeFlags = NodeFlags(1 << 2);

    pub const fn contains(self, other: NodeFlags) -> bool {
        self.0 & other.0 == other.0
    }

    pub const fn complement(self) -> NodeFlags {
        NodeFlags(!self.0)
    }
}

impl std::ops::BitOr for NodeFlags {
    type Output = NodeFlags;

    fn bitor(self, rhs: NodeFlags) -> NodeFlags {
        NodeFlags(self.0 | rhs.0)
    }
}

impl std::ops::BitAnd for NodeFlags {
    type Output = NodeFlags;

    fn bitand(self, rhs: NodeFlags) -> NodeFlags {
        NodeFlags(self.0 & rhs.0)
    }
}

/// **What the paint loop needs to know about a node's kind, in one byte.**
///
/// Not a replacement for [`ElementKind`] — a projection of it. See the module
/// doc for why the projection exists; [`NodeShape::of`] is the only place that
/// computes it, so a new kind is routed in exactly one `match`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum NodeShape {
    /// An axis-aligned rectangle: painted as a quad, never as a path.
    #[default]
    Rectangle,
    RoundedRectangle,
    Ellipse,
    Diamond,
    Triangle,
    /// A React-Flow-style node body: a rounded quad with handles.
    GraphNode,
    /// Text, an image, a frame, a freehand stroke, an embed, a custom kind —
    /// everything whose painter is a later phase's. Deliberately **not** drawn
    /// as a rectangle: a kind that silently paints as something else is a
    /// missing feature that looks implemented.
    Other,
}

impl NodeShape {
    pub fn of(kind: &ElementKind) -> NodeShape {
        match kind {
            ElementKind::GraphNode(GraphNodeKind::Custom(_)) | ElementKind::Custom(_) => {
                NodeShape::Other
            }
            ElementKind::GraphNode(_) => NodeShape::GraphNode,
            ElementKind::Shape(ShapeKind::Rectangle) => NodeShape::Rectangle,
            ElementKind::Shape(ShapeKind::RoundedRectangle) => NodeShape::RoundedRectangle,
            ElementKind::Shape(ShapeKind::Ellipse) => NodeShape::Ellipse,
            ElementKind::Shape(ShapeKind::Diamond) => NodeShape::Diamond,
            ElementKind::Shape(ShapeKind::Triangle) => NodeShape::Triangle,
            _ => NodeShape::Other,
        }
    }

    /// Whether edges may attach to this node. §4's whole-node connection mode
    /// applies to graph nodes; a drawn shape is decoration until §8's free
    /// linear elements arrive.
    pub fn is_connectable(self) -> bool {
        matches!(self, NodeShape::GraphNode)
    }
}

/// A node's metadata: read on load, on save and by §43's renderer registry, and
/// never by the paint loop. One struct rather than three arrays because nothing
/// iterates it.
#[derive(Debug, Clone, PartialEq)]
pub struct NodeCold {
    pub kind: ElementKind,
    /// The node's own text.
    ///
    /// `Arc<str>` rather than `String` for one reason and it is a per-frame
    /// one: a [`TextPrimitive`](crate::render::plan::TextPrimitive) carries the
    /// text it draws, and cloning a `String` per visible label per frame is
    /// exactly the allocation §40 rule 10 is about — 1,584 of them on Phase 4's
    /// dense scene. An `Arc` clone is a refcount bump. The label is written
    /// rarely and read every frame, which is the shape `Arc` is for.
    pub label: Option<Arc<str>>,
    /// The container (§11) this node belongs to. An [`ElementId`] rather than a
    /// [`NodeIndex`] because the hierarchy index that resolves it is a later
    /// phase's, and a half-built index is worse than none.
    pub parent: Option<ElementId>,
}

/// What a node needs to exist.
#[derive(Debug, Clone, PartialEq)]
pub struct NodeSpec {
    pub id: ElementId,
    pub kind: ElementKind,
    pub position: Vec2,
    pub size: Vec2,
    pub z: i32,
    pub style: ElementStyle,
    pub label: Option<String>,
    pub parent: Option<ElementId>,
    pub hidden: bool,
    pub locked: bool,
}

impl NodeSpec {
    pub fn new(id: ElementId, kind: ElementKind, position: Vec2, size: Vec2) -> NodeSpec {
        NodeSpec {
            id,
            kind,
            position,
            size,
            z: 0,
            style: ElementStyle::default(),
            label: None,
            parent: None,
            hidden: false,
            locked: false,
        }
    }
}

/// Every node in the world.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NodeStore {
    // ---- hot ----
    positions: Vec<Vec2>,
    sizes: Vec<Vec2>,
    shapes: Vec<NodeShape>,
    flags: Vec<NodeFlags>,

    /// **§23's cache version, per node.** Bumped by every write that changes
    /// what the node looks like — position, size, style, flags — so a
    /// tessellation cache keyed on it misses exactly when the geometry it
    /// flattened is no longer the geometry that is there. One `u32` per node
    /// against a re-comparison of a rectangle and a whole `ElementStyle`.
    versions: Vec<u32>,

    // ---- warm ----
    ids: Vec<ElementId>,
    z: Vec<i32>,
    handles: Vec<CompactList>,
    styles: Vec<ElementStyle>,

    // ---- cold ----
    cold: Vec<NodeCold>,
}

impl NodeStore {
    pub fn new() -> NodeStore {
        NodeStore::default()
    }

    pub fn len(&self) -> usize {
        self.positions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.positions.is_empty()
    }

    pub fn contains(&self, node: NodeIndex) -> bool {
        node.index() < self.positions.len()
    }

    /// Every node index, in insertion order.
    ///
    /// **Not a visibility query.** Phase 4's spatial index answers "which nodes
    /// are on screen"; this is for a whole-document pass — save, zoom-to-fit,
    /// a benchmark — and §40 rule 1 forbids calling it per frame to find
    /// visible nodes.
    pub fn indices(&self) -> impl ExactSizeIterator<Item = NodeIndex> + use<> {
        (0..self.positions.len() as u32).map(NodeIndex::new)
    }

    pub fn reserve(&mut self, additional: usize) {
        self.positions.reserve(additional);
        self.sizes.reserve(additional);
        self.shapes.reserve(additional);
        self.flags.reserve(additional);
        self.versions.reserve(additional);
        self.ids.reserve(additional);
        self.z.reserve(additional);
        self.handles.reserve(additional);
        self.styles.reserve(additional);
        self.cold.reserve(additional);
    }

    pub fn push(&mut self, spec: NodeSpec) -> NodeIndex {
        let index = NodeIndex::new(self.positions.len() as u32);

        self.positions.push(spec.position);
        self.sizes.push(spec.size);
        self.shapes.push(NodeShape::of(&spec.kind));
        let mut flags = NodeFlags::NONE;
        if spec.hidden {
            flags = flags | NodeFlags::HIDDEN;
        }
        if spec.locked {
            flags = flags | NodeFlags::LOCKED;
        }
        self.flags.push(flags);
        self.versions.push(0);

        self.ids.push(spec.id);
        self.z.push(spec.z);
        self.handles.push(CompactList::new());
        self.styles.push(spec.style);

        self.cold.push(NodeCold {
            kind: spec.kind,
            label: spec.label.map(Arc::from),
            parent: spec.parent,
        });

        index
    }

    // ---- hot reads ------------------------------------------------------

    pub fn position(&self, node: NodeIndex) -> Vec2 {
        self.positions[node.index()]
    }

    pub fn size(&self, node: NodeIndex) -> Vec2 {
        self.sizes[node.index()]
    }

    /// The node's world rectangle — **the culling and hit-testing unit**, and
    /// what Phase 4's spatial index will store.
    pub fn bounds(&self, node: NodeIndex) -> Rect {
        Rect::new(self.positions[node.index()], self.sizes[node.index()])
    }

    pub fn shape(&self, node: NodeIndex) -> NodeShape {
        self.shapes[node.index()]
    }

    /// **The appearance version of one node** — see the `versions` field.
    ///
    /// A missing node answers 0 rather than panicking: a cache key for a node
    /// that does not exist should miss, not crash a frame.
    pub fn version(&self, node: NodeIndex) -> u32 {
        self.versions.get(node.index()).copied().unwrap_or(0)
    }

    pub fn flags(&self, node: NodeIndex) -> NodeFlags {
        self.flags[node.index()]
    }

    pub fn is_hidden(&self, node: NodeIndex) -> bool {
        self.flags[node.index()].contains(NodeFlags::HIDDEN)
    }

    pub fn is_locked(&self, node: NodeIndex) -> bool {
        self.flags[node.index()].contains(NodeFlags::LOCKED)
    }

    pub fn is_selected(&self, node: NodeIndex) -> bool {
        self.flags[node.index()].contains(NodeFlags::SELECTED)
    }

    /// The whole hot geometry array, for a pass that wants it — a spatial
    /// rebuild, a bounds union — without an accessor call per node.
    pub fn positions(&self) -> &[Vec2] {
        &self.positions
    }

    pub fn sizes(&self) -> &[Vec2] {
        &self.sizes
    }

    // ---- warm and cold reads --------------------------------------------

    pub fn id(&self, node: NodeIndex) -> ElementId {
        self.ids[node.index()]
    }

    pub fn z(&self, node: NodeIndex) -> i32 {
        self.z[node.index()]
    }

    pub fn style(&self, node: NodeIndex) -> &ElementStyle {
        &self.styles[node.index()]
    }

    pub fn cold(&self, node: NodeIndex) -> &NodeCold {
        &self.cold[node.index()]
    }

    pub fn kind(&self, node: NodeIndex) -> &ElementKind {
        &self.cold[node.index()].kind
    }

    /// The node's handles, in the order they were added.
    pub fn handles(&self, node: NodeIndex) -> impl ExactSizeIterator<Item = HandleIndex> + '_ {
        self.handles[node.index()]
            .as_slice()
            .iter()
            .map(|&raw| HandleIndex::new(raw))
    }

    pub fn handle_count(&self, node: NodeIndex) -> usize {
        self.handles[node.index()].len()
    }

    // ---- writes ---------------------------------------------------------
    //
    // None of these marks anything dirty. `GraphWorld` owns the dirty
    // propagation, because the propagation rule crosses the stores — a moved
    // node dirties its *edges* — and a store that marked its own flags would
    // give a second, incomplete path to the same job.

    pub fn set_position(&mut self, node: NodeIndex, position: Vec2) {
        self.positions[node.index()] = position;
        self.touch(node);
    }

    pub fn set_size(&mut self, node: NodeIndex, size: Vec2) {
        self.sizes[node.index()] = size;
        self.touch(node);
    }

    pub fn set_style(&mut self, node: NodeIndex, style: ElementStyle) {
        self.styles[node.index()] = style;
        self.touch(node);
    }

    /// A mutable style. **Bumps the version on the way out**, unconditionally,
    /// because the caller may or may not write and this store cannot tell —
    /// a spurious cache miss is a rebuilt path, a missed one is a stale
    /// picture.
    pub fn style_mut(&mut self, node: NodeIndex) -> &mut ElementStyle {
        self.versions[node.index()] = self.versions[node.index()].wrapping_add(1);
        &mut self.styles[node.index()]
    }

    pub fn set_flag(&mut self, node: NodeIndex, flag: NodeFlags, on: bool) {
        let slot = &mut self.flags[node.index()];
        *slot = if on {
            *slot | flag
        } else {
            *slot & flag.complement()
        };
        self.touch(node);
    }

    pub fn set_label(&mut self, node: NodeIndex, label: Option<String>) {
        self.cold[node.index()].label = label.map(Arc::from);
        self.touch(node);
    }

    /// Records that this node's appearance changed. See the `versions` field.
    fn touch(&mut self, node: NodeIndex) {
        let slot = &mut self.versions[node.index()];
        *slot = slot.wrapping_add(1);
    }

    /// Records that `handle` belongs to `node`. Called by
    /// [`GraphWorld`](crate::runtime::GraphWorld), which owns the handle arena.
    pub fn attach_handle(&mut self, node: NodeIndex, handle: HandleIndex) {
        self.handles[node.index()].push(handle.raw());
    }
}

#[cfg(test)]
mod version_tests {
    use super::{NodeFlags, NodeSpec, NodeStore};
    use crate::{
        geometry::Vec2,
        models::{ElementId, ElementKind, ElementStyle, NodeIndex},
    };

    /// One named write, for the enumeration below.
    type Write = (&'static str, fn(&mut NodeStore));

    fn store() -> NodeStore {
        let mut store = NodeStore::new();
        store.push(NodeSpec::new(
            ElementId::new(1),
            ElementKind::default(),
            Vec2::ZERO,
            Vec2::new(160.0, 60.0),
        ));
        store
    }

    /// §23 asks for cache keys based on geometry/style versions. This is the
    /// property a tessellation cache rests on: **every write that changes what
    /// the node looks like moves the version**, so a key that compares equal
    /// really does describe the same picture.
    #[test]
    fn every_appearance_changing_write_moves_the_version() {
        let node = NodeIndex::new(0);
        let writes: [Write; 6] = [
            ("set_position", |s| {
                s.set_position(NodeIndex::new(0), Vec2::new(5.0, 5.0))
            }),
            ("set_size", |s| {
                s.set_size(NodeIndex::new(0), Vec2::new(80.0, 40.0))
            }),
            ("set_style", |s| {
                s.set_style(NodeIndex::new(0), ElementStyle::default())
            }),
            ("style_mut", |s| {
                s.style_mut(NodeIndex::new(0)).opacity = 0.5;
            }),
            ("set_flag", |s| {
                s.set_flag(NodeIndex::new(0), NodeFlags::SELECTED, true)
            }),
            ("set_label", |s| {
                s.set_label(NodeIndex::new(0), Some("x".into()))
            }),
        ];

        for (name, write) in writes {
            let mut store = store();
            let before = store.version(node);
            write(&mut store);
            assert_ne!(store.version(node), before, "{name} left the version alone");
        }
    }

    /// The other half: a read must not invalidate anything, or a pure pan
    /// would miss the cache on every element it looked at.
    #[test]
    fn reading_a_node_leaves_its_version_alone() {
        let store = store();
        let node = NodeIndex::new(0);
        let before = store.version(node);

        let _ = store.bounds(node);
        let _ = store.style(node);
        let _ = store.shape(node);

        assert_eq!(store.version(node), before);
    }

    /// A key built for a node that is not there must miss rather than panic —
    /// a frame is not the place to discover a stale index.
    #[test]
    fn an_absent_node_has_a_version_rather_than_a_panic() {
        assert_eq!(store().version(NodeIndex::new(9_999)), 0);
    }
}

#[cfg(test)]
mod tests {
    use super::{NodeFlags, NodeShape, NodeSpec, NodeStore};
    use crate::{
        geometry::{Rect, Vec2},
        models::{
            CustomKind, ElementId, ElementKind, GraphNodeKind, HandleIndex, NodeIndex, ShapeKind,
        },
    };

    fn spec(id: u64, kind: ElementKind, x: f32, y: f32) -> NodeSpec {
        NodeSpec::new(
            ElementId::new(id),
            kind,
            Vec2::new(x, y),
            Vec2::new(160.0, 60.0),
        )
    }

    #[test]
    fn a_pushed_node_reads_back_as_it_went_in() {
        let mut store = NodeStore::new();
        let node = store.push(spec(1, ElementKind::Shape(ShapeKind::Ellipse), 10.0, 20.0));

        assert_eq!(store.len(), 1);
        assert_eq!(store.id(node), ElementId::new(1));
        assert_eq!(store.position(node), Vec2::new(10.0, 20.0));
        assert_eq!(
            store.bounds(node),
            Rect::new(Vec2::new(10.0, 20.0), Vec2::new(160.0, 60.0))
        );
        assert_eq!(store.shape(node), NodeShape::Ellipse);
        assert!(!store.is_hidden(node));
    }

    /// The projection the paint loop reads. A kind carrying a `String` must not
    /// reach it, and an unimplemented kind must not masquerade as a rectangle.
    #[test]
    fn every_kind_projects_onto_a_paintable_shape() {
        assert_eq!(
            NodeShape::of(&ElementKind::GraphNode(GraphNodeKind::Input)),
            NodeShape::GraphNode
        );
        assert_eq!(
            NodeShape::of(&ElementKind::Shape(ShapeKind::RoundedRectangle)),
            NodeShape::RoundedRectangle
        );
        assert_eq!(
            NodeShape::of(&ElementKind::Shape(ShapeKind::Custom(CustomKind::new("x")))),
            NodeShape::Other
        );
        assert_eq!(
            NodeShape::of(&ElementKind::GraphNode(GraphNodeKind::Custom(
                CustomKind::new("x")
            ))),
            NodeShape::Other
        );
        assert_eq!(NodeShape::of(&ElementKind::Text), NodeShape::Other);
        assert_eq!(NodeShape::of(&ElementKind::Frame), NodeShape::Other);
    }

    #[test]
    fn only_graph_nodes_accept_a_whole_node_connection() {
        assert!(NodeShape::GraphNode.is_connectable());
        assert!(!NodeShape::Rectangle.is_connectable());
        assert!(!NodeShape::Other.is_connectable());
    }

    #[test]
    fn flags_set_and_clear_without_disturbing_their_neighbours() {
        let mut store = NodeStore::new();
        let node = store.push(spec(1, ElementKind::default(), 0.0, 0.0));

        store.set_flag(node, NodeFlags::SELECTED, true);
        store.set_flag(node, NodeFlags::LOCKED, true);
        assert!(store.is_selected(node));
        assert!(store.is_locked(node));

        store.set_flag(node, NodeFlags::SELECTED, false);
        assert!(!store.is_selected(node));
        assert!(store.is_locked(node));
    }

    #[test]
    fn hidden_and_locked_come_through_the_spec() {
        let mut store = NodeStore::new();
        let mut hidden = spec(1, ElementKind::default(), 0.0, 0.0);
        hidden.hidden = true;
        hidden.locked = true;

        let node = store.push(hidden);

        assert!(store.is_hidden(node));
        assert!(store.is_locked(node));
    }

    #[test]
    fn handles_attach_in_order_and_stay_with_their_node() {
        let mut store = NodeStore::new();
        let first = store.push(spec(1, ElementKind::default(), 0.0, 0.0));
        let second = store.push(spec(2, ElementKind::default(), 0.0, 0.0));

        store.attach_handle(first, HandleIndex::new(0));
        store.attach_handle(second, HandleIndex::new(1));
        store.attach_handle(first, HandleIndex::new(2));

        assert_eq!(
            store.handles(first).collect::<Vec<_>>(),
            vec![HandleIndex::new(0), HandleIndex::new(2)]
        );
        assert_eq!(
            store.handles(second).collect::<Vec<_>>(),
            vec![HandleIndex::new(1)]
        );
        assert_eq!(store.handle_count(first), 2);
    }

    #[test]
    fn indices_cover_the_store_exactly_once() {
        let mut store = NodeStore::new();
        for id in 0..5u64 {
            store.push(spec(id + 1, ElementKind::default(), 0.0, 0.0));
        }

        let indices: Vec<_> = store.indices().collect();

        assert_eq!(indices.len(), 5);
        assert_eq!(indices.first(), Some(&NodeIndex::new(0)));
        assert_eq!(indices.last(), Some(&NodeIndex::new(4)));
        assert!(store.contains(NodeIndex::new(4)));
        assert!(!store.contains(NodeIndex::new(5)));
    }
}
