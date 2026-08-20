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
//! # Append-only, and Phase 7's answer: a tombstone
//!
//! There is still no `remove`. Every index in the world — an edge's endpoints,
//! an adjacency entry, a handle's owner — is a slot number, so removing a node
//! means either a tombstone (which every iteration then has to skip) or a
//! swap-remove (which moves another node's index out from under everything
//! holding it). Phase 3 left the choice to the command layer, because undo is
//! what decides it, and **§30's history decides it in favour of the
//! tombstone**:
//!
//! - An undo entry *is* a held index. A swap-remove moves some other node into
//!   the freed slot, and every entry already on the undo stack that named that
//!   node now names a different one. The corruption is silent and appears three
//!   steps later — exactly the defect Phase 7 exists to make unexpressible.
//! - Undo of a removal has to put the element back **at the same index**, so
//!   that entries recorded either side of it stay valid. A tombstone does that
//!   by flipping one bit; a swap-remove cannot do it at all.
//!
//! So [`NodeFlags::REMOVED`] is the removal, [`NodeStore::is_live`] is the
//! question everything else asks, and the cost — a skipped slot in
//! whole-document passes — is paid where it is cheapest.
//!
//! **Compaction is a document round-trip, not a background sweep.**
//! [`GraphWorld::to_document`](crate::runtime::GraphWorld::to_document) skips
//! tombstones and `from_document` builds a world with none, so saving and
//! reopening compacts. Nothing else may, because anything that renumbers slots
//! invalidates the history that made them.
//!
//! **This file names no UI framework.**

use std::sync::Arc;

use crate::{
    geometry::{Rect, Vec2},
    models::{
        ElementId, ElementKind, ElementStyle, GraphNodeKind, HandleIndex, LinearKind, NodeImage,
        NodeIndex, ShapeKind,
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
    /// **Deleted, as a tombstone** — see the module doc's "Append-only" note
    /// for why removal is a flag rather than a `Vec::remove`. A removed node is
    /// not painted, not hit-tested, not indexed and not saved, but its slot and
    /// every index into it survive, which is what lets an undo entry recorded
    /// before the removal still name the right element afterwards.
    pub const REMOVED: NodeFlags = NodeFlags(1 << 3);

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
    /// §7's free line: an **open** outline, so it is stroked and never filled,
    /// and it is never degraded to its bounding quad — a line drawn as a solid
    /// box is not a simplification of a line.
    Line,
    /// §7's free arrow: [`Line`](NodeShape::Line) with a head at the end.
    Arrow,
    /// §9's standalone text: **a body that is nothing but its text**.
    ///
    /// It has no outline at all — not an empty one, and not a transparent
    /// rectangle. A text element is its glyphs, so
    /// [`outline_for_node`](crate::render::shapes::outline_for_node) answers
    /// `None` for it and the whole shape/fill/stroke path is skipped; what
    /// reaches the plan is one [`TextPrimitive`](crate::render::plan::TextPrimitive)
    /// and nothing else. Its rectangle is still real — it is what the spatial
    /// index stores, what the selection ring is drawn around and what the text
    /// is laid out into — it is simply never painted.
    Text,
    /// §10's embedded raster image: **a rectangle whose interior is a
    /// picture**, painted from
    /// [`FlowNode::image`](crate::models::FlowNode::image)'s handle.
    ///
    /// Like [`Text`](NodeShape::Text) it has **no outline** — an image is its
    /// pixels, and a fill under it would never be seen — so
    /// [`outline_for_node`](crate::render::shapes::outline_for_node) answers
    /// `None` and the fill/stroke path is skipped. Unlike text it is *not*
    /// harmless to fall through to the quad rung: a quad would paint a solid
    /// box in the picture's place at the moment a user zoomed out. It is its
    /// own arm in the plan for that reason.
    Image,
    /// A frame, a freehand stroke, an embed, a custom kind — everything whose
    /// painter is a later phase's. Deliberately **not** drawn as a rectangle: a
    /// kind that silently paints as something else is a missing feature that
    /// looks implemented.
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
            ElementKind::Linear(LinearKind::Line) => NodeShape::Line,
            ElementKind::Linear(LinearKind::Arrow) => NodeShape::Arrow,
            ElementKind::Text => NodeShape::Text,
            ElementKind::Image => NodeShape::Image,
            // **An elbow is not a diagonal.** Its legs need waypoints, and a
            // node stores a rectangle; drawing it as a straight line would be
            // a different element wearing its name.
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
    /// The element's hyperlink — see [`FlowNode::link`](crate::models::FlowNode::link).
    ///
    /// Cold, and it stays cold: a link is read when the panel draws, when a
    /// press follows one and when the document is saved. Nothing per frame
    /// asks, so it does not earn a hot array and it does not earn an `Arc`
    /// either — the label's `Arc` is there because a `TextPrimitive` carries a
    /// clone of it every frame, and nothing carries this.
    pub link: Option<String>,
    /// **§10's picture, as a handle and a crop** — see
    /// [`FlowNode::image`](crate::models::FlowNode::image).
    ///
    /// Cold rather than hot, and it is worth saying why given that it is read
    /// every frame an image is on screen: a hot array is for a value the *whole
    /// visible set* is walked for, and this is read once per visible image
    /// rather than once per visible node. Sixteen bytes in the cold row against
    /// sixteen bytes per node in a hot one, on a document where almost no node
    /// is a picture.
    pub image: Option<NodeImage>,
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
    pub link: Option<String>,
    /// §10's picture. `None` for every kind but [`ElementKind::Image`].
    pub image: Option<NodeImage>,
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
            link: None,
            image: None,
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

    /// **§9's shaped-line version, per node** — deliberately *not* the one
    /// above.
    ///
    /// A shaped line depends on the text, the font and the width it is wrapped
    /// into. It does **not** depend on where the node is, and that difference
    /// is worth a second array: a node's appearance version bumps on every
    /// move, so a dragged node keyed on it would re-shape its label sixty times
    /// a second — 7–11 µs each against 1.7 µs to paint a cached one, for a line
    /// that has not changed a glyph. Four bytes per node against that, and 400
    /// KB at a hundred thousand nodes.
    ///
    /// Bumped by everything that changes what the glyphs would be — the label,
    /// the style, a resize (which changes the wrap width) — and by nothing
    /// else. `moving_a_node_does_not_reshape_its_label` is the property.
    text_versions: Vec<u32>,

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
        self.text_versions.reserve(additional);
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
        self.text_versions.push(0);

        self.ids.push(spec.id);
        self.z.push(spec.z);
        self.handles.push(CompactList::new());
        self.styles.push(spec.style);

        self.cold.push(NodeCold {
            kind: spec.kind,
            label: spec.label.map(Arc::from),
            parent: spec.parent,
            link: spec.link,
            image: spec.image,
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

    /// **The version this node's shaped line is keyed on** (§9) — see the
    /// `text_versions` field for why it is not [`version`](NodeStore::version).
    pub fn text_version(&self, node: NodeIndex) -> u32 {
        self.text_versions.get(node.index()).copied().unwrap_or(0)
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

    /// Whether this node has been deleted. See [`NodeFlags::REMOVED`].
    pub fn is_removed(&self, node: NodeIndex) -> bool {
        self.flags[node.index()].contains(NodeFlags::REMOVED)
    }

    /// **The question every reader of the document asks**: does this slot hold
    /// a node that is really there? A slot that was never allocated and a slot
    /// whose node was deleted answer the same way, so a caller holding a stale
    /// index cannot tell them apart and does not have to.
    pub fn is_live(&self, node: NodeIndex) -> bool {
        self.contains(node) && !self.is_removed(node)
    }

    /// Every node that is really there, in insertion order. The whole-document
    /// counterpart of [`indices`](NodeStore::indices) — save, zoom-to-fit, a
    /// spatial rebuild — and **not** a visibility query either.
    pub fn live_indices(&self) -> impl Iterator<Item = NodeIndex> + '_ {
        self.indices().filter(|node| !self.is_removed(*node))
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
        // A resize changes the width the label wraps into, which is part of
        // what `shape_line` produces — so this one *is* a re-shape.
        self.touch_text(node);
    }

    pub fn set_style(&mut self, node: NodeIndex, style: ElementStyle) {
        self.styles[node.index()] = style;
        self.touch(node);
        self.touch_text(node);
    }

    /// A mutable style. **Bumps the version on the way out**, unconditionally,
    /// because the caller may or may not write and this store cannot tell —
    /// a spurious cache miss is a rebuilt path, a missed one is a stale
    /// picture.
    pub fn style_mut(&mut self, node: NodeIndex) -> &mut ElementStyle {
        self.versions[node.index()] = self.versions[node.index()].wrapping_add(1);
        self.text_versions[node.index()] = self.text_versions[node.index()].wrapping_add(1);
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
        self.touch_text(node);
    }

    /// **The node's place in the paint order.**
    ///
    /// `touch` because the *frame* changes — [`render::scene`](crate::render::scene)
    /// orders its walk by this — while the node's own geometry does not, and no
    /// `touch_text`, because glyphs do not depend on depth.
    pub fn set_z(&mut self, node: NodeIndex, z: i32) {
        self.z[node.index()] = z;
        self.touch(node);
    }

    /// Replaces the node's hyperlink. Cold, and not part of any cache key: a
    /// link changes nothing that is drawn.
    pub fn set_link(&mut self, node: NodeIndex, link: Option<String>) {
        self.cold[node.index()].link = link;
    }

    /// **Replaces the node's picture, or clears it** (§10).
    ///
    /// `touch` because a crop changes what is drawn inside the same rectangle —
    /// the frame is identical and the pixels are not — and **no `touch_text`**,
    /// because an image has no glyphs and a document full of labelled shapes
    /// must not re-shape when somebody crops a screenshot.
    pub fn set_image(&mut self, node: NodeIndex, image: Option<NodeImage>) {
        self.cold[node.index()].image = image;
        self.touch(node);
    }

    /// Records that this node's appearance changed. See the `versions` field.
    fn touch(&mut self, node: NodeIndex) {
        let slot = &mut self.versions[node.index()];
        *slot = slot.wrapping_add(1);
    }

    /// Records that this node's *glyphs* would come out differently. See the
    /// `text_versions` field for the one write that deliberately does not call
    /// this: a move.
    fn touch_text(&mut self, node: NodeIndex) {
        let slot = &mut self.text_versions[node.index()];
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

    /// **The one write that must move the appearance version and leave the
    /// text version alone** (§9), and the four that must move both.
    ///
    /// This is what makes dragging a labelled node free of re-shaping. Keyed on
    /// the appearance version the label would be re-shaped sixty times a second
    /// during a drag — 7–11 µs each against 1.7 µs to paint a cached one — for
    /// a line whose glyphs have not changed. A resize *is* in the second group,
    /// because it changes the width the run wraps into and that is part of what
    /// the text system produced.
    #[test]
    fn moving_a_node_does_not_reshape_its_label() {
        let node = NodeIndex::new(0);

        let mut moved = store();
        let (appearance, text) = (moved.version(node), moved.text_version(node));
        moved.set_position(node, Vec2::new(500.0, 500.0));
        assert_ne!(moved.version(node), appearance, "the geometry moved");
        assert_eq!(
            moved.text_version(node),
            text,
            "the glyphs did not, so the shaped line must survive"
        );

        let reshaping: [Write; 4] = [
            ("set_size", |s| {
                s.set_size(NodeIndex::new(0), Vec2::new(80.0, 40.0))
            }),
            ("set_style", |s| {
                s.set_style(NodeIndex::new(0), ElementStyle::default())
            }),
            ("style_mut", |s| {
                s.style_mut(NodeIndex::new(0)).font.size = crate::models::FontSize::Large;
            }),
            ("set_label", |s| {
                s.set_label(NodeIndex::new(0), Some("x".into()))
            }),
        ];

        for (name, write) in reshaping {
            let mut store = store();
            let before = store.text_version(node);
            write(&mut store);
            assert_ne!(
                store.text_version(node),
                before,
                "{name} changes the glyphs and must re-shape"
            );
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
        assert_eq!(store().text_version(NodeIndex::new(9_999)), 0);
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
        // Text left `Other` in Phase 10, and it left it the only way anything
        // may: a painter first, then the projection, then the tool.
        assert_eq!(NodeShape::of(&ElementKind::Text), NodeShape::Text);
        // And an image left it the same way in Phase 12.
        assert_eq!(NodeShape::of(&ElementKind::Image), NodeShape::Image);
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
