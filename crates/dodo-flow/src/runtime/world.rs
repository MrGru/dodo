//! [`GraphWorld`] — §17's runtime, and the one place the dirty-propagation rule
//! is written down.
//!
//! # The rule, in one function
//!
//! §19 draws the propagation as a diagram; [`GraphWorld::move_node`] is that
//! diagram:
//!
//! ```text
//! drag node 42
//!   +-- update node 42's position          NodeStore::set_position
//!   +-- mark its render transform dirty     NodeDirty::POSITION
//!   +-- update its spatial-index entry      NodeDirty::SPATIAL -> the spatial queue
//!   +-- find incident edges                 AdjacencyIndex::incident_edges  (§40 rule 2)
//!   +-- mark only those edge geometries      EdgeDirty::GEOMETRY
//! ```
//!
//! Nothing in it is proportional to the size of the graph, and
//! `moving_one_node_in_a_huge_graph_rebuilds_only_its_own_edges` at the bottom
//! of this file asserts exactly that on 100,000 nodes and 500,000 edges. That
//! test is the reason the stores, the adjacency index and the dirty flags are
//! shaped the way they are; if it ever needs relaxing, the architecture has
//! stopped being worth its complexity.
//!
//! # Why the stores do not mark their own flags
//!
//! `NodeStore::set_position` moves a node and marks nothing. It would be
//! natural to have it mark `POSITION` itself — and then moving a node through
//! the store would produce a node whose *edges* were never invalidated, which
//! is a half-correct path to the same job and the kind of thing that is
//! discovered as a rendering artefact months later. The stores are storage; the
//! world is what knows that a node has edges.
//!
//! # What is deliberately missing
//!
//! - **No removal.** See [`NodeStore`](crate::runtime::NodeStore)'s module doc:
//!   removal is a command-layer question (§30, Phase 7) because undo is what
//!   decides between a tombstone and a swap-remove.
//! - **No spatial index.** [`DirtyState::spatial_updates`] collects what Phase
//!   4's uniform grid will consume. `hit_test` takes its candidates as an
//!   argument for the same reason — see [`crate::runtime::hit`].
//! - **No hierarchy index.** §11's frames and groups move their children;
//!   [`NodeCold::parent`](crate::runtime::NodeCold::parent) records the
//!   relationship and nothing resolves it yet.
//!
//! **This file names no UI framework.**

use std::collections::HashMap;
use std::sync::Arc;

use crate::{
    geometry::{
        Attachment, EdgeRoute, Rect, ResizeCorner, RouteOptions, Side, Vec2, distance_to_segment,
    },
    models::{
        Connector, ConnectorAttachment, ConnectorEnd, DocumentSettings, EdgeIndex, EdgeRouting,
        ElementId, ElementKind, ElementStyle, Endpoint, FlowDocument, FlowEdge, FlowNode, Handle,
        HandleDirection, HandleId, HandleIndex, HandlePlacement, IdAllocator, ImageHandle,
        ImageResource, Metadata, NodeImage, NodeIndex, handle_world_position,
    },
    runtime::{
        AdjacencyIndex, BoxQuery, BoxSelectMode, ConnectionError, ConnectionRules, ConnectorSnap,
        DirtyState, EdgeDirty, EdgeEnd, EdgeFlags, EdgeGeometryStore, EdgeSpec, EdgeStore,
        HandleSpec, HandleStore, HitTolerance, NodeDirty, NodeFlags, NodeSpec, NodeStore,
        PointerTarget, SelectionSet,
    },
};

/// What a document brought with it that the world could not represent.
///
/// Returned rather than logged, because dodo installs no logger and because the
/// caller is the only one who knows whether a dangling edge is worth telling
/// the user about. An empty report is the normal case.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LoadReport {
    /// Edges whose source or target named a node the document does not contain.
    /// They are **dropped**: an edge with one end nowhere has no geometry, and
    /// keeping it would mean every routing path handling a case that only a
    /// corrupt file produces.
    pub dangling_edges: Vec<ElementId>,
    /// Edges naming a handle that its node does not have. They are **kept**,
    /// attached to the node itself — §4's whole-node connection mode is exactly
    /// the fallback this case wants, and the author's connection survives.
    pub unresolved_handles: Vec<ElementId>,
    /// Straight connectors whose persisted attachment named a missing,
    /// connector, or otherwise invalid target. The endpoint is kept at its
    /// persisted point and safely detached.
    pub unresolved_connector_attachments: Vec<ElementId>,
}

impl LoadReport {
    pub fn is_clean(&self) -> bool {
        self.dangling_edges.is_empty()
            && self.unresolved_handles.is_empty()
            && self.unresolved_connector_attachments.is_empty()
    }
}

/// §17's runtime: the stores, the indices and the change tracking, in one owner.
#[derive(Debug, Clone, Default)]
pub struct GraphWorld {
    nodes: NodeStore,
    edges: EdgeStore,
    handles: HandleStore,
    adjacency: AdjacencyIndex,
    /// Target node -> bound straight-connector endpoints. The connector
    /// counterpart of `adjacency`, so moving a target never scans the document.
    connector_bindings: Vec<Vec<(NodeIndex, ConnectorEnd)>>,
    geometry: EdgeGeometryStore,
    dirty: DirtyState,
    rules: ConnectionRules,

    /// §28's selection, as compact ids. Kept here rather than on the view
    /// because the store flags (`NodeFlags::SELECTED`) are the painter's
    /// answer and this is the command layer's, and two owners is how they
    /// drift — [`GraphWorld::set_node_selected`] writes both or neither.
    selection: SelectionSet,

    /// Document id → runtime index. Touched on load, on save and when an
    /// external reference (an undo entry, a clipboard paste) has to be resolved
    /// — never per frame, which is why a `HashMap` is the right shape here and
    /// would be the wrong one inside the paint loop (§40 rule 9).
    node_by_id: HashMap<ElementId, NodeIndex>,
    edge_by_id: HashMap<ElementId, EdgeIndex>,

    ids: IdAllocator,
    settings: DocumentSettings,
    metadata: Metadata,

    /// **How many elements sit off the default layer**, nodes and edges
    /// together — the one question [`GraphWorld::is_layered`] answers, and the
    /// reason it is a counter rather than a scan.
    ///
    /// Every frame asks whether z-order is in play (see
    /// [`render::scene`](crate::render::scene)), and the answer decides both
    /// whether the planning walk is sorted and whether a quad-bodied element
    /// has to be promoted to a path to take its place in the order. A scan of
    /// the document per frame is exactly what §40 rule 1 forbids; a counter
    /// maintained by the two writers that can change a `z` costs one branch per
    /// edit.
    ///
    /// **Tombstoned elements are counted too**, deliberately. A removed element
    /// keeps its `z`, an undo can bring it back, and counting only live ones
    /// would mean deleting the front-most shape silently reordered everything
    /// else for a frame. Over-counting keeps the frame in layered mode, which
    /// is the safe direction: it costs paths, never correctness.
    nonzero_z: u32,

    /// **§10's pictures, one `Arc` each** — the runtime half of
    /// [`FlowDocument::images`](crate::models::FlowDocument::images).
    ///
    /// Keyed by content hash, so the sharing rule (*do not duplicate raw image
    /// bytes per element*) holds here for the same reason it holds in the
    /// document: there is nowhere to put a second copy. Duplicating an image
    /// element copies a [`NodeImage`] — a `u64` and four floats — and the
    /// `Arc` is not even touched.
    ///
    /// **Nothing is ever removed from it**, and that is deliberate. A resource
    /// is not an element: removal is a tombstone precisely so an undo can bring
    /// an element back, and a store that dropped the bytes when the last
    /// element referencing them went would make that undo restore a hole.
    /// [`to_document`](GraphWorld::to_document) writes only the resources live
    /// elements name, so an orphan costs memory for the session and nothing in
    /// the file.
    images: HashMap<ImageHandle, Arc<ImageResource>>,
}

impl GraphWorld {
    pub fn new() -> GraphWorld {
        GraphWorld::default()
    }

    // ---- loading and saving ---------------------------------------------

    /// Builds a world from a document, resolving every string id to a runtime
    /// index exactly once (§4).
    ///
    /// Connections are added under [`ConnectionRules::PERMISSIVE`] whatever the
    /// world's own rules will be: the file is the authority, and refusing an
    /// edge that a document says exists would silently destroy the author's
    /// work the next time they saved.
    pub fn from_document(document: &FlowDocument) -> (GraphWorld, LoadReport) {
        let mut world = GraphWorld::new();
        let mut report = LoadReport::default();

        world.settings = document.settings.clone();
        world.metadata = document.metadata.clone();
        world.ids = document.ids.clone();
        world.images = document
            .images
            .iter()
            .map(|(&handle, resource)| (handle, Arc::new(resource.clone())))
            .collect();
        world.reserve(document.nodes.len(), document.edges.len());

        for node in &document.nodes {
            let index = world.add_node(NodeSpec {
                id: node.id,
                kind: node.kind.clone(),
                position: node.position,
                size: node.size,
                z: node.z,
                style: node.style.clone(),
                label: node.label.clone(),
                parent: node.parent,
                link: node.link.clone(),
                image: node.image,
                connector: node.connector,
                hidden: node.hidden,
                locked: node.locked,
            });

            for handle in &node.handles {
                world.add_handle(
                    index,
                    HandleSpec {
                        id: handle.id.clone(),
                        placement: handle.placement,
                        direction: handle.direction,
                        offset: handle.offset,
                        max_connections: handle.max_connections,
                        hidden: handle.hidden,
                    },
                );
            }
        }

        world.rebuild_connector_bindings(&mut report);

        let rules = std::mem::replace(&mut world.rules, ConnectionRules::PERMISSIVE);

        for edge in &document.edges {
            let (Some(source_node), Some(target_node)) = (
                world.node_index(edge.source.node),
                world.node_index(edge.target.node),
            ) else {
                report.dangling_edges.push(edge.id);
                continue;
            };

            let source = world.resolve_end(source_node, edge.source.handle.as_ref());
            let target = world.resolve_end(target_node, edge.target.handle.as_ref());
            if (edge.source.handle.is_some() && source.handle.is_none())
                || (edge.target.handle.is_some() && target.handle.is_none())
            {
                report.unresolved_handles.push(edge.id);
            }

            let spec = EdgeSpec {
                id: edge.id,
                source,
                target,
                routing: edge.routing,
                style: edge.style.clone(),
                label: edge.label.clone(),
                link: edge.link.clone(),
                z: edge.z,
                hidden: edge.hidden,
            };

            // Permissive rules refuse nothing a well-formed document can hold,
            // so this cannot fail — but the result is inspected rather than
            // discarded, so that a future rule addition surfaces here instead of
            // dropping an edge quietly.
            if world.connect_with(spec).is_err() {
                report.dangling_edges.push(edge.id);
            }
        }

        world.rules = rules;
        world.nonzero_z = document
            .nodes
            .iter()
            .filter(|node| node.z != 0)
            .count()
            .saturating_add(document.edges.iter().filter(|edge| edge.z != 0).count())
            as u32;
        (world, report)
    }

    /// Writes the world back into a document.
    ///
    /// The inverse of [`from_document`](GraphWorld::from_document) for
    /// everything the world holds. It is not a general "save": the command
    /// layer (Phase 7) is what will keep a document and a world in step during
    /// editing, and this is the load/save round trip that proves the runtime
    /// loses nothing on the way through.
    pub fn to_document(&self) -> FlowDocument {
        let mut document = FlowDocument::new();
        document.settings = self.settings.clone();
        document.metadata = self.metadata.clone();
        document.ids = self.ids.clone();

        // **Tombstones are dropped here, and this is the only compaction
        // there is** — see `NodeStore`'s module doc. A saved document has no
        // holes, so reopening it renumbers every slot; nothing else may,
        // because the undo history holds slot numbers.
        document.nodes.reserve(self.nodes.len());
        for node in self.nodes.live_indices() {
            let cold = self.nodes.cold(node);
            document.nodes.push(FlowNode {
                id: self.nodes.id(node),
                kind: cold.kind.clone(),
                position: self.nodes.position(node),
                size: self.nodes.size(node),
                z: self.nodes.z(node),
                parent: cold.parent,
                label: cold.label.as_deref().map(str::to_owned),
                handles: self
                    .nodes
                    .handles(node)
                    .map(|handle| Handle {
                        id: self.handles.id(handle).clone(),
                        placement: self.handles.placement(handle),
                        direction: self.handles.direction(handle),
                        offset: self.handles.offset(handle),
                        max_connections: self.handles.limit(handle),
                        hidden: self.handles.is_hidden(handle),
                    })
                    .collect(),
                style: self.nodes.style(node).clone(),
                link: cold.link.clone(),
                image: cold.image,
                connector: cold.connector,
                hidden: self.nodes.is_hidden(node),
                locked: self.nodes.is_locked(node),
            });
        }

        // **Only the pictures live elements name.** The store keeps every
        // resource it has ever seen so an undo can restore an element that
        // still points at one; a *file* has no undo stack, so writing an orphan
        // would embed a photograph nobody can see and nobody can delete. The
        // walk is over the saved nodes rather than over the store, so the
        // question asked is "what does this document need?" rather than "what
        // is left over?".
        for node in &document.nodes {
            let Some(image) = node.image else {
                continue;
            };
            if let Some(resource) = self.images.get(&image.handle) {
                document
                    .images
                    .entry(image.handle)
                    .or_insert_with(|| resource.as_ref().clone());
            }
        }

        document.edges.reserve(self.edges.len());
        for edge in self.edges.live_indices() {
            document.edges.push(FlowEdge {
                id: self.edges.id(edge),
                source: self.endpoint(self.edges.source(edge)),
                target: self.endpoint(self.edges.target(edge)),
                routing: self.edges.routing(edge),
                label: self.edges.label(edge).map(|it| it.to_string()),
                style: self.edges.style(edge).clone(),
                link: self.edges.link(edge).map(str::to_owned),
                z: self.edges.z(edge),
                hidden: self.edges.is_hidden(edge),
            });
        }

        document
    }

    // ---- §10's image resources ------------------------------------------

    /// **Files a picture's bytes under their own content hash**, and answers
    /// the handle an element should carry.
    ///
    /// Adding bytes to this table is deliberately **not** an edit and records
    /// no undo step, which is the one place [`FlowEditor`](crate::commands::FlowEditor)'s
    /// invariant needs reading carefully. That invariant is about what
    /// [`to_document`](GraphWorld::to_document) can observe *in a node or an
    /// edge*, and a resource nothing references is written to no file — so
    /// registering one changes nothing a document can see, and un-registering
    /// it would break the redo that is about to name it. The undoable half is
    /// the element, and it goes through the applier like everything else.
    pub fn insert_image(&mut self, resource: ImageResource) -> ImageHandle {
        let handle = resource.handle();
        self.images
            .entry(handle)
            .or_insert_with(|| Arc::new(resource));
        handle
    }

    /// The bytes behind a handle, shared. `None` for a handle whose resource is
    /// missing — a hand-edited file, or a merge that took the nodes and left
    /// the pictures — which the renderer draws as a placeholder.
    pub fn image(&self, handle: ImageHandle) -> Option<&Arc<ImageResource>> {
        self.images.get(&handle)
    }

    /// How many distinct pictures the world holds. **The sharing rule as a
    /// number**, and what the tests assert against.
    pub fn image_count(&self) -> usize {
        self.images.len()
    }

    fn endpoint(&self, end: EdgeEnd) -> Endpoint {
        Endpoint {
            node: self.nodes.id(end.node),
            handle: end
                .handle
                .get()
                .map(|handle| self.handles.id(handle).clone()),
        }
    }

    fn resolve_end(&self, node: NodeIndex, handle: Option<&HandleId>) -> EdgeEnd {
        match handle.and_then(|id| self.handle_index(node, id)) {
            Some(handle) => EdgeEnd::handle(node, handle),
            None => EdgeEnd::node(node),
        }
    }

    // ---- accessors -------------------------------------------------------

    pub fn nodes(&self) -> &NodeStore {
        &self.nodes
    }

    pub fn edges(&self) -> &EdgeStore {
        &self.edges
    }

    pub fn handles(&self) -> &HandleStore {
        &self.handles
    }

    pub fn adjacency(&self) -> &AdjacencyIndex {
        &self.adjacency
    }

    pub fn geometry(&self) -> &EdgeGeometryStore {
        &self.geometry
    }

    pub fn dirty(&self) -> &DirtyState {
        &self.dirty
    }

    /// For the consumer of an invalidation — the renderer clearing what it has
    /// drawn, the spatial index draining its two queues.
    pub fn dirty_mut(&mut self) -> &mut DirtyState {
        &mut self.dirty
    }

    /// Drops both spatial queues, once the index has consumed them.
    ///
    /// The frame order this belongs to is fixed and each step depends on the
    /// one before it:
    ///
    /// ```text
    /// rebuild_dirty_geometry()   routes become current
    /// SpatialIndex::sync()       re-indexes from those routes
    /// clear_spatial_updates()    the queues are spent
    /// SpatialIndex::query_visible()
    /// ```
    ///
    /// Syncing before the rebuild would index an edge at the place it used to
    /// be, which is a missing or ghost edge one frame later — the failure mode
    /// culling bugs always take.
    pub fn clear_spatial_updates(&mut self) {
        self.dirty.clear_spatial_updates();
    }

    pub fn rules(&self) -> ConnectionRules {
        self.rules
    }

    pub fn set_rules(&mut self, rules: ConnectionRules) {
        self.rules = rules;
    }

    pub fn settings(&self) -> &DocumentSettings {
        &self.settings
    }

    pub fn settings_mut(&mut self) -> &mut DocumentSettings {
        &mut self.settings
    }

    pub fn route_options(&self) -> &RouteOptions {
        self.geometry.options()
    }

    pub fn set_route_options(&mut self, options: RouteOptions) {
        // `set_options` has already invalidated every route; this queues them,
        // because a stale route nobody rebuilds is an edge that stops being
        // painted. The only operation in this file that touches the whole
        // graph, and a settings change rather than an interaction.
        self.geometry.set_options(options);
        for edge in self.edges.live_indices() {
            self.dirty
                .mark_edge(edge, EdgeDirty::GEOMETRY | EdgeDirty::SPATIAL);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty() && self.edges.is_empty()
    }

    /// Whether this slot holds a node that has not been deleted — the question
    /// every reader asks, and the one a caller holding an index from before a
    /// removal must ask. See [`NodeStore::is_live`].
    pub fn node_is_live(&self, node: NodeIndex) -> bool {
        self.nodes.is_live(node)
    }

    pub fn edge_is_live(&self, edge: EdgeIndex) -> bool {
        self.edges.is_live(edge)
    }

    pub fn node_index(&self, id: ElementId) -> Option<NodeIndex> {
        self.node_by_id.get(&id).copied()
    }

    pub fn edge_index(&self, id: ElementId) -> Option<EdgeIndex> {
        self.edge_by_id.get(&id).copied()
    }

    /// A node's handle by its document name. Walks the node's own handles —
    /// proportional to that node's handle count, never to the world's.
    pub fn handle_index(&self, node: NodeIndex, id: &HandleId) -> Option<HandleIndex> {
        self.nodes
            .handles(node)
            .find(|handle| self.handles.id(*handle) == id)
    }

    /// The world rectangle enclosing every node, or `None` when there are none.
    pub fn content_bounds(&self) -> Option<Rect> {
        Rect::of_rects(
            self.nodes
                .live_indices()
                .map(|node| self.nodes.bounds(node)),
        )
    }

    pub fn reserve(&mut self, nodes: usize, edges: usize) {
        self.nodes.reserve(nodes);
        self.adjacency.reserve(nodes);
        self.connector_bindings.reserve(nodes);
        self.node_by_id.reserve(nodes);
        self.edges.reserve(edges);
        self.geometry.reserve(edges);
        self.edge_by_id.reserve(edges);
    }

    // ---- building --------------------------------------------------------

    /// Issues a fresh document id. The only way an element should get one.
    pub fn next_id(&mut self) -> ElementId {
        self.ids.next_id()
    }

    /// Adds a node. Every parallel array — the store's, the adjacency index's,
    /// the dirty flags' — grows here and only here, so they cannot get out of
    /// step.
    pub fn add_node(&mut self, spec: NodeSpec) -> NodeIndex {
        self.ids.observe(spec.id);
        let id = spec.id;
        let index = self.nodes.push(spec);

        self.adjacency.push_node();
        self.connector_bindings.push(Vec::new());
        self.dirty.push_node();
        self.node_by_id.insert(id, index);
        self.bind_connector(index);
        self.refresh_connector(index);

        // A brand-new node has never been drawn or indexed, so it is dirty from
        // birth rather than from its first move.
        self.dirty
            .mark_node(index, NodeDirty::POSITION | NodeDirty::SPATIAL);
        index
    }

    /// Adds a node with the given kind at the given place, allocating its id.
    pub fn create_node(&mut self, kind: ElementKind, position: Vec2, size: Vec2) -> NodeIndex {
        let id = self.next_id();
        self.add_node(NodeSpec::new(id, kind, position, size))
    }

    /// Adds a handle to a node (§4's dynamic handles).
    ///
    /// The node's incident edges are **not** invalidated: a new handle carries
    /// no connection, so no existing route changes. Adding one to a node whose
    /// edges float onto its border is the one case where that is arguable, and
    /// it is not the case a caller means.
    pub fn add_handle(&mut self, node: NodeIndex, spec: HandleSpec) -> HandleIndex {
        let handle = self.handles.push(node, spec);
        self.nodes.attach_handle(node, handle);
        self.dirty.mark_node(node, NodeDirty::HANDLES);
        handle
    }

    /// Where a handle sits in world space, right now.
    pub fn handle_position(&self, handle: HandleIndex) -> Vec2 {
        let node = self.handles.owner(handle);
        self.handles.world_position(handle, self.nodes.bounds(node))
    }

    // ---- connecting ------------------------------------------------------

    /// Connects two endpoints under this world's rules (§4).
    pub fn connect(
        &mut self,
        source: EdgeEnd,
        target: EdgeEnd,
    ) -> Result<EdgeIndex, ConnectionError> {
        let id = self.next_id();
        self.connect_with(EdgeSpec::new(id, source, target))
    }

    /// Connects with a fully described edge — a routing, a style, a label.
    pub fn connect_with(&mut self, spec: EdgeSpec) -> Result<EdgeIndex, ConnectionError> {
        self.validate_connection(spec.source, spec.target)?;

        self.ids.observe(spec.id);
        let id = spec.id;
        let (source, target) = (spec.source, spec.target);
        let edge = self.edges.push(spec);

        self.geometry.push_edge();
        self.dirty.push_edge();
        self.edge_by_id.insert(id, edge);
        self.adjacency.connect(edge, source.node, target.node);
        self.dirty.mark_edge(edge, EdgeDirty::GEOMETRY);

        Ok(edge)
    }

    /// Whether these two endpoints could be connected. The question a
    /// connection tool asks on hover, and the same code the connect path runs.
    pub fn can_connect(&self, source: EdgeEnd, target: EdgeEnd) -> bool {
        self.validate_connection(source, target).is_ok()
    }

    /// **§4's validation rules and connection limits**, in one place.
    ///
    /// Every check is proportional to the endpoints' own degree, never to the
    /// graph: the duplicate check and the limit check both walk the source
    /// node's incident edges through the adjacency index (§40 rule 2).
    pub fn validate_connection(
        &self,
        source: EdgeEnd,
        target: EdgeEnd,
    ) -> Result<(), ConnectionError> {
        self.check_end(source, HandleDirection::Source)?;
        self.check_end(target, HandleDirection::Target)?;

        if source.node == target.node && !self.rules.allow_self_connections {
            return Err(ConnectionError::SelfConnection(source.node));
        }

        if !self.rules.allow_duplicate_edges
            && let Some(existing) = self.existing_edge(source, target)
        {
            return Err(ConnectionError::Duplicate(existing));
        }

        self.check_limit(source)?;
        self.check_limit(target)?;
        Ok(())
    }

    /// One end's structural and directional validity.
    fn check_end(&self, end: EdgeEnd, role: HandleDirection) -> Result<(), ConnectionError> {
        if !self.nodes.is_live(end.node) {
            return Err(ConnectionError::UnknownNode(end.node));
        }

        let Some(handle) = end.handle.get() else {
            if self.rules.require_handles {
                return Err(ConnectionError::HandleRequired);
            }
            // Whole-node mode is only offered by kinds that mean it; a drawn
            // rectangle is decoration until §8's free linear elements arrive.
            if !self.rules.allow_unconnectable_nodes && !self.nodes.shape(end.node).is_connectable()
            {
                return Err(ConnectionError::NodeNotConnectable(end.node));
            }
            return Ok(());
        };

        if !self.handles.contains(handle) {
            return Err(ConnectionError::UnknownHandle(handle));
        }
        if self.handles.owner(handle) != end.node {
            return Err(ConnectionError::HandleNotOnNode {
                node: end.node,
                handle,
            });
        }

        // `Loose` is React Flow's bidirectional handle and accepts either role;
        // anything else has to match the role it is being used in.
        let direction = self.handles.direction(handle);
        if direction != HandleDirection::Loose && direction != role {
            return Err(ConnectionError::DirectionMismatch { handle });
        }

        Ok(())
    }

    /// §4's connection limits. Counts the connections already on this handle by
    /// walking its **node's** incident edges, which is degree-proportional.
    fn check_limit(&self, end: EdgeEnd) -> Result<(), ConnectionError> {
        let Some(handle) = end.handle.get() else {
            return Ok(());
        };
        let Some(limit) = self.handles.limit(handle) else {
            return Ok(());
        };

        let used = self
            .adjacency
            .incident_edges(end.node)
            .filter(|edge| self.edges.is_live(*edge) && self.uses_handle(*edge, handle))
            .count() as u32;

        if used >= limit {
            Err(ConnectionError::HandleAtLimit { handle, limit })
        } else {
            Ok(())
        }
    }

    fn uses_handle(&self, edge: EdgeIndex, handle: HandleIndex) -> bool {
        self.edges.source(edge).handle.get() == Some(handle)
            || self.edges.target(edge).handle.get() == Some(handle)
    }

    /// An edge already joining these two endpoints, if there is one.
    ///
    /// Ends are compared exactly — same node *and* same handle — so two edges
    /// between the same pair of nodes through different ports are not
    /// duplicates. Direction is not ignored either: A→B and B→A are different
    /// connections in a directed graph, and treating them as one would make it
    /// impossible to draw a two-way relationship.
    pub fn existing_edge(&self, source: EdgeEnd, target: EdgeEnd) -> Option<EdgeIndex> {
        // **A removed edge is not a duplicate.** Without this, undoing a
        // disconnect and then reconnecting the same pair by hand would be
        // refused by a tombstone nobody can see — the adjacency index keeps
        // deleted edges listed, on purpose, so that restoring one costs
        // nothing.
        self.adjacency.outgoing(source.node).find(|edge| {
            self.edges.is_live(*edge)
                && self.edges.source(*edge) == source
                && self.edges.target(*edge) == target
        })
    }

    // ---- the propagation rule -------------------------------------------

    /// **Moves a node, and invalidates exactly what that move changed** (§19).
    ///
    /// The whole architecture's reason for existing. See the module doc for the
    /// diagram this implements, and the property test at the bottom of this
    /// file for the assertion that it stays true.
    pub fn move_node(&mut self, node: NodeIndex, delta: Vec2) {
        if !self.nodes.contains(node) || (delta.x == 0.0 && delta.y == 0.0) {
            return;
        }

        self.set_node_position(node, self.nodes.position(node) + delta);
    }

    /// Moves a node to an absolute position.
    ///
    /// **A connector is moved by rebuilding its ordered segment inside the new
    /// rectangle**, never by writing the rectangle and leaving the segment
    /// behind — see [`Connector::with_bounds`]. Endpoint identity survives, and
    /// a bound endpoint is pulled back onto its target afterwards, because the
    /// attachment outranks a translation nobody aimed at it.
    pub fn set_node_position(&mut self, node: NodeIndex, position: Vec2) {
        if !self.nodes.contains(node) || self.nodes.position(node) == position {
            return;
        }

        match self.nodes.connector(node) {
            Some(connector) => {
                let bounds = Rect::new(position, self.nodes.size(node));
                self.nodes
                    .set_connector(node, Some(connector.with_bounds(bounds)));
                self.refresh_connector(node);
            }
            None => self.nodes.set_position(node, position),
        }
        self.invalidate_geometry_of(node, NodeDirty::POSITION);
        self.refresh_bound_connectors(node);
    }

    /// Resizes a node. Its handles are fractions of its edges, so every
    /// incident route moves with it — the same propagation as a move.
    ///
    /// A connector has no rectangle of its own to resize; the new extent is
    /// pushed through [`Connector::with_bounds`] so its two ordered endpoints
    /// land on the corners they already occupied.
    pub fn set_node_size(&mut self, node: NodeIndex, size: Vec2) {
        if !self.nodes.contains(node) || self.nodes.size(node) == size {
            return;
        }

        match self.nodes.connector(node) {
            Some(connector) => {
                let bounds = Rect::new(self.nodes.position(node), size);
                self.nodes
                    .set_connector(node, Some(connector.with_bounds(bounds)));
                self.refresh_connector(node);
            }
            None => self.nodes.set_size(node, size),
        }
        self.invalidate_geometry_of(node, NodeDirty::SIZE);
        self.refresh_bound_connectors(node);
    }

    /// Replaces a straight connector's ordered geometry and endpoint bindings.
    /// The opposite endpoint is untouched unless it is present in `connector`
    /// with a different value; callers editing one end build that value from
    /// the current connector.
    pub fn set_node_connector(&mut self, node: NodeIndex, connector: Connector) {
        if !self.nodes.contains(node) || self.nodes.connector(node) == Some(connector) {
            return;
        }

        self.unbind_connector(node);
        self.nodes.set_connector(node, Some(connector));
        self.bind_connector(node);
        self.refresh_connector(node);
        self.invalidate_geometry_of(node, NodeDirty::SIZE);
    }

    /// Builds the persisted attachment and its resolved point on `target`'s
    /// direction-appropriate edge.
    pub fn connector_attachment(
        &self,
        target: NodeIndex,
        toward: Vec2,
    ) -> Option<(ConnectorAttachment, Vec2)> {
        if !self.valid_connector_target(target, None) {
            return None;
        }

        let bounds = self.nodes.bounds(target).normalized();
        let side = Side::facing(bounds, toward);
        let point = floating_point(bounds, side, toward);
        let anchor = Vec2::new(
            if bounds.width() > f32::EPSILON {
                (point.x - bounds.origin.x) / bounds.width()
            } else {
                0.5
            },
            if bounds.height() > f32::EPSILON {
                (point.y - bounds.origin.y) / bounds.height()
            } else {
                0.5
            },
        );
        Some((
            ConnectorAttachment {
                element: self.nodes.id(target),
                anchor,
            },
            point,
        ))
    }

    fn valid_connector_target(&self, target: NodeIndex, connector: Option<NodeIndex>) -> bool {
        self.nodes.is_live(target)
            && connector != Some(target)
            && !matches!(self.nodes.kind(target), ElementKind::Linear(_))
    }

    fn attachment_point(&self, attachment: ConnectorAttachment) -> Option<Vec2> {
        let target = self.node_index(attachment.element)?;
        if !self.valid_connector_target(target, None) {
            return None;
        }
        let bounds = self.nodes.bounds(target).normalized();
        let anchor = Vec2::new(
            attachment.anchor.x.clamp(0.0, 1.0),
            attachment.anchor.y.clamp(0.0, 1.0),
        );
        Some(bounds.origin + Vec2::new(bounds.width() * anchor.x, bounds.height() * anchor.y))
    }

    fn bind_connector(&mut self, connector: NodeIndex) {
        let Some(geometry) = self.nodes.connector(connector) else {
            return;
        };
        for end in [ConnectorEnd::Start, ConnectorEnd::End] {
            let Some(attachment) = geometry.endpoint(end).attachment else {
                continue;
            };
            let Some(target) = self.node_index(attachment.element) else {
                continue;
            };
            if self.valid_connector_target(target, Some(connector)) {
                self.connector_bindings[target.index()].push((connector, end));
            }
        }
    }

    fn unbind_connector(&mut self, connector: NodeIndex) {
        let Some(geometry) = self.nodes.connector(connector) else {
            return;
        };
        for end in [ConnectorEnd::Start, ConnectorEnd::End] {
            let Some(attachment) = geometry.endpoint(end).attachment else {
                continue;
            };
            let Some(target) = self.node_index(attachment.element) else {
                continue;
            };
            if let Some(bindings) = self.connector_bindings.get_mut(target.index()) {
                bindings.retain(|binding| *binding != (connector, end));
            }
        }
    }

    fn refresh_connector(&mut self, connector: NodeIndex) {
        let Some(mut geometry) = self.nodes.connector(connector) else {
            return;
        };
        let mut changed = false;
        for end in [ConnectorEnd::Start, ConnectorEnd::End] {
            let Some(attachment) = geometry.endpoint(end).attachment else {
                continue;
            };
            let Some(point) = self.attachment_point(attachment) else {
                continue;
            };
            let endpoint = geometry.endpoint_mut(end);
            changed |= endpoint.point != point;
            endpoint.point = point;
        }
        if changed {
            self.nodes.set_connector(connector, Some(geometry));
        }
    }

    fn refresh_bound_connectors(&mut self, target: NodeIndex) {
        let Some(bindings) = self.connector_bindings.get(target.index()).cloned() else {
            return;
        };
        for (connector, _) in bindings {
            if !self.nodes.is_live(connector) {
                continue;
            }
            let before = self.nodes.connector(connector);
            self.refresh_connector(connector);
            if self.nodes.connector(connector) != before {
                self.invalidate_geometry_of(connector, NodeDirty::POSITION);
            }
        }
    }

    fn rebuild_connector_bindings(&mut self, report: &mut LoadReport) {
        for bindings in &mut self.connector_bindings {
            bindings.clear();
        }

        for connector in self.nodes.live_indices().collect::<Vec<_>>() {
            let Some(mut geometry) = self.nodes.connector(connector) else {
                continue;
            };
            let mut changed = false;
            for end in [ConnectorEnd::Start, ConnectorEnd::End] {
                let endpoint = geometry.endpoint_mut(end);
                let Some(attachment) = endpoint.attachment else {
                    continue;
                };
                let Some(target) = self.node_index(attachment.element) else {
                    endpoint.attachment = None;
                    changed = true;
                    report
                        .unresolved_connector_attachments
                        .push(self.nodes.id(connector));
                    continue;
                };
                if !self.valid_connector_target(target, Some(connector)) {
                    endpoint.attachment = None;
                    changed = true;
                    report
                        .unresolved_connector_attachments
                        .push(self.nodes.id(connector));
                    continue;
                }
                if let Some(point) = self.attachment_point(attachment) {
                    endpoint.point = point;
                }
                self.connector_bindings[target.index()].push((connector, end));
            }
            if changed || self.nodes.connector(connector) != Some(geometry) {
                self.nodes.set_connector(connector, Some(geometry));
            }
        }
    }

    /// Replaces a node's style (§32, and §30's `UpdateStyle`).
    ///
    /// Marks `SPATIAL` as well as `STYLE` because
    /// [`node_painted_bounds`](crate::spatial::node_painted_bounds) inflates by
    /// half the stroke width — a thicker outline is a bigger painted rectangle,
    /// and an index that did not hear about it culls the node's own edge away.
    pub fn set_node_style(&mut self, node: NodeIndex, style: ElementStyle) {
        if !self.nodes.contains(node) || self.nodes.style(node) == &style {
            return;
        }

        self.nodes.set_style(node, style);
        self.dirty
            .mark_node(node, NodeDirty::STYLE | NodeDirty::SPATIAL);
    }

    /// Replaces an edge's style. `SPATIAL` for the same reason as
    /// [`set_node_style`](GraphWorld::set_node_style), plus the arrow marker,
    /// which [`edge_painted_bounds`](crate::spatial::edge_painted_bounds) also
    /// counts. The route itself does not change, so no geometry is rebuilt.
    pub fn set_edge_style(&mut self, edge: EdgeIndex, style: ElementStyle) {
        if !self.edges.contains(edge) || self.edges.style(edge) == &style {
            return;
        }

        self.edges.set_style(edge, style);
        self.dirty
            .mark_edge(edge, EdgeDirty::STYLE | EdgeDirty::SPATIAL);
    }

    /// Replaces a node's label (§30's `EditText`, and §9's text as far as the
    /// engine has it: a node's caption, not a text element's body).
    pub fn set_node_label(&mut self, node: NodeIndex, label: Option<String>) {
        if !self.nodes.contains(node) || self.nodes.cold(node).label.as_deref() == label.as_deref()
        {
            return;
        }

        self.nodes.set_label(node, label);
        self.dirty.mark_node(node, NodeDirty::TEXT);
    }

    /// **Replaces the node's picture, or clears it** (§10).
    ///
    /// [`NodeDirty::STYLE`] and nothing else — the same flag
    /// [`set_node_z`](GraphWorld::set_node_z) raises, and for the same reason:
    /// what is *drawn* changed while the rectangle did not, so there is no
    /// route to rebuild, no spatial entry to update and no glyph to re-shape.
    /// The bytes are not here at all — they are the
    /// [`insert_image`](GraphWorld::insert_image) table's, and this writes the
    /// handle that names them.
    pub fn set_node_image(&mut self, node: NodeIndex, image: Option<NodeImage>) {
        if !self.nodes.contains(node) || self.nodes.cold(node).image == image {
            return;
        }

        self.nodes.set_image(node, image);
        self.dirty.mark_node(node, NodeDirty::STYLE);
    }

    /// Replaces an edge's label. No geometry and no spatial entry: the label is
    /// drawn at the route's midpoint, which has not moved.
    pub fn set_edge_label(&mut self, edge: EdgeIndex, label: Option<String>) {
        if !self.edges.contains(edge) || self.edges.label(edge).map(Arc::as_ref) == label.as_deref()
        {
            return;
        }

        self.edges.set_label(edge, label);
        self.dirty.mark_edge(edge, EdgeDirty::LABEL);
    }

    /// **Whether anything in this world has left the default layer.**
    ///
    /// The frame's fast path. `false` means every element shares one `z`, so
    /// nobody has expressed an order and the planner is free to use the one the
    /// batching prefers; `true` means somebody pressed a Layers button and the
    /// frame owes them the order they asked for. See
    /// [`nonzero_z`](GraphWorld::nonzero_z) for why it is a counter and why it
    /// counts tombstones.
    pub fn is_layered(&self) -> bool {
        self.nonzero_z > 0
    }

    /// Moves a node in the paint order (§32's z, and the property panel's
    /// Layers row).
    ///
    /// No geometry and no spatial entry: depth changes nothing about where the
    /// node is or how big it is. What it does change is the *frame*, so the
    /// node's appearance version bumps — see
    /// [`NodeStore::set_z`](crate::runtime::NodeStore::set_z).
    pub fn set_node_z(&mut self, node: NodeIndex, z: i32) {
        if !self.nodes.contains(node) || self.nodes.z(node) == z {
            return;
        }

        self.count_z(self.nodes.z(node), z);
        self.nodes.set_z(node, z);
        self.dirty.mark_node(node, NodeDirty::STYLE);
    }

    /// Moves an edge in the paint order. See [`set_node_z`](GraphWorld::set_node_z).
    pub fn set_edge_z(&mut self, edge: EdgeIndex, z: i32) {
        if !self.edges.contains(edge) || self.edges.z(edge) == z {
            return;
        }

        self.count_z(self.edges.z(edge), z);
        self.edges.set_z(edge, z);
        self.dirty.mark_edge(edge, EdgeDirty::STYLE);
    }

    /// Keeps [`nonzero_z`](GraphWorld::nonzero_z) exact across one `z` write —
    /// including the write that undoes it, which is what makes the counter
    /// symmetric rather than sticky.
    fn count_z(&mut self, was: i32, now: i32) {
        match (was == 0, now == 0) {
            (true, false) => self.nonzero_z += 1,
            (false, true) => self.nonzero_z = self.nonzero_z.saturating_sub(1),
            _ => {}
        }
    }

    /// Replaces a node's hyperlink. Marks nothing dirty: a link is never
    /// painted, so no cache and no index can be stale because of it.
    pub fn set_node_link(&mut self, node: NodeIndex, link: Option<String>) {
        if !self.nodes.contains(node) || self.nodes.cold(node).link == link {
            return;
        }

        self.nodes.set_link(node, link);
    }

    /// Replaces an edge's hyperlink. See [`set_node_link`](GraphWorld::set_node_link).
    pub fn set_edge_link(&mut self, edge: EdgeIndex, link: Option<String>) {
        if !self.edges.contains(edge) || self.edges.link(edge) == link.as_deref() {
            return;
        }

        self.edges.set_link(edge, link);
    }

    // ---- removal, as a tombstone (§30) -----------------------------------

    /// **Removes a node and every live edge attached to it**, appending those
    /// edges to `cascaded`. Returns whether the node itself changed state.
    ///
    /// The cascade is here rather than in the caller because an edge with one
    /// end nowhere has no geometry — `from_document` already drops those as
    /// corrupt — so "remove the node, forget the edges" is not a state this
    /// world may reach. The caller gets the list back so that
    /// [`restore_node`](GraphWorld::restore_node) plus
    /// [`restore_edge`](GraphWorld::restore_edge) can put back exactly what
    /// went, and nothing that was already gone.
    ///
    /// The node keeps its slot; see [`NodeStore`]'s
    /// module doc for why undo makes that the only workable choice.
    pub fn remove_node(&mut self, node: NodeIndex, cascaded: &mut Vec<EdgeIndex>) -> bool {
        if !self.nodes.is_live(node) {
            return false;
        }

        let incident: Vec<EdgeIndex> = self
            .adjacency
            .incident_edges(node)
            .filter(|edge| self.edges.is_live(*edge))
            .collect();
        for edge in incident {
            if self.remove_edge(edge) {
                cascaded.push(edge);
            }
        }

        self.set_node_selected(node, false);
        self.nodes.set_flag(node, NodeFlags::REMOVED, true);
        // `SPATIAL` is what makes the index drop it: `SpatialIndex::sync` reads
        // `is_live` and removes rather than re-places. `STYLE` is the render
        // invalidation — nothing about the node's *position* changed.
        self.dirty
            .mark_node(node, NodeDirty::STYLE | NodeDirty::SPATIAL);
        true
    }

    /// Puts a removed node back at its own index.
    ///
    /// **Its edges are not restored.** The history names them explicitly, which
    /// is what keeps an edge deleted before its node deleted after the node
    /// comes back — restoring the node's whole neighbourhood would resurrect
    /// edges the author had already thrown away.
    pub fn restore_node(&mut self, node: NodeIndex) -> bool {
        if !self.nodes.contains(node) || !self.nodes.is_removed(node) {
            return false;
        }

        self.nodes.set_flag(node, NodeFlags::REMOVED, false);
        self.dirty
            .mark_node(node, NodeDirty::POSITION | NodeDirty::SPATIAL);
        true
    }

    /// Removes an edge — §30's *disconnect*, and what a node removal cascades
    /// into. Its adjacency entries stay, so restoring it costs one bit.
    pub fn remove_edge(&mut self, edge: EdgeIndex) -> bool {
        if !self.edges.is_live(edge) {
            return false;
        }

        self.set_edge_selected(edge, false);
        self.edges.set_flag(edge, EdgeFlags::REMOVED, true);
        // Invalidating the route is what drops it from the spatial index:
        // `edge_painted_bounds` answers `None` without one, and `sync` removes
        // rather than re-places. It is also what stops a stale route being
        // painted in the frame between the edit and the next rebuild.
        self.invalidate_edge_geometry(edge);
        true
    }

    /// Puts a removed edge back, and queues its route for rebuild — the route
    /// was invalidated on the way out, and the nodes it joins may have moved
    /// since.
    pub fn restore_edge(&mut self, edge: EdgeIndex) -> bool {
        if !self.edges.contains(edge) || !self.edges.is_removed(edge) {
            return false;
        }

        self.edges.set_flag(edge, EdgeFlags::REMOVED, false);
        self.invalidate_edge_geometry(edge);
        true
    }

    pub fn set_node_selected(&mut self, node: NodeIndex, selected: bool) {
        if !self.nodes.contains(node) || self.nodes.is_selected(node) == selected {
            return;
        }

        self.nodes.set_flag(node, NodeFlags::SELECTED, selected);
        // The flag and the set are written together, here and nowhere else.
        if selected {
            self.selection.insert_node(node);
        } else {
            self.selection.remove_node(node);
        }
        // Selection changes how a node is painted and nothing about where
        // anything is, so no edge is touched. The distinction is the whole
        // point of having flags per change rather than one "dirty" bit.
        self.dirty.mark_node(node, NodeDirty::STYLE);
    }

    pub fn set_edge_selected(&mut self, edge: EdgeIndex, selected: bool) {
        if !self.edges.contains(edge) || self.edges.is_selected(edge) == selected {
            return;
        }

        self.edges.set_flag(edge, EdgeFlags::SELECTED, selected);
        if selected {
            self.selection.insert_edge(edge);
        } else {
            self.selection.remove_edge(edge);
        }
        self.dirty.mark_edge(edge, EdgeDirty::STYLE);
    }

    // ---- selection (§28) -------------------------------------------------

    /// What is selected, as compact runtime ids — never cloned elements.
    pub fn selection(&self) -> &SelectionSet {
        &self.selection
    }

    /// Deselects everything, touching only what was actually selected.
    ///
    /// Proportional to the *selection*, not to the document: the set is what
    /// makes that possible, and it is why §28 asks for one rather than for a
    /// scan over the flags.
    pub fn clear_selection(&mut self) {
        // Taken out so the stores can be written while it is read, and handed
        // back cleared so a rubber band that replaces its selection sixty
        // times a second reuses the same allocations (§40 rule 13).
        let mut spent = std::mem::take(&mut self.selection);
        for &node in spent.nodes() {
            self.nodes.set_flag(node, NodeFlags::SELECTED, false);
            self.dirty.mark_node(node, NodeDirty::STYLE);
        }
        for &edge in spent.edges() {
            self.edges.set_flag(edge, EdgeFlags::SELECTED, false);
            self.dirty.mark_edge(edge, EdgeDirty::STYLE);
        }
        spent.clear();
        self.selection = spent;
    }

    /// Replaces the selection with at most one node — a plain click.
    pub fn select_only(&mut self, node: Option<NodeIndex>) {
        if let Some(node) = node
            && self.selection.single_node() == Some(node)
        {
            return;
        }

        self.clear_selection();
        if let Some(node) = node {
            self.set_node_selected(node, true);
        }
    }

    /// **§28's box selection, narrow phase.**
    ///
    /// The candidates are parameters, exactly as [`GraphWorld::hit_test`]'s
    /// are, because the broad phase is [`crate::spatial::SpatialIndex`]'s and
    /// this file must never be the place a scan over the document appears.
    /// Pass an empty iterator for either kind to leave it alone.
    ///
    /// Returns how many elements changed state, so a caller can decide whether
    /// the frame needs repainting without diffing anything.
    pub fn apply_box_selection(
        &mut self,
        query: BoxQuery,
        nodes: impl IntoIterator<Item = NodeIndex>,
        edges: impl IntoIterator<Item = EdgeIndex>,
    ) -> u32 {
        if !query.additive {
            self.clear_selection();
        }

        let rect = query.rect.normalized();
        let mut changed = 0;

        for node in nodes {
            // A **locked** element is not selectable — §26's behaviour, and the
            // same answer `views::flow` gives a press that lands on one.
            if !self.nodes.is_live(node)
                || self.nodes.is_hidden(node)
                || self.nodes.is_locked(node)
                || self.selection.contains_node(node)
            {
                continue;
            }
            let bounds = self.nodes.bounds(node);
            let inside = match query.mode {
                BoxSelectMode::Touch => rect.intersects(bounds),
                BoxSelectMode::Enclose => rect.contains_rect(bounds),
            };
            if inside {
                self.set_node_selected(node, true);
                changed += 1;
            }
        }

        for edge in edges {
            if !self.edges.is_live(edge)
                || self.edges.is_hidden(edge)
                || self.selection.contains_edge(edge)
            {
                continue;
            }
            // A stale route is not selected rather than selected at where it
            // used to be: the caller runs after `rebuild_dirty_geometry`, so a
            // stale route here means the edge has no geometry at all.
            let Some(route) = self.geometry.route(edge) else {
                continue;
            };
            let inside = match query.mode {
                BoxSelectMode::Touch => route.intersects_rect(rect, query.tolerance),
                // The control hull, not the curve: "entirely inside" is only
                // *stricter* if the bound is the outer one, and the hull
                // contains the curve.
                BoxSelectMode::Enclose => rect.contains_rect(route.bounds()),
            };
            if inside {
                self.set_edge_selected(edge, true);
                changed += 1;
            }
        }

        changed
    }

    /// Hides or shows a node. **Its edges are left alone**: an edge carries its
    /// own hidden flag, and deciding that hiding a node hides its connections is
    /// a document-level policy rather than a geometric fact.
    pub fn set_node_hidden(&mut self, node: NodeIndex, hidden: bool) {
        if !self.nodes.contains(node) || self.nodes.is_hidden(node) == hidden {
            return;
        }

        self.nodes.set_flag(node, NodeFlags::HIDDEN, hidden);
        self.dirty.mark_node(node, NodeDirty::STYLE);
    }

    pub fn set_edge_routing(&mut self, edge: EdgeIndex, routing: EdgeRouting) {
        if !self.edges.contains(edge) || self.edges.routing(edge) == routing {
            return;
        }

        self.edges.set_routing(edge, routing);
        self.invalidate_edge_geometry(edge);
    }

    /// The geometry half of a node change: the node itself, its spatial entry,
    /// and **only** its incident edges.
    fn invalidate_geometry_of(&mut self, node: NodeIndex, cause: NodeDirty) {
        self.dirty.mark_node(node, cause | NodeDirty::SPATIAL);

        // Disjoint field borrows: the adjacency index is read while the dirty
        // state is written, which is exactly what the split into two fields
        // buys and what a single "runtime state" struct would have cost.
        // Disjoint field borrows again, and the reason this loop is not
        // `invalidate_edge_geometry`: that method takes `&mut self`, which the
        // adjacency iterator is holding.
        let (dirty, geometry, edges) = (&mut self.dirty, &mut self.geometry, &self.edges);
        for edge in self.adjacency.incident_edges(node) {
            // A tombstoned edge stays listed in the adjacency index — that is
            // what makes restoring it free — so it is skipped here rather than
            // rerouted for a node it no longer joins.
            if !edges.is_live(edge) {
                continue;
            }
            dirty.mark_edge(edge, EdgeDirty::GEOMETRY | EdgeDirty::SPATIAL);
            geometry.invalidate(edge);
        }
    }

    /// Marks one edge's route stale, in **both** of the places that record it.
    ///
    /// The two are different questions and both are needed: the dirty queue is
    /// *what to rebuild*, and is drained; the geometry store's validity is
    /// *whether this route may be painted*, and outlives the drain. Setting one
    /// without the other paints an edge hanging off a node that has already
    /// moved, so nothing sets them separately — this is the only writer.
    fn invalidate_edge_geometry(&mut self, edge: EdgeIndex) {
        self.dirty
            .mark_edge(edge, EdgeDirty::GEOMETRY | EdgeDirty::SPATIAL);
        self.geometry.invalidate(edge);
    }

    /// Every edge attached to `node` — §20's required call, proportional to the
    /// node's degree.
    pub fn incident_edges(&self, node: NodeIndex) -> impl Iterator<Item = EdgeIndex> + '_ {
        self.adjacency.incident_edges(node)
    }

    // ---- deriving --------------------------------------------------------

    /// **Rebuilds the routes that are stale, and nothing else.** Returns how
    /// many were rebuilt.
    ///
    /// Called once at the top of a frame. When nothing moved, the dirty queue is
    /// empty and this costs one branch — which is what makes §40 rule 6 (never
    /// reroute unchanged edges during a pure pan) hold by construction rather
    /// than by care.
    pub fn rebuild_dirty_geometry(&mut self) -> u32 {
        let queue = self.dirty.take_edge_queue();
        let mut rebuilt = 0;

        for &edge in &queue {
            let flags = self.dirty.clear_edge(edge);
            if flags.intersects(EdgeDirty::GEOMETRY | EdgeDirty::ENDPOINTS)
                && self.edges.is_live(edge)
            {
                self.rebuild_route(edge);
                rebuilt += 1;
            }
        }

        self.dirty.restore_edge_queue(queue);
        rebuilt
    }

    /// Rebuilds every route whether or not it is stale. For a caller that has
    /// just replaced the world wholesale; never on a frame path.
    pub fn rebuild_all_geometry(&mut self) -> u32 {
        for edge in self.edges.live_indices().collect::<Vec<_>>() {
            self.invalidate_edge_geometry(edge);
        }
        self.rebuild_dirty_geometry()
    }

    fn rebuild_route(&mut self, edge: EdgeIndex) {
        let source = self.edges.source(edge);
        let target = self.edges.target(edge);
        let routing = self.edges.routing(edge);

        // Each end is aimed at the other, which is what makes a floating
        // attachment pick the side that faces its partner (§4).
        let source_attachment = self.attachment(source, self.anchor(target));
        let target_attachment = self.attachment(target, self.anchor(source));

        self.geometry
            .rebuild(edge, routing, source_attachment, target_attachment);
    }

    /// The point an end is "at" for the purpose of aiming the other end: its
    /// handle if it has one, its node's centre if it does not.
    fn anchor(&self, end: EdgeEnd) -> Vec2 {
        match end.handle.get() {
            Some(handle) => self.handle_position(handle),
            None => self.nodes.bounds(end.node).center(),
        }
    }

    /// Where an edge attaches to one of its ends, and which way it sets off.
    ///
    /// A handle endpoint attaches at the handle and leaves along its placement.
    /// A whole-node endpoint is §4's **floating connection point**: the side
    /// facing the other end, at the point on that side nearest to it, so the
    /// attachment slides along the border as the nodes move rather than
    /// snapping between corners.
    pub fn attachment(&self, end: EdgeEnd, toward: Vec2) -> Attachment {
        let bounds = self.nodes.bounds(end.node);

        match end.handle.get() {
            Some(handle) => Attachment::new(
                handle_world_position(
                    self.handles.placement(handle),
                    self.handles.offset(handle),
                    bounds,
                ),
                side_of(self.handles.placement(handle)),
            ),
            None => {
                let side = Side::facing(bounds, toward);
                Attachment::new(floating_point(bounds, side, toward), side)
            }
        }
    }

    /// The route for an edge, or `None` if it is stale or absent.
    pub fn route(&self, edge: EdgeIndex) -> Option<&EdgeRoute> {
        self.geometry.route(edge)
    }

    // ---- hit testing -----------------------------------------------------

    /// **The narrow phase** (§29): which of `candidates` is under `point`.
    ///
    /// The candidate set is a parameter — see [`crate::runtime::hit`] for why
    /// that is the seam Phase 4's broad phase plugs into rather than a linear
    /// scan waiting to be deleted.
    ///
    /// Handles beat bodies, whatever their z, because a handle sits *on* its
    /// node and a press within grabbing distance of one always means the
    /// connection. Among bodies the topmost wins, by `z` and then by insertion
    /// order — the same order the painter draws in, reversed.
    pub fn hit_test(
        &self,
        point: Vec2,
        candidates: impl IntoIterator<Item = NodeIndex>,
        tolerance: HitTolerance,
    ) -> PointerTarget {
        let radius_squared = tolerance.handle_radius * tolerance.handle_radius;
        let mut best_handle: Option<(HandleIndex, NodeIndex, f32)> = None;
        let mut best_node: Option<(NodeIndex, i32)> = None;

        for node in candidates {
            if !self.nodes.is_live(node) || self.nodes.is_hidden(node) {
                continue;
            }

            for handle in self.nodes.handles(node) {
                let distance = (self.handle_position(handle) - point).length_squared();
                if distance <= radius_squared
                    && best_handle.is_none_or(|(_, _, best)| distance < best)
                {
                    best_handle = Some((handle, node, distance));
                }
            }

            let hits_body = self.nodes.connector(node).map_or_else(
                || self.nodes.bounds(node).contains_point(point),
                |connector| {
                    distance_to_segment(point, connector.start.point, connector.end.point)
                        <= tolerance.edge_radius
                },
            );
            if hits_body {
                let z = self.nodes.z(node);
                if best_node.is_none_or(|(best, best_z)| (z, node.raw()) >= (best_z, best.raw())) {
                    best_node = Some((node, z));
                }
            }
        }

        match (best_handle, best_node) {
            (Some((handle, node, _)), _) => PointerTarget::Handle { node, handle },
            (None, Some((node, _))) => PointerTarget::Node(node),
            (None, None) => PointerTarget::Empty,
        }
    }

    /// Which of a selected straight connector's two ordered endpoints is under
    /// the pointer. Exactly two candidates; rectangle corners never enter.
    pub fn hit_test_connector_endpoint(
        &self,
        point: Vec2,
        node: NodeIndex,
        tolerance: HitTolerance,
    ) -> Option<ConnectorEnd> {
        if !self.node_is_live(node) || self.nodes.is_hidden(node) || self.nodes.is_locked(node) {
            return None;
        }
        let connector = self.nodes.connector(node)?;
        let radius_squared = tolerance.grip_radius * tolerance.grip_radius;
        [ConnectorEnd::Start, ConnectorEnd::End]
            .into_iter()
            .map(|end| {
                (
                    end,
                    (connector.endpoint(end).point - point).length_squared(),
                )
            })
            .filter(|(_, distance)| *distance <= radius_squared)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(end, _)| end)
    }

    /// The nearest valid non-connector target within `radius` of an endpoint,
    /// with the direction-appropriate persisted anchor and resolved point.
    pub fn snap_connector_endpoint(
        &self,
        point: Vec2,
        toward: Vec2,
        candidates: impl IntoIterator<Item = NodeIndex>,
        exclude: Option<NodeIndex>,
        radius: f32,
    ) -> Option<ConnectorSnap> {
        let mut best: Option<(ConnectorSnap, f32, i32)> = None;
        for target in candidates {
            if !self.valid_connector_target(target, exclude) || self.nodes.is_hidden(target) {
                continue;
            }
            let bounds = self.nodes.bounds(target).normalized();
            let min = bounds.min();
            let max = bounds.max();
            let closest = Vec2::new(point.x.clamp(min.x, max.x), point.y.clamp(min.y, max.y));
            let distance = (closest - point).length_squared();
            if distance > radius * radius {
                continue;
            }
            let Some((attachment, snapped)) = self.connector_attachment(target, toward) else {
                continue;
            };
            let snap = ConnectorSnap {
                target,
                point: snapped,
                attachment,
            };
            let z = self.nodes.z(target);
            let replace = match best.as_ref() {
                None => true,
                Some((current, best_distance, best_z)) => {
                    distance < *best_distance
                        || (distance == *best_distance
                            && (z, target.raw()) > (*best_z, current.target.raw()))
                }
            };
            if replace {
                best = Some((snap, distance, z));
            }
        }
        best.map(|(snap, _, _)| snap)
    }

    /// **§29's narrow phase for a resize grip** (Phase 12): which corner of
    /// `node`'s frame is within [`HitTolerance::grip_radius`] of `point`, or
    /// `None`.
    ///
    /// A third ranked call rather than a branch inside
    /// [`hit_test`](GraphWorld::hit_test), and the ranking is the *opposite*
    /// end from [`hit_test_edge`](GraphWorld::hit_test_edge)'s: a caller asks
    /// this **first**, because a grip is drawn on top of everything and is the
    /// smallest target on the canvas. Fused into `hit_test` it would also have
    /// had to learn something that is not the world's business — **whether the
    /// grips are on screen at all**. They are drawn only for the element the
    /// selection ring is around, and only at a zoom where the ring itself is
    /// drawn; a hit test that answered `ResizeGrip` on a frame that drew none
    /// would be an invisible control stealing every press near a corner.
    ///
    /// So the caller passes the node whose grips it actually drew, and gets
    /// back the corner or nothing.
    pub fn hit_test_grip(
        &self,
        point: Vec2,
        node: NodeIndex,
        tolerance: HitTolerance,
    ) -> Option<ResizeCorner> {
        if !self.node_is_live(node) || self.nodes.is_hidden(node) || self.nodes.is_locked(node) {
            return None;
        }

        let frame = self.nodes.bounds(node);
        let radius_squared = tolerance.grip_radius * tolerance.grip_radius;

        ResizeCorner::ALL
            .iter()
            .copied()
            .map(|corner| (corner, (corner.of(frame) - point).length_squared()))
            .filter(|&(_, distance)| distance <= radius_squared)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(corner, _)| corner)
    }

    /// **§29's narrow phase for an edge**: the nearest edge whose drawn line
    /// passes within [`HitTolerance::edge_radius`] of `point`, or `None`.
    ///
    /// A second call rather than a third loop inside [`hit_test`](GraphWorld::hit_test),
    /// and the split is the ranking: bodies and handles win over edges, so a
    /// caller asks this **only** when `hit_test` answered `Empty`. Fused into
    /// one function, a labelled edge passing under a node would fight the node
    /// for every press, and which one won would depend on a distance nobody can
    /// see.
    ///
    /// The candidates come from the caller, exactly as `hit_test`'s do and for
    /// the same §40 rule 1 reason — the spatial index's edge grid answers this,
    /// never a scan. `flatten` is how finely each route is walked, in world
    /// units; a caller with a zoom passes one screen pixel's worth.
    ///
    /// Nearest rather than topmost, because two edges crossing have no
    /// meaningful z between them at the crossing point and the one whose line
    /// the pointer is actually closest to is the one it was aimed at.
    pub fn hit_test_edge(
        &self,
        point: Vec2,
        candidates: impl IntoIterator<Item = EdgeIndex>,
        tolerance: HitTolerance,
        flatten: f32,
    ) -> Option<EdgeIndex> {
        let mut best: Option<(EdgeIndex, f32)> = None;

        for edge in candidates {
            if !self.edges.is_live(edge) || self.edges.is_hidden(edge) {
                continue;
            }
            // A stale route is not hit rather than hit where it used to be —
            // the same judgement `apply_box_selection` makes one screen above.
            let Some(route) = self.geometry.route(edge) else {
                continue;
            };
            let Some(distance) = route.distance_to_point(point, tolerance.edge_radius, flatten)
            else {
                continue;
            };
            if best.is_none_or(|(_, closest)| distance < closest) {
                best = Some((edge, distance));
            }
        }

        best.map(|(edge, _)| edge)
    }
}

/// A handle's document placement as the geometry layer's side.
///
/// The two enums are deliberately separate — see [`Side`]'s own doc — and this
/// is the single crossing between them.
fn side_of(placement: HandlePlacement) -> Side {
    match placement {
        HandlePlacement::Top => Side::Top,
        HandlePlacement::Right => Side::Right,
        HandlePlacement::Bottom => Side::Bottom,
        HandlePlacement::Left => Side::Left,
    }
}

/// The point on `side` of `bounds` nearest to `toward` — §4's floating
/// connection point.
///
/// Clamped to the side's own extent, so the attachment slides along the border
/// as the other end moves and stops at the corner rather than running off it.
fn floating_point(bounds: Rect, side: Side, toward: Vec2) -> Vec2 {
    let bounds = bounds.normalized();
    let min = bounds.min();
    let max = bounds.max();

    match side {
        Side::Top => Vec2::new(toward.x.clamp(min.x, max.x), min.y),
        Side::Bottom => Vec2::new(toward.x.clamp(min.x, max.x), max.y),
        Side::Left => Vec2::new(min.x, toward.y.clamp(min.y, max.y)),
        Side::Right => Vec2::new(max.x, toward.y.clamp(min.y, max.y)),
    }
}

#[cfg(test)]
mod tests {
    use super::{GraphWorld, LoadReport};
    use crate::geometry::Rect;
    use crate::{
        geometry::{Side, Vec2},
        models::{
            Connector, ConnectorAttachment, ConnectorEndpoint, EdgeRouting, ElementId, ElementKind,
            Endpoint, FlowDocument, GraphNodeKind, Handle, HandleDirection, HandleId,
            HandlePlacement, LinearKind, NodeIndex, ShapeKind,
        },
        runtime::{
            BoxQuery, BoxSelectMode, ConnectionError, ConnectionRules, EdgeEnd, HandleSpec,
            HitTolerance, NodeDirty, NodeFlags, NodeSpec, PointerTarget,
        },
    };

    fn bound_connector_world() -> (GraphWorld, NodeIndex, NodeIndex, NodeIndex) {
        let mut world = GraphWorld::new();
        let a = world.create_node(
            ElementKind::Shape(ShapeKind::Rectangle),
            Vec2::new(0.0, 0.0),
            Vec2::new(100.0, 100.0),
        );
        let b = world.create_node(
            ElementKind::Shape(ShapeKind::Rectangle),
            Vec2::new(300.0, 0.0),
            Vec2::new(100.0, 100.0),
        );
        let (start_attachment, start) = world
            .connector_attachment(a, world.nodes().bounds(b).center())
            .unwrap();
        let (end_attachment, end) = world
            .connector_attachment(b, world.nodes().bounds(a).center())
            .unwrap();
        let mut connector = Connector::new(start, end);
        connector.start.attachment = Some(start_attachment);
        connector.end.attachment = Some(end_attachment);
        let id = world.next_id();
        let mut spec = NodeSpec::new(
            id,
            ElementKind::Linear(LinearKind::Arrow),
            connector.bounds().origin,
            connector.bounds().size,
        );
        spec.connector = Some(connector);
        let line = world.add_node(spec);
        (world, a, b, line)
    }

    #[test]
    fn a_bound_connector_follows_both_elements_moving_and_resizing() {
        let (mut world, a, b, line) = bound_connector_world();
        let before = world.nodes().connector(line).unwrap();

        world.move_node(a, Vec2::new(40.0, 30.0));
        let moved = world.nodes().connector(line).unwrap();
        assert_eq!(
            moved.start.point,
            before.start.point + Vec2::new(40.0, 30.0)
        );
        assert_eq!(moved.end, before.end, "moving A changed B's endpoint");

        world.set_node_size(b, Vec2::new(100.0, 200.0));
        let resized = world.nodes().connector(line).unwrap();
        assert_eq!(
            resized.start, moved.start,
            "resizing B changed A's endpoint"
        );
        assert_eq!(resized.end.point, Vec2::new(300.0, 100.0));
    }

    #[test]
    fn a_connector_is_hit_only_near_its_actual_segment() {
        let (world, _, _, line) = bound_connector_world();
        let connector = world.nodes().connector(line).unwrap();
        let midpoint = connector.midpoint();
        let tolerance = HitTolerance::new(9.0);

        assert_eq!(
            world.hit_test(midpoint, [line], tolerance),
            PointerTarget::Node(line)
        );
        assert_eq!(
            world.hit_test(
                Vec2::new(
                    connector.bounds().origin.x,
                    connector.bounds().max().y + 30.0
                ),
                [line],
                tolerance,
            ),
            PointerTarget::Empty
        );
    }

    #[test]
    fn connector_snap_chooses_directional_edges_and_reports_feedback() {
        let mut world = GraphWorld::new();
        let target = world.create_node(
            ElementKind::Shape(ShapeKind::Rectangle),
            Vec2::new(100.0, 100.0),
            Vec2::new(100.0, 80.0),
        );
        let cases = [
            (Vec2::new(300.0, 140.0), Vec2::new(1.0, 0.5)),
            (Vec2::new(0.0, 140.0), Vec2::new(0.0, 0.5)),
            (Vec2::new(150.0, 0.0), Vec2::new(0.5, 0.0)),
            (Vec2::new(150.0, 300.0), Vec2::new(0.5, 1.0)),
        ];
        for (toward, expected_anchor) in cases {
            let (attachment, _) = world.connector_attachment(target, toward).unwrap();
            assert_eq!(attachment.anchor, expected_anchor);
        }

        let snap = world
            .snap_connector_endpoint(
                Vec2::new(205.0, 140.0),
                Vec2::new(300.0, 140.0),
                [target],
                None,
                12.0,
            )
            .expect("nearby target snaps");
        assert_eq!(snap.target, target);
        assert_eq!(snap.point, Vec2::new(200.0, 140.0));
        assert_eq!(snap.attachment.anchor, Vec2::new(1.0, 0.5));
    }

    #[test]
    fn unresolved_connector_attachments_detach_without_moving_the_endpoint() {
        let mut document = FlowDocument::new();
        let mut node = crate::models::FlowNode::new(
            document.next_id(),
            ElementKind::Linear(LinearKind::Line),
            Vec2::ZERO,
            Vec2::new(80.0, 20.0),
        );
        let mut connector = node.connector.unwrap();
        connector.start = ConnectorEndpoint {
            point: Vec2::new(7.0, 9.0),
            attachment: Some(ConnectorAttachment {
                element: ElementId::new(999),
                anchor: Vec2::new(1.0, 0.5),
            }),
        };
        node.connector = Some(connector);
        document.nodes.push(node);

        let (world, report) = GraphWorld::from_document(&document);
        let loaded = world.nodes().connector(NodeIndex::new(0)).unwrap();
        assert_eq!(loaded.start.point, Vec2::new(7.0, 9.0));
        assert!(loaded.start.attachment.is_none());
        assert_eq!(report.unresolved_connector_attachments.len(), 1);
    }

    /// **§10's rule, as a count**: the bytes exist once however many elements
    /// name them, and the document written back holds one copy.
    #[test]
    fn two_elements_showing_one_picture_hold_one_copy_of_it() {
        use crate::models::{ImageFormat, ImageResource, NodeImage};

        let mut world = GraphWorld::new();
        let bytes: Vec<u8> = (0..64u8).collect();
        let first =
            world.insert_image(ImageResource::new(ImageFormat::Png, 120, 80, bytes.clone()));
        // The same file again — a second insert, not a second copy.
        let second = world.insert_image(ImageResource::new(ImageFormat::Png, 120, 80, bytes));

        assert_eq!(first, second);
        assert_eq!(world.image_count(), 1);

        let a = world.create_node(ElementKind::Image, Vec2::ZERO, Vec2::new(120.0, 80.0));
        let b = world.create_node(
            ElementKind::Image,
            Vec2::new(200.0, 0.0),
            Vec2::new(60.0, 40.0),
        );
        world.set_node_image(a, Some(NodeImage::new(first)));
        world.set_node_image(b, Some(NodeImage::new(second)));

        // The `Arc` is the proof that "shared" means shared rather than equal.
        let left = world.image(first).expect("the resource is there");
        let right = world.image(second).expect("the resource is there");
        assert!(std::sync::Arc::ptr_eq(left, right));

        let document = world.to_document();
        assert_eq!(document.images.len(), 1);
        assert_eq!(document.nodes.len(), 2);
    }

    /// A picture nothing shows any more is kept in memory — an undo may want it
    /// — and written to no file.
    #[test]
    fn a_document_carries_only_the_pictures_its_elements_name() {
        use crate::models::{ImageFormat, ImageResource, NodeImage};

        let mut world = GraphWorld::new();
        let used = world.insert_image(ImageResource::new(ImageFormat::Png, 10, 10, vec![1, 2, 3]));
        let orphan = world.insert_image(ImageResource::new(ImageFormat::Png, 10, 10, vec![9, 9]));

        let node = world.create_node(ElementKind::Image, Vec2::ZERO, Vec2::new(10.0, 10.0));
        world.set_node_image(node, Some(NodeImage::new(used)));

        assert_eq!(world.image_count(), 2, "the store keeps both");
        let document = world.to_document();
        assert_eq!(document.images.len(), 1, "the file keeps one");
        assert!(document.image(used).is_some());
        assert!(document.image(orphan).is_none());

        // And a removed element takes its picture out of the file while leaving
        // it in the store, so an undo has something to restore.
        world.remove_node(node, &mut Vec::new());
        assert!(world.to_document().images.is_empty());
        assert_eq!(world.image_count(), 2);
    }

    /// **The whole of §10 through a load and a save**, including the crop,
    /// because a round trip that lost it would be a picture that reopens
    /// uncropped with nothing to say why.
    #[test]
    fn an_image_element_survives_the_world_round_trip() {
        use crate::models::{ImageCrop, ImageFormat, ImageResource, NodeImage};

        let mut document = FlowDocument::new();
        let handle =
            document.insert_image(ImageResource::new(ImageFormat::Jpeg, 640, 480, vec![7; 32]));
        let id = document.add_node(
            ElementKind::Image,
            Vec2::new(12.0, 34.0),
            Vec2::new(200.0, 150.0),
        );
        document.node_mut(id).unwrap().image =
            Some(NodeImage::new(handle).with_crop(ImageCrop::new(0.2, 0.1, 0.5, 0.5)));

        let (world, report) = GraphWorld::from_document(&document);
        assert!(report.is_clean());
        assert_eq!(world.image_count(), 1);

        let back = world.to_document();
        assert_eq!(back.nodes[0].image, document.nodes[0].image);
        assert_eq!(back.images, document.images);
    }

    /// **Every corner of the selected element is grabbable**, and nothing else
    /// is — a grip on an element nobody selected would be a resize starting
    /// from a press the user meant as a drag.
    #[test]
    fn a_resize_grip_is_found_at_each_corner_of_one_element() {
        use crate::geometry::ResizeCorner;

        let mut world = GraphWorld::new();
        let node = world.create_node(
            ElementKind::Shape(ShapeKind::Rectangle),
            Vec2::new(100.0, 100.0),
            Vec2::new(200.0, 80.0),
        );
        let tolerance = HitTolerance::at_zoom(1.0);
        let frame = world.nodes().bounds(node);

        for corner in ResizeCorner::ALL.iter().copied() {
            assert_eq!(
                world.hit_test_grip(corner.of(frame), node, tolerance),
                Some(corner),
                "{}",
                corner.name()
            );
        }

        // The middle of the element is the body, not a grip.
        assert_eq!(world.hit_test_grip(frame.center(), node, tolerance), None);

        // A locked element cannot be resized, for the same reason it cannot be
        // dragged: §26's lock is about the whole element, not about one gesture.
        world.nodes.set_flag(node, NodeFlags::LOCKED, true);
        assert_eq!(
            world.hit_test_grip(frame.min(), node, tolerance),
            None,
            "a locked element offered a grip"
        );
    }

    fn graph_node(world: &mut GraphWorld, x: f32, y: f32) -> NodeIndex {
        let node = world.create_node(
            ElementKind::GraphNode(GraphNodeKind::Default),
            Vec2::new(x, y),
            Vec2::new(160.0, 60.0),
        );
        world.add_handle(
            node,
            HandleSpec::new("out", HandlePlacement::Right, HandleDirection::Source),
        );
        world.add_handle(
            node,
            HandleSpec::new("in", HandlePlacement::Left, HandleDirection::Target),
        );
        node
    }

    /// Source handle of `node`, by construction of [`graph_node`].
    fn out(world: &GraphWorld, node: NodeIndex) -> EdgeEnd {
        EdgeEnd::handle(
            node,
            world.handle_index(node, &HandleId::new("out")).unwrap(),
        )
    }

    fn to_in(world: &GraphWorld, node: NodeIndex) -> EdgeEnd {
        EdgeEnd::handle(
            node,
            world.handle_index(node, &HandleId::new("in")).unwrap(),
        )
    }

    /// Two nodes, one edge between their handles.
    fn pair() -> (GraphWorld, NodeIndex, NodeIndex) {
        let mut world = GraphWorld::new();
        let a = graph_node(&mut world, 0.0, 0.0);
        let b = graph_node(&mut world, 400.0, 200.0);
        let (source, target) = (out(&world, a), to_in(&world, b));
        world.connect(source, target).expect("a valid connection");
        world.rebuild_dirty_geometry();
        // A freshly built node is dirty from birth — it has never been drawn or
        // indexed — so a test about what *one action* invalidates has to start
        // from a world that has been drawn once.
        world.dirty_mut().clear_all();
        (world, a, b)
    }

    // ---- the property the architecture exists for ------------------------

    // ---- §28: box selection ---------------------------------------------

    /// Three nodes in a row, joined left-to-right, with every route built.
    fn row(world: &mut GraphWorld) -> [NodeIndex; 3] {
        let a = graph_node(world, 0.0, 0.0);
        let b = graph_node(world, 400.0, 0.0);
        let c = graph_node(world, 800.0, 0.0);
        world
            .connect(out(world, a), to_in(world, b))
            .expect("valid");
        world
            .connect(out(world, b), to_in(world, c))
            .expect("valid");
        world.rebuild_all_geometry();
        [a, b, c]
    }

    /// Every node, as the broad phase would have handed them over. **A scan is
    /// legitimate in a test** — it is the oracle standing in for the spatial
    /// index, which this file must never reach for.
    fn all_nodes(world: &GraphWorld) -> Vec<NodeIndex> {
        world.nodes().indices().collect()
    }

    fn all_edges(world: &GraphWorld) -> Vec<crate::models::EdgeIndex> {
        world.edges().indices().collect()
    }

    #[test]
    fn a_band_selects_what_it_touches_and_nothing_else() {
        let mut world = GraphWorld::new();
        let [a, b, c] = row(&mut world);

        // Over the first node only.
        let band = Rect::new(Vec2::new(-20.0, -20.0), Vec2::new(200.0, 100.0));
        let changed =
            world.apply_box_selection(BoxQuery::at_zoom(band, 1.0), all_nodes(&world), Vec::new());

        assert_eq!(changed, 1);
        assert!(world.selection().contains_node(a));
        assert!(!world.selection().contains_node(b));
        assert!(!world.selection().contains_node(c));
        // The store flag and the set agree, which is the invariant that makes
        // the painter and the command layer see the same selection.
        assert!(world.nodes().is_selected(a));
        assert!(!world.nodes().is_selected(b));
    }

    #[test]
    fn enclose_is_stricter_than_touch() {
        let mut world = GraphWorld::new();
        let [a, _, _] = row(&mut world);

        // Clips the node's left half only.
        let band = Rect::new(Vec2::new(-20.0, -20.0), Vec2::new(100.0, 100.0));

        world.apply_box_selection(
            BoxQuery::at_zoom(band, 1.0).with_mode(BoxSelectMode::Enclose),
            all_nodes(&world),
            Vec::new(),
        );
        assert!(
            !world.selection().contains_node(a),
            "Enclose selected a node the band only clipped"
        );

        world.apply_box_selection(
            BoxQuery::at_zoom(band, 1.0).with_mode(BoxSelectMode::Touch),
            all_nodes(&world),
            Vec::new(),
        );
        assert!(world.selection().contains_node(a));
    }

    /// **The case a bounds-only test gets wrong**, and the reason
    /// `EdgeRoute::intersects_rect` flattens the curve: an edge crossing the
    /// band is selected even though neither of its ends is inside it.
    #[test]
    fn an_edge_crossing_the_band_is_selected_with_both_ends_outside() {
        let mut world = GraphWorld::new();
        row(&mut world);

        // A tall, narrow band in the gap between the first two nodes: it
        // touches no node at all, and the edge runs straight through it.
        let band = Rect::new(Vec2::new(250.0, -200.0), Vec2::new(40.0, 400.0));
        let changed = world.apply_box_selection(
            BoxQuery::at_zoom(band, 1.0),
            all_nodes(&world),
            all_edges(&world),
        );

        assert_eq!(changed, 1, "the band should have caught exactly one edge");
        assert!(world.selection().nodes().is_empty());
        assert_eq!(world.selection().edges().len(), 1);
        assert!(world.edges().is_selected(world.selection().edges()[0]));
    }

    /// And the case that must *not* be selected: a band beside the edge, whose
    /// bounding box overlaps the route's but which the curve never enters.
    #[test]
    fn a_band_beside_an_edge_does_not_select_it() {
        let mut world = GraphWorld::new();
        row(&mut world);

        let band = Rect::new(Vec2::new(250.0, 200.0), Vec2::new(40.0, 100.0));
        world.apply_box_selection(
            BoxQuery::at_zoom(band, 1.0),
            all_nodes(&world),
            all_edges(&world),
        );

        assert!(
            world.selection().is_empty(),
            "the band selected an edge it misses"
        );
    }

    #[test]
    fn a_band_replaces_the_selection_unless_it_is_additive() {
        let mut world = GraphWorld::new();
        let [a, b, _] = row(&mut world);

        let first = Rect::new(Vec2::new(-20.0, -20.0), Vec2::new(200.0, 100.0));
        let second = Rect::new(Vec2::new(380.0, -20.0), Vec2::new(200.0, 100.0));

        world.apply_box_selection(BoxQuery::at_zoom(first, 1.0), all_nodes(&world), Vec::new());
        world.apply_box_selection(
            BoxQuery::at_zoom(second, 1.0),
            all_nodes(&world),
            Vec::new(),
        );
        assert_eq!(
            world.selection().single_node(),
            Some(b),
            "the band did not replace"
        );
        assert!(
            !world.nodes().is_selected(a),
            "a deselected node kept its flag"
        );

        world.apply_box_selection(
            BoxQuery::at_zoom(first, 1.0).additive(true),
            all_nodes(&world),
            Vec::new(),
        );
        assert_eq!(world.selection().nodes().len(), 2);
        assert!(world.selection().contains_node(a));
        assert!(world.selection().contains_node(b));
    }

    #[test]
    fn a_locked_or_hidden_element_is_never_selected_by_a_band() {
        let mut world = GraphWorld::new();
        let [a, b, _] = row(&mut world);
        world.nodes.set_flag(a, NodeFlags::LOCKED, true);
        world.set_node_hidden(b, true);

        let band = Rect::new(Vec2::new(-500.0, -500.0), Vec2::new(2_000.0, 1_000.0));
        world.apply_box_selection(BoxQuery::at_zoom(band, 1.0), all_nodes(&world), Vec::new());

        assert!(
            !world.selection().contains_node(a),
            "a locked node was selected"
        );
        assert!(
            !world.selection().contains_node(b),
            "a hidden node was selected"
        );
    }

    /// §28: the selection holds ids, and clearing it is proportional to the
    /// selection rather than to the document.
    #[test]
    fn clearing_the_selection_touches_only_what_was_selected() {
        let mut world = GraphWorld::new();
        for index in 0..200 {
            graph_node(&mut world, index as f32 * 10.0, 0.0);
        }
        world.set_node_selected(NodeIndex::new(3), true);
        world.set_node_selected(NodeIndex::new(7), true);
        world.dirty_mut().clear_all();

        world.clear_selection();

        assert!(world.selection().is_empty());
        assert!(!world.nodes().is_selected(NodeIndex::new(3)));
        assert_eq!(
            world.dirty().dirty_nodes().len(),
            2,
            "clearing a two-node selection invalidated {} nodes",
            world.dirty().dirty_nodes().len()
        );
    }

    #[test]
    fn select_only_replaces_and_is_idempotent() {
        let mut world = GraphWorld::new();
        let [a, b, _] = row(&mut world);

        world.select_only(Some(a));
        world.select_only(Some(a));
        assert_eq!(world.selection().single_node(), Some(a));

        world.select_only(Some(b));
        assert_eq!(world.selection().single_node(), Some(b));
        assert!(!world.nodes().is_selected(a));

        world.select_only(None);
        assert!(world.selection().is_empty());
    }

    /// The stale-route case: an edge whose geometry has been invalidated and
    /// not rebuilt is not selected at where it used to be.
    #[test]
    fn a_band_does_not_select_an_edge_with_a_stale_route() {
        let mut world = GraphWorld::new();
        row(&mut world);
        world.set_edge_routing(crate::models::EdgeIndex::new(0), EdgeRouting::Step);

        let band = Rect::new(Vec2::new(-500.0, -500.0), Vec2::new(2_000.0, 1_000.0));
        world.apply_box_selection(BoxQuery::at_zoom(band, 1.0), Vec::new(), all_edges(&world));

        assert!(
            !world
                .selection()
                .contains_edge(crate::models::EdgeIndex::new(0))
        );
        assert!(
            world
                .selection()
                .contains_edge(crate::models::EdgeIndex::new(1))
        );
    }

    /// **§19's target, asserted rather than claimed.**
    ///
    /// 100,000 nodes and 500,000 edges; move one node with four connected
    /// edges; the work must be one node, four edge geometry rebuilds and one
    /// spatial update — proportional to the moved node's own degree and to
    /// nothing else. This is the test the whole runtime is shaped by: if it
    /// starts failing because something added a pass over the graph, the
    /// architecture has stopped earning its complexity.
    #[test]
    fn moving_one_node_in_a_huge_graph_rebuilds_only_its_own_edges() {
        const NODES: usize = 100_000;
        const EDGES: usize = 500_000;

        let mut world = GraphWorld::new();
        world.set_rules(ConnectionRules::PERMISSIVE);
        world.reserve(NODES, EDGES);

        for index in 0..NODES {
            world.create_node(
                ElementKind::GraphNode(GraphNodeKind::Default),
                Vec2::new((index % 400) as f32 * 200.0, (index / 400) as f32 * 120.0),
                Vec2::new(160.0, 60.0),
            );
        }

        // A ring plus four chords, which gives every node degree ten and keeps
        // the construction O(edges) — the graph's shape does not matter here,
        // only that it is enormous and that one node's degree is known.
        for index in 0..EDGES {
            let source = NodeIndex::new((index % NODES) as u32);
            let target = NodeIndex::new(((index * 7 + 1) % NODES) as u32);
            world
                .connect(EdgeEnd::node(source), EdgeEnd::node(target))
                .expect("whole-node connections between graph nodes");
        }

        // One more node, deliberately given exactly four edges, and every route
        // brought up to date so the measurement starts from a clean world.
        let subject = world.create_node(
            ElementKind::GraphNode(GraphNodeKind::Default),
            Vec2::new(-5_000.0, -5_000.0),
            Vec2::new(160.0, 60.0),
        );
        for neighbour in 0..4u32 {
            world
                .connect(
                    EdgeEnd::node(subject),
                    EdgeEnd::node(NodeIndex::new(neighbour * 1_000)),
                )
                .expect("valid");
        }
        // Every edge was marked when it was connected. Clearing the flags is
        // the cheap way to start from a clean world — building half a million
        // routes would measure the route builder rather than the propagation,
        // and `moving_a_node_leaves_every_unconnected_edge_alone` checks the
        // built-route side of it on a graph small enough to be exact.
        world.dirty_mut().clear_all();

        assert_eq!(world.nodes().len(), NODES + 1);
        assert_eq!(world.edges().len(), EDGES + 4);
        assert_eq!(world.adjacency().degree(subject), 4);
        assert!(world.dirty().is_clean(), "the world starts clean");

        let rebuilds_before = world.geometry().rebuild_count();

        world.move_node(subject, Vec2::new(25.0, -10.0));

        // Exactly one node invalidated, exactly one spatial update queued,
        // exactly four edges marked — before anything is rebuilt.
        assert_eq!(world.dirty().dirty_nodes(), &[subject]);
        assert_eq!(world.dirty().spatial_updates(), &[subject]);
        assert_eq!(world.dirty().dirty_edges().len(), 4);
        assert!(
            world
                .dirty()
                .node_flags(subject)
                .contains(NodeDirty::POSITION)
        );

        let rebuilt = world.rebuild_dirty_geometry();

        assert_eq!(rebuilt, 4, "one rebuild per incident edge, and no more");
        assert_eq!(
            world.geometry().rebuild_count() - rebuilds_before,
            4,
            "the store agrees with the caller"
        );
        assert!(world.dirty().dirty_edges().is_empty());
    }

    /// The same rule from the other side: a node that is *not* moved has none
    /// of its edges touched, however busy its neighbours are.
    #[test]
    fn moving_a_node_leaves_every_unconnected_edge_alone() {
        let mut world = GraphWorld::new();
        world.set_rules(ConnectionRules::PERMISSIVE);
        let a = graph_node(&mut world, 0.0, 0.0);
        let b = graph_node(&mut world, 400.0, 0.0);
        let c = graph_node(&mut world, 800.0, 0.0);
        let d = graph_node(&mut world, 1200.0, 0.0);

        let ab = world.connect(out(&world, a), to_in(&world, b)).unwrap();
        let cd = world.connect(out(&world, c), to_in(&world, d)).unwrap();
        world.rebuild_all_geometry();
        world.dirty_mut().clear_all();

        world.move_node(a, Vec2::new(10.0, 0.0));

        assert_eq!(world.dirty().dirty_edges(), &[ab]);
        assert!(
            world.geometry().is_valid(cd),
            "an unrelated edge is untouched"
        );
        assert!(!world.geometry().is_valid(ab));
        assert_eq!(world.rebuild_dirty_geometry(), 1);
    }

    /// A pure pan changes the viewport and nothing in the world, so no route is
    /// invalidated — §40 rule 6, held by construction.
    #[test]
    fn a_frame_with_nothing_moving_rebuilds_nothing() {
        let (mut world, _, _) = pair();

        assert_eq!(world.rebuild_dirty_geometry(), 0);
        assert_eq!(world.rebuild_dirty_geometry(), 0);
    }

    /// Dragging emits many small moves. Each one marks the same edges, and the
    /// queue must not grow with the gesture.
    #[test]
    fn a_long_drag_queues_each_incident_edge_once() {
        let (mut world, a, _) = pair();

        for _ in 0..120 {
            world.move_node(a, Vec2::new(1.0, 0.0));
        }

        assert_eq!(world.dirty().dirty_edges().len(), 1);
        assert_eq!(world.rebuild_dirty_geometry(), 1);
    }

    #[test]
    fn a_zero_delta_move_changes_nothing_and_marks_nothing() {
        let (mut world, a, _) = pair();

        world.move_node(a, Vec2::ZERO);

        assert!(world.dirty().dirty_edges().is_empty());
        assert!(world.dirty().dirty_nodes().is_empty());
    }

    /// Selecting a node repaints it and reroutes nothing — the reason the dirty
    /// flags are per change rather than one bit.
    #[test]
    fn selecting_a_node_touches_no_edge() {
        let (mut world, a, _) = pair();

        world.set_node_selected(a, true);

        assert_eq!(world.dirty().dirty_nodes(), &[a]);
        assert!(world.dirty().dirty_edges().is_empty());
        assert!(world.nodes().is_selected(a));
    }

    #[test]
    fn resizing_a_node_reroutes_its_edges_because_its_handles_moved() {
        let (mut world, a, _) = pair();

        world.set_node_size(a, Vec2::new(300.0, 60.0));

        assert_eq!(world.dirty().dirty_edges().len(), 1);
        assert!(world.dirty().node_flags(a).contains(NodeDirty::SIZE));
    }

    // ---- routing ---------------------------------------------------------

    #[test]
    fn a_route_runs_between_the_two_handles_it_connects() {
        let (world, a, b) = pair();
        let edge = world.edges().indices().next().unwrap();

        let route = world.route(edge).expect("built by the fixture");
        let source = world.handle_position(world.handle_index(a, &HandleId::new("out")).unwrap());
        let target = world.handle_position(world.handle_index(b, &HandleId::new("in")).unwrap());

        assert_eq!(route.start(), source);
        assert_eq!(route.end(), target);
        assert_eq!(route.start_side(), Side::Right);
        assert_eq!(route.end_side(), Side::Left);
    }

    #[test]
    fn a_moved_node_takes_its_route_with_it() {
        let (mut world, a, _) = pair();
        let edge = world.edges().indices().next().unwrap();
        let before = world.route(edge).unwrap().start();

        world.move_node(a, Vec2::new(30.0, 40.0));
        world.rebuild_dirty_geometry();

        assert_eq!(
            world.route(edge).unwrap().start(),
            before + Vec2::new(30.0, 40.0)
        );
    }

    /// §4's floating connection point: an endpoint with no handle attaches to
    /// the side facing its partner, and follows it around the node.
    #[test]
    fn a_whole_node_endpoint_attaches_to_the_side_facing_its_partner() {
        let mut world = GraphWorld::new();
        let a = graph_node(&mut world, 0.0, 0.0);
        let b = graph_node(&mut world, 600.0, 0.0);
        let edge = world.connect(EdgeEnd::node(a), EdgeEnd::node(b)).unwrap();
        world.rebuild_dirty_geometry();

        assert_eq!(world.route(edge).unwrap().start_side(), Side::Right);
        assert_eq!(world.route(edge).unwrap().end_side(), Side::Left);

        // Drag the far node round to the other side; the attachment follows.
        world.set_node_position(b, Vec2::new(-600.0, 0.0));
        world.rebuild_dirty_geometry();

        assert_eq!(world.route(edge).unwrap().start_side(), Side::Left);
        assert_eq!(world.route(edge).unwrap().end_side(), Side::Right);
    }

    #[test]
    fn changing_an_edges_routing_rebuilds_only_that_edge() {
        let (mut world, _, _) = pair();
        let edge = world.edges().indices().next().unwrap();

        world.set_edge_routing(edge, EdgeRouting::SmoothStep);

        assert_eq!(world.dirty().dirty_edges(), &[edge]);
        assert_eq!(world.rebuild_dirty_geometry(), 1);
        assert_eq!(
            world.route(edge).unwrap().routing(),
            EdgeRouting::SmoothStep
        );
    }

    #[test]
    fn changing_the_route_options_invalidates_every_edge() {
        let (mut world, _, _) = pair();
        let mut options = *world.route_options();
        options.step_offset = 60.0;

        world.set_route_options(options);

        assert_eq!(world.dirty().dirty_edges().len(), world.edges().len());
    }

    // ---- connection validation (§4) --------------------------------------

    #[test]
    fn a_source_handle_may_not_be_a_target_and_the_other_way_round() {
        let mut world = GraphWorld::new();
        let a = graph_node(&mut world, 0.0, 0.0);
        let b = graph_node(&mut world, 400.0, 0.0);

        let backwards = world.connect(to_in(&world, a), out(&world, b));

        assert!(matches!(
            backwards,
            Err(ConnectionError::DirectionMismatch { .. })
        ));
    }

    #[test]
    fn a_loose_handle_accepts_either_role() {
        let mut world = GraphWorld::new();
        let a = graph_node(&mut world, 0.0, 0.0);
        let b = world.create_node(
            ElementKind::GraphNode(GraphNodeKind::Default),
            Vec2::new(400.0, 0.0),
            Vec2::new(160.0, 60.0),
        );
        let loose = world.add_handle(
            b,
            HandleSpec::new("either", HandlePlacement::Left, HandleDirection::Loose),
        );

        assert!(
            world
                .connect(out(&world, a), EdgeEnd::handle(b, loose))
                .is_ok()
        );
        assert!(
            world
                .connect(EdgeEnd::handle(b, loose), to_in(&world, a))
                .is_ok(),
            "the same handle, now as a source"
        );
    }

    #[test]
    fn a_handle_at_its_limit_refuses_the_next_connection() {
        let mut world = GraphWorld::new();
        let a = graph_node(&mut world, 0.0, 0.0);
        let b = graph_node(&mut world, 400.0, 0.0);
        let c = graph_node(&mut world, 800.0, 0.0);
        let single = world.add_handle(
            c,
            HandleSpec::new("only", HandlePlacement::Left, HandleDirection::Target).with_limit(1),
        );

        assert!(
            world
                .connect(out(&world, a), EdgeEnd::handle(c, single))
                .is_ok()
        );
        let second = world.connect(out(&world, b), EdgeEnd::handle(c, single));

        assert!(matches!(
            second,
            Err(ConnectionError::HandleAtLimit { limit: 1, .. })
        ));
    }

    /// A limit counts that handle's own connections, not its node's — a node
    /// with a full input and an empty output is not full.
    #[test]
    fn a_limit_counts_the_handle_rather_than_the_node() {
        let mut world = GraphWorld::new();
        let a = graph_node(&mut world, 0.0, 0.0);
        let b = graph_node(&mut world, 400.0, 0.0);
        let c = graph_node(&mut world, 800.0, 0.0);
        let limited = world.add_handle(
            b,
            HandleSpec::new("one_in", HandlePlacement::Top, HandleDirection::Target).with_limit(1),
        );

        world
            .connect(out(&world, a), EdgeEnd::handle(b, limited))
            .unwrap();

        assert!(
            world.connect(out(&world, b), to_in(&world, c)).is_ok(),
            "a different handle on the same node is unaffected"
        );
    }

    #[test]
    fn a_self_connection_is_refused_unless_the_rules_allow_it() {
        let mut world = GraphWorld::new();
        let a = graph_node(&mut world, 0.0, 0.0);

        assert!(matches!(
            world.connect(out(&world, a), to_in(&world, a)),
            Err(ConnectionError::SelfConnection(_))
        ));

        world.set_rules(ConnectionRules {
            allow_self_connections: true,
            ..ConnectionRules::DEFAULT
        });
        assert!(world.connect(out(&world, a), to_in(&world, a)).is_ok());
    }

    #[test]
    fn the_same_pair_of_handles_is_not_connected_twice() {
        let mut world = GraphWorld::new();
        let a = graph_node(&mut world, 0.0, 0.0);
        let b = graph_node(&mut world, 400.0, 0.0);
        let first = world.connect(out(&world, a), to_in(&world, b)).unwrap();

        assert_eq!(
            world.connect(out(&world, a), to_in(&world, b)),
            Err(ConnectionError::Duplicate(first))
        );
    }

    /// Two edges between the same nodes through different ports are not
    /// duplicates, and neither is the reverse direction.
    #[test]
    fn a_different_port_or_direction_is_not_a_duplicate() {
        let mut world = GraphWorld::new();
        let a = graph_node(&mut world, 0.0, 0.0);
        let b = graph_node(&mut world, 400.0, 0.0);
        let second_in = world.add_handle(
            b,
            HandleSpec::new("in2", HandlePlacement::Top, HandleDirection::Target),
        );

        world.connect(out(&world, a), to_in(&world, b)).unwrap();

        assert!(
            world
                .connect(out(&world, a), EdgeEnd::handle(b, second_in))
                .is_ok()
        );
        assert!(
            world.connect(out(&world, b), to_in(&world, a)).is_ok(),
            "A->B and B->A are different connections"
        );
    }

    #[test]
    fn a_handle_that_belongs_to_another_node_is_refused() {
        let mut world = GraphWorld::new();
        let a = graph_node(&mut world, 0.0, 0.0);
        let b = graph_node(&mut world, 400.0, 0.0);
        let b_in = world.handle_index(b, &HandleId::new("in")).unwrap();

        let wrong = world.connect(out(&world, a), EdgeEnd::handle(a, b_in));

        assert!(matches!(
            wrong,
            Err(ConnectionError::HandleNotOnNode { .. })
        ));
    }

    #[test]
    fn an_unknown_node_or_handle_is_refused_rather_than_panicking() {
        let mut world = GraphWorld::new();
        let a = graph_node(&mut world, 0.0, 0.0);

        assert!(matches!(
            world.connect(out(&world, a), EdgeEnd::node(NodeIndex::new(99))),
            Err(ConnectionError::UnknownNode(_))
        ));
        assert!(matches!(
            world.connect(
                out(&world, a),
                EdgeEnd::handle(a, crate::models::HandleIndex::new(99))
            ),
            Err(ConnectionError::UnknownHandle(_))
        ));
    }

    /// §4's whole-node mode is available to graph nodes and not to drawn
    /// shapes, which have no graph semantics at all.
    #[test]
    fn a_drawn_shape_does_not_accept_a_whole_node_connection() {
        let mut world = GraphWorld::new();
        let a = graph_node(&mut world, 0.0, 0.0);
        let shape = world.create_node(
            ElementKind::Shape(ShapeKind::Ellipse),
            Vec2::new(400.0, 0.0),
            Vec2::new(100.0, 100.0),
        );

        assert!(matches!(
            world.connect(out(&world, a), EdgeEnd::node(shape)),
            Err(ConnectionError::NodeNotConnectable(_))
        ));
    }

    #[test]
    fn requiring_handles_refuses_a_whole_node_endpoint() {
        let mut world = GraphWorld::new();
        world.set_rules(ConnectionRules {
            require_handles: true,
            ..ConnectionRules::DEFAULT
        });
        let a = graph_node(&mut world, 0.0, 0.0);
        let b = graph_node(&mut world, 400.0, 0.0);

        assert_eq!(
            world.connect(out(&world, a), EdgeEnd::node(b)),
            Err(ConnectionError::HandleRequired)
        );
    }

    #[test]
    fn a_refused_connection_adds_no_edge_and_marks_nothing() {
        let mut world = GraphWorld::new();
        let a = graph_node(&mut world, 0.0, 0.0);
        world.dirty_mut().clear_all();

        assert!(world.connect(out(&world, a), to_in(&world, a)).is_err());

        assert_eq!(world.edges().len(), 0);
        assert!(world.dirty().is_clean());
    }

    #[test]
    fn can_connect_agrees_with_connect() {
        let mut world = GraphWorld::new();
        let a = graph_node(&mut world, 0.0, 0.0);
        let b = graph_node(&mut world, 400.0, 0.0);

        assert!(world.can_connect(out(&world, a), to_in(&world, b)));
        assert!(!world.can_connect(to_in(&world, a), out(&world, b)));
    }

    // ---- hit testing -----------------------------------------------------

    #[test]
    fn a_press_inside_a_node_hits_its_body() {
        let (world, a, _) = pair();

        let hit = world.hit_test(
            Vec2::new(80.0, 30.0),
            world.nodes().indices(),
            HitTolerance::new(9.0),
        );

        assert_eq!(hit, PointerTarget::Node(a));
    }

    #[test]
    fn a_press_near_a_handle_hits_the_handle_rather_than_the_body() {
        let (world, a, _) = pair();
        let out_handle = world.handle_index(a, &HandleId::new("out")).unwrap();
        let position = world.handle_position(out_handle);

        let hit = world.hit_test(
            position + Vec2::new(-3.0, 2.0),
            world.nodes().indices(),
            HitTolerance::new(9.0),
        );

        assert_eq!(
            hit,
            PointerTarget::Handle {
                node: a,
                handle: out_handle
            }
        );
    }

    #[test]
    fn a_press_on_nothing_hits_nothing() {
        let (world, _, _) = pair();

        let hit = world.hit_test(
            Vec2::new(-500.0, -500.0),
            world.nodes().indices(),
            HitTolerance::default(),
        );

        assert!(hit.is_empty());
    }

    #[test]
    fn a_hidden_node_is_not_hit() {
        let (mut world, a, _) = pair();
        world.set_node_hidden(a, true);

        let hit = world.hit_test(
            Vec2::new(80.0, 30.0),
            world.nodes().indices(),
            HitTolerance::default(),
        );

        assert!(hit.is_empty());
    }

    /// Overlapping bodies resolve to the topmost, which is the one the painter
    /// drew last.
    #[test]
    fn overlapping_nodes_resolve_to_the_one_on_top() {
        let mut world = GraphWorld::new();
        let under = graph_node(&mut world, 0.0, 0.0);
        let over = graph_node(&mut world, 10.0, 10.0);
        assert_ne!(under, over);

        let hit = world.hit_test(
            Vec2::new(60.0, 30.0),
            world.nodes().indices(),
            HitTolerance::new(1.0),
        );

        assert_eq!(hit, PointerTarget::Node(over));
        assert_ne!(hit, PointerTarget::Node(under));
    }

    // ---- §29's edge narrow phase (§9's double-click needs it) -----------

    /// **An edge is hit by its drawn line, not by its bounding box.**
    ///
    /// The whole reason the narrow phase is a distance: an edge from (0,0) to
    /// (400,200) has a bounding box of 80,000 square units and covers a few
    /// hundred of them. A box test would make the empty corners of every edge
    /// unclickable canvas.
    #[test]
    fn an_edge_is_hit_near_its_line_and_missed_in_the_corners_of_its_box() {
        let (world, a, b) = pair();
        let edge = world.incident_edges(a).next().expect("the pair is joined");
        let route = world.route(edge).expect("geometry was rebuilt");
        let tolerance = HitTolerance::new(9.0);

        let on_the_line = route.midpoint(1.0);
        assert_eq!(
            world.hit_test_edge(on_the_line, world.edges().indices(), tolerance, 1.0),
            Some(edge)
        );

        // A corner of the route's own bounding box, far from the curve.
        let corner = Vec2::new(route.bounds().min().x, route.bounds().max().y);
        assert_eq!(
            world.hit_test_edge(corner, world.edges().indices(), tolerance, 1.0),
            None,
            "the box is not the edge"
        );

        // And well outside it.
        assert_eq!(
            world.hit_test_edge(
                Vec2::new(-5_000.0, -5_000.0),
                world.edges().indices(),
                tolerance,
                1.0
            ),
            None
        );
        let _ = b;
    }

    /// **The tolerance holds in screen pixels at every zoom** (§29), through
    /// the real narrow phase rather than only through the arithmetic.
    ///
    /// A thin line is the hardest thing on the canvas to aim at, and the
    /// world-space band it is grabbed by has to shrink as the camera comes in
    /// and grow as it goes out, or an edge is unclickable at one end of the
    /// zoom range and swallows the canvas at the other. Four screen pixels off
    /// the line is a hit at every camera; twelve is a miss at every camera —
    /// both stated in *screen* pixels and converted per zoom, which is the
    /// point.
    #[test]
    fn an_edge_stays_clickable_at_every_zoom() {
        let (world, a, _) = pair();
        let edge = world.incident_edges(a).next().expect("the pair is joined");
        let route = world.route(edge).expect("geometry was rebuilt");

        for zoom in [0.1_f32, 0.5, 1.0, 2.0, 10.0] {
            let tolerance = HitTolerance::at_zoom(zoom);
            // One screen pixel in world units, exactly what `views::flow`
            // passes as the flattening step.
            let flatten = 1.0 / zoom;

            // Straight down from the midpoint: the route runs left to right
            // between two nodes on the same row, so this is across the line.
            let midpoint = route.midpoint(flatten);
            let near = midpoint + Vec2::new(0.0, 4.0 / zoom);
            let far = midpoint + Vec2::new(0.0, 12.0 / zoom);

            assert_eq!(
                world.hit_test_edge(near, world.edges().indices(), tolerance, flatten),
                Some(edge),
                "four screen pixels off the line missed at zoom {zoom}"
            );
            assert_eq!(
                world.hit_test_edge(far, world.edges().indices(), tolerance, flatten),
                None,
                "twelve screen pixels off the line hit at zoom {zoom}"
            );
        }
    }

    /// A removed edge is not hit. Removal is a tombstone, so its row and its
    /// route survive — and a hit test that read them would let a user label an
    /// edge that is not there.
    #[test]
    fn a_removed_edge_is_not_hit() {
        let (mut world, a, _) = pair();
        let edge = world.incident_edges(a).next().unwrap();
        let on_the_line = world.route(edge).unwrap().midpoint(1.0);

        world.remove_edge(edge);

        assert_eq!(
            world.hit_test_edge(
                on_the_line,
                world.edges().indices(),
                HitTolerance::new(9.0),
                1.0
            ),
            None
        );
    }

    // ---- load and save ---------------------------------------------------

    fn document() -> FlowDocument {
        let mut document = FlowDocument::new();
        let a = document.add_node(
            ElementKind::GraphNode(GraphNodeKind::Input),
            Vec2::new(0.0, 0.0),
            Vec2::new(160.0, 60.0),
        );
        let b = document.add_node(
            ElementKind::GraphNode(GraphNodeKind::Output),
            Vec2::new(400.0, 120.0),
            Vec2::new(160.0, 60.0),
        );

        document.node_mut(a).unwrap().handles = vec![Handle::new(
            "out",
            HandlePlacement::Right,
            HandleDirection::Source,
        )];
        document.node_mut(b).unwrap().handles = vec![Handle::new(
            "in",
            HandlePlacement::Left,
            HandleDirection::Target,
        )];
        document.add_edge(Endpoint::handle(a, "out"), Endpoint::handle(b, "in"));
        document
    }

    /// The round trip that proves the runtime loses nothing: a document becomes
    /// a world and comes back byte-identical.
    #[test]
    fn a_document_survives_the_round_trip_through_the_runtime() {
        let original = document();

        let (world, report) = GraphWorld::from_document(&original);
        let restored = world.to_document();

        assert!(report.is_clean(), "{report:?}");
        assert_eq!(restored, original);
    }

    #[test]
    fn loading_resolves_every_endpoint_to_a_runtime_index() {
        let (world, _) = GraphWorld::from_document(&document());
        let edge = world.edges().indices().next().unwrap();

        assert_eq!(world.nodes().len(), 2);
        assert_eq!(world.handles().len(), 2);
        assert_eq!(world.edges().source(edge).node, NodeIndex::new(0));
        assert!(world.edges().source(edge).handle.get().is_some());
        assert_eq!(world.adjacency().degree(NodeIndex::new(0)), 1);
    }

    /// A file the loader cannot fully honour must say so rather than
    /// pretending, and must not lose the edges it *can* place.
    #[test]
    fn an_edge_naming_a_missing_node_is_reported_and_dropped() {
        let mut document = document();
        document.edges.push(crate::models::FlowEdge::new(
            ElementId::new(999),
            Endpoint::node(ElementId::new(12_345)),
            Endpoint::node(ElementId::new(1)),
        ));

        let (world, report) = GraphWorld::from_document(&document);

        assert_eq!(report.dangling_edges, vec![ElementId::new(999)]);
        assert_eq!(world.edges().len(), 1);
    }

    /// An edge naming a handle its node does not have keeps its connection,
    /// attached to the node itself — §4's whole-node mode as the fallback.
    #[test]
    fn an_edge_naming_a_missing_handle_falls_back_to_the_node() {
        let mut document = document();
        document.edges[0].target = Endpoint::handle(ElementId::new(2), "nonexistent");

        let (world, report) = GraphWorld::from_document(&document);
        let edge = world.edges().indices().next().unwrap();

        assert_eq!(report.unresolved_handles.len(), 1);
        assert_eq!(world.edges().len(), 1);
        assert!(world.edges().target(edge).handle.is_none());
    }

    /// A document may legitimately contain what the interactive rules would
    /// refuse — a self-loop, a duplicate — and loading must not delete it.
    #[test]
    fn loading_honours_the_file_rather_than_the_interactive_rules() {
        let mut document = document();
        document.add_edge(
            Endpoint::handle(ElementId::new(1), "out"),
            Endpoint::handle(ElementId::new(2), "in"),
        );
        document.add_edge(
            Endpoint::node(ElementId::new(1)),
            Endpoint::node(ElementId::new(1)),
        );

        let (world, report) = GraphWorld::from_document(&document);

        assert!(report.is_clean(), "{report:?}");
        assert_eq!(world.edges().len(), 3);
        assert_eq!(
            world.rules(),
            ConnectionRules::DEFAULT,
            "the permissive rules are for the load, not for what follows it"
        );
    }

    #[test]
    fn an_empty_report_is_the_clean_one() {
        assert!(LoadReport::default().is_clean());
    }

    /// **§13's structural claim, as a test**: switching between clean and
    /// hand-drawn is a *renderer* strategy, so it must not touch the graph.
    ///
    /// Nothing is recreated, no element version moves, no edge is queued for a
    /// rebuild and no spatial update is raised — so the toggle costs one field
    /// write and one repaint whatever the document's size, and the geometry
    /// cache's entries stay valid for the mode that is being switched away
    /// from. A sketch mode that rewrote elements would be a second document,
    /// which is exactly what §13 forbids.
    #[test]
    fn switching_render_style_touches_no_element() {
        let mut world = GraphWorld::new();
        let a = graph_node(&mut world, 0.0, 0.0);
        let b = graph_node(&mut world, 400.0, 120.0);
        let (source, target) = (out(&world, a), to_in(&world, b));
        world.connect(source, target).expect("a valid connection");
        world.rebuild_all_geometry();
        world.dirty_mut().clear_all();

        let before = world.to_document();
        let node_versions: Vec<u32> = world
            .nodes()
            .indices()
            .map(|node| world.nodes().version(node))
            .collect();
        let edge_versions: Vec<u32> = world
            .edges()
            .indices()
            .map(|edge| world.geometry().version(edge))
            .collect();

        world.settings_mut().render_style = crate::models::RenderStyle::Sketch;

        assert_eq!(
            world.rebuild_dirty_geometry(),
            0,
            "no route was invalidated"
        );
        assert!(
            world.dirty().spatial_updates().is_empty(),
            "no element moved, so the index has nothing to do"
        );
        assert_eq!(
            node_versions,
            world
                .nodes()
                .indices()
                .map(|node| world.nodes().version(node))
                .collect::<Vec<_>>(),
        );
        assert_eq!(
            edge_versions,
            world
                .edges()
                .indices()
                .map(|edge| world.geometry().version(edge))
                .collect::<Vec<_>>(),
        );

        // And the document differs in exactly one field.
        let after = world.to_document();
        assert_eq!(after.nodes, before.nodes);
        assert_eq!(after.edges, before.edges);
        assert_ne!(after.settings.render_style, before.settings.render_style);
    }
}
