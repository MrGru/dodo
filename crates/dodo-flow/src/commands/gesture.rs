//! [`apply_gesture`] — the mapping from §25's interaction effects to §30's
//! commands, **on this side of the UI-framework line**.
//!
//! # Why it is not in the view
//!
//! It was, until this phase: `views::flow`'s `apply` matched on
//! [`InteractionEffect`] and called `GraphWorld::move_node` directly. That match
//! is the only place a drag becomes an edit, so it is the only place the
//! coalescing can be got wrong — and while it lived in a file that needs a
//! `Window` to build a view, "a whole drag is one undo step" could only be
//! checked by dragging something.
//!
//! Here it is a pure function of an editor and an effect, and
//! `a_press_sixty_moves_and_a_release_is_one_undo_step` at the bottom drives
//! the real [`InteractionMachine`](crate::interaction::InteractionMachine) through
//! a real drag with no window anywhere.
//! That test is the phase's coalescing requirement, asserted rather than
//! demonstrated.
//!
//! # What is deliberately still the view's
//!
//! Two effects are not the document's:
//!
//! - `PanBy` moves the camera, and the camera is the view's.
//! - `CommitBoxSelect` hands back a world rectangle and needs
//!   [`SpatialIndex`](crate::spatial::SpatialIndex)'s broad phase to say what is
//!   in it. The narrow phase is [`FlowEditor::apply_box_selection`]; the caller
//!   supplies the candidates, exactly as §28 draws it.
//!
//! Both are left alone here rather than half-handled, and the caller matches
//! them itself.
//!
//! **This file names no UI framework.**

use crate::{
    commands::{EditCommand, EditError, NodeDraft, editor::FlowEditor},
    geometry::Rect,
    interaction::{CanvasTool, ConnectorCreation, InteractionEffect},
    models::{EdgeIndex, ElementId, HandleDirection, HandlePlacement, NodeIndex},
    runtime::{EdgeEnd, EdgeSpec, HandleSpec, NodeSpec, PointerTarget},
};

/// What one effect did to the document.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GestureReport {
    /// Whether the world changed, so the caller can decide about repainting
    /// without diffing anything.
    pub changed: bool,
    /// The element a creation gesture added, when the effect was one.
    ///
    /// Carried so the caller can select what the user just drew without asking
    /// the world what changed — and so a test can assert that a creation
    /// produced exactly one node.
    pub created: Option<NodeIndex>,
    /// The answer a dropped connection got, when the effect was one.
    ///
    /// Carried rather than logged: dodo installs no logger, and a refusal is an
    /// ordinary answer that the caller may want to show, trace or ignore.
    pub connection: Option<Result<EdgeIndex, EditError>>,
}

impl GestureReport {
    fn changed(changed: bool) -> GestureReport {
        GestureReport {
            changed,
            created: None,
            connection: None,
        }
    }
}

/// The handles a **created graph node** is born with (§4).
///
/// A node with no handles cannot be connected to anything, so a palette that
/// placed one would produce something that looks like a graph node and takes
/// part in no graph. One source on the right and one target on the left is
/// React Flow's own default and what the launcher's demo document uses.
///
/// A drawn shape gets none: §4's connection rules already refuse an edge to a
/// non-graph node, and handles on a rectangle would be dots the user cannot
/// use.
fn handles_for(tool: CanvasTool) -> Vec<HandleSpec> {
    if tool == CanvasTool::GraphNode {
        vec![
            HandleSpec::new("out", HandlePlacement::Right, HandleDirection::Source),
            HandleSpec::new("in", HandlePlacement::Left, HandleDirection::Target),
        ]
    } else {
        Vec::new()
    }
}

/// **A finished creation gesture, as §30's `AddNodes`.**
///
/// The one place a tool becomes an edit, and it is deliberately three lines of
/// mapping over [`CanvasTool::element_kind`] rather than a `match` of its own:
/// a tool that creates nothing returns `None` there, so this cannot be reached
/// with `Select` or `Hand` even if a caller tries.
///
/// The id is [`ElementId::NONE`] — the applier allocates one from the world's
/// allocator, so nothing above the command layer has to know the id space.
fn create(
    editor: &mut FlowEditor,
    tool: CanvasTool,
    rect: Rect,
    connector: Option<ConnectorCreation>,
) -> GestureReport {
    let Some(kind) = tool.element_kind() else {
        return GestureReport::default();
    };

    let rect = rect.normalized();
    let mut spec = NodeSpec::new(ElementId::NONE, kind, rect.origin, rect.size);
    if let Some(connector) = connector {
        spec.connector = Some(editor.connector_between(
            connector.start,
            connector.end,
            connector.start_target,
            connector.end_target,
        ));
    }
    let draft = NodeDraft::new(spec).with_handles(handles_for(tool));

    let Ok(summary) = editor.apply(EditCommand::AddNodes(vec![draft])) else {
        return GestureReport::default();
    };
    let created = summary.added_nodes.first().copied();

    // **Selection is not an edit** — `commands::editor`'s module doc says why —
    // so selecting what was just drawn adds nothing to the history and the
    // creation stays one undo step.
    if let Some(node) = created {
        editor.select_only(Some(node));
    }

    GestureReport {
        changed: summary.changed,
        created,
        connection: None,
    }
}

/// **Applies the document half of one interaction effect.**
///
/// See the module doc for the two effects this deliberately ignores.
pub fn apply_gesture(editor: &mut FlowEditor, effect: InteractionEffect) -> GestureReport {
    match effect {
        // **The propagation rule, entered from a gesture — as a command.** What
        // the move invalidates is still `GraphWorld::move_node`'s decision one
        // layer down; what changed is that nothing above can call it. The whole
        // drag is one undo step because the press opened a gesture and every
        // move inside it coalesces (§30).
        InteractionEffect::DragNodeBy { node, delta } => {
            let changed = if editor.world().nodes().connector(node).is_some() {
                editor.translate_connector(node, delta)
            } else {
                editor.move_node_or_group(node, delta)
            };
            GestureReport::changed(changed)
        }

        // **A press selects, and shift extends** (Phase 10.5). Before it, this
        // arm was an unconditional `select_only` — so shift-clicking a second
        // node replaced the selection with it, and the multi-select the box
        // band already produced could not be built up a node at a time.
        InteractionEffect::BeginNodeDrag { node, additive } => {
            editor.begin_gesture();
            if additive {
                editor.set_node_selected(node, true);
            } else {
                editor.select_only(Some(node));
            }
            GestureReport::changed(true)
        }

        // **The gesture Phase 10 left out, in one arm** — `runtime::hit`'s doc
        // has why it waited and why waiting was wrong. Additive *adds*, never
        // toggles: the same meaning `BoxQuery::additive` gives the word, so
        // shift means one thing on this canvas rather than two.
        InteractionEffect::SelectEdge { edge, additive } => {
            if !additive {
                editor.clear_selection();
            }
            editor.set_edge_selected(edge, true);
            GestureReport::changed(true)
        }

        InteractionEffect::EndNodeDrag { .. } => {
            editor.end_gesture();
            GestureReport::changed(false)
        }

        // ---- §12's resize, the same four arms a drag has ----
        InteractionEffect::BeginResize { node, start } => {
            if !editor.world().node_is_live(node) {
                return GestureReport::default();
            }
            editor.begin_gesture();
            editor.begin_resize_node_or_group(node, start);
            // A loose multi-selection owns one outer grip. Keep that selection;
            // only an otherwise-unselected subject replaces it.
            if !editor.world().selection().contains_node(node) {
                editor.select_only(Some(node));
            }
            GestureReport::changed(true)
        }

        // One absolute transform command per frame: each supersedes the last
        // while its inverse retains the rectangles captured at the press.
        InteractionEffect::ResizeNodeTo { node, rect } => {
            GestureReport::changed(editor.resize_node_or_group(node, rect))
        }

        InteractionEffect::EndResize { .. } => {
            editor.finish_transform_gesture();
            editor.end_gesture();
            GestureReport::changed(false)
        }

        InteractionEffect::BeginRotate { node, centre } => {
            if !editor.world().node_is_live(node) {
                return GestureReport::default();
            }
            editor.begin_gesture();
            editor.begin_rotate_node_or_group(node, centre);
            if !editor.world().selection().contains_node(node) {
                editor.select_only(Some(node));
            }
            GestureReport::changed(true)
        }
        InteractionEffect::RotateBy {
            node,
            centre,
            total,
            ..
        } => GestureReport::changed(editor.rotate_node_or_group(node, centre, total)),
        InteractionEffect::EndRotate { .. } => {
            editor.finish_transform_gesture();
            editor.end_gesture();
            GestureReport::changed(false)
        }
        InteractionEffect::CancelRotate => {
            let changed = editor.abandon_gesture();
            editor.finish_transform_gesture();
            GestureReport::changed(changed)
        }

        InteractionEffect::BeginConnectorEndpointDrag { node } => {
            editor.begin_gesture();
            editor.select_only(Some(node));
            GestureReport::changed(true)
        }
        InteractionEffect::MoveConnectorEndpoint {
            node,
            end,
            point,
            target,
        } => GestureReport::changed(editor.set_connector_endpoint(node, end, point, target)),
        InteractionEffect::EndConnectorEndpointDrag { .. } => {
            editor.end_gesture();
            GestureReport::changed(false)
        }
        InteractionEffect::CancelConnectorEndpointDrag => {
            GestureReport::changed(editor.abandon_gesture())
        }

        // Abandoned exactly as a drag is, and for the same reason: the entries
        // the gesture recorded carry where the element was, and putting it back
        // by applying them in reverse leaves nothing on the stack.
        InteractionEffect::CancelResize { .. } => {
            let changed = editor.abandon_gesture();
            editor.finish_transform_gesture();
            GestureReport::changed(changed)
        }

        // **An abandoned drag is not an undo step**, so its entries are
        // discarded rather than reversed by another edit — a "move back" left
        // on the stack is a step the user never took, and the next undo would
        // walk through it.
        //
        // The effect's own `revert` delta is ignored on purpose: the discarded
        // entries carry the exact starting positions, and a summed delta does
        // not put a node back exactly (see [`EditCommand::SetNodePositions`]).
        InteractionEffect::CancelNodeDrag { .. } => {
            GestureReport::changed(editor.abandon_gesture())
        }

        InteractionEffect::BeginBoxSelect(_) => {
            editor.select_only(None);
            GestureReport::changed(true)
        }

        InteractionEffect::BeginConnect(source) => {
            editor.select_only(Some(source.node));
            GestureReport::changed(true)
        }

        // **The validation is the world's** (§4) — this only says where the
        // drop landed. §4's whole-node mode is the `Node` arm: dropping on a
        // body connects to the node and the router picks a point on its border.
        InteractionEffect::CommitConnect { source, target } => {
            let end = match target {
                PointerTarget::Handle { node, handle } => EdgeEnd::handle(node, handle),
                PointerTarget::Node(node) => EdgeEnd::node(node),
                // An edge is not a connection target: §8 connects nodes, and
                // dropping one edge on another has no meaning to give it. Same
                // answer as empty canvas — the connection is abandoned.
                // Neither is an edge nor a grip: §8 connects nodes, and
                // dropping a connection on a corner of the selection ring has
                // no meaning to give it. Same answer as empty canvas — the
                // connection is abandoned.
                PointerTarget::Empty
                | PointerTarget::Edge(_)
                | PointerTarget::ResizeGrip { .. }
                | PointerTarget::RotationGrip { .. }
                | PointerTarget::ConnectorEndpoint { .. } => {
                    return GestureReport::default();
                }
            };

            let spec = EdgeSpec::new(
                ElementId::NONE,
                EdgeEnd::handle(source.node, source.handle),
                end,
            );
            let result = editor
                .apply(EditCommand::Connect(vec![spec]))
                .map(|summary| summary.added_edges[0]);

            GestureReport {
                changed: result.is_ok(),
                created: None,
                connection: Some(result),
            }
        }

        // **§45's whole point, in one arm.** The tool changed no document state
        // while it was active; the element appears here, once, through the same
        // applier every other edit uses — so it undoes and redoes with no code
        // in `commands/` knowing that a palette exists.
        InteractionEffect::CommitCreate {
            tool,
            rect,
            connector,
        } => create(editor, tool, rect, connector),

        // An abandoned creation has nothing to undo: the tool never wrote to
        // the document, so there is no draft element and no history entry.
        InteractionEffect::CancelCreate => GestureReport::default(),

        _ => GestureReport::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::apply_gesture;
    use crate::{
        commands::{EditCommand, FlowEditor, NodeDraft},
        geometry::Vec2,
        interaction::{
            CanvasTool, InputModifiers, InteractionEvent, InteractionMachine, PointerButton,
        },
        models::{
            Connector, ElementId, ElementKind, GraphNodeKind, LinearKind, NodeIndex, ShapeKind,
        },
        runtime::{ConnectionRules, NodeSpec, PointerTarget},
    };

    fn editor_with_two_nodes() -> (FlowEditor, NodeIndex, NodeIndex) {
        let mut editor = FlowEditor::new();
        editor.set_rules(ConnectionRules::PERMISSIVE);
        let added = editor
            .apply(EditCommand::AddNodes(vec![draft(0.0), draft(400.0)]))
            .unwrap()
            .added_nodes;
        (editor, added[0], added[1])
    }

    fn draft(x: f32) -> NodeDraft {
        NodeDraft::new(NodeSpec::new(
            ElementId::NONE,
            ElementKind::GraphNode(GraphNodeKind::Default),
            Vec2::new(x, 0.0),
            Vec2::new(160.0, 80.0),
        ))
    }

    fn press(target: PointerTarget, at: Vec2) -> InteractionEvent {
        InteractionEvent::PointerDown {
            screen: at,
            world: at,
            button: PointerButton::Left,
            modifiers: InputModifiers::default(),
            pan_key_held: false,
            target,
        }
    }

    #[test]
    fn straight_connectors_preserve_start_and_end_in_all_eight_directions() {
        let cases = [
            (Vec2::new(10.0, 10.0), Vec2::new(90.0, 10.0)),
            (Vec2::new(90.0, 10.0), Vec2::new(10.0, 10.0)),
            (Vec2::new(10.0, 10.0), Vec2::new(10.0, 90.0)),
            (Vec2::new(10.0, 90.0), Vec2::new(10.0, 10.0)),
            (Vec2::new(10.0, 10.0), Vec2::new(90.0, 90.0)),
            (Vec2::new(90.0, 90.0), Vec2::new(10.0, 10.0)),
            (Vec2::new(10.0, 90.0), Vec2::new(90.0, 10.0)),
            (Vec2::new(90.0, 10.0), Vec2::new(10.0, 90.0)),
        ];

        for tool in [CanvasTool::Line, CanvasTool::Arrow] {
            for (start, end) in cases {
                let mut editor = FlowEditor::new();
                let mut machine = InteractionMachine::new();
                machine.handle(InteractionEvent::SelectTool(tool));
                machine.handle(press(PointerTarget::Empty, start));
                machine.handle(InteractionEvent::PointerMove {
                    screen: end,
                    world: end,
                });
                let effect = machine.handle(InteractionEvent::PointerUp {
                    button: PointerButton::Left,
                    world: end,
                    target: PointerTarget::Empty,
                });
                let report = apply_gesture(&mut editor, effect);
                let connector = editor
                    .world()
                    .nodes()
                    .connector(report.created.expect("connector created"))
                    .unwrap();
                assert_eq!(connector.start.point, start, "{tool:?}");
                assert_eq!(connector.end.point, end, "{tool:?}");
            }
        }
    }

    /// **The three connectors a drawing gesture can produce**, through the
    /// whole machine and the real mapping: element-to-element,
    /// element-to-free, and free-to-element. The view resolves each end's
    /// target from its spatial snap before sending the press and the release —
    /// the machine is world-free — so the targets are supplied the same way
    /// here.
    #[test]
    fn a_drawn_connector_binds_whichever_ends_landed_on_an_element() {
        let cases = [
            (true, true, "element to element"),
            (true, false, "element to free"),
            (false, true, "free to element"),
        ];

        for (bind_start, bind_end, what) in cases {
            let (mut editor, a, b) = editor_with_two_nodes();
            let start = Vec2::new(160.0, 40.0);
            let end = Vec2::new(400.0, 40.0);

            let mut machine = InteractionMachine::new();
            machine.handle(InteractionEvent::SelectTool(CanvasTool::Arrow));
            machine.handle(press(
                if bind_start {
                    PointerTarget::Node(a)
                } else {
                    PointerTarget::Empty
                },
                start,
            ));
            machine.handle(InteractionEvent::PointerMove {
                screen: end,
                world: end,
            });
            let effect = machine.handle(InteractionEvent::PointerUp {
                button: PointerButton::Left,
                world: end,
                target: if bind_end {
                    PointerTarget::Node(b)
                } else {
                    PointerTarget::Empty
                },
            });

            let report = apply_gesture(&mut editor, effect);
            let arrow = report.created.expect("a connector is created");
            let connector = editor.world().nodes().connector(arrow).unwrap();

            assert_eq!(
                connector.start.attachment.map(|it| it.element),
                bind_start.then(|| editor.world().nodes().id(a)),
                "{what}: start"
            );
            assert_eq!(
                connector.end.attachment.map(|it| it.element),
                bind_end.then(|| editor.world().nodes().id(b)),
                "{what}: end"
            );

            // A bound end tracks its element; a free one stays where it was
            // released, whatever moves around it.
            let before = editor.world().nodes().connector(arrow).unwrap();
            editor
                .apply(EditCommand::move_node(a, Vec2::new(0.0, 300.0)))
                .unwrap();
            editor
                .apply(EditCommand::move_node(b, Vec2::new(0.0, -300.0)))
                .unwrap();
            let after = editor.world().nodes().connector(arrow).unwrap();

            assert_eq!(
                after.start.point != before.start.point,
                bind_start,
                "{what}: the start followed the wrong rule"
            );
            assert_eq!(
                after.end.point != before.end.point,
                bind_end,
                "{what}: the end followed the wrong rule"
            );
        }
    }

    fn grouped_rotation_fixture(
        centre: Vec2,
        inner_offset: f32,
    ) -> (FlowEditor, NodeIndex, [NodeIndex; 3]) {
        let mut editor = FlowEditor::new();
        let size = Vec2::new(2.0, 2.0);
        let specs = [-1_000.0, inner_offset, 1_000.0].map(|offset| {
            NodeDraft::new(NodeSpec::new(
                ElementId::NONE,
                ElementKind::GraphNode(GraphNodeKind::Default),
                centre + Vec2::new(offset, 0.0) - size * 0.5,
                size,
            ))
        });
        let members: [NodeIndex; 3] = editor
            .apply(EditCommand::AddNodes(specs.into()))
            .unwrap()
            .added_nodes
            .try_into()
            .unwrap();
        for member in members {
            editor.set_node_selected(member, true);
        }
        assert!(editor.group_selection());
        let group = editor.world().selection().single_node().unwrap();
        (editor, group, members)
    }

    fn rotate_in_steps(editor: &mut FlowEditor, group: NodeIndex, centre: Vec2, angles: &[f32]) {
        let mut machine = InteractionMachine::new();
        apply_gesture(
            editor,
            machine.handle(InteractionEvent::BeginRotate {
                node: group,
                centre,
                pointer: centre + Vec2::new(100.0, 0.0),
            }),
        );
        for &angle in angles {
            apply_gesture(
                editor,
                machine.handle(InteractionEvent::MoveRotate {
                    world: centre + Vec2::new(angle.cos(), angle.sin()) * 100.0,
                    shift: false,
                }),
            );
        }
        apply_gesture(
            editor,
            machine.handle(InteractionEvent::PointerUp {
                button: PointerButton::Left,
                world: Vec2::ZERO,
                target: PointerTarget::Empty,
            }),
        );
    }

    #[test]
    fn a_group_rotation_with_a_member_at_its_centre_undoes_exactly() {
        let centre = Vec2::new(10_000.0, 20_000.0);
        let (mut editor, group, members) = grouped_rotation_fixture(centre, 0.0);
        let before = members.map(|node| editor.world().nodes().position(node));
        let depth = editor.history().undo_depth();

        rotate_in_steps(
            &mut editor,
            group,
            centre,
            &[0.001, 0.002, 0.003, 0.01, 0.1],
        );

        assert_eq!(editor.history().undo_depth(), depth + 1);
        assert!(editor.undo());
        assert_eq!(editor.history().undo_depth(), depth);
        assert_eq!(
            members.map(|node| editor.world().nodes().position(node)),
            before
        );
    }

    #[test]
    fn tiny_group_rotation_frames_do_not_change_the_history_member_set() {
        let centre = Vec2::new(1_000_000.0, 1_000_000.0);
        let (mut editor, group, members) = grouped_rotation_fixture(centre, 0.125);
        let before = members.map(|node| editor.world().nodes().position(node));
        let depth = editor.history().undo_depth();
        let angles: Vec<f32> = (1..=200).map(|step| step as f32 * 0.001).collect();

        rotate_in_steps(&mut editor, group, centre, &angles);

        assert_eq!(editor.history().undo_depth(), depth + 1);
        assert!(editor.undo());
        assert_eq!(editor.history().undo_depth(), depth);
        assert_eq!(
            members.map(|node| editor.world().nodes().position(node)),
            before
        );
    }

    #[test]
    fn group_rotation_is_rigid_across_many_frames_and_a_full_turn() {
        let centre = Vec2::new(4_000.0, 3_000.0);
        let (start, group, members) = grouped_rotation_fixture(centre, 0.0);
        let mut stepped = start.clone();
        let mut single = start.clone();
        let theta = 1.1_f32;
        let angles: Vec<f32> = (1..=60).map(|step| theta * step as f32 / 60.0).collect();

        rotate_in_steps(&mut stepped, group, centre, &angles);
        rotate_in_steps(&mut single, group, centre, &[theta]);

        assert_eq!(
            members.map(|node| stepped.world().nodes().position(node)),
            members.map(|node| single.world().nodes().position(node)),
            "incremental pointer frames changed the final orbit"
        );
        assert_eq!(
            members.map(|node| stepped.world().nodes().angle(node)),
            members.map(|node| single.world().nodes().angle(node)),
            "members did not receive the same total angle"
        );

        let before = start.world().nodes().bounds(group);
        let mut full_turn = start;
        let turn: Vec<f32> = (1..=72)
            .map(|step| std::f32::consts::TAU * step as f32 / 72.0)
            .collect();
        rotate_in_steps(&mut full_turn, group, centre, &turn);
        let after = full_turn.world().nodes().bounds(group);
        assert!((after.min() - before.min()).length() < 1e-3, "{after:?}");
        assert!((after.max() - before.max()).length() < 1e-3, "{after:?}");
    }

    #[test]
    fn awkward_group_rotation_subjects_stay_headless_and_safe() {
        // A nested group: the outer gesture must reach leaves without trying
        // to assign geometry to the bodyless inner group.
        let (mut nested, a, b) = editor_with_two_nodes();
        let c = nested
            .apply(EditCommand::AddNodes(vec![draft(800.0)]))
            .unwrap()
            .added_nodes[0];
        nested.set_node_selected(a, true);
        nested.set_node_selected(b, true);
        assert!(nested.group_selection());
        let inner = nested.world().selection().single_node().unwrap();
        nested.set_node_selected(c, true);
        assert!(nested.group_selection());
        let outer = nested.world().selection().single_node().unwrap();
        let centre = nested.world().nodes().bounds(outer).center();
        rotate_in_steps(&mut nested, outer, centre, &[0.2, 0.4]);
        assert!(nested.world().nodes().angle(a) > 0.39);
        assert_eq!(nested.world().nodes().angle(inner), 0.0);

        // A pre-rotated image and a zero-height free arrow share one group.
        let mut mixed = FlowEditor::new();
        let mut line = NodeSpec::new(
            ElementId::NONE,
            ElementKind::Linear(LinearKind::Arrow),
            Vec2::ZERO,
            Vec2::new(200.0, 0.0),
        );
        line.connector = Some(Connector::new(Vec2::ZERO, Vec2::new(200.0, 0.0)));
        let image = NodeSpec::new(
            ElementId::NONE,
            ElementKind::Image,
            Vec2::new(40.0, 80.0),
            Vec2::new(120.0, 80.0),
        );
        let nodes = mixed
            .apply(EditCommand::AddNodes(vec![
                NodeDraft::new(line),
                NodeDraft::new(image),
            ]))
            .unwrap()
            .added_nodes;
        mixed
            .apply(EditCommand::RotateElements {
                nodes: vec![nodes[1]],
                edges: Vec::new(),
                delta: 0.3,
            })
            .unwrap();
        for node in &nodes {
            mixed.set_node_selected(*node, true);
        }
        assert!(mixed.group_selection());
        let group = mixed.world().selection().single_node().unwrap();
        let centre = mixed.world().nodes().bounds(group).center();
        rotate_in_steps(&mut mixed, group, centre, &[0.1, 0.2]);
        assert!(mixed.world().nodes().connector(nodes[0]).is_some());

        // A group can legitimately be left with one member after deletion.
        let (mut singleton, group, members) = grouped_rotation_fixture(Vec2::ZERO, 0.0);
        singleton.clear_selection();
        singleton.set_node_selected(members[1], true);
        singleton.set_node_selected(members[2], true);
        assert!(singleton.delete_selection());
        singleton.select_only(Some(group));
        let centre = singleton.world().nodes().bounds(group).center();
        rotate_in_steps(&mut singleton, group, centre, &[0.25]);
        assert!((singleton.world().nodes().angle(members[0]) - 0.25).abs() < 1e-5);

        // Undo/redo restores the same group slot; beginning immediately after
        // either operation must capture only the live descendants.
        let (mut restored, group, _) = grouped_rotation_fixture(Vec2::ZERO, 0.0);
        let centre = restored.world().nodes().bounds(group).center();
        assert!(restored.undo());
        rotate_in_steps(&mut restored, group, centre, &[0.1]);
        assert!(restored.redo());
        restored.select_only(Some(group));
        let centre = restored.world().nodes().bounds(group).center();
        rotate_in_steps(&mut restored, group, centre, &[0.3]);
    }

    #[test]
    fn a_label_editor_masks_a_rotation_press_in_the_machine() {
        let mut machine = InteractionMachine::new();
        machine.handle(InteractionEvent::DoubleClick {
            world: Vec2::ZERO,
            target: PointerTarget::Node(NodeIndex::new(0)),
        });
        assert_eq!(
            machine.handle(InteractionEvent::BeginRotate {
                node: NodeIndex::new(0),
                centre: Vec2::ZERO,
                pointer: Vec2::new(1.0, 0.0),
            }),
            crate::interaction::InteractionEffect::None
        );
    }

    #[test]
    fn a_loose_multi_selection_rotates_as_one_selection() {
        let (mut editor, a, b) = editor_with_two_nodes();
        editor.set_node_selected(a, true);
        editor.set_node_selected(b, true);
        let centre = crate::geometry::Rect::of_rects(
            [a, b]
                .into_iter()
                .map(|node| editor.world().nodes().rotated_bounds(node)),
        )
        .unwrap()
        .center();
        let before = [a, b].map(|node| editor.world().nodes().bounds(node).center());

        rotate_in_steps(&mut editor, a, centre, &[std::f32::consts::FRAC_PI_2]);

        assert_eq!(editor.world().selection().nodes().len(), 2);
        for (index, node) in [a, b].into_iter().enumerate() {
            let expected = before[index].rotated_about(centre, std::f32::consts::FRAC_PI_2);
            assert!((editor.world().nodes().bounds(node).center() - expected).length() < 1e-3);
        }
    }

    /// A group of {shape, arrow, shape} laid out horizontally, matching the
    /// repro: a rectangle on the left, an ellipse on the right, and an arrow
    /// between them. `bound` decides whether the arrow's ends attach to the two
    /// shapes (Excalidraw's default) or float free. The group centre is the
    /// origin, so the rotated coordinates below stay easy to read.
    fn arrow_group(bound: bool) -> (FlowEditor, NodeIndex, NodeIndex, Vec2) {
        let mut editor = FlowEditor::new();
        editor.set_rules(ConnectionRules::PERMISSIVE);
        let shape = |kind, x: f32| {
            NodeDraft::new(NodeSpec::new(
                ElementId::NONE,
                ElementKind::Shape(kind),
                Vec2::new(x - 50.0, -50.0),
                Vec2::new(100.0, 100.0),
            ))
        };
        let added = editor
            .apply(EditCommand::AddNodes(vec![
                shape(ShapeKind::Rectangle, -300.0),
                shape(ShapeKind::Ellipse, 300.0),
            ]))
            .unwrap()
            .added_nodes;
        let (rect, ell) = (added[0], added[1]);

        let (start, end) = (Vec2::new(-250.0, 0.0), Vec2::new(250.0, 0.0));
        let connector = if bound {
            editor.connector_between(start, end, Some(rect), Some(ell))
        } else {
            Connector::new(start, end)
        };
        let mut spec = NodeSpec::new(
            ElementId::NONE,
            ElementKind::Linear(LinearKind::Arrow),
            start,
            end - start,
        );
        spec.connector = Some(connector);
        let arrow = editor
            .apply(EditCommand::AddNodes(vec![NodeDraft::new(spec)]))
            .unwrap()
            .added_nodes[0];

        for node in [rect, ell, arrow] {
            editor.set_node_selected(node, true);
        }
        assert!(editor.group_selection());
        let group = editor.world().selection().single_node().unwrap();
        let centre = editor.world().nodes().bounds(group).center();
        (editor, group, arrow, centre)
    }

    /// **What the user sees**: a connector's endpoints as drawn, with the node
    /// angle the renderer would apply folded in (see `render::scene`).
    fn drawn_arrow(editor: &FlowEditor, arrow: NodeIndex) -> [Vec2; 2] {
        let c = editor.world().nodes().connector(arrow).unwrap();
        let angle = editor.world().nodes().angle(arrow);
        let mid = c.bounds().center();
        [
            c.start.point.rotated_about(mid, angle),
            c.end.point.rotated_about(mid, angle),
        ]
    }

    /// **The reported bug, as a property.** Rotating a group carries its arrow
    /// with it exactly as it carries the shapes: the drawn endpoints end up
    /// rotated about the group centre, still spanning the two shapes — not left
    /// horizontal in the middle.
    ///
    /// Before the fix, the connector's node angle was stacked with the group
    /// rotation while the renderer also turned its endpoints, so a 90° turn
    /// double-rotated the arrow back to horizontal: the concrete `after`
    /// endpoints came out `(250, 0)`/`(-250, 0)` instead of the `(0, -250)` /
    /// `(0, 250)` this asserts. Both a bound and a free-standing arrow are
    /// checked, and an arbitrary angle alongside the right angle.
    #[test]
    fn a_rotated_group_carries_its_arrow_like_its_shapes() {
        for bound in [true, false] {
            for &angle in &[std::f32::consts::FRAC_PI_2, 0.7] {
                let (mut editor, group, arrow, centre) = arrow_group(bound);

                let before = drawn_arrow(&editor, arrow);
                assert_eq!(before, [Vec2::new(-250.0, 0.0), Vec2::new(250.0, 0.0)]);

                rotate_in_steps(&mut editor, group, centre, &[angle]);

                let after = drawn_arrow(&editor, arrow);
                let expect = before.map(|p| p.rotated_about(centre, angle));
                for (got, want) in after.into_iter().zip(expect) {
                    assert!(
                        (got - want).length() < 1e-2,
                        "bound={bound} angle={angle}: arrow endpoint {got:?} did not rotate with the group (want {want:?})"
                    );
                }
            }
        }
    }

    /// A concrete 90° case, spelled out, and the guarantee that dragging it in
    /// many frames costs no more history than doing it in one. The gesture
    /// records two absolute streams — the shapes' transforms and the arrow's
    /// connector — and each coalesces to a single entry, so a whole turn is one
    /// undo step whatever the frame count.
    #[test]
    fn a_group_arrow_rotation_is_one_bounded_undo_step() {
        let quarter = std::f32::consts::FRAC_PI_2;

        let (mut single, group, arrow, centre) = arrow_group(true);
        let depth = single.history().undo_depth();
        rotate_in_steps(&mut single, group, centre, &[quarter]);

        // The horizontal arrow now spans the vertically stacked shapes.
        let [start, end] = drawn_arrow(&single, arrow);
        assert!(
            (start - Vec2::new(0.0, -250.0)).length() < 1e-2,
            "{start:?}"
        );
        assert!((end - Vec2::new(0.0, 250.0)).length() < 1e-2, "{end:?}");

        let one_step = single.history().undo_depth();
        assert!(single.undo());
        assert_eq!(
            single.history().undo_depth(),
            depth,
            "one undo did not cover the whole group-arrow rotation"
        );
        assert_eq!(
            drawn_arrow(&single, arrow),
            [Vec2::new(-250.0, 0.0), Vec2::new(250.0, 0.0)]
        );

        // Sixty frames must leave the same, bounded number of entries.
        let (mut stepped, group, _, centre) = arrow_group(true);
        let angles: Vec<f32> = (1..=60).map(|s| quarter * s as f32 / 60.0).collect();
        rotate_in_steps(&mut stepped, group, centre, &angles);
        assert_eq!(
            stepped.history().undo_depth(),
            one_step,
            "per-frame rotation grew the undo stack"
        );
    }

    /// A bound arrow stays attached through the turn: after a 90° group
    /// rotation its ends still sit on the rectangle and the ellipse, which is
    /// the whole point of the connection surviving.
    #[test]
    fn a_bound_arrow_stays_attached_after_a_group_rotation() {
        let (mut editor, group, arrow, centre) = arrow_group(true);
        let connector = editor.world().nodes().connector(arrow).unwrap();
        assert!(connector.start.attachment.is_some() && connector.end.attachment.is_some());

        rotate_in_steps(&mut editor, group, centre, &[std::f32::consts::FRAC_PI_2]);

        let connector = editor.world().nodes().connector(arrow).unwrap();
        assert!(
            connector.start.attachment.is_some() && connector.end.attachment.is_some(),
            "the rotation severed the arrow's bindings"
        );
    }

    #[test]
    fn a_rotation_drag_is_one_undo_step() {
        let (mut editor, node, _) = editor_with_two_nodes();
        let depth = editor.history().undo_depth();
        let mut machine = InteractionMachine::new();
        apply_gesture(
            &mut editor,
            machine.handle(InteractionEvent::BeginRotate {
                node,
                centre: Vec2::new(80.0, 40.0),
                pointer: Vec2::new(80.0, -20.0),
            }),
        );
        for degrees in [5.0_f32, 15.0, 30.0, 45.0] {
            let angle = (-90.0 + degrees).to_radians();
            apply_gesture(
                &mut editor,
                machine.handle(InteractionEvent::MoveRotate {
                    world: Vec2::new(80.0, 40.0) + Vec2::new(angle.cos(), angle.sin()) * 60.0,
                    shift: false,
                }),
            );
        }
        apply_gesture(
            &mut editor,
            machine.handle(InteractionEvent::PointerUp {
                button: PointerButton::Left,
                world: Vec2::ZERO,
                target: PointerTarget::Empty,
            }),
        );

        assert!((editor.world().nodes().angle(node) - 45.0_f32.to_radians()).abs() < 1e-4);
        assert_eq!(editor.history().undo_depth(), depth + 1);
        assert!(editor.undo());
        assert!(editor.world().nodes().angle(node).abs() < 1e-5);
    }

    /// Drives one whole resize the way a person performs it, through the real
    /// machine and the real mapping.
    ///
    /// Answers the frame the drag ended on and the number of undo steps it
    /// cost, because those are the two questions every resize test below asks.
    fn resize_in_steps(
        editor: &mut FlowEditor,
        node: NodeIndex,
        corner: crate::geometry::ResizeCorner,
        to: Vec2,
        keeps_aspect: bool,
        steps: usize,
    ) -> (crate::geometry::Rect, usize) {
        let depth = editor.history().undo_depth();
        let frame = editor.world().nodes().bounds(node);

        let mut machine = InteractionMachine::new();
        let effect = machine.handle(InteractionEvent::BeginResize {
            node,
            corner,
            frame,
            keeps_aspect,
        });
        apply_gesture(editor, effect);

        let start = corner.of(frame);
        for step in 1..=steps {
            let at = start + (to - start) * (step as f32 / steps as f32);
            let effect = machine.handle(InteractionEvent::PointerMove {
                screen: at,
                world: at,
            });
            apply_gesture(editor, effect);
        }

        let effect = machine.handle(InteractionEvent::PointerUp {
            button: PointerButton::Left,
            world: to,
            target: PointerTarget::Empty,
        });
        apply_gesture(editor, effect);

        (
            editor.world().nodes().bounds(node),
            editor.history().undo_depth() - depth,
        )
    }

    fn resize(
        editor: &mut FlowEditor,
        node: NodeIndex,
        corner: crate::geometry::ResizeCorner,
        to: Vec2,
        keeps_aspect: bool,
    ) -> (crate::geometry::Rect, usize) {
        resize_in_steps(editor, node, corner, to, keeps_aspect, 10)
    }

    /// **A whole resize is one undo press**, however many moves it took. The
    /// absolute transform from the latest frame supersedes the prior one while
    /// retaining the inverse captured at the press.
    #[test]
    fn a_resize_drag_is_one_undo_step_and_undoes_exactly() {
        use crate::geometry::ResizeCorner;

        let (mut editor, node, _) = editor_with_two_nodes();
        let before = editor.world().nodes().bounds(node);

        let (after, steps) = resize(
            &mut editor,
            node,
            ResizeCorner::BottomRight,
            Vec2::new(300.0, 200.0),
            false,
        );

        assert_eq!(steps, 1, "a resize drag cost {steps} undo presses");
        assert!((after.size.x - 300.0).abs() < 1e-3, "{after:?}");
        assert!((after.size.y - 200.0).abs() < 1e-3, "{after:?}");

        assert!(editor.undo());
        let restored = editor.world().nodes().bounds(node);
        assert!(
            (restored.origin - before.origin).length() < 1e-3
                && (restored.size - before.size).length() < 1e-3,
            "{restored:?} is not {before:?}"
        );
    }

    #[test]
    fn group_resize_is_absolute_and_places_a_rotated_member_from_its_start_centre() {
        use crate::geometry::ResizeCorner;

        let centre = Vec2::new(4_000.0, 3_000.0);
        let (mut start, group, members) = grouped_rotation_fixture(centre, 0.0);
        start
            .apply(EditCommand::RotateElements {
                nodes: vec![members[0]],
                edges: Vec::new(),
                delta: 0.6,
            })
            .unwrap();
        let frame = start.world().nodes().bounds(group).normalized();
        let old_centres = members.map(|node| start.world().nodes().bounds(node).center());
        let old_angles = members.map(|node| start.world().nodes().angle(node));
        let to = frame.max() + Vec2::new(700.0, 300.0);
        let target = crate::geometry::Rect::from_corners(frame.min(), to);
        let scale = Vec2::new(
            target.width() / frame.width(),
            target.height() / frame.height(),
        );
        let mut stepped = start.clone();
        let mut single = start.clone();

        let (_, entries) = resize_in_steps(
            &mut stepped,
            group,
            ResizeCorner::BottomRight,
            to,
            false,
            30,
        );
        resize_in_steps(&mut single, group, ResizeCorner::BottomRight, to, false, 1);

        assert_eq!(entries, 1, "the resize grew history per frame");
        assert_eq!(
            members.map(|node| stepped.world().nodes().bounds(node)),
            members.map(|node| single.world().nodes().bounds(node)),
            "per-frame resize diverged from the one-step target"
        );
        for (index, member) in members.into_iter().enumerate() {
            let expected = target.origin + (old_centres[index] - frame.origin).scale(scale);
            let actual = stepped.world().nodes().bounds(member).center();
            assert!((actual - expected).length() < 1e-3, "{member}: {actual:?}");
            assert_eq!(stepped.world().nodes().angle(member), old_angles[index]);
        }

        assert!(stepped.undo());
        assert_eq!(
            members.map(|node| stepped.world().nodes().bounds(node)),
            members.map(|node| start.world().nodes().bounds(node))
        );
    }

    /// **§10's aspect lock, through the gesture that spends it.** A locked drag
    /// keeps the picture's proportions whatever the pointer does; the free one
    /// beside it is the same drag with the lock off, and it is what a
    /// shift-drag produces.
    #[test]
    fn a_locked_resize_keeps_the_picture_s_shape_and_a_free_one_does_not() {
        use crate::geometry::ResizeCorner;

        let (mut editor, node, _) = editor_with_two_nodes();
        let aspect = {
            let size = editor.world().nodes().size(node);
            size.x / size.y
        };

        let (locked, _) = resize(
            &mut editor,
            node,
            ResizeCorner::BottomRight,
            Vec2::new(320.0, 400.0),
            true,
        );
        assert!(
            (locked.size.x / locked.size.y - aspect).abs() < 1e-3,
            "the lock let the shape drift: {locked:?}"
        );

        editor.undo();

        let (free, _) = resize(
            &mut editor,
            node,
            ResizeCorner::BottomRight,
            Vec2::new(320.0, 400.0),
            false,
        );
        assert!(
            (free.size.x / free.size.y - aspect).abs() > 0.5,
            "the free drag kept the shape anyway: {free:?}"
        );
    }

    /// **Dragging the top-left grip moves the origin**, which is why the effect
    /// carries a whole rectangle rather than a size.
    #[test]
    fn resizing_from_the_top_left_holds_the_bottom_right_still() {
        use crate::geometry::ResizeCorner;

        let (mut editor, node, _) = editor_with_two_nodes();
        let before = editor.world().nodes().bounds(node);

        let (after, _) = resize(
            &mut editor,
            node,
            ResizeCorner::TopLeft,
            before.min() - Vec2::new(40.0, 20.0),
            false,
        );

        assert!((after.max() - before.max()).length() < 1e-3, "{after:?}");
        assert!((after.min() - (before.min() - Vec2::new(40.0, 20.0))).length() < 1e-3);
    }

    /// An abandoned resize is not an undo step, exactly as an abandoned drag is
    /// not: the entries the gesture recorded are applied in reverse and
    /// discarded.
    #[test]
    fn an_abandoned_resize_leaves_nothing_on_the_stack() {
        use crate::geometry::ResizeCorner;

        let (mut editor, node, _) = editor_with_two_nodes();
        let before = editor.world().nodes().bounds(node);
        let depth = editor.history().undo_depth();

        let mut machine = InteractionMachine::new();
        let effect = machine.handle(InteractionEvent::BeginResize {
            node,
            corner: ResizeCorner::BottomRight,
            frame: before,
            keeps_aspect: false,
        });
        apply_gesture(&mut editor, effect);
        let effect = machine.handle(InteractionEvent::PointerMove {
            screen: Vec2::new(500.0, 500.0),
            world: Vec2::new(500.0, 500.0),
        });
        apply_gesture(&mut editor, effect);

        let effect = machine.handle(InteractionEvent::Cancel);
        apply_gesture(&mut editor, effect);

        assert_eq!(editor.history().undo_depth(), depth);
        let restored = editor.world().nodes().bounds(node);
        assert!(
            (restored.size - before.size).length() < 1e-3,
            "{restored:?} is not {before:?}"
        );
    }

    /// Drives the real state machine through the drag a person performs, and
    /// feeds every effect through the real mapping. **The phase's coalescing
    /// requirement, with no window anywhere.**
    #[test]
    fn a_press_sixty_moves_and_a_release_is_one_undo_step() {
        let (mut editor, node, _) = editor_with_two_nodes();
        let start = editor.world().nodes().position(node);
        // The fixture's own `AddNodes` is a step too; the drag has to add
        // exactly one more.
        let depth = editor.history().undo_depth();

        let mut machine = InteractionMachine::new();
        let mut at = Vec2::new(10.0, 10.0);
        let effect = machine.handle(press(PointerTarget::Node(node), at));
        apply_gesture(&mut editor, effect);

        for _ in 0..60 {
            at += Vec2::new(3.0, 1.0);
            let effect = machine.handle(InteractionEvent::PointerMove {
                screen: at,
                world: at,
            });
            apply_gesture(&mut editor, effect);
        }

        let effect = machine.handle(InteractionEvent::PointerUp {
            button: PointerButton::Left,
            world: at,
            target: PointerTarget::Node(node),
        });
        apply_gesture(&mut editor, effect);

        assert_eq!(
            editor.world().nodes().position(node),
            start + Vec2::new(180.0, 60.0)
        );
        assert_eq!(
            editor.history().undo_depth(),
            depth + 1,
            "the drag recorded an entry per mouse move"
        );

        assert!(editor.undo());
        assert_eq!(
            editor.world().nodes().position(node),
            start,
            "one undo did not cover the whole drag"
        );
        assert_eq!(
            editor.history().undo_depth(),
            depth,
            "the drag was more than one undo step"
        );
    }

    /// `Esc` mid-drag puts the node back **and leaves nothing on the stack**:
    /// a drag the user cancelled is not a step to walk back through later.
    #[test]
    fn escaping_a_drag_restores_the_node_and_records_nothing() {
        let (mut editor, node, other) = editor_with_two_nodes();
        // One real edit before the drag, so a cancel that reached too far would
        // be visible as this one being undone.
        editor
            .apply(EditCommand::move_node(other, Vec2::new(5.0, 5.0)))
            .unwrap();
        let start = editor.world().nodes().position(node);
        let depth = editor.history().undo_depth();

        let mut machine = InteractionMachine::new();
        let at = Vec2::new(10.0, 10.0);
        apply_gesture(
            &mut editor,
            machine.handle(press(PointerTarget::Node(node), at)),
        );
        for step in 1..=5 {
            let to = at + Vec2::new(step as f32 * 4.0, 0.0);
            apply_gesture(
                &mut editor,
                machine.handle(InteractionEvent::PointerMove {
                    screen: to,
                    world: to,
                }),
            );
        }
        assert_ne!(editor.world().nodes().position(node), start);

        apply_gesture(&mut editor, machine.handle(InteractionEvent::Cancel));

        assert_eq!(editor.world().nodes().position(node), start);
        assert_eq!(
            editor.history().undo_depth(),
            depth,
            "the cancelled drag left a step on the stack"
        );
        assert!(!editor.can_redo());
    }

    /// A press and release that never travelled is a click. It selects, and it
    /// must not consume the undo press that the edit before it earned.
    #[test]
    fn a_click_that_never_moved_records_no_step() {
        let (mut editor, node, other) = editor_with_two_nodes();
        editor
            .apply(EditCommand::move_node(other, Vec2::new(5.0, 5.0)))
            .unwrap();
        let depth = editor.history().undo_depth();

        let mut machine = InteractionMachine::new();
        let at = Vec2::new(10.0, 10.0);
        apply_gesture(
            &mut editor,
            machine.handle(press(PointerTarget::Node(node), at)),
        );
        apply_gesture(
            &mut editor,
            machine.handle(InteractionEvent::PointerUp {
                button: PointerButton::Left,
                world: at,
                target: PointerTarget::Node(node),
            }),
        );

        assert_eq!(editor.history().undo_depth(), depth);
        assert_eq!(editor.world().selection().single_node(), Some(node));
    }

    /// A connection made by dragging out of a handle is an ordinary edit, so it
    /// undoes like one — and the report carries §4's refusal rather than
    /// swallowing it.
    #[test]
    fn a_dropped_connection_is_an_undoable_edit_and_reports_its_refusal() {
        let (mut editor, a, b) = editor_with_two_nodes();
        let handle = editor.world().nodes().handles(a).next();
        assert!(handle.is_none(), "the fixture's nodes carry no handles");

        // Whole-node mode instead, which is the same command with a different
        // end. Built directly, since the machine's connect gesture starts at a
        // handle and this fixture has none.
        let summary = editor
            .apply(EditCommand::Connect(vec![crate::runtime::EdgeSpec::new(
                ElementId::NONE,
                crate::runtime::EdgeEnd::node(a),
                crate::runtime::EdgeEnd::node(b),
            )]))
            .unwrap();
        assert_eq!(summary.added_edges.len(), 1);

        assert!(editor.undo());
        assert!(!editor.world().edge_is_live(summary.added_edges[0]));
        assert!(editor.redo());
        assert!(editor.world().edge_is_live(summary.added_edges[0]));
    }

    // ---- Phase 10.5: an edge is clickable -------------------------------

    /// A permissive edge between the fixture's two nodes, built directly —
    /// whole-node mode, since the fixture carries no handles for the connect
    /// gesture to start at.
    fn edge_between(
        editor: &mut FlowEditor,
        a: NodeIndex,
        b: NodeIndex,
    ) -> crate::models::EdgeIndex {
        editor
            .apply(EditCommand::Connect(vec![crate::runtime::EdgeSpec::new(
                ElementId::NONE,
                crate::runtime::EdgeEnd::node(a),
                crate::runtime::EdgeEnd::node(b),
            )]))
            .unwrap()
            .added_edges[0]
    }

    fn press_with(target: PointerTarget, at: Vec2, modifiers: InputModifiers) -> InteractionEvent {
        InteractionEvent::PointerDown {
            screen: at,
            world: at,
            button: PointerButton::Left,
            modifiers,
            pan_key_held: false,
            target,
        }
    }

    /// **What Phase 10 could not do**, driven through the real machine and the
    /// real applier: press an edge, delete it, undo.
    ///
    /// Before Phase 10.5 the press started a rubber band, so an edge could only
    /// be selected by banding over it and `Delete` could reach one no other
    /// way.
    #[test]
    fn a_press_on_an_edge_selects_it_so_delete_can_reach_it() {
        let (mut editor, a, b) = editor_with_two_nodes();
        let edge = edge_between(&mut editor, a, b);
        let mut machine = InteractionMachine::new();

        let report = apply_gesture(
            &mut editor,
            machine.handle(press(PointerTarget::Edge(edge), Vec2::new(200.0, 40.0))),
        );

        assert!(report.changed, "the selection ring has to be repainted");
        assert_eq!(editor.world().selection().edges(), [edge]);
        assert!(
            editor.world().selection().nodes().is_empty(),
            "clicking an edge selected a node as well"
        );

        assert!(editor.delete_selection());
        assert!(!editor.world().edge_is_live(edge));

        assert!(editor.undo());
        assert!(
            editor.world().edge_is_live(edge),
            "one undo did not bring the edge back"
        );
    }

    /// A press on an edge replaces whatever was selected, and shift adds to it
    /// — the same two answers a node press gives, which is the point.
    #[test]
    fn shift_extends_the_selection_and_a_plain_press_replaces_it() {
        let (mut editor, a, b) = editor_with_two_nodes();
        let edge = edge_between(&mut editor, a, b);
        let mut machine = InteractionMachine::new();
        let at = Vec2::new(200.0, 40.0);

        // A node, then the edge with shift: both are selected.
        apply_gesture(
            &mut editor,
            machine.handle(press(PointerTarget::Node(a), at)),
        );
        apply_gesture(
            &mut editor,
            machine.handle(InteractionEvent::PointerUp {
                button: PointerButton::Left,
                world: at,
                target: PointerTarget::Node(a),
            }),
        );
        apply_gesture(
            &mut editor,
            machine.handle(press_with(
                PointerTarget::Edge(edge),
                at,
                InputModifiers::shift(),
            )),
        );

        assert_eq!(editor.world().selection().nodes(), [a]);
        assert_eq!(editor.world().selection().edges(), [edge]);

        // And the second node with shift: a selection built up one press at a
        // time, which a plain `select_only` could never produce.
        apply_gesture(
            &mut editor,
            machine.handle(press_with(
                PointerTarget::Node(b),
                at,
                InputModifiers::shift(),
            )),
        );
        assert_eq!(editor.world().selection().nodes(), [a, b]);
        assert_eq!(editor.world().selection().edges(), [edge]);

        // That press opened a node drag; release it, or the machine is busy and
        // ignores everything after.
        apply_gesture(
            &mut editor,
            machine.handle(InteractionEvent::PointerUp {
                button: PointerButton::Left,
                world: at,
                target: PointerTarget::Node(b),
            }),
        );

        // Deleting that mixed selection is one undo step, and the undo restores
        // all of it — the property `delete_selection` was written for and the
        // reason the edge arm needed no change there.
        let depth = editor.history().undo_depth();
        assert!(editor.delete_selection());
        assert_eq!(editor.history().undo_depth(), depth + 1);
        assert!(editor.undo());
        assert!(editor.world().node_is_live(a) && editor.world().node_is_live(b));
        assert!(editor.world().edge_is_live(edge));

        // A plain press then replaces the lot.
        apply_gesture(
            &mut editor,
            machine.handle(press(PointerTarget::Edge(edge), at)),
        );
        assert_eq!(editor.world().selection().edges(), [edge]);
        assert!(editor.world().selection().nodes().is_empty());
    }

    /// **A press on empty canvas still clears and still bands.** The edge arm
    /// was carved out of this one, and trading the band for the click would be
    /// the easiest way to make this phase a regression.
    #[test]
    fn a_press_on_empty_canvas_still_clears_the_selection_and_bands() {
        let (mut editor, a, b) = editor_with_two_nodes();
        let edge = edge_between(&mut editor, a, b);
        let mut machine = InteractionMachine::new();
        let at = Vec2::new(200.0, 40.0);

        apply_gesture(
            &mut editor,
            machine.handle(press(PointerTarget::Edge(edge), at)),
        );
        assert_eq!(editor.world().selection().edges(), [edge]);

        let effect = machine.handle(press(PointerTarget::Empty, Vec2::new(900.0, 900.0)));
        assert!(
            matches!(
                effect,
                crate::interaction::InteractionEffect::BeginBoxSelect(_)
            ),
            "empty canvas stopped starting a band"
        );
        apply_gesture(&mut editor, effect);
        assert!(editor.world().selection().is_empty());
    }

    /// The two effects this file deliberately does not handle must fall through
    /// untouched, or the caller's own arms would never run.
    #[test]
    fn the_camera_and_the_rubber_band_are_left_to_the_caller() {
        let (mut editor, _, _) = editor_with_two_nodes();
        let before = editor.to_document();

        for effect in [
            crate::interaction::InteractionEffect::PanBy(Vec2::new(10.0, 10.0)),
            crate::interaction::InteractionEffect::CommitBoxSelect(
                crate::interaction::BoxSelection {
                    rect: crate::geometry::Rect::new(Vec2::ZERO, Vec2::new(1e4, 1e4)),
                    additive: false,
                },
            ),
        ] {
            let report = apply_gesture(&mut editor, effect);
            assert!(!report.changed);
            assert_eq!(report.connection, None);
        }

        assert_eq!(editor.to_document(), before);
    }

    // ---- §45's creation, end to end -------------------------------------

    /// Drives the real state machine through the drag a person performs with a
    /// tool selected, and feeds every effect through the real mapping — the
    /// creation counterpart of
    /// [`a_press_sixty_moves_and_a_release_is_one_undo_step`](tests::a_press_sixty_moves_and_a_release_is_one_undo_step),
    /// with no window anywhere.
    fn draw(
        editor: &mut FlowEditor,
        tool: CanvasTool,
        from: Vec2,
        to: Vec2,
    ) -> super::GestureReport {
        let mut machine = InteractionMachine::new();
        machine.handle(InteractionEvent::SelectTool(tool));

        let mut report = apply_gesture(
            editor,
            machine.handle(InteractionEvent::PointerDown {
                screen: from,
                world: from,
                button: PointerButton::Left,
                modifiers: InputModifiers::default(),
                pan_key_held: false,
                target: PointerTarget::Empty,
            }),
        );

        // Ten moves, so the gesture is a real drag rather than a press and a
        // release with a rectangle smuggled in.
        for step in 1..=10 {
            let at = from + (to - from) * (step as f32 / 10.0);
            report = apply_gesture(
                editor,
                machine.handle(InteractionEvent::PointerMove {
                    screen: at,
                    world: at,
                }),
            );
        }

        let _ = report;
        apply_gesture(
            editor,
            machine.handle(InteractionEvent::PointerUp {
                button: PointerButton::Left,
                world: to,
                target: PointerTarget::Empty,
            }),
        )
    }

    /// **Every creating tool produces the element it advertises**, at the
    /// geometry the drag described — the assertion that the palette's promise
    /// and the document's content are the same thing.
    #[test]
    fn each_tool_creates_its_own_kind_at_the_dragged_rectangle() {
        // **The Text tool is excluded because it does not create on release**,
        // and that is asserted separately rather than skipped silently — see
        // `the_text_tool_writes_nothing_until_there_is_text` below.
        for tool in CanvasTool::ALL
            .iter()
            .filter(|tool| tool.creates() && !tool.edits_text_on_release())
        {
            let mut editor = FlowEditor::new();
            let report = draw(
                &mut editor,
                *tool,
                Vec2::new(20.0, 30.0),
                Vec2::new(140.0, 110.0),
            );

            let node = report.created.expect("a drag must create something");
            assert!(report.changed, "{} recorded no change", tool.name());
            assert_eq!(
                editor.world().nodes().kind(node),
                &tool.element_kind().unwrap(),
                "{} created the wrong kind",
                tool.name()
            );
            assert_eq!(editor.world().nodes().position(node), Vec2::new(20.0, 30.0));
            assert_eq!(editor.world().nodes().size(node), Vec2::new(120.0, 80.0));
        }
    }

    /// **The Text tool writes nothing to the document until there is text.**
    ///
    /// Drives the real machine through the real drag, exactly as the tests
    /// beside it do, and asserts the two halves: the release adds no element
    /// and no undo step, and the commit adds exactly one of each. The failure
    /// this prevents is an *invisible* element — a text node with no glyphs
    /// draws nothing at all, so a create-on-release tool would leave one on the
    /// canvas every time a user pressed Escape.
    #[test]
    fn the_text_tool_writes_nothing_until_there_is_text() {
        use crate::interaction::{InteractionEffect, TextTarget};

        let mut editor = FlowEditor::new();
        let mut machine = InteractionMachine::new();
        machine.handle(InteractionEvent::SelectTool(CanvasTool::Text));

        machine.handle(press(PointerTarget::Empty, Vec2::new(20.0, 30.0)));
        machine.handle(InteractionEvent::PointerMove {
            screen: Vec2::new(220.0, 52.0),
            world: Vec2::new(220.0, 52.0),
        });
        let effect = machine.handle(InteractionEvent::PointerUp {
            button: PointerButton::Left,
            world: Vec2::new(220.0, 52.0),
            target: PointerTarget::Empty,
        });
        apply_gesture(&mut editor, effect);

        let InteractionEffect::BeginTextEdit(target) = effect else {
            panic!("the text tool must open an editor, not commit: {effect:?}");
        };
        assert_eq!(
            editor.world().nodes().len(),
            0,
            "the release must not have added anything"
        );
        assert_eq!(editor.history().undo_depth(), 0);

        // Abandoning costs nothing, because nothing was written.
        assert!(!editor.commit_text(target, "   "));
        assert_eq!(editor.world().nodes().len(), 0);
        assert_eq!(editor.history().undo_depth(), 0);

        assert!(editor.commit_text(target, "hello"));
        assert_eq!(editor.world().nodes().len(), 1);
        assert_eq!(editor.history().undo_depth(), 1, "one undo step, not two");

        let node = crate::models::NodeIndex::new(0);
        assert_eq!(editor.world().nodes().kind(node), &ElementKind::Text);
        assert_eq!(
            editor.world().nodes().cold(node).label.as_deref(),
            Some("hello")
        );
        assert_eq!(
            editor.world().nodes().bounds(node),
            crate::geometry::Rect::new(Vec2::new(20.0, 30.0), Vec2::new(200.0, 22.0)),
            "the element occupies the rectangle that was dragged"
        );
        assert_eq!(target, TextTarget::New(editor.world().nodes().bounds(node)));

        assert!(editor.undo());
        assert_eq!(
            editor.world().nodes().live_indices().count(),
            0,
            "one press of undo takes the whole thing away"
        );
    }

    /// A created graph node is born connectable. One that was not would look
    /// like a node and take part in no graph, which is the sort of half-made
    /// thing this phase exists to stop.
    #[test]
    fn a_created_graph_node_is_born_with_its_handles() {
        let mut editor = FlowEditor::new();
        let node = draw(
            &mut editor,
            CanvasTool::GraphNode,
            Vec2::ZERO,
            Vec2::new(160.0, 80.0),
        )
        .created
        .unwrap();

        assert_eq!(editor.world().nodes().handle_count(node), 2);

        // A drawn shape gets none: §4 refuses an edge to one anyway, so handles
        // on it would be dots that do nothing.
        let mut editor = FlowEditor::new();
        let shape = draw(
            &mut editor,
            CanvasTool::Rectangle,
            Vec2::ZERO,
            Vec2::new(100.0, 100.0),
        )
        .created
        .unwrap();
        assert_eq!(editor.world().nodes().handle_count(shape), 0);
    }

    /// **The phase's contract**: a created element survives undo and redo.
    ///
    /// Creating outside the command layer is the exact defect Phase 7 made hard
    /// to express, and this is what would notice it — the element would appear
    /// on screen and the undo stack would be empty.
    #[test]
    fn a_created_element_survives_undo_and_redo() {
        let mut editor = FlowEditor::new();
        let before = editor.to_document();

        let node = draw(
            &mut editor,
            CanvasTool::Diamond,
            Vec2::new(5.0, 5.0),
            Vec2::new(205.0, 105.0),
        )
        .created
        .unwrap();
        let after = editor.to_document();
        assert_eq!(after.nodes.len(), 1);

        assert!(editor.can_undo(), "a creation must be undoable");
        assert!(editor.undo());
        assert!(!editor.world().node_is_live(node));
        assert_eq!(
            editor.to_document().nodes,
            before.nodes,
            "undo must leave the elements as it found them"
        );

        assert!(editor.redo());
        assert!(editor.world().node_is_live(node));
        assert_eq!(
            editor.to_document().nodes,
            after.nodes,
            "redo must put back the same element, at the same index"
        );
    }

    /// **One thing an undone creation does not restore, deliberately: the id
    /// allocator.**
    ///
    /// `to_document` carries the allocator's next id, and undoing an
    /// `AddNodes` does not wind it back — so the document after undo is not
    /// byte-equal to the document before, and the property above compares the
    /// *elements* rather than the whole struct.
    ///
    /// It is the safe direction, and the same judgement §23's cache version
    /// makes for the same reason: an id that went backwards would be reissued
    /// to a different element while the undo stack still holds entries naming
    /// the first, which is the silent corruption Phase 7 exists to prevent. The
    /// cost is a gap in the id sequence of a document that was drawn and undone
    /// — nothing reads ids as a dense range.
    #[test]
    fn an_undone_creation_does_not_reissue_its_id() {
        let mut editor = FlowEditor::new();
        let first = draw(
            &mut editor,
            CanvasTool::Rectangle,
            Vec2::ZERO,
            Vec2::new(100.0, 100.0),
        )
        .created
        .unwrap();
        let first_id = editor.world().nodes().id(first);

        assert!(editor.undo());

        let second = draw(
            &mut editor,
            CanvasTool::Rectangle,
            Vec2::ZERO,
            Vec2::new(100.0, 100.0),
        )
        .created
        .unwrap();
        assert_ne!(
            editor.world().nodes().id(second),
            first_id,
            "an undone element's id must not be handed to its replacement"
        );
    }

    /// One creation is **one** undo step, however many moves the drag emitted —
    /// the same coalescing requirement a node drag has, arrived at differently:
    /// a creation writes once, on the release.
    #[test]
    fn a_whole_creation_drag_is_one_undo_step() {
        let mut editor = FlowEditor::new();
        draw(
            &mut editor,
            CanvasTool::Ellipse,
            Vec2::ZERO,
            Vec2::new(300.0, 200.0),
        );

        assert_eq!(editor.history().undo_depth(), 1);
        assert!(editor.undo());
        assert!(!editor.can_undo(), "the drag left more than one step");
    }

    /// **§45's rule, asserted rather than argued**: activating any tool, and
    /// every effect that is not a commit, leaves the document untouched.
    #[test]
    fn activating_a_tool_changes_no_document_state() {
        let (mut editor, _, _) = editor_with_two_nodes();
        let before = editor.to_document();
        let depth = editor.history().undo_depth();

        for tool in CanvasTool::ALL {
            let mut machine = InteractionMachine::new();
            let effect = machine.handle(InteractionEvent::SelectTool(*tool));
            let report = apply_gesture(&mut editor, effect);
            assert!(!report.changed, "{} changed the document", tool.name());
        }

        assert_eq!(editor.to_document(), before);
        assert_eq!(editor.history().undo_depth(), depth);
    }

    /// An abandoned creation leaves no element and no undo step — there is
    /// nothing to reverse, because the tool never wrote anything.
    #[test]
    fn an_abandoned_creation_leaves_nothing_behind() {
        let mut editor = FlowEditor::new();
        let mut machine = InteractionMachine::new();
        machine.handle(InteractionEvent::SelectTool(CanvasTool::Rectangle));
        apply_gesture(
            &mut editor,
            machine.handle(InteractionEvent::PointerDown {
                screen: Vec2::ZERO,
                world: Vec2::ZERO,
                button: PointerButton::Left,
                modifiers: InputModifiers::default(),
                pan_key_held: false,
                target: PointerTarget::Empty,
            }),
        );
        apply_gesture(
            &mut editor,
            machine.handle(InteractionEvent::PointerMove {
                screen: Vec2::splat(90.0),
                world: Vec2::splat(90.0),
            }),
        );

        let report = apply_gesture(&mut editor, machine.handle(InteractionEvent::Cancel));

        assert!(!report.changed);
        assert_eq!(report.created, None);
        assert_eq!(editor.world().nodes().len(), 0);
        assert!(!editor.can_undo());
    }

    /// A creation selects what it just drew, and does it **without** an undo
    /// step: selection is view state, so the creation stays one press of undo.
    #[test]
    fn a_creation_selects_what_it_drew_without_recording_it() {
        let mut editor = FlowEditor::new();
        let node = draw(
            &mut editor,
            CanvasTool::Line,
            Vec2::ZERO,
            Vec2::new(200.0, 60.0),
        )
        .created
        .unwrap();

        assert!(editor.world().nodes().is_selected(node));
        assert_eq!(editor.history().undo_depth(), 1);
    }
}
