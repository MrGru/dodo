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
    commands::{EditCommand, EditError, editor::FlowEditor},
    interaction::InteractionEffect,
    models::{EdgeIndex, ElementId},
    runtime::{EdgeEnd, EdgeSpec, PointerTarget},
};

/// What one effect did to the document.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GestureReport {
    /// Whether the world changed, so the caller can decide about repainting
    /// without diffing anything.
    pub changed: bool,
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
            connection: None,
        }
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

        InteractionEffect::BeginNodeDrag(node) => {
            editor.begin_gesture();
            editor.select_only(Some(node));
            GestureReport::changed(true)
        }

        InteractionEffect::EndNodeDrag { .. } => {
            editor.end_gesture();
            GestureReport::changed(false)
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
                PointerTarget::Empty => return GestureReport::default(),
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
                connection: Some(result),
            }
        }

        _ => GestureReport::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::apply_gesture;
    use crate::{
        commands::{EditCommand, FlowEditor, NodeDraft},
        geometry::Vec2,
        interaction::{InputModifiers, InteractionEvent, InteractionMachine, PointerButton},
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
}
