//! [`FlowDocument`] — everything that is written to disk, and nothing else.
//!
//! Requirements §31 draws the line this file sits on:
//!
//! > Do not serialize: GPUI entities, canvas paths, tessellation data, spatial
//! > indexes, adjacency caches that are trivially rebuilt, other runtime-only
//! > caches.
//!
//! Every one of those is derived from what is here, and every one of them is
//! rebuilt on load. Three mechanisms keep the rule from being a convention
//! somebody has to remember:
//!
//! 1. **The runtime indices are not serializable at all** — see
//!    [`ids`](crate::models::ids). A field holding a `NodeIndex` cannot be
//!    added to a struct in this file, because it would not compile.
//! 2. **This crate's dependency on `gpui` is one-directional.** `models/` does
//!    not name it, so a GPUI entity cannot end up in a document even by
//!    accident.
//! 3. **`document_holds_no_runtime_state` pins the serialized top-level key
//!    set**, so adding a cache to this struct fails a test rather than shipping.
//!
//! **This is not a store.** §17's SoA `NodeStore` / `EdgeStore` arrive in
//! `runtime/` later and are built *from* this. `FlowDocument` is a plain
//! `Vec`-of-structs because it is walked twice in a document's life — once on
//! load, once on save — and clarity beats layout at that frequency. The engine
//! never iterates it per frame.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{
    geometry::{Rect, Vec2},
    models::{
        ElementKind, LinearKind,
        ids::{ElementId, HandleId, IdAllocator},
        image::{ImageHandle, ImageResource, NodeImage},
        style::{EdgeRouting, ElementStyle, RenderQuality, RenderStyle, SketchStyle},
    },
};

/// Where a handle sits on its node's edge (§4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum HandlePlacement {
    Top,
    Right,
    #[default]
    Bottom,
    Left,
}

/// What a handle accepts (§4). `Loose` is React Flow's bidirectional handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum HandleDirection {
    #[default]
    Source,
    Target,
    Loose,
}

/// A connection point on a node (§4).
///
/// **`hidden` does not mean disconnected.** §4 asks for hidden handles that
/// remain geometrically connectable, so this is a paint flag only; routing and
/// hit-testing ignore it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Handle {
    pub id: HandleId,
    pub placement: HandlePlacement,
    pub direction: HandleDirection,
    /// Position along the placement edge, `0.0` at the top/left corner of that
    /// edge and `1.0` at the other end. `0.5` is centred, which is where a
    /// node's single handle sits.
    ///
    /// A fraction rather than a world offset so a resized node keeps its
    /// handles distributed — the arbitrary placement §4 also asks for is a
    /// fraction outside `0.0..=1.0`, which this deliberately does not clamp.
    pub offset: f32,
    /// `None` is unlimited (§4's connection limits).
    pub max_connections: Option<u32>,
    pub hidden: bool,
}

impl Default for Handle {
    fn default() -> Handle {
        Handle {
            id: HandleId::new(""),
            placement: HandlePlacement::default(),
            direction: HandleDirection::default(),
            offset: 0.5,
            max_connections: None,
            hidden: false,
        }
    }
}

impl Handle {
    pub fn new(
        id: impl Into<String>,
        placement: HandlePlacement,
        direction: HandleDirection,
    ) -> Handle {
        Handle {
            id: HandleId::new(id),
            placement,
            direction,
            ..Handle::default()
        }
    }

    /// Where this handle sits in world space, given its node's bounds.
    ///
    /// Here rather than in `geometry/` because it is a property of the handle
    /// model; the transform to screen space is `geometry/transform.rs`'s job
    /// and this never does it.
    pub fn world_position(&self, node_bounds: Rect) -> Vec2 {
        handle_world_position(self.placement, self.offset, node_bounds)
    }
}

/// **The one formula for where a handle sits**, shared by this model and by the
/// runtime's SoA [`HandleStore`](crate::runtime::HandleStore).
///
/// Free-standing because the runtime store holds a handle's placement and
/// offset in separate arrays and never assembles a [`Handle`] to ask. Two
/// copies of this would be a handle that painted in one place and routed from
/// another, which is a maddening bug to chase and costs nothing to prevent.
///
/// The offset is a fraction along the placement edge, measured from its top or
/// left corner, so a resized node keeps its handles distributed. It is **not**
/// clamped: §4's arbitrary placement is an offset outside `0.0..=1.0`, which
/// puts the handle off the edge on purpose.
pub fn handle_world_position(placement: HandlePlacement, offset: f32, node_bounds: Rect) -> Vec2 {
    let b = node_bounds.normalized();
    match placement {
        HandlePlacement::Top => Vec2::new(b.origin.x + b.width() * offset, b.origin.y),
        HandlePlacement::Bottom => b.origin + Vec2::new(b.width() * offset, b.height()),
        HandlePlacement::Left => Vec2::new(b.origin.x, b.origin.y + b.height() * offset),
        HandlePlacement::Right => b.origin + Vec2::new(b.width(), b.height() * offset),
    }
}

/// Which ordered end of a straight connector is being addressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConnectorEnd {
    Start,
    End,
}

/// A semantic attachment from a connector endpoint to another element.
///
/// `anchor` is normalized in the target's bounds. Keeping the target id as
/// well as the coincident point is what makes the endpoint follow a move or a
/// resize after save/load and undo/redo.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ConnectorAttachment {
    pub element: ElementId,
    pub anchor: Vec2,
}

/// One ordered endpoint of a straight connector.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ConnectorEndpoint {
    pub point: Vec2,
    pub attachment: Option<ConnectorAttachment>,
}

impl ConnectorEndpoint {
    pub fn free(point: Vec2) -> ConnectorEndpoint {
        ConnectorEndpoint {
            point,
            attachment: None,
        }
    }
}

/// The authoritative geometry of a straight line or arrow.
///
/// The bounding rectangle is derived and never swaps endpoint identity.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Connector {
    pub start: ConnectorEndpoint,
    pub end: ConnectorEndpoint,
}

impl Connector {
    pub fn new(start: Vec2, end: Vec2) -> Connector {
        Connector {
            start: ConnectorEndpoint::free(start),
            end: ConnectorEndpoint::free(end),
        }
    }

    /// Compatibility geometry for the rectangle representation written by
    /// document versions 1-3.
    pub fn from_rect(position: Vec2, size: Vec2) -> Connector {
        Connector::new(position, position + size)
    }

    pub fn endpoint(self, end: ConnectorEnd) -> ConnectorEndpoint {
        match end {
            ConnectorEnd::Start => self.start,
            ConnectorEnd::End => self.end,
        }
    }

    pub fn endpoint_mut(&mut self, end: ConnectorEnd) -> &mut ConnectorEndpoint {
        match end {
            ConnectorEnd::Start => &mut self.start,
            ConnectorEnd::End => &mut self.end,
        }
    }

    pub fn opposite(self, end: ConnectorEnd) -> ConnectorEndpoint {
        match end {
            ConnectorEnd::Start => self.end,
            ConnectorEnd::End => self.start,
        }
    }

    pub fn bounds(self) -> Rect {
        Rect::from_corners(self.start.point, self.end.point)
    }

    /// The label anchor and the inline editor's anchor: **the true segment
    /// midpoint**, which is a statement about the ordered endpoints rather than
    /// about any rectangle they happen to span.
    pub fn midpoint(self) -> Vec2 {
        (self.start.point + self.end.point) * 0.5
    }

    /// Rebuilds the segment so its derived rectangle is exactly `bounds`,
    /// **keeping each endpoint on the corner it already occupies**.
    ///
    /// This is how a whole-connector move or a rectangle-shaped resize reaches
    /// a connector without the rectangle ever becoming the authority: the
    /// corner each endpoint sits on is read off the current segment, so
    /// `start` stays `start` in all eight orientations. Setting an absolute
    /// rectangle this way is exact, which is what keeps
    /// [`EditCommand::SetNodePositions`](crate::commands::EditCommand::SetNodePositions)
    /// a sound inverse for a connector as well as for a node.
    pub fn with_bounds(self, bounds: Rect) -> Connector {
        let bounds = bounds.normalized();
        let (min, max) = (bounds.min(), bounds.max());
        let axis = |a: f32, b: f32, min: f32, max: f32| {
            if a <= b { (min, max) } else { (max, min) }
        };
        let (start_x, end_x) = axis(self.start.point.x, self.end.point.x, min.x, max.x);
        let (start_y, end_y) = axis(self.start.point.y, self.end.point.y, min.y, max.y);

        let mut moved = self;
        moved.start.point = Vec2::new(start_x, start_y);
        moved.end.point = Vec2::new(end_x, end_y);
        moved
    }
}

/// One element of the document.
///
/// Named `FlowNode` rather than `Element` because the type it most often sits
/// beside is [`FlowEdge`], and "node" is what every graph vocabulary the
/// requirements draw on calls it — even for a drawn shape, which is a node with
/// no handles.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FlowNode {
    pub id: ElementId,
    pub kind: ElementKind,
    /// The node's top-left corner in world space.
    pub position: Vec2,
    pub size: Vec2,
    /// Paint order among siblings; higher is nearer the viewer. Hit-testing
    /// walks it in reverse, which is why it is stored rather than implied by
    /// the `Vec` order — reordering a `Vec` invalidates every index into it.
    pub z: i32,
    /// The container (§11's frame or group) this node belongs to, if any.
    pub parent: Option<ElementId>,
    /// The node's own text. `None` is not the same as `Some("")`: an empty
    /// label still reserves its line, an absent one does not.
    pub label: Option<String>,
    /// Ordered geometry and semantic endpoint attachments for a straight line
    /// or arrow. `None` for every other kind.
    pub connector: Option<Connector>,
    pub handles: Vec<Handle>,
    pub style: ElementStyle,
    /// **A hyperlink on the element** — the property panel's Link action.
    ///
    /// A field of its own rather than a key in [`Metadata`], because the panel
    /// edits it and every edit in this engine is a typed command over a typed
    /// field; a free-form map would put one user-facing property outside the
    /// undo vocabulary. It is stored verbatim and **not validated here**: what
    /// counts as followable is the platform's answer, and `views/` is where a
    /// platform lives.
    pub link: Option<String>,
    /// **§10's picture, as a handle and a crop** — never as bytes.
    ///
    /// `Some` on an [`ElementKind::Image`] and `None` on everything else. The
    /// two could have been one field — a kind carrying its handle — and are
    /// deliberately not: [`ElementKind`] is matched on by the renderer, the
    /// hit-tester and the registry, it is `Copy`-sized by
    /// `kind_is_not_oversized`, and giving one variant a payload would put a
    /// document's largest table behind every `match` in the engine.
    ///
    /// The bytes are [`FlowDocument::images`]'s, keyed by
    /// [`NodeImage::handle`]. See [`image`](crate::models::image) for why they
    /// are shared by content hash and why they are embedded rather than
    /// referenced.
    pub image: Option<NodeImage>,
    pub hidden: bool,
    pub locked: bool,
}

impl Default for FlowNode {
    fn default() -> FlowNode {
        FlowNode {
            id: ElementId::NONE,
            kind: ElementKind::default(),
            position: Vec2::ZERO,
            size: Vec2::new(150.0, 40.0),
            z: 0,
            parent: None,
            label: None,
            connector: None,
            handles: Vec::new(),
            style: ElementStyle::default(),
            link: None,
            image: None,
            hidden: false,
            locked: false,
        }
    }
}

impl FlowNode {
    pub fn new(id: ElementId, kind: ElementKind, position: Vec2, size: Vec2) -> FlowNode {
        let connector = matches!(
            kind,
            ElementKind::Linear(LinearKind::Line | LinearKind::Arrow)
        )
        .then(|| Connector::from_rect(position, size));
        FlowNode {
            id,
            kind,
            position,
            size,
            connector,
            ..FlowNode::default()
        }
    }

    /// The node's world-space rectangle. **The culling and hit-testing unit** —
    /// the spatial index stores exactly this. A connector derives it from its
    /// ordered segment; the rectangle never defines or reorders that segment.
    pub fn bounds(&self) -> Rect {
        self.connector
            .map(Connector::bounds)
            .unwrap_or_else(|| Rect::new(self.position, self.size))
    }

    pub fn handle(&self, id: &HandleId) -> Option<&Handle> {
        self.handles.iter().find(|h| &h.id == id)
    }
}

/// One end of an edge.
///
/// `handle` is optional because §4 asks for a whole-node connection mode: an
/// endpoint with no handle attaches to the node itself, and the edge router
/// picks the nearest point on its border.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Endpoint {
    pub node: ElementId,
    pub handle: Option<HandleId>,
}

impl Endpoint {
    pub fn node(node: ElementId) -> Endpoint {
        Endpoint { node, handle: None }
    }

    pub fn handle(node: ElementId, handle: impl Into<String>) -> Endpoint {
        Endpoint {
            node,
            handle: Some(HandleId::new(handle)),
        }
    }
}

/// A connection between two endpoints (§8).
///
/// **It stores no geometry.** The route is derived from the two endpoints and
/// [`routing`](FlowEdge::routing), it is cached in the geometry cache, and it
/// is rebuilt when either endpoint moves — which is the graph engine's
/// dirty-propagation requirement, and would be defeated by persisting a `Vec<Point>` here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FlowEdge {
    pub id: ElementId,
    pub source: Endpoint,
    pub target: Endpoint,
    pub routing: EdgeRouting,
    pub label: Option<String>,
    pub style: ElementStyle,
    /// A hyperlink on the edge. See [`FlowNode::link`].
    pub link: Option<String>,
    pub z: i32,
    pub hidden: bool,
}

impl Default for FlowEdge {
    fn default() -> FlowEdge {
        FlowEdge {
            id: ElementId::NONE,
            source: Endpoint::node(ElementId::NONE),
            target: Endpoint::node(ElementId::NONE),
            routing: EdgeRouting::default(),
            label: None,
            style: ElementStyle::default(),
            link: None,
            z: 0,
            hidden: false,
        }
    }
}

impl FlowEdge {
    pub fn new(id: ElementId, source: Endpoint, target: Endpoint) -> FlowEdge {
        FlowEdge {
            id,
            source,
            target,
            ..FlowEdge::default()
        }
    }
}

/// Document-wide authoring choices.
///
/// [`render_style`](DocumentSettings::render_style) is here because a
/// hand-drawn diagram is hand-drawn every time it is opened — it is the
/// author's choice, not the viewer's. [`render_quality`](DocumentSettings::render_quality)
/// is here for the opposite reason and is the weaker of the two: it *is* a
/// viewer preference, but a document that was authored as a 100,000-node scene
/// carries the knowledge that it needs a coarse tolerance, and a later
/// application-level setting can still override it at the render boundary.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DocumentSettings {
    pub render_style: RenderStyle,
    /// The hand [`RenderStyle::Sketch`] draws with. Carried whatever the
    /// current style is, so switching to sketch and back is a toggle rather
    /// than a settings edit — see [`DocumentSettings::sketch_request`].
    pub sketch: SketchStyle,
    pub render_quality: RenderQuality,
    /// The style a newly created element starts from.
    pub default_style: ElementStyle,
}

impl DocumentSettings {
    /// **The one question the renderer asks about §13's style**: what hand, if
    /// any, is this frame drawn with?
    ///
    /// `Option<SketchStyle>` rather than a comparison against
    /// [`RenderStyle::Sketch`] at each call site, so a later variant —
    /// blueprint, presentation — decides here whether it wants perturbed
    /// geometry and no painter has to learn about it. A `roughness` of zero
    /// answers `None` too: a hand that does not move is a clean drawing, and
    /// paying the sketch path's cost for it would be paying for nothing.
    pub fn sketch_request(&self) -> Option<SketchStyle> {
        match self.render_style {
            RenderStyle::Clean => None,
            RenderStyle::Sketch => (self.sketch.roughness > 0.0).then_some(self.sketch),
        }
    }
}

/// Free-form metadata (§31), carried through a load/save cycle untouched.
///
/// A `serde_json::Value` map rather than a typed struct on purpose: it is the
/// forward-compatibility valve. A field a future version adds here survives a
/// round trip through this build instead of being dropped, which is the
/// difference between "an older dodo can open it" and "an older dodo destroys
/// it".
pub type Metadata = serde_json::Map<String, serde_json::Value>;

/// The persistent document.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct FlowDocument {
    pub nodes: Vec<FlowNode>,
    pub edges: Vec<FlowEdge>,
    /// **§10's pictures, one copy each** (§10: *do not duplicate raw image
    /// bytes per element*).
    ///
    /// Keyed by content hash, so two elements showing one picture — and the
    /// Duplicate action — share the entry by construction rather than by the
    /// insert path remembering to look. A `BTreeMap` rather than a `HashMap`
    /// because the map is written to a file: an iteration order that varies run
    /// to run would make two saves of one unchanged document differ, which
    /// turns every diff of a canvas file into noise.
    ///
    /// It is the one part of a document whose size is not proportional to the
    /// diagram, and [`image`](crate::models::image)'s module doc carries the
    /// argument for embedding it anyway.
    pub images: BTreeMap<ImageHandle, ImageResource>,
    pub settings: DocumentSettings,
    pub metadata: Metadata,
    /// The id watermark. Serialized so a reopened document never reissues an id
    /// that an undo history or a clipboard may still name — see
    /// [`IdAllocator`].
    pub ids: IdAllocator,
}

impl FlowDocument {
    pub fn new() -> FlowDocument {
        FlowDocument::default()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty() && self.edges.is_empty()
    }

    /// The resource an element's [`NodeImage`] names, or `None` for a document
    /// whose table has lost it — a hand edit, or a merge that took the nodes
    /// and not the pictures. `None` is drawn as a placeholder rather than
    /// refused: a diagram with one missing picture is still a diagram.
    pub fn image(&self, handle: ImageHandle) -> Option<&ImageResource> {
        self.images.get(&handle)
    }

    /// Files a resource under its own handle and returns it, leaving an entry
    /// that is already there alone — **the sharing rule, as the only way in**.
    pub fn insert_image(&mut self, resource: ImageResource) -> ImageHandle {
        let handle = resource.handle();
        self.images.entry(handle).or_insert(resource);
        handle
    }

    /// Issues a fresh id. The only way an element should get one.
    pub fn next_id(&mut self) -> ElementId {
        self.ids.next_id()
    }

    /// Adds a node, giving it a fresh id and returning it.
    pub fn add_node(&mut self, kind: ElementKind, position: Vec2, size: Vec2) -> ElementId {
        let id = self.next_id();
        self.nodes.push(FlowNode::new(id, kind, position, size));
        id
    }

    /// Connects two endpoints, giving the edge a fresh id and returning it.
    ///
    /// Does **not** validate that the endpoints exist — §4's connection
    /// validation is the graph engine's job, and a document loaded
    /// from disk may legitimately contain a dangling edge that the loader
    /// reports rather than silently drops.
    pub fn add_edge(&mut self, source: Endpoint, target: Endpoint) -> ElementId {
        let id = self.next_id();
        self.edges.push(FlowEdge::new(id, source, target));
        id
    }

    pub fn node(&self, id: ElementId) -> Option<&FlowNode> {
        self.nodes.iter().find(|n| n.id == id)
    }

    pub fn node_mut(&mut self, id: ElementId) -> Option<&mut FlowNode> {
        self.nodes.iter_mut().find(|n| n.id == id)
    }

    pub fn edge(&self, id: ElementId) -> Option<&FlowEdge> {
        self.edges.iter().find(|e| e.id == id)
    }

    /// The world rectangle enclosing every node, or `None` for a document with
    /// no nodes.
    ///
    /// `None` rather than `Rect::ZERO` because zoom-to-fit on an empty document
    /// must not frame the origin as though something were there — see
    /// [`Rect::of_rects`].
    ///
    /// Hidden nodes count. They are hidden, not absent, and framing the
    /// document must not move when a layer is toggled.
    pub fn content_bounds(&self) -> Option<Rect> {
        Rect::of_rects(self.nodes.iter().map(FlowNode::bounds))
    }

    /// Lifts the id watermark above every id present.
    ///
    /// Called by the loader: a document produced by another build, a merge or a
    /// hand edit may hold ids above the stored watermark, and issuing a
    /// duplicate would make two elements indistinguishable.
    pub fn reseed_ids(&mut self) {
        let highest = self
            .nodes
            .iter()
            .map(|n| n.id)
            .chain(self.edges.iter().map(|e| e.id))
            .max();

        if let Some(highest) = highest {
            self.ids.observe(highest);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DocumentSettings, Endpoint, FlowDocument, FlowEdge, FlowNode, Handle, HandleDirection,
        HandlePlacement,
    };
    use crate::{
        geometry::{Rect, Vec2},
        models::{ElementKind, ShapeKind, ids::ElementId},
    };

    fn document() -> FlowDocument {
        let mut doc = FlowDocument::new();
        let a = doc.add_node(
            ElementKind::default(),
            Vec2::new(0.0, 0.0),
            Vec2::new(150.0, 40.0),
        );
        let b = doc.add_node(
            ElementKind::Shape(ShapeKind::Diamond),
            Vec2::new(300.0, 200.0),
            Vec2::new(100.0, 100.0),
        );
        doc.add_edge(Endpoint::handle(a, "out"), Endpoint::handle(b, "in"));
        doc
    }

    #[test]
    fn a_new_document_is_empty_and_frames_nothing() {
        let doc = FlowDocument::new();

        assert!(doc.is_empty());
        assert_eq!(doc.content_bounds(), None);
    }

    #[test]
    fn every_element_gets_a_distinct_id() {
        let doc = document();

        let ids: Vec<ElementId> = doc
            .nodes
            .iter()
            .map(|n| n.id)
            .chain(doc.edges.iter().map(|e| e.id))
            .collect();

        let mut unique = ids.clone();
        unique.sort();
        unique.dedup();

        assert_eq!(ids.len(), 3);
        assert_eq!(unique.len(), 3);
        assert!(!ids.contains(&ElementId::NONE));
    }

    #[test]
    fn lookup_finds_elements_by_id_and_misses_cleanly() {
        let doc = document();
        let id = doc.nodes[1].id;

        assert_eq!(
            doc.node(id).map(|n| n.position),
            Some(Vec2::new(300.0, 200.0))
        );
        assert!(doc.edge(doc.edges[0].id).is_some());
        assert!(doc.node(ElementId::new(9_999)).is_none());
        assert!(doc.edge(ElementId::new(9_999)).is_none());
    }

    #[test]
    fn content_bounds_encloses_every_node() {
        let bounds = document().content_bounds().expect("two nodes");

        assert_eq!(bounds, Rect::new(Vec2::ZERO, Vec2::new(400.0, 300.0)));
    }

    #[test]
    fn content_bounds_covers_nodes_at_negative_coordinates() {
        // The canvas is infinite in both directions; a bounds routine that
        // starts from the origin instead of from the first element would frame
        // the wrong region for a document drawn up and left.
        let mut doc = FlowDocument::new();
        doc.add_node(
            ElementKind::default(),
            Vec2::new(-500.0, -300.0),
            Vec2::new(100.0, 50.0),
        );
        doc.add_node(
            ElementKind::default(),
            Vec2::new(-100.0, -100.0),
            Vec2::new(100.0, 50.0),
        );

        assert_eq!(
            doc.content_bounds(),
            Some(Rect::new(
                Vec2::new(-500.0, -300.0),
                Vec2::new(500.0, 250.0)
            ))
        );
    }

    #[test]
    fn node_bounds_is_position_and_size() {
        let node = FlowNode::new(
            ElementId::new(1),
            ElementKind::default(),
            Vec2::new(10.0, 20.0),
            Vec2::new(30.0, 40.0),
        );

        assert_eq!(
            node.bounds(),
            Rect::new(Vec2::new(10.0, 20.0), Vec2::new(30.0, 40.0))
        );
    }

    #[test]
    fn handles_sit_on_the_edge_their_placement_names() {
        let bounds = Rect::new(Vec2::new(100.0, 100.0), Vec2::new(200.0, 80.0));

        let cases = [
            (HandlePlacement::Top, 0.5, Vec2::new(200.0, 100.0)),
            (HandlePlacement::Bottom, 0.5, Vec2::new(200.0, 180.0)),
            (HandlePlacement::Left, 0.5, Vec2::new(100.0, 140.0)),
            (HandlePlacement::Right, 0.5, Vec2::new(300.0, 140.0)),
            (HandlePlacement::Top, 0.0, Vec2::new(100.0, 100.0)),
            (HandlePlacement::Top, 1.0, Vec2::new(300.0, 100.0)),
            (HandlePlacement::Right, 0.25, Vec2::new(300.0, 120.0)),
        ];

        for (placement, offset, expected) in cases {
            let handle = Handle {
                offset,
                ..Handle::new("h", placement, HandleDirection::Source)
            };
            assert_eq!(
                handle.world_position(bounds),
                expected,
                "{placement:?} at {offset}"
            );
        }
    }

    #[test]
    fn a_hidden_handle_is_still_placed() {
        // §4: hidden handles remain geometrically connectable.
        let handle = Handle {
            hidden: true,
            ..Handle::new("h", HandlePlacement::Right, HandleDirection::Source)
        };

        assert_eq!(
            handle.world_position(Rect::new(Vec2::ZERO, Vec2::new(100.0, 100.0))),
            Vec2::new(100.0, 50.0)
        );
    }

    #[test]
    fn a_handle_is_found_by_its_id() {
        let mut node = FlowNode::default();
        node.handles.push(Handle::new(
            "out",
            HandlePlacement::Right,
            HandleDirection::Source,
        ));

        assert!(node.handle(&crate::models::HandleId::new("out")).is_some());
        assert!(node.handle(&crate::models::HandleId::new("in")).is_none());
    }

    #[test]
    fn reseed_lifts_the_watermark_above_ids_this_build_did_not_issue() {
        let mut doc = FlowDocument::new();
        doc.nodes.push(FlowNode::new(
            ElementId::new(4_000),
            ElementKind::default(),
            Vec2::ZERO,
            Vec2::ONE,
        ));
        doc.edges.push(FlowEdge::new(
            ElementId::new(9_000),
            Endpoint::node(ElementId::new(4_000)),
            Endpoint::node(ElementId::new(4_000)),
        ));

        doc.reseed_ids();

        assert_eq!(doc.next_id(), ElementId::new(9_001));
    }

    #[test]
    fn document_holds_no_runtime_state() {
        // §31's rule, pinned. Adding a spatial index, an adjacency cache or a
        // tessellation to `FlowDocument` fails here rather than shipping a
        // document format that carries state it should have rebuilt.
        let value = serde_json::to_value(document()).unwrap();
        let mut keys: Vec<&str> = value
            .as_object()
            .expect("a document is a JSON object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort();

        assert_eq!(
            keys,
            ["edges", "ids", "images", "metadata", "nodes", "settings"]
        );
    }

    /// **The sharing rule, at the document level**: one file inserted twice is
    /// one entry, and two elements naming it are two handles.
    #[test]
    fn one_picture_inserted_twice_is_one_resource() {
        use crate::models::image::{ImageFormat, ImageResource, NodeImage};

        let mut doc = FlowDocument::new();
        let bytes = vec![137u8, 80, 78, 71, 13, 10, 26, 10];
        let first = doc.insert_image(ImageResource::new(ImageFormat::Png, 64, 32, bytes.clone()));
        let second = doc.insert_image(ImageResource::new(ImageFormat::Png, 64, 32, bytes));

        assert_eq!(first, second);
        assert_eq!(doc.images.len(), 1, "the bytes were stored twice");

        let a = doc.add_node(ElementKind::Image, Vec2::ZERO, Vec2::new(64.0, 32.0));
        let b = doc.add_node(ElementKind::Image, Vec2::ONE, Vec2::new(64.0, 32.0));
        doc.node_mut(a).unwrap().image = Some(NodeImage::new(first));
        doc.node_mut(b).unwrap().image = Some(NodeImage::new(second));

        assert_eq!(doc.images.len(), 1);
        assert!(doc.image(first).is_some());
    }

    #[test]
    fn settings_travel_with_the_document() {
        let mut doc = FlowDocument::new();
        doc.settings.render_style = crate::models::RenderStyle::Sketch;
        doc.settings.render_quality = crate::models::RenderQuality::DRAFT;

        let json = serde_json::to_string(&doc.settings).unwrap();
        let back: DocumentSettings = serde_json::from_str(&json).unwrap();

        assert_eq!(back, doc.settings);
    }
}
