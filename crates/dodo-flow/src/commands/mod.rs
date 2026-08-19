//! §30's commands and undo — **every edit as a delta through one path**.
//!
//! ```text
//! edit.rs      the delta vocabulary, and the coalescing rule
//! apply.rs     the one mutation path, returning each edit's inverse
//! history.rs   the undo/redo stacks, and gesture grouping
//! editor.rs    the world and the history welded together
//! keys.rs      §26's binding table, as a function of the host
//! ```
//!
//! Start at [`editor`]: it states the invariant this phase exists for and how
//! the type system holds it. [`apply`] says why the inverse is a return value
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
pub mod history;
pub mod keys;

pub use apply::{EditOutcome, apply};
pub use edit::{EditCommand, EditError, NodeDraft};
pub use editor::{EditSummary, FlowEditor};
pub use history::{CommandHistory, GestureId, HistoryEntry};
pub use keys::{Binding, EditAction};

#[cfg(test)]
mod tests {
    use super::{EditCommand, FlowEditor, NodeDraft};
    use crate::{
        geometry::{Rect, RouteSegment, Vec2, Viewport},
        models::{
            EdgeIndex, EdgeRouting, ElementId, ElementKind, ElementStyle, FlowDocument,
            GraphNodeKind, NodeIndex, ShapeKind,
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
    /// So this is the frame's whole input: what the viewport query returns, where
    /// every visible element is, and what the routes are. Two of these being equal
    /// is two frames being identical.
    #[derive(Debug, PartialEq)]
    struct Frame {
        visible_nodes: Vec<NodeIndex>,
        visible_edges: Vec<EdgeIndex>,
        node_bounds: Vec<(NodeIndex, Rect)>,
        edge_bounds: Vec<(EdgeIndex, Rect)>,
        routes: Vec<(EdgeIndex, Vec2, Vec<RouteSegment>)>,
        indexed_nodes: usize,
        indexed_edges: usize,
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

        let world = editor.world();
        Frame {
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

        // A node with edges, moved far enough to cross cells.
        let node = NodeIndex::new(3);
        editor
            .apply(EditCommand::move_node(node, Vec2::new(517.0, -389.0)))
            .unwrap();
        let moved = frame(&mut editor, &mut index, &viewport);
        assert_ne!(moved, before, "the move changed nothing to undo");

        assert!(editor.undo());
        let after = frame(&mut editor, &mut index, &viewport);

        assert_eq!(after, before, "undo left the derived state somewhere else");
    }

    /// The same property for the edit that changes the *shape* of the world
    /// rather than its geometry: a removal takes elements out of the index, and
    /// undoing has to put them back where they were, routes and all.
    #[test]
    fn a_repaint_after_undoing_a_removal_is_the_frame_the_removal_never_happened() {
        let (mut editor, mut index, viewport) = scene_editor();
        let before = frame(&mut editor, &mut index, &viewport);
        let document_before = editor.to_document();

        let node = NodeIndex::new(2);
        let degree = editor.world().incident_edges(node).count();
        assert!(degree > 0, "the scene changed; pick a connected node");

        editor
            .apply(EditCommand::remove(vec![node], Vec::new()))
            .unwrap();
        let removed = frame(&mut editor, &mut index, &viewport);
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

    /// Undo and redo have to be walkable in both directions any number of times
    /// without drifting — the failure this phase is guarding against does not
    /// appear on the first press.
    #[test]
    fn undo_and_redo_round_trip_repeatedly_without_drifting() {
        let (mut editor, mut index, viewport) = scene_editor();
        let clean = frame(&mut editor, &mut index, &viewport);
        let clean_document = editor.to_document();

        let node = NodeIndex::new(4);
        editor
            .apply(EditCommand::move_node(node, Vec2::new(90.0, 40.0)))
            .unwrap();
        editor
            .apply(EditCommand::resize_node(node, Vec2::new(220.0, 130.0)))
            .unwrap();
        editor
            .apply(EditCommand::remove(vec![NodeIndex::new(1)], Vec::new()))
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
