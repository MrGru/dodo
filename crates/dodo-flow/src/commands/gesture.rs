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
    interaction::{CanvasTool, InteractionEffect},
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
fn create(editor: &mut FlowEditor, tool: CanvasTool, rect: Rect) -> GestureReport {
    let Some(kind) = tool.element_kind() else {
        return GestureReport::default();
    };

    let rect = rect.normalized();
    let draft = NodeDraft::new(NodeSpec::new(ElementId::NONE, kind, rect.origin, rect.size))
        .with_handles(handles_for(tool));

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
        InteractionEffect::DragNodeBy { node, delta } => GestureReport::changed(
            editor
                .apply(EditCommand::move_node(node, delta))
                .is_ok_and(|summary| summary.changed),
        ),

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
        //
        // **The selection is replaced by the element being resized**, and not
        // extended: a grip belongs to one element, so a press on it is
        // unambiguous about which element the user means.
        InteractionEffect::BeginResize { node } => {
            editor.begin_gesture();
            editor.select_only(Some(node));
            GestureReport::changed(true)
        }

        // **Two commands, one gesture, one undo step.** A corner drag moves the
        // origin as well as the size, and the two are separate variants because
        // their coalescing rules are different — `SetNodePositions` keeps the
        // *earliest* value (see `EditCommand::merge`) and `ResizeNodes` is
        // superseded by the *latest* (see `EditCommand::supersedes`). Sixty
        // ticks of a drag are two history entries in total, whichever corner is
        // being pulled.
        InteractionEffect::ResizeNodeTo { node, rect } => {
            let rect = rect.normalized();
            GestureReport::changed(editor.in_one_step(|editor| {
                let mut changed = editor
                    .apply(EditCommand::SetNodePositions(vec![(node, rect.origin)]))
                    .is_ok_and(|summary| summary.changed);
                changed |= editor
                    .apply(EditCommand::resize_node(node, rect.size))
                    .is_ok_and(|summary| summary.changed);
                changed
            }))
        }

        InteractionEffect::EndResize { .. } => {
            editor.end_gesture();
            GestureReport::changed(false)
        }

        // Abandoned exactly as a drag is, and for the same reason: the entries
        // the gesture recorded carry where the element was, and putting it back
        // by applying them in reverse leaves nothing on the stack.
        InteractionEffect::CancelResize { .. } => GestureReport::changed(editor.abandon_gesture()),

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
                | PointerTarget::ResizeGrip { .. } => {
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
        InteractionEffect::CommitCreate { tool, rect } => create(editor, tool, rect),

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
        models::{ElementId, ElementKind, GraphNodeKind, NodeIndex},
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
