//! §30's commands and undo — **every edit as a delta through one path**.
//!
//! ```text
//! edit.rs      the delta vocabulary, and the coalescing rule
//! apply.rs     the one mutation path, returning each edit's inverse
//! history.rs   the undo/redo stacks, and gesture grouping
//! editor.rs    the world and the history welded together
//! gesture.rs   §25's interaction effects, as commands
//! keys.rs      §26's binding table, as a function of the host
//! ```
//!
//! Start at [`editor`]: it states the invariant this phase exists for and how
//! the type system holds it. [`mod@apply`] says why the inverse is a return value
//! rather than a second enum, [`history`] records what
//! `gpui_component::history::History` was evaluated for and what specifically
//! did not fit, and [`edit`] lists which of §30's commands are here and which
//! are honestly absent because the engine cannot perform them.
//!
//! **Every file in this module names no UI framework.** That is not incidental:
//! the history has to sit beside the stores, and the stores are below the line.
//!
//! # The property that made the phase worth doing
//!
//! Undoing a node move restores the node's position, its spatial-index entry,
//! its incident edges' routes, the dirty flags and the geometry cache — and
//! **not one line below deals with any of those**. An undo is an ordinary edit
//! applied through the ordinary mutators, so §19's propagation runs for it
//! exactly as it ran for the edit. The tests at the bottom of this file assert
//! that as a frame-level property: a repaint after undo produces the same
//! visible set, the same routes and the same painted vertex count as never
//! having made the edit at all.

pub mod apply;
pub mod edit;
pub mod editor;
pub mod gesture;
pub mod history;
pub mod keys;
pub mod layers;

pub use apply::{EditOutcome, apply};
pub use edit::{EditCommand, EditError, NodeDraft};
pub use editor::{EditSummary, FlowEditor};
pub use gesture::{GestureReport, apply_gesture};
pub use history::{CommandHistory, GestureId, HistoryEntry};
pub use keys::{Binding, EditAction};
pub use layers::{DepthSpan, LayerAction};

#[cfg(test)]
mod tests {
    use super::{EditCommand, FlowEditor, NodeDraft};
    use crate::{
        geometry::{Rect, RouteSegment, Vec2, Viewport},
        models::{
            Color, EdgeIndex, EdgeRouting, ElementId, ElementKind, ElementStyle, FlowDocument,
            GraphNodeKind, NodeIndex, ShapeKind,
        },
        render::{
            GridSettings, Outline, PaintPlan, SceneInk, SceneOptions, SceneStats,
            cache::{GeometryOwner, GeometryPart},
            grid::GridLimits,
            plan::{PathPaint, PathPrimitive, PrimitiveSink, QuadPrimitive, TextPrimitive},
            registry::NodeRendererRegistry,
            scene,
            snapshot::RenderSnapshot,
        },
        runtime::{BoxQuery, ConnectionRules, EdgeEnd, EdgeSpec, GraphWorld, NodeSpec},
        scenes::SceneSpec,
        spatial::{SpatialIndex, VisibleSet},
    };

    /// **Everything a frame reads, reduced to something comparable.**
    ///
    /// The phase brief's sharpest requirement is that a test asserting the
    /// *document* came back is not enough: the spatial index, the routes and the
    /// dirty state are what a frame actually paints from, and an undo that
    /// restored the document while leaving a node indexed at its old place would
    /// pass a document test and paint a ghost.
    ///
    /// So this is the frame's whole input **and its whole output**: what the
    /// viewport query returns, where every visible element is, what the routes
    /// are, and the [`PaintPlan`] the scene planner builds out of all of it.
    /// The plan is every quad, path and glyph the painter would be handed, in
    /// order — two of these being equal is two frames being pixel-identical
    /// short of the GPU.
    #[derive(Debug, PartialEq)]
    struct Frame {
        visible_nodes: Vec<NodeIndex>,
        visible_edges: Vec<EdgeIndex>,
        node_bounds: Vec<(NodeIndex, Rect)>,
        edge_bounds: Vec<(EdgeIndex, Rect)>,
        routes: Vec<(EdgeIndex, Vec2, Vec<RouteSegment>)>,
        indexed_nodes: usize,
        indexed_edges: usize,
        painted: Painted,
        scene: SceneStats,
    }

    /// Everything the painter is handed, in paint order.
    #[derive(Debug, Default)]
    struct Painted {
        quads: Vec<QuadPrimitive>,
        paths: Vec<PaintedPath>,
        texts: Vec<TextPrimitive>,
        /// §10's pictures, as the painter is handed them: a frame, a handle
        /// and a crop. Part of the equality below, so an undone crop that
        /// restored the document and not the frame would fail there.
        images: Vec<crate::render::plan::ImagePrimitive>,
        /// The §23 cache versions this frame filed its geometry under, in
        /// order. Collected but **not** part of the equality — see
        /// [`PaintedPath`].
        versions: Vec<u32>,
    }

    /// One path as the painter sees it, **minus its cache version**.
    ///
    /// The version is deliberately outside the comparison, and this is the one
    /// place the phase's "the same frame" claim has a caveat worth stating. A
    /// §23 cache version only ever goes *up* — it is bumped by every write that
    /// changes what an element looks like, and the write that undoes one is
    /// still a write — so after an undo the geometry is identical and its cache
    /// key is new. That is the safe direction and it is on purpose: a spurious
    /// miss costs one tessellation, where a version that returned to a value it
    /// had held before would serve geometry from a state the element is no
    /// longer in.
    ///
    /// So the outline, the paint, the tolerance and *which element and part*
    /// the path belongs to are compared exactly, and the version is asserted
    /// separately to have moved forward.
    #[derive(Debug, Clone, PartialEq)]
    struct PaintedPath {
        outline: Outline,
        paint: PathPaint,
        quality: crate::models::RenderQuality,
        filed_under: Option<(GeometryOwner, GeometryPart)>,
    }

    /// **Written out rather than derived, to leave `versions` out of it.** See
    /// [`PaintedPath`] for why a §23 cache version is expected to differ after
    /// an undo while the geometry filed under it is expected not to.
    impl PartialEq for Painted {
        fn eq(&self, other: &Painted) -> bool {
            self.quads == other.quads
                && self.paths == other.paths
                && self.texts == other.texts
                && self.images == other.images
        }
    }

    impl PrimitiveSink for Painted {
        fn quad(&mut self, quad: &QuadPrimitive) {
            self.quads.push(*quad);
        }

        fn path(&mut self, path: &PathPrimitive) -> u32 {
            if let Some(key) = path.key {
                self.versions.push(key.version);
            }
            self.paths.push(PaintedPath {
                outline: path.outline.clone(),
                paint: path.paint,
                quality: path.quality,
                filed_under: path.key.map(|key| (key.owner, key.part)),
            });
            path.estimated_vertices()
        }

        fn text(&mut self, text: &TextPrimitive) -> u32 {
            self.texts.push(text.clone());
            0
        }

        fn image(&mut self, image: &crate::render::plan::ImagePrimitive) -> u32 {
            self.images.push(*image);
            1
        }
    }

    /// A theme's colours, fixed. The canvas is theme-driven and this test is
    /// not about the theme.
    fn ink() -> SceneInk {
        SceneInk {
            fill: Color::rgb(0.2, 0.2, 0.2),
            stroke: Color::rgb(0.9, 0.9, 0.9),
            edge: Color::rgb(0.7, 0.7, 0.7),
            handle: Color::rgb(0.3, 0.6, 1.0),
            accent: Color::rgb(1.0, 0.6, 0.2),
            text: Color::rgb(0.95, 0.95, 0.95),
        }
    }

    /// Runs the frame the way [`crate::views::FlowView`] does — rebuild stale
    /// routes, sync the index, spend the queues, query — and reports what came
    /// out. The order is the one `GraphWorld::clear_spatial_updates` documents;
    /// doing it in any other order is how culling bugs happen.
    fn frame(editor: &mut FlowEditor, index: &mut SpatialIndex, viewport: &Viewport) -> Frame {
        editor.rebuild_dirty_geometry();
        index.sync(editor.world());
        editor.clear_spatial_updates();

        let mut visible = VisibleSet::new();
        index.query_visible(editor.world(), viewport, &mut visible);

        // The rest of the frame, exactly as `views::flow` builds it: extract
        // §24's snapshot from the visible set, then plan the scene from the
        // snapshot. Neither step needs a window, which is the whole reason the
        // crate holds the UI-framework line where it does.
        let budgets = crate::budgets::current();
        let registry = NodeRendererRegistry::with_generic_kinds();
        let mut snapshot = RenderSnapshot::new();
        snapshot.extract(
            editor.world(),
            &visible,
            viewport,
            &budgets,
            &registry,
            None,
            None,
            Rect::new(Vec2::ZERO, viewport.size()),
        );

        let mut plan = PaintPlan::new();
        let scene = scene::plan_scene(
            &mut plan,
            editor.world(),
            &snapshot,
            viewport,
            ink(),
            &SceneOptions::new(GridSettings::default(), GridLimits::from_budgets(&budgets)),
        );

        // Through `paint_into`, so this collects exactly what a real painter is
        // handed, in the order the paint-order contract puts it in.
        let mut painted = Painted::default();
        plan.paint_into(&mut painted);

        let world = editor.world();
        Frame {
            painted,
            scene,
            visible_nodes: visible.nodes().to_vec(),
            visible_edges: visible.edges().to_vec(),
            node_bounds: visible
                .nodes()
                .iter()
                .map(|node| (*node, crate::spatial::node_painted_bounds(world, *node)))
                .collect(),
            edge_bounds: visible
                .edges()
                .iter()
                .filter_map(|edge| {
                    crate::spatial::edge_painted_bounds(world, *edge).map(|rect| (*edge, rect))
                })
                .collect(),
            routes: world
                .edges()
                .live_indices()
                .filter_map(|edge| {
                    world
                        .route(edge)
                        .map(|route| (edge, route.start(), route.segments().to_vec()))
                })
                .collect(),
            indexed_nodes: index.nodes().len(),
            indexed_edges: index.edges().len(),
        }
    }

    /// A node the frame paints something for that also has an edge — so moving
    /// it changes the plan and the routes together.
    fn visible_connected_node(frame: &Frame, world: &GraphWorld) -> NodeIndex {
        *frame
            .visible_nodes
            .iter()
            .find(|node| world.incident_edges(**node).count() > 0)
            .expect("the scene changed; it has no visible connected node")
    }

    fn scene_editor() -> (FlowEditor, SpatialIndex, Viewport) {
        let spec = SceneSpec::SMALL;
        let document = crate::scenes::build(&spec).to_document();
        let (mut editor, report) = FlowEditor::from_document(&document);
        assert!(report.is_clean());

        editor.rebuild_all_geometry();
        let mut index = SpatialIndex::for_world(editor.world());
        index.rebuild(editor.world());
        editor.clear_spatial_updates();

        (editor, index, spec.viewport(Vec2::new(1440.0, 900.0)))
    }

    /// **The property this phase exists for.**
    ///
    /// Move a node, repaint, undo, repaint — and the frame after the undo has to
    /// be byte-identical to the frame before the edit. Not the document: the
    /// visible set, the painted bounds, the routes and the index occupancy.
    #[test]
    fn a_repaint_after_undo_is_the_frame_the_edit_never_happened() {
        let (mut editor, mut index, viewport) = scene_editor();
        let before = frame(&mut editor, &mut index, &viewport);
        // A frame that draws nothing would pass every assertion below without
        // meaning any of them.
        assert!(!before.painted.paths.is_empty() && !before.painted.quads.is_empty());
        assert!(before.scene.edges > 0 && !before.visible_nodes.is_empty());

        // **A node the frame actually paints**, so the plan is under test and
        // not only the indices. Moved far enough to cross spatial cells.
        let node = visible_connected_node(&before, editor.world());
        editor
            .apply(EditCommand::move_node(node, Vec2::new(517.0, -389.0)))
            .unwrap();
        let moved = frame(&mut editor, &mut index, &viewport);
        assert_ne!(
            moved.painted.paths, before.painted.paths,
            "the move never reached the paint plan, so nothing below is tested"
        );

        assert!(editor.undo());
        let after = frame(&mut editor, &mut index, &viewport);

        assert_eq!(after, before, "undo left the derived state somewhere else");
        // The one thing that is deliberately *not* equal — see `PaintedPath`.
        assert!(
            after
                .painted
                .versions
                .iter()
                .zip(&before.painted.versions)
                .any(|(now, then)| now > then),
            "a §23 cache version returned to a value it had held before"
        );
    }

    /// The same property for the edit that changes the *shape* of the world
    /// rather than its geometry: a removal takes elements out of the index, and
    /// undoing has to put them back where they were, routes and all.
    #[test]
    fn a_repaint_after_undoing_a_removal_is_the_frame_the_removal_never_happened() {
        let (mut editor, mut index, viewport) = scene_editor();
        let before = frame(&mut editor, &mut index, &viewport);
        let document_before = editor.to_document();

        let node = visible_connected_node(&before, editor.world());
        let degree = editor.world().incident_edges(node).count();

        editor
            .apply(EditCommand::remove(vec![node], Vec::new()))
            .unwrap();
        let removed = frame(&mut editor, &mut index, &viewport);
        assert_ne!(
            removed.painted.paths, before.painted.paths,
            "the removal never reached the paint plan"
        );
        assert!(
            removed.indexed_nodes < before.indexed_nodes,
            "the removed node stayed in the spatial index"
        );
        assert_eq!(
            removed.indexed_edges,
            before.indexed_edges - degree,
            "the removed node's edges stayed in the spatial index"
        );

        assert!(editor.undo());
        let after = frame(&mut editor, &mut index, &viewport);

        assert_eq!(after, before);
        assert_eq!(editor.to_document(), document_before);
    }

    /// **Phase 9's whole delete path, at the frame level.**
    ///
    /// Select a connected node, press Delete, and three things must be true at
    /// once: the node and *every one of its edges* leave the spatial index, the
    /// whole removal is a single undo press, and the frame after that press is
    /// the frame before the deletion. The cascade is the part worth asserting
    /// here rather than in the applier — an edge with one end nowhere has no
    /// geometry, so "delete the node, keep the edges" is a state that paints
    /// wrongly rather than failing.
    #[test]
    fn deleting_the_selection_takes_the_incident_edges_and_undoes_in_one_press() {
        let (mut editor, mut index, viewport) = scene_editor();
        let before = frame(&mut editor, &mut index, &viewport);
        let document_before = editor.to_document();

        let node = visible_connected_node(&before, editor.world());
        let degree = editor.world().incident_edges(node).count();
        assert!(degree > 0, "the scene changed; this node has no edges");

        editor.select_only(Some(node));
        let depth = editor.history().undo_depth();
        assert!(editor.delete_selection(), "nothing was deleted");

        assert!(!editor.world().node_is_live(node));
        assert_eq!(
            editor.history().undo_depth(),
            depth + 1,
            "deleting a node and its edges must be one undo step"
        );

        let deleted = frame(&mut editor, &mut index, &viewport);
        assert_eq!(
            deleted.indexed_edges,
            before.indexed_edges - degree,
            "the deleted node's edges stayed in the spatial index"
        );

        assert!(editor.undo());
        let after = frame(&mut editor, &mut index, &viewport);
        assert_eq!(after, before);
        assert_eq!(editor.to_document(), document_before);
    }

    /// **§9's text does not outlive what it is attached to, and comes back
    /// with it.**
    ///
    /// Phase 9 found that `set_presence` cascades a node's edges and nothing
    /// above it did. Text has the same shape of hazard by construction —
    /// a node's text is a field of the node and an edge's label a field of the
    /// edge, so nothing can strand one — and the reason this is a test rather
    /// than a sentence is the *undo*: a restored element has to come back with
    /// its words, and that only holds because the tombstone keeps the whole
    /// row rather than clearing it.
    #[test]
    fn deleting_a_labelled_node_takes_its_edges_labels_with_it_and_undo_returns_them() {
        use crate::interaction::TextTarget;

        let (mut editor, mut index, viewport) = scene_editor();
        let before = frame(&mut editor, &mut index, &viewport);

        let node = visible_connected_node(&before, editor.world());
        let edge = editor
            .world()
            .incident_edges(node)
            .next()
            .expect("visible_connected_node has at least one edge");

        editor.commit_text(TextTarget::Node(node), "the host");
        editor.commit_text(TextTarget::Edge(edge), "the label");
        let document_before = editor.to_document();

        editor.select_only(Some(node));
        assert!(editor.delete_selection());

        assert!(!editor.world().node_is_live(node));
        assert!(
            !editor.world().edge_is_live(edge),
            "the labelled edge went with its node"
        );
        // The words are unreachable rather than lingering: `text_of` reads
        // liveness, so a removed element has no text even though its row
        // survives as a tombstone.
        assert_eq!(editor.text_of(TextTarget::Node(node)), None);
        assert_eq!(editor.text_of(TextTarget::Edge(edge)), None);

        assert!(editor.undo());
        assert_eq!(editor.text_of(TextTarget::Node(node)), Some("the host"));
        assert_eq!(editor.text_of(TextTarget::Edge(edge)), Some("the label"));
        assert_eq!(
            editor.to_document(),
            document_before,
            "one press of undo restores the elements and their words"
        );
    }

    /// A rubber band takes edges as well as nodes, so Delete has to remove
    /// both — and an edge deleted *on its own* must not take its nodes with it.
    #[test]
    fn deleting_a_selected_edge_leaves_the_nodes_it_joined() {
        let (mut editor, mut index, viewport) = scene_editor();
        let before = frame(&mut editor, &mut index, &viewport);

        let node = visible_connected_node(&before, editor.world());
        let edge = editor
            .world()
            .incident_edges(node)
            .next()
            .expect("visible_connected_node has at least one edge");
        let nodes_before = editor.world().nodes().live_indices().count();

        editor.clear_selection();
        editor.set_edge_selected(edge, true);
        assert!(editor.delete_selection());

        assert!(!editor.world().edge_is_live(edge));
        assert_eq!(
            editor.world().nodes().live_indices().count(),
            nodes_before,
            "deleting an edge removed a node with it"
        );

        assert!(editor.undo());
        assert!(editor.world().edge_is_live(edge));
    }

    /// Undo and redo have to be walkable in both directions any number of times
    /// without drifting — the failure this phase is guarding against does not
    /// appear on the first press.
    #[test]
    fn undo_and_redo_round_trip_repeatedly_without_drifting() {
        let (mut editor, mut index, viewport) = scene_editor();
        let clean = frame(&mut editor, &mut index, &viewport);
        let clean_document = editor.to_document();

        let node = visible_connected_node(&clean, editor.world());
        let doomed = *clean
            .visible_nodes
            .iter()
            .find(|other| **other != node)
            .expect("the scene shows more than one node");
        editor
            .apply(EditCommand::move_node(node, Vec2::new(90.0, 40.0)))
            .unwrap();
        editor
            .apply(EditCommand::resize_node(node, Vec2::new(220.0, 130.0)))
            .unwrap();
        editor
            .apply(EditCommand::remove(vec![doomed], Vec::new()))
            .unwrap();
        let edited = frame(&mut editor, &mut index, &viewport);
        let edited_document = editor.to_document();

        for _ in 0..3 {
            while editor.undo() {}
            assert_eq!(frame(&mut editor, &mut index, &viewport), clean);
            assert_eq!(editor.to_document(), clean_document);

            while editor.redo() {}
            assert_eq!(frame(&mut editor, &mut index, &viewport), edited);
            assert_eq!(editor.to_document(), edited_document);
        }
    }

    /// **§30's coalescing, end to end.** A drag arrives as one `MoveNodes` per
    /// mouse move; one undo has to put the node back at the start of the drag,
    /// not one mouse move back.
    #[test]
    fn a_drag_of_sixty_moves_undoes_in_one_press() {
        let (mut editor, _, _) = scene_editor();
        let node = NodeIndex::new(0);
        let start = editor.world().nodes().position(node);

        editor.begin_gesture();
        for _ in 0..60 {
            editor
                .apply(EditCommand::move_node(node, Vec2::new(2.0, 1.0)))
                .unwrap();
        }
        editor.end_gesture();

        assert_eq!(
            editor.history().undo_depth(),
            1,
            "the drag recorded one entry per mouse move"
        );
        assert_eq!(
            editor.world().nodes().position(node),
            start + Vec2::new(120.0, 60.0)
        );

        assert!(editor.undo());
        assert_eq!(editor.world().nodes().position(node), start);
        assert!(!editor.can_undo(), "the drag was more than one undo step");

        assert!(editor.redo());
        assert_eq!(
            editor.world().nodes().position(node),
            start + Vec2::new(120.0, 60.0)
        );
    }

    /// The dirty state is derived too, and the propagation rule is what the
    /// whole engine is shaped around: undoing a move must invalidate the moved
    /// node's own edges and **only** those.
    #[test]
    fn undoing_a_move_invalidates_the_same_edges_the_move_did() {
        let (mut editor, _, _) = scene_editor();
        let node = NodeIndex::new(3);
        let incident: Vec<EdgeIndex> = editor.world().incident_edges(node).collect();
        assert!(!incident.is_empty());

        editor.rebuild_dirty_geometry();
        editor.clear_spatial_updates();
        editor.dirty_mut().clear_all();

        editor
            .apply(EditCommand::move_node(node, Vec2::new(10.0, 10.0)))
            .unwrap();
        let after_edit: Vec<EdgeIndex> = editor.world().dirty().dirty_edges().to_vec();

        editor.rebuild_dirty_geometry();
        editor.clear_spatial_updates();
        editor.dirty_mut().clear_all();

        assert!(editor.undo());
        let after_undo: Vec<EdgeIndex> = editor.world().dirty().dirty_edges().to_vec();

        assert_eq!(after_edit, incident);
        assert_eq!(
            after_undo, incident,
            "undo invalidated a different set of edges than the edit did"
        );
    }

    /// **The invariant, walked.** Every door on the editor that is not `apply`
    /// must leave the document's elements exactly as it found them — otherwise
    /// "one mutation path" is a comment rather than a fact.
    #[test]
    fn nothing_but_apply_changes_an_element_of_the_document() {
        let (mut editor, mut index, viewport) = scene_editor();
        let before = editor.to_document();
        let node = NodeIndex::new(0);

        editor.select_only(Some(node));
        editor.set_node_selected(node, true);
        editor.set_edge_selected(EdgeIndex::new(0), true);
        let mut candidates = Vec::new();
        index.node_candidates(
            Rect::new(Vec2::new(-1e5, -1e5), Vec2::new(2e5, 2e5)),
            &mut candidates,
        );
        editor.apply_box_selection(
            BoxQuery::at_zoom(Rect::new(Vec2::new(-1e5, -1e5), Vec2::new(2e5, 2e5)), 1.0),
            candidates,
            Vec::new(),
        );
        editor.clear_selection();

        editor.begin_gesture();
        editor.end_gesture();
        editor.rebuild_dirty_geometry();
        editor.rebuild_all_geometry();
        index.sync(editor.world());
        editor.clear_spatial_updates();
        let mut visible = VisibleSet::new();
        index.query_visible(editor.world(), &viewport, &mut visible);
        editor.dirty_mut().clear_all();
        editor.undo();
        editor.redo();

        let after = editor.to_document();
        assert_eq!(after.nodes, before.nodes);
        assert_eq!(after.edges, before.edges);
    }

    /// A tombstone must not reach a file. Saving is the only compaction there
    /// is, so a saved document has to be one a fresh load produces the same
    /// world from.
    #[test]
    fn a_removed_element_never_reaches_the_document_and_a_save_compacts_it() {
        let mut editor = FlowEditor::new();
        editor.set_rules(ConnectionRules::PERMISSIVE);

        let added = editor
            .apply(EditCommand::AddNodes(vec![
                graph_node(0.0),
                graph_node(300.0),
                graph_node(600.0),
            ]))
            .unwrap()
            .added_nodes;
        editor
            .apply(EditCommand::Connect(vec![
                EdgeSpec::new(
                    ElementId::NONE,
                    EdgeEnd::node(added[0]),
                    EdgeEnd::node(added[1]),
                ),
                EdgeSpec::new(
                    ElementId::NONE,
                    EdgeEnd::node(added[1]),
                    EdgeEnd::node(added[2]),
                ),
            ]))
            .unwrap();

        editor
            .apply(EditCommand::remove(vec![added[1]], Vec::new()))
            .unwrap();

        let saved = editor.to_document();
        assert_eq!(saved.nodes.len(), 2, "a tombstoned node was written out");
        assert!(saved.edges.is_empty(), "an edge of a removed node survived");

        // The compaction: reloading renumbers, and the reloaded world holds no
        // holes at all.
        let (reloaded, report) = GraphWorld::from_document(&saved);
        assert!(
            report.is_clean(),
            "the saved document was not self-consistent"
        );
        assert_eq!(reloaded.nodes().len(), 2);
        assert_eq!(reloaded.nodes().live_indices().count(), 2);
    }

    /// A removed node keeps its slot, and that is exactly what lets an entry
    /// recorded *before* the removal still name the right element afterwards.
    /// This is the case a swap-remove would corrupt.
    #[test]
    fn an_undo_entry_recorded_before_a_removal_still_names_the_right_node() {
        let mut editor = FlowEditor::new();
        let added = editor
            .apply(EditCommand::AddNodes(vec![
                graph_node(0.0),
                graph_node(300.0),
            ]))
            .unwrap()
            .added_nodes;
        let (first, second) = (added[0], added[1]);

        // An entry naming `second`, recorded first.
        editor
            .apply(EditCommand::move_node(second, Vec2::new(50.0, 0.0)))
            .unwrap();
        // Then `first` goes away — under a swap-remove, `second` would slide
        // into slot 0 and the entry above would name a different node.
        editor
            .apply(EditCommand::remove(vec![first], Vec::new()))
            .unwrap();

        assert!(editor.undo(), "undo the removal");
        assert!(editor.undo(), "undo the move");

        assert_eq!(
            editor.world().nodes().position(second),
            Vec2::new(300.0, 0.0),
            "the earlier entry moved the wrong node back"
        );
        assert_eq!(editor.world().nodes().position(first), Vec2::new(0.0, 0.0));
    }

    /// Adding and undoing must be repeatable without the document growing: a
    /// redo has to restore the node that was there, never allocate a new one
    /// beside it.
    #[test]
    fn redoing_an_add_restores_the_same_node_rather_than_making_another() {
        let mut editor = FlowEditor::new();
        let node = editor
            .apply(EditCommand::AddNodes(vec![graph_node(0.0)]))
            .unwrap()
            .added_nodes[0];
        let id = editor.world().nodes().id(node);

        for _ in 0..3 {
            assert!(editor.undo());
            assert_eq!(editor.to_document().nodes.len(), 0);
            assert!(editor.redo());
            assert_eq!(editor.to_document().nodes.len(), 1);
            assert_eq!(editor.world().nodes().id(node), id);
        }

        assert_eq!(
            editor.world().nodes().len(),
            1,
            "each redo allocated a new slot"
        );
    }

    /// Undoing a disconnect must leave a live edge that the connection rules
    /// then treat as live — the duplicate check walks the adjacency index, and
    /// a tombstone left in it would refuse a legitimate reconnection.
    #[test]
    fn a_restored_edge_is_a_real_edge_to_the_connection_rules() {
        let mut editor = FlowEditor::new();
        editor.set_rules(ConnectionRules::default());
        let added = editor
            .apply(EditCommand::AddNodes(vec![
                graph_node(0.0),
                graph_node(300.0),
            ]))
            .unwrap()
            .added_nodes;
        let (a, b) = (added[0], added[1]);

        let edge = editor
            .apply(EditCommand::Connect(vec![EdgeSpec::new(
                ElementId::NONE,
                EdgeEnd::node(a),
                EdgeEnd::node(b),
            )]))
            .unwrap()
            .added_edges[0];

        assert!(
            !editor
                .world()
                .can_connect(EdgeEnd::node(a), EdgeEnd::node(b))
        );

        editor.apply(EditCommand::disconnect(vec![edge])).unwrap();
        assert!(
            editor
                .world()
                .can_connect(EdgeEnd::node(a), EdgeEnd::node(b)),
            "a tombstoned edge is still refusing its own reconnection"
        );

        assert!(editor.undo());
        assert!(
            !editor
                .world()
                .can_connect(EdgeEnd::node(a), EdgeEnd::node(b)),
            "the restored edge is invisible to the duplicate check"
        );
    }

    /// Style and label edits are deltas too, and they round-trip through the
    /// same stack. Grouped here rather than in `apply.rs` because this is the
    /// editor's view of them: one press each way.
    #[test]
    fn style_and_label_edits_undo_and_redo_one_press_at_a_time() {
        let mut editor = FlowEditor::new();
        let node = editor
            .apply(EditCommand::AddNodes(vec![graph_node(0.0)]))
            .unwrap()
            .added_nodes[0];
        let plain = editor.world().nodes().style(node).clone();

        let mut bold = ElementStyle::default();
        bold.stroke.width = 6.0;
        editor
            .apply(EditCommand::style_node(node, bold.clone()))
            .unwrap();
        editor
            .apply(EditCommand::label_node(node, Some("step one".into())))
            .unwrap();

        assert!(editor.undo());
        assert_eq!(editor.world().nodes().cold(node).label, None);
        assert_eq!(editor.world().nodes().style(node), &bold);

        assert!(editor.undo());
        assert_eq!(editor.world().nodes().style(node), &plain);

        assert!(editor.redo() && editor.redo());
        assert_eq!(editor.world().nodes().style(node), &bold);
        assert_eq!(
            editor.world().nodes().cold(node).label.as_deref(),
            Some("step one")
        );
    }

    /// Routing is document data and undoable, and changing it has to rebuild
    /// the route in both directions — the visible failure would be an edge that
    /// undoes its style while keeping its shape.
    #[test]
    fn undoing_a_routing_change_puts_the_route_back() {
        let (mut editor, _, _) = scene_editor();
        let edge = EdgeIndex::new(0);
        editor.rebuild_all_geometry();
        let before = editor.world().route(edge).unwrap().segments().to_vec();

        editor
            .apply(EditCommand::SetEdgeRouting(vec![(edge, EdgeRouting::Step)]))
            .unwrap();
        editor.rebuild_dirty_geometry();
        assert_ne!(editor.world().route(edge).unwrap().segments(), before);

        assert!(editor.undo());
        editor.rebuild_dirty_geometry();
        assert_eq!(editor.world().route(edge).unwrap().segments(), before);
    }

    /// An empty history answers both presses without touching anything, so a
    /// user hammering undo on a fresh document does not repaint sixty times.
    #[test]
    fn undo_on_an_empty_history_changes_nothing_and_says_so() {
        let mut editor = FlowEditor::new();
        assert!(!editor.undo());
        assert!(!editor.redo());
        assert_eq!(editor.to_document(), FlowDocument::new());
    }

    fn graph_node(x: f32) -> NodeDraft {
        NodeDraft::new(NodeSpec::new(
            ElementId::NONE,
            ElementKind::GraphNode(GraphNodeKind::Default),
            Vec2::new(x, 0.0),
            Vec2::new(160.0, 80.0),
        ))
    }

    // ---- Phase 11: the property panel's edits -------------------------

    /// An editor holding three shapes and one connection, which is enough for
    /// every panel operation below to have something to be wrong about.
    fn panel_editor() -> (FlowEditor, Vec<NodeIndex>) {
        let mut editor = FlowEditor::new();
        editor.set_rules(ConnectionRules::PERMISSIVE);
        let nodes: Vec<NodeIndex> = (0..3)
            .map(|i| {
                editor
                    .apply(EditCommand::AddNodes(vec![graph_node(i as f32 * 200.0)]))
                    .unwrap()
                    .added_nodes[0]
            })
            .collect();
        editor
            .apply(EditCommand::Connect(vec![EdgeSpec::new(
                ElementId::NONE,
                EdgeEnd::node(nodes[0]),
                EdgeEnd::node(nodes[1]),
            )]))
            .unwrap();
        (editor, nodes)
    }

    /// **Every property the panel writes is an edit, and every edit undoes.**
    ///
    /// One test rather than fifteen, because the interesting claim is the
    /// *shape* — the panel hands a closure to one method and the method does
    /// the rest — and fifteen copies of it would only assert that fifteen
    /// closures were written correctly. Each row here changes a different field
    /// through the same door.
    #[test]
    fn every_style_the_panel_writes_changes_the_document_and_undoes() {
        use crate::models::{FillStyle, FontFamily, FontSize, Sloppiness, TextAlign};

        /// One row of the table below: a name, the write the panel's control
        /// makes, and the question that says whether it took.
        type StyleRow = (
            &'static str,
            Box<dyn Fn(&mut ElementStyle)>,
            Box<dyn Fn(&ElementStyle) -> bool>,
        );

        let rows: Vec<StyleRow> = vec![
            (
                "stroke colour",
                Box::new(|style: &mut ElementStyle| {
                    style.stroke.color = Some(Color::rgb(1.0, 0.0, 0.0))
                }),
                Box::new(|style: &ElementStyle| {
                    style.stroke.color == Some(Color::rgb(1.0, 0.0, 0.0))
                }),
            ),
            (
                "background",
                Box::new(|style: &mut ElementStyle| style.fill = Some(Color::TRANSPARENT)),
                Box::new(|style: &ElementStyle| style.fill == Some(Color::TRANSPARENT)),
            ),
            (
                "fill style",
                Box::new(|style: &mut ElementStyle| style.fill_style = FillStyle::CrossHatch),
                Box::new(|style: &ElementStyle| style.fill_style == FillStyle::CrossHatch),
            ),
            (
                "stroke width",
                Box::new(|style: &mut ElementStyle| style.stroke.width = 4.0),
                Box::new(|style: &ElementStyle| style.stroke.width == 4.0),
            ),
            (
                "stroke dash",
                Box::new(|style: &mut ElementStyle| {
                    style.stroke.dash = crate::models::DashPattern::new(vec![2.0, 6.0])
                }),
                Box::new(|style: &ElementStyle| !style.stroke.dash.is_solid()),
            ),
            (
                "sloppiness",
                Box::new(|style: &mut ElementStyle| style.sloppiness = Sloppiness::Cartoonist),
                Box::new(|style: &ElementStyle| style.sloppiness == Sloppiness::Cartoonist),
            ),
            (
                "corner radius",
                Box::new(|style: &mut ElementStyle| style.corner_radius = 12.0),
                Box::new(|style: &ElementStyle| style.corner_radius == 12.0),
            ),
            (
                "opacity",
                Box::new(|style: &mut ElementStyle| style.opacity = 0.4),
                Box::new(|style: &ElementStyle| style.opacity == 0.4),
            ),
            (
                "font family",
                Box::new(|style: &mut ElementStyle| style.font.family = FontFamily::Code),
                Box::new(|style: &ElementStyle| style.font.family == FontFamily::Code),
            ),
            (
                "font size",
                Box::new(|style: &mut ElementStyle| style.font.size = FontSize::ExtraLarge),
                Box::new(|style: &ElementStyle| style.font.size == FontSize::ExtraLarge),
            ),
            (
                "text align",
                Box::new(|style: &mut ElementStyle| style.font.align = TextAlign::Right),
                Box::new(|style: &ElementStyle| style.font.align == TextAlign::Right),
            ),
            (
                "arrowheads",
                Box::new(|style: &mut ElementStyle| {
                    style.end_marker = crate::models::ArrowMarker::Dot
                }),
                Box::new(|style: &ElementStyle| {
                    style.end_marker == crate::models::ArrowMarker::Dot
                }),
            ),
        ];

        for (name, write, holds) in rows {
            let (mut editor, nodes) = panel_editor();
            editor.select_only(Some(nodes[0]));
            let before = editor.world().nodes().style(nodes[0]).clone();

            assert!(editor.restyle_selection(&write), "{name} changed nothing");
            assert!(
                holds(editor.world().nodes().style(nodes[0])),
                "{name} did not take"
            );

            assert!(editor.undo(), "{name} left no undo step");
            assert_eq!(
                editor.world().nodes().style(nodes[0]),
                &before,
                "{name} did not come back"
            );

            assert!(editor.redo());
            assert!(
                holds(editor.world().nodes().style(nodes[0])),
                "{name} did not redo"
            );
        }
    }

    /// A restyle that touches a node *and* an edge is two commands and must
    /// still be one press of undo — otherwise a mixed selection restyled once
    /// takes two presses to put back and nobody can tell why.
    #[test]
    fn restyling_a_mixed_selection_is_one_undo_step() {
        let (mut editor, nodes) = panel_editor();
        let edge = EdgeIndex::new(0);
        editor.select_only(Some(nodes[0]));
        editor.set_edge_selected(edge, true);
        let before = editor.history().undo_depth();

        assert!(editor.restyle_selection(|style| style.opacity = 0.25));
        assert_eq!(editor.world().nodes().style(nodes[0]).opacity, 0.25);
        assert_eq!(editor.world().edges().style(edge).opacity, 0.25);

        // Two commands — a node style and an edge style — and therefore two
        // entries. One press has to take both, which is exactly what gesture
        // grouping is for: merging keeps the stack small, grouping keeps the
        // *step* whole, and they are deliberately different mechanisms.
        assert_eq!(editor.history().undo_depth(), before + 2);
        assert!(editor.undo());
        assert_eq!(editor.world().nodes().style(nodes[0]).opacity, 1.0);
        assert_eq!(editor.world().edges().style(edge).opacity, 1.0);
        assert_eq!(
            editor.history().undo_depth(),
            before,
            "one press should have been enough"
        );
    }

    /// **A slider drag is one undo step and one history entry**, which are two
    /// different claims and the second is the one `EditCommand::supersedes`
    /// exists for. Sixty ticks that each pushed an entry would still undo in
    /// one press through gesture grouping — and would also evict sixty older
    /// steps from a bounded stack.
    #[test]
    fn dragging_the_opacity_slider_is_one_step_and_one_entry() {
        let (mut editor, nodes) = panel_editor();
        editor.select_only(Some(nodes[0]));
        let before = editor.history().undo_depth();

        editor.begin_gesture();
        for tick in 0..60 {
            let opacity = 1.0 - tick as f32 * 0.01;
            editor.restyle_selection(|style| style.opacity = opacity);
        }
        editor.end_gesture();

        assert!((editor.world().nodes().style(nodes[0]).opacity - 0.41).abs() < 1e-5);
        assert_eq!(
            editor.history().undo_depth(),
            before + 1,
            "sixty ticks must coalesce into one entry"
        );

        assert!(editor.undo());
        assert_eq!(editor.world().nodes().style(nodes[0]).opacity, 1.0);
        assert_eq!(editor.history().undo_depth(), before);
    }

    /// **Depth survives a save, a load and an undo**, which are three different
    /// paths through three different modules and the phase brief names all
    /// three.
    #[test]
    fn depth_survives_serialization_and_undo() {
        let (mut editor, nodes) = panel_editor();
        editor.select_only(Some(nodes[0]));
        assert!(!editor.world().is_layered());

        assert!(editor.reorder_selection(crate::commands::LayerAction::BringToFront));
        let raised = editor.world().nodes().z(nodes[0]);
        assert!(raised > 0);
        assert!(editor.world().is_layered());

        // Serialization.
        let json = serde_json::to_string(&editor.to_document()).expect("a document serializes");
        let back: FlowDocument = serde_json::from_str(&json).expect("and comes back");
        let (reloaded, report) = FlowEditor::from_document(&back);
        assert!(report.dangling_edges.is_empty());
        assert_eq!(reloaded.world().nodes().z(nodes[0]), raised);
        assert!(
            reloaded.world().is_layered(),
            "a reloaded document has to know it is layered, or its frame is drawn in the wrong order"
        );

        // Undo.
        assert!(editor.undo());
        assert_eq!(editor.world().nodes().z(nodes[0]), 0);
        assert!(
            !editor.world().is_layered(),
            "undoing the only reorder puts the frame back on its fast path"
        );
        assert!(editor.redo());
        assert_eq!(editor.world().nodes().z(nodes[0]), raised);
    }

    /// The four buttons through the editor rather than through the arithmetic:
    /// one gesture, one press of undo, and a press with nowhere to go that
    /// consumes nothing.
    #[test]
    fn the_layer_buttons_reorder_the_selection_and_refuse_when_there_is_nowhere_to_go() {
        use crate::commands::LayerAction;

        let (mut editor, nodes) = panel_editor();
        editor.select_only(Some(nodes[1]));

        assert!(editor.reorder_selection(LayerAction::BringToFront));
        assert!(
            !editor.reorder_selection(LayerAction::BringToFront),
            "already at the front"
        );
        assert!(editor.reorder_selection(LayerAction::SendToBack));
        assert!(
            !editor.reorder_selection(LayerAction::SendToBack),
            "already at the back"
        );

        // Two real presses and two refusals, so exactly two undo steps stand
        // above the edits that built the document.
        let depth = editor.history().undo_depth();
        assert!(editor.undo());
        assert!(editor.undo());
        assert_eq!(editor.history().undo_depth(), depth - 2);
        assert_eq!(editor.world().nodes().z(nodes[1]), 0);
    }

    /// Duplicating copies the nodes, the edges *between* them, and the handles
    /// — and selects the copies, so a second press walks across the canvas.
    #[test]
    fn duplicating_copies_the_selection_its_internal_edges_and_undoes_in_one_press() {
        let (mut editor, nodes) = panel_editor();
        editor.select_only(Some(nodes[0]));
        editor.set_node_selected(nodes[1], true);

        let before = editor.to_document();
        assert!(editor.duplicate_selection());

        let after = editor.to_document();
        assert_eq!(after.nodes.len(), before.nodes.len() + 2);
        assert_eq!(
            after.edges.len(),
            before.edges.len() + 1,
            "the edge joining the two copied nodes is copied with them"
        );
        assert_eq!(
            editor.world().selection().nodes().len(),
            2,
            "the copies become the selection"
        );

        assert!(editor.undo());
        assert_eq!(editor.to_document().nodes, before.nodes);
        assert_eq!(editor.to_document().edges, before.edges);
    }

    /// An edge with only one end in the selection is **not** copied: the copy
    /// would attach to the original node and leave two edges converging on it.
    #[test]
    fn duplicating_one_end_of_a_connection_copies_no_edge() {
        let (mut editor, nodes) = panel_editor();
        editor.select_only(Some(nodes[0]));

        let before = editor.to_document().edges.len();
        assert!(editor.duplicate_selection());
        assert_eq!(editor.to_document().edges.len(), before);
    }

    /// A link is document data: it is set through a command, it survives a
    /// round trip, and it undoes.
    #[test]
    fn a_link_is_stored_undone_and_round_tripped() {
        let (mut editor, nodes) = panel_editor();
        editor.select_only(Some(nodes[0]));
        assert_eq!(editor.selection_link(), None);

        assert!(editor.set_selection_link("  https://example.invalid/spec  "));
        assert_eq!(
            editor.selection_link(),
            Some("https://example.invalid/spec")
        );

        let json = serde_json::to_string(&editor.to_document()).unwrap();
        let back: FlowDocument = serde_json::from_str(&json).unwrap();
        let (reloaded, _) = FlowEditor::from_document(&back);
        reloaded.world().nodes().live_indices().for_each(|node| {
            if node == nodes[0] {
                assert_eq!(
                    reloaded.world().nodes().cold(node).link.as_deref(),
                    Some("https://example.invalid/spec")
                );
            }
        });

        // Empty clears, exactly as an empty text commit does.
        assert!(editor.set_selection_link("   "));
        assert_eq!(editor.selection_link(), None);
        assert!(editor.undo());
        assert_eq!(
            editor.selection_link(),
            Some("https://example.invalid/spec")
        );
    }

    /// **Select a node, an edge and a text element in turn, and watch the panel
    /// change.**
    ///
    /// The launcher check the phase brief asks for, as far as it can be asked
    /// without a window: this is the whole chain from §28's selection to the
    /// rows the panel draws, and the only thing between it and the screen is
    /// the drawing. `properties`'s own test asserts the *table*; this asserts
    /// that a real selection of a real element reaches the right column of it.
    #[test]
    fn selecting_each_kind_in_turn_changes_the_rows_the_panel_draws() {
        use crate::properties::{PanelSection, sections_for, selection_items};

        let (mut editor, nodes) = panel_editor();
        let text = editor
            .apply(EditCommand::AddNodes(vec![NodeDraft::new({
                let mut spec = NodeSpec::new(
                    ElementId::NONE,
                    ElementKind::Text,
                    Vec2::new(0.0, 400.0),
                    Vec2::new(180.0, 22.0),
                );
                spec.label = Some("a sentence".into());
                spec
            })]))
            .unwrap()
            .added_nodes[0];

        let rows = |editor: &FlowEditor| sections_for(&selection_items(editor.world()));

        // Nothing selected: no panel at all, rather than a card of labels over
        // nothing.
        editor.clear_selection();
        assert!(rows(&editor).is_empty());

        editor.select_only(Some(nodes[0]));
        let node_rows = rows(&editor);
        assert!(node_rows.contains(&PanelSection::Background));
        assert!(node_rows.contains(&PanelSection::Fill));
        assert!(node_rows.contains(&PanelSection::Corners));
        assert!(!node_rows.contains(&PanelSection::ArrowType));

        editor.clear_selection();
        editor.set_edge_selected(EdgeIndex::new(0), true);
        let edge_rows = rows(&editor);
        assert!(edge_rows.contains(&PanelSection::ArrowType));
        assert!(edge_rows.contains(&PanelSection::Arrowheads));
        assert!(!edge_rows.contains(&PanelSection::Background));

        editor.clear_selection();
        editor.select_only(Some(text));
        let text_rows = rows(&editor);
        assert!(text_rows.contains(&PanelSection::FontFamilyRow));
        assert!(text_rows.contains(&PanelSection::FontSizeRow));
        assert!(text_rows.contains(&PanelSection::TextAlignRow));
        assert!(!text_rows.contains(&PanelSection::StrokeWidth));

        // The three that are on every one of them, whatever is selected.
        for rows in [&node_rows, &edge_rows, &text_rows] {
            for section in [
                PanelSection::Opacity,
                PanelSection::Layers,
                PanelSection::Actions,
            ] {
                assert!(rows.contains(&section), "{section:?} is on every panel");
            }
        }
    }

    /// **The four text rows appear when the selection has a label**, driven
    /// through a real caret commit rather than by writing the field.
    ///
    /// The chain the phase adds, end to end: type into a shape, and the panel
    /// grows Font family, Font size, Text align and vertical align — clear the
    /// words and it loses them again. `properties`'s own test states the table;
    /// this asserts that a real element's real content reaches the right column
    /// of it.
    #[test]
    fn the_text_rows_appear_when_the_selection_has_a_label() {
        use crate::{
            interaction::TextTarget,
            properties::{PanelSection, sections_for, selection_items},
        };

        const TEXT_ROWS: [PanelSection; 4] = [
            PanelSection::FontFamilyRow,
            PanelSection::FontSizeRow,
            PanelSection::TextAlignRow,
            PanelSection::VerticalAlignRow,
        ];

        let (mut editor, nodes) = panel_editor();
        let rows = |editor: &FlowEditor| sections_for(&selection_items(editor.world()));

        editor.select_only(Some(nodes[0]));
        for row in TEXT_ROWS {
            assert!(
                !rows(&editor).contains(&row),
                "an unlabelled shape offers {}",
                row.name()
            );
        }

        assert!(editor.commit_text(TextTarget::Node(nodes[0]), "step one"));
        for row in TEXT_ROWS {
            assert!(
                rows(&editor).contains(&row),
                "a labelled shape does not offer {}",
                row.name()
            );
        }

        // A second, unlabelled shape beside it: the panel's intersection rule
        // takes the rows away rather than offering a control that would apply
        // to half of what is selected.
        editor.set_node_selected(nodes[1], true);
        for row in TEXT_ROWS {
            assert!(
                !rows(&editor).contains(&row),
                "a half-labelled selection offers {}",
                row.name()
            );
        }

        // And an edge earns them the same way.
        editor.clear_selection();
        editor.set_edge_selected(EdgeIndex::new(0), true);
        assert!(!rows(&editor).contains(&PanelSection::FontSizeRow));
        assert!(editor.commit_text(TextTarget::Edge(EdgeIndex::new(0)), "carries"));
        for row in TEXT_ROWS {
            assert!(
                rows(&editor).contains(&row),
                "a labelled edge does not offer {}",
                row.name()
            );
        }

        // Clearing the words takes the rows with them: an empty label is no
        // label, and a Font size row over nothing is a control that does
        // nothing.
        assert!(editor.commit_text(TextTarget::Edge(EdgeIndex::new(0)), "   "));
        for row in TEXT_ROWS {
            assert!(
                !rows(&editor).contains(&row),
                "an emptied edge label left {} behind",
                row.name()
            );
        }
    }

    /// **The rows survive save, reopen, undo and redo** — because what they
    /// edit is ordinary style, written through the ordinary applier and
    /// serialized with everything else.
    ///
    /// Asserted as the round trip rather than as four separate claims: a field
    /// added to the model and forgotten in the format is silent, and the
    /// symptom is a diagram that opens with every label back in the middle.
    #[test]
    fn a_moved_label_survives_a_round_trip_and_an_undo() {
        use crate::{
            interaction::TextTarget,
            models::{FlowDocument, TextAlign, VerticalAlign},
        };

        let (mut editor, nodes) = panel_editor();
        editor.select_only(Some(nodes[0]));
        editor.commit_text(TextTarget::Node(nodes[0]), "step one");

        assert!(editor.restyle_selection(|style| {
            style.font.align = TextAlign::Right;
            style.font.vertical_align = VerticalAlign::Bottom;
            style.font.size = crate::models::FontSize::ExtraLarge;
        }));

        let font = |editor: &FlowEditor| editor.world().nodes().style(nodes[0]).font.clone();
        let moved = font(&editor);
        assert_eq!(moved.vertical_align, VerticalAlign::Bottom);

        // Undo puts it back where the label was born, and redo returns it.
        assert!(editor.undo());
        assert_eq!(font(&editor).vertical_align, VerticalAlign::Middle);
        assert_eq!(font(&editor).align, TextAlign::Center);
        assert!(editor.redo());
        assert_eq!(font(&editor), moved);

        // And a save/reopen keeps it: the same node, by id, out of the file.
        let document = editor.world().to_document();
        let json = document.to_json().expect("serializes");
        let reopened = FlowDocument::from_json(&json).expect("reopens");
        let id = editor.world().nodes().id(nodes[0]);
        let node = reopened
            .nodes
            .iter()
            .find(|it| it.id == id)
            .expect("the shape is still there");
        assert_eq!(node.style.font.align, TextAlign::Right);
        assert_eq!(node.style.font.vertical_align, VerticalAlign::Bottom);
        assert_eq!(node.style.font.size, crate::models::FontSize::ExtraLarge);
    }

    /// A deleted element must not keep offering its properties. It stays in the
    /// selection set until something clears it — `delete_selection` deliberately
    /// does not — so this is the guard that keeps the panel from editing a
    /// tombstone.
    #[test]
    fn a_deleted_element_draws_no_panel() {
        use crate::properties::{sections_for, selection_items};

        let (mut editor, nodes) = panel_editor();
        editor.select_only(Some(nodes[0]));
        assert!(!sections_for(&selection_items(editor.world())).is_empty());

        assert!(editor.delete_selection());
        assert!(sections_for(&selection_items(editor.world())).is_empty());
    }

    /// **What "followed" means for a link**, up to the one call that needs a
    /// platform.
    ///
    /// `App::open_url` is the last line of the view's handler and cannot be
    /// driven headlessly; everything in front of it can, and this is that:
    /// which element a press resolves to a link, which ones resolve to none,
    /// and the tombstone rule.
    #[test]
    fn a_press_resolves_to_the_link_under_it_and_to_nothing_when_there_is_none() {
        use crate::runtime::PointerTarget;

        let (mut editor, nodes) = panel_editor();
        editor.select_only(Some(nodes[0]));
        editor.set_selection_link("https://example.invalid/a");

        assert_eq!(
            editor.link_at(PointerTarget::Node(nodes[0])),
            Some("https://example.invalid/a")
        );
        assert_eq!(editor.link_at(PointerTarget::Node(nodes[1])), None);
        assert_eq!(editor.link_at(PointerTarget::Empty), None);

        // An edge carries one too, and it is reachable now that a press can
        // land on one.
        let edge = EdgeIndex::new(0);
        editor.clear_selection();
        editor.set_edge_selected(edge, true);
        editor.set_selection_link("https://example.invalid/b");
        assert_eq!(
            editor.link_at(PointerTarget::Edge(edge)),
            Some("https://example.invalid/b")
        );

        // **A deleted element hands back nothing**, even though its row — and
        // its link — are still there. Removal is a tombstone, and following a
        // link on something nobody can see is worse than finding none.
        editor.clear_selection();
        editor.set_node_selected(nodes[0], true);
        assert!(editor.delete_selection());
        assert_eq!(editor.link_at(PointerTarget::Node(nodes[0])), None);
        assert!(editor.undo());
        assert_eq!(
            editor.link_at(PointerTarget::Node(nodes[0])),
            Some("https://example.invalid/a")
        );
    }

    /// Not every element is a graph node; a plain shape has to be editable too,
    /// and its `NodeShape` projection has to survive the round trip.
    #[test]
    fn a_shape_node_round_trips_with_its_projection_intact() {
        let mut editor = FlowEditor::new();
        let node = editor
            .apply(EditCommand::AddNodes(vec![NodeDraft::new(NodeSpec::new(
                ElementId::NONE,
                ElementKind::Shape(ShapeKind::Diamond),
                Vec2::new(0.0, 0.0),
                Vec2::new(40.0, 40.0),
            ))]))
            .unwrap()
            .added_nodes[0];

        assert_eq!(
            editor.world().nodes().shape(node),
            crate::runtime::NodeShape::Diamond
        );
        editor
            .apply(EditCommand::remove(vec![node], Vec::new()))
            .unwrap();
        assert!(editor.undo());
        assert_eq!(
            editor.world().nodes().shape(node),
            crate::runtime::NodeShape::Diamond
        );
    }
}
