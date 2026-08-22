//! [`apply`] — **the one mutation path**, and the function that produces the
//! inverse of every edit as it makes it.
//!
//! # What "one path" buys, and what makes it hold
//!
//! An edit that changes the world without telling the history does not fail
//! then. It fails three undos later, when a delta is applied to a state that
//! no longer matches the one it was recorded against — a node ends up somewhere
//! nobody moved it, an edge points at a node that came back without it. That
//! defect has no stack trace and no reproduction, and it is the reason this
//! phase's real deliverable is the *shape* rather than the feature.
//!
//! So the shape is enforced twice, once by the type system and once here:
//!
//! - [`FlowEditor`](super::FlowEditor) owns its [`GraphWorld`] privately and
//!   **never lends `&mut` to it**. Everything that can reach a world with a
//!   history attached goes through [`FlowEditor::apply`](super::FlowEditor::apply),
//!   because there is no other reference to reach it with. `commands/editor.rs`
//!   has a unit test that reads its own source and fails if a
//!   `&mut GraphWorld` ever escapes.
//! - This function is the only caller of the world's element mutators, and it
//!   returns the inverse *from the same match arm that made the change*.
//!   Forgetting the inverse is not an omission you can commit; it is a missing
//!   return value.
//!
//! # Why the inverse is computed rather than recorded
//!
//! The alternative — snapshot the affected elements before the edit and diff
//! afterwards — is simpler to write and quietly quadratic: a drag would
//! snapshot its node sixty times a second, and a "select all and restyle" would
//! snapshot the document. Each arm below already knows exactly what it changed,
//! so it says so. The cost of an inverse is the cost of the delta.
//!
//! # Partial application, and the one place it is possible
//!
//! Every arm but [`EditCommand::Connect`] either changes nothing or cannot
//! fail. `Connect` can fail on its second edge — §4's connection limits depend
//! on the edges already made — so it **rolls back** the edges it had made
//! before returning the error. An `Err` from this function always means the
//! world is as it was.
//!
//! **This file names no UI framework.**

use crate::{
    commands::edit::{EditCommand, EditError},
    models::{EdgeIndex, ElementId, NodeIndex},
    runtime::GraphWorld,
};

/// What an edit did, beside changing the world.
#[derive(Debug, Clone, PartialEq)]
pub struct EditOutcome {
    /// The command that undoes this one. Applying it returns the command that
    /// redoes it, which is how [`super::history`] walks in both directions with
    /// one applier.
    pub inverse: EditCommand,
    /// Nodes this edit created, in the order they were drafted. Empty for
    /// everything but [`EditCommand::AddNodes`] — a restore does not create.
    pub added_nodes: Vec<NodeIndex>,
    /// Edges this edit created.
    pub added_edges: Vec<EdgeIndex>,
    /// Whether anything actually changed. A move of zero, a style set to the
    /// style it already had, a removal of something already removed: all
    /// legitimate calls, none of them an undo step.
    pub changed: bool,
}

impl EditOutcome {
    /// The answer for a command that turned out to change nothing. Its inverse
    /// is an empty removal, which is the cheapest command that is also a no-op
    /// — so a caller that records it anyway is harmless rather than wrong.
    fn unchanged() -> EditOutcome {
        EditOutcome {
            inverse: EditCommand::remove(Vec::new(), Vec::new()),
            added_nodes: Vec::new(),
            added_edges: Vec::new(),
            changed: false,
        }
    }

    fn from_inverse(inverse: EditCommand) -> EditOutcome {
        EditOutcome {
            changed: !inverse.is_trivially_empty(),
            inverse,
            added_nodes: Vec::new(),
            added_edges: Vec::new(),
        }
    }
}

/// **Applies one delta and returns the delta that undoes it.**
///
/// See the module doc for why this is the only function that mutates a world
/// under a history, and why the inverse comes back rather than being recorded
/// separately.
pub fn apply(world: &mut GraphWorld, command: EditCommand) -> Result<EditOutcome, EditError> {
    if command.is_trivially_empty() {
        return Ok(EditOutcome::unchanged());
    }

    match command {
        EditCommand::AddNodes(drafts) => {
            let mut added = Vec::with_capacity(drafts.len());
            for draft in drafts {
                let mut spec = draft.spec;
                if spec.id == ElementId::NONE {
                    spec.id = world.next_id();
                }
                let node = world.add_node(spec);
                for handle in draft.handles {
                    world.add_handle(node, handle);
                }
                added.push(node);
            }

            Ok(EditOutcome {
                inverse: EditCommand::remove(added.clone(), Vec::new()),
                added_nodes: added,
                added_edges: Vec::new(),
                changed: true,
            })
        }

        EditCommand::Connect(specs) => {
            let mut added = Vec::with_capacity(specs.len());
            for mut spec in specs {
                if spec.id == ElementId::NONE {
                    spec.id = world.next_id();
                }
                match world.connect_with(spec) {
                    Ok(edge) => added.push(edge),
                    Err(error) => {
                        // The rollback the module doc promises. Removing an
                        // edge we made a moment ago is exact: nothing else has
                        // touched it, so its tombstone restores the world to
                        // the state this call found.
                        for edge in added {
                            world.remove_edge(edge);
                        }
                        return Err(error.into());
                    }
                }
            }

            Ok(EditOutcome {
                inverse: EditCommand::disconnect(added.clone()),
                added_nodes: Vec::new(),
                added_edges: added,
                changed: true,
            })
        }

        EditCommand::SetPresence {
            nodes,
            edges,
            present,
        } => Ok(EditOutcome::from_inverse(set_presence(
            world, &nodes, &edges, present,
        ))),

        EditCommand::MoveNodes { nodes, delta } => {
            // **The inverse is where they were, not the opposite translation.**
            // `EditCommand::SetNodePositions` carries the arithmetic; the short
            // version is that sixty rounded additions and one rounded
            // subtraction do not meet, and this phase's own property test is
            // what notices.
            //
            // It names only the nodes that actually moved, so that a drag emits
            // the same node list on every mouse move and its entries coalesce.
            let mut before = Vec::with_capacity(nodes.len());
            for node in nodes {
                if !world.node_is_live(node) {
                    continue;
                }
                let was = world.nodes().position(node);
                world.move_node(node, delta);
                before.push((node, was));
            }

            Ok(EditOutcome::from_inverse(EditCommand::SetNodePositions(
                before,
            )))
        }

        EditCommand::SetNodePositions(items) => {
            let mut before = Vec::with_capacity(items.len());
            for (node, position) in items {
                if !world.node_is_live(node) {
                    continue;
                }
                let was = world.nodes().position(node);
                if was == position {
                    continue;
                }
                world.set_node_position(node, position);
                before.push((node, was));
            }

            Ok(EditOutcome::from_inverse(EditCommand::SetNodePositions(
                before,
            )))
        }

        EditCommand::ResizeNodes(items) => {
            let mut before = Vec::with_capacity(items.len());
            for (node, size) in items {
                if !world.node_is_live(node) {
                    continue;
                }
                let was = world.nodes().size(node);
                if was == size {
                    continue;
                }
                world.set_node_size(node, size);
                before.push((node, was));
            }

            Ok(EditOutcome::from_inverse(EditCommand::ResizeNodes(before)))
        }

        EditCommand::SetNodeConnectors(items) => {
            let mut before = Vec::with_capacity(items.len());
            for (node, connector) in items {
                if !world.node_is_live(node) {
                    continue;
                }
                let Some(was) = world.nodes().connector(node) else {
                    continue;
                };
                if was == connector {
                    continue;
                }
                world.set_node_connector(node, connector);
                before.push((node, was));
            }

            Ok(EditOutcome::from_inverse(EditCommand::SetNodeConnectors(
                before,
            )))
        }

        EditCommand::SetNodeStyles(items) => {
            let mut before = Vec::with_capacity(items.len());
            for (node, style) in items {
                if !world.node_is_live(node) {
                    continue;
                }
                let was = world.nodes().style(node).clone();
                if was == style {
                    continue;
                }
                world.set_node_style(node, style);
                before.push((node, was));
            }

            Ok(EditOutcome::from_inverse(EditCommand::SetNodeStyles(
                before,
            )))
        }

        EditCommand::SetEdgeStyles(items) => {
            let mut before = Vec::with_capacity(items.len());
            for (edge, style) in items {
                if !world.edge_is_live(edge) {
                    continue;
                }
                let was = world.edges().style(edge).clone();
                if was == style {
                    continue;
                }
                world.set_edge_style(edge, style);
                before.push((edge, was));
            }

            Ok(EditOutcome::from_inverse(EditCommand::SetEdgeStyles(
                before,
            )))
        }

        EditCommand::SetEdgeRouting(items) => {
            let mut before = Vec::with_capacity(items.len());
            for (edge, routing) in items {
                if !world.edge_is_live(edge) {
                    continue;
                }
                let was = world.edges().routing(edge);
                if was == routing {
                    continue;
                }
                world.set_edge_routing(edge, routing);
                before.push((edge, was));
            }

            Ok(EditOutcome::from_inverse(EditCommand::SetEdgeRouting(
                before,
            )))
        }

        EditCommand::SetNodeLabels(items) => {
            let mut before = Vec::with_capacity(items.len());
            for (node, label) in items {
                if !world.node_is_live(node) {
                    continue;
                }
                let was = world.nodes().cold(node).label.as_deref().map(str::to_owned);
                if was.as_deref() == label.as_deref() {
                    continue;
                }
                world.set_node_label(node, label);
                before.push((node, was));
            }

            Ok(EditOutcome::from_inverse(EditCommand::SetNodeLabels(
                before,
            )))
        }

        // **§10's picture, and the crop with it.** The same shape as every
        // other absolute per-element write: skip what is not live, skip what
        // would not change, and record what was there. The bytes are nowhere
        // near this — a `NodeImage` is a handle and four floats, so the undo
        // stack holds crops rather than photographs.
        EditCommand::SetNodeImages(items) => {
            let mut before = Vec::with_capacity(items.len());
            for (node, image) in items {
                if !world.node_is_live(node) {
                    continue;
                }
                let was = world.nodes().cold(node).image;
                if was == image {
                    continue;
                }
                world.set_node_image(node, image);
                before.push((node, was));
            }

            Ok(EditOutcome::from_inverse(EditCommand::SetNodeImages(
                before,
            )))
        }

        EditCommand::SetEdgeLabels(items) => {
            let mut before = Vec::with_capacity(items.len());
            for (edge, label) in items {
                if !world.edge_is_live(edge) {
                    continue;
                }
                let was = world.edges().label(edge).map(|it| it.to_string());
                if was.as_deref() == label.as_deref() {
                    continue;
                }
                world.set_edge_label(edge, label);
                before.push((edge, was));
            }

            Ok(EditOutcome::from_inverse(EditCommand::SetEdgeLabels(
                before,
            )))
        }

        EditCommand::SetNodeZ(items) => {
            let mut before = Vec::with_capacity(items.len());
            for (node, z) in items {
                if !world.node_is_live(node) {
                    continue;
                }
                let was = world.nodes().z(node);
                if was == z {
                    continue;
                }
                world.set_node_z(node, z);
                before.push((node, was));
            }

            Ok(EditOutcome::from_inverse(EditCommand::SetNodeZ(before)))
        }

        EditCommand::SetEdgeZ(items) => {
            let mut before = Vec::with_capacity(items.len());
            for (edge, z) in items {
                if !world.edge_is_live(edge) {
                    continue;
                }
                let was = world.edges().z(edge);
                if was == z {
                    continue;
                }
                world.set_edge_z(edge, z);
                before.push((edge, was));
            }

            Ok(EditOutcome::from_inverse(EditCommand::SetEdgeZ(before)))
        }

        EditCommand::SetNodeLinks(items) => {
            let mut before = Vec::with_capacity(items.len());
            for (node, link) in items {
                if !world.node_is_live(node) {
                    continue;
                }
                let was = world.nodes().cold(node).link.clone();
                if was == link {
                    continue;
                }
                world.set_node_link(node, link);
                before.push((node, was));
            }

            Ok(EditOutcome::from_inverse(EditCommand::SetNodeLinks(before)))
        }

        EditCommand::SetEdgeLinks(items) => {
            let mut before = Vec::with_capacity(items.len());
            for (edge, link) in items {
                if !world.edge_is_live(edge) {
                    continue;
                }
                let was = world.edges().link(edge).map(str::to_owned);
                if was == link {
                    continue;
                }
                world.set_edge_link(edge, link);
                before.push((edge, was));
            }

            Ok(EditOutcome::from_inverse(EditCommand::SetEdgeLinks(before)))
        }

        // The one arm that names no element, and therefore the one that has no
        // liveness check to make: a document setting is always there. Reading
        // the old value out on the way past is what makes the inverse a
        // `SetRenderStyle` of what was, so undo and redo are the same arm.
        EditCommand::SetRenderStyle(style) => {
            let was = world.settings().render_style;
            if was == style {
                return Ok(EditOutcome::unchanged());
            }
            world.settings_mut().render_style = style;

            Ok(EditOutcome::from_inverse(EditCommand::SetRenderStyle(was)))
        }
    }
}

/// Removal and its undo, in one function because they are one operation with a
/// sign.
///
/// **Order matters in both directions and it is not the same order.** Going
/// out, the explicitly named edges go first so they are recorded as themselves
/// rather than as part of a node's cascade; coming back, the nodes go first,
/// because an edge cannot be restored onto a node that is not there yet.
///
/// The returned command names exactly what flipped — never what was asked for —
/// so restoring cannot resurrect an edge that was already deleted before its
/// node was, and removing twice is a no-op rather than a double entry.
fn set_presence(
    world: &mut GraphWorld,
    nodes: &[NodeIndex],
    edges: &[EdgeIndex],
    present: bool,
) -> EditCommand {
    let mut flipped_nodes = Vec::new();
    let mut flipped_edges = Vec::new();

    if present {
        for &node in nodes {
            if world.restore_node(node) {
                flipped_nodes.push(node);
            }
        }
        for &edge in edges {
            if world.restore_edge(edge) {
                flipped_edges.push(edge);
            }
        }
    } else {
        for &edge in edges {
            if world.remove_edge(edge) {
                flipped_edges.push(edge);
            }
        }
        for &node in nodes {
            // The cascade appends the node's own live edges, so the inverse
            // names them and a restore brings the neighbourhood back with it.
            if world.remove_node(node, &mut flipped_edges) {
                flipped_nodes.push(node);
            }
        }
    }

    EditCommand::SetPresence {
        nodes: flipped_nodes,
        edges: flipped_edges,
        present: !present,
    }
}

#[cfg(test)]
mod tests {
    use super::{EditOutcome, apply};
    use crate::{
        commands::edit::{EditCommand, EditError, NodeDraft},
        geometry::Vec2,
        models::{ElementId, ElementKind, ElementStyle, GraphNodeKind, ShapeKind},
        runtime::{ConnectionError, ConnectionRules, EdgeEnd, GraphWorld, NodeSpec},
    };

    fn world_with_two_connected_nodes() -> GraphWorld {
        let mut world = GraphWorld::new();
        world.set_rules(ConnectionRules::PERMISSIVE);
        let a = world.create_node(
            ElementKind::GraphNode(GraphNodeKind::Default),
            Vec2::new(0.0, 0.0),
            Vec2::new(100.0, 60.0),
        );
        let b = world.create_node(
            ElementKind::GraphNode(GraphNodeKind::Default),
            Vec2::new(300.0, 0.0),
            Vec2::new(100.0, 60.0),
        );
        world
            .connect(EdgeEnd::node(a), EdgeEnd::node(b))
            .expect("permissive rules connect two graph nodes");
        world.rebuild_all_geometry();
        world
    }

    fn draft(x: f32) -> NodeDraft {
        NodeDraft::new(NodeSpec::new(
            ElementId::NONE,
            ElementKind::Shape(ShapeKind::Rectangle),
            Vec2::new(x, 0.0),
            Vec2::new(40.0, 40.0),
        ))
    }

    /// A document reduced to what an undo is answerable for.
    ///
    /// **The id allocator's watermark is deliberately not part of it.** Adding
    /// a node consumes an id; undoing the add gives back the node's slot but
    /// not its id, and it must not — the ids on the undo stack, on the
    /// clipboard and in a half-written save all still name it, and reissuing
    /// one would make two elements share a name. So the watermark only ever
    /// goes up, a redo reuses the id it already had, and `to_document` is
    /// compared for its elements rather than for its counter.
    fn elements(
        document: &crate::models::FlowDocument,
    ) -> (Vec<crate::models::FlowNode>, Vec<crate::models::FlowEdge>) {
        (document.nodes.clone(), document.edges.clone())
    }

    /// The contract the whole history rests on: what comes back undoes what
    /// went in, for every variant that can run today.
    #[test]
    fn every_command_returns_a_delta_that_puts_the_document_back() {
        /// One case: a command built against the fixture world, since a few
        /// of them need to read what is already there.
        type Case = Box<dyn Fn(&GraphWorld) -> EditCommand>;

        let cases: Vec<Case> = vec![
            Box::new(|_| EditCommand::AddNodes(vec![draft(500.0)])),
            Box::new(|_| {
                EditCommand::move_node(crate::models::NodeIndex::new(0), Vec2::new(7.0, -3.0))
            }),
            Box::new(|_| {
                EditCommand::resize_node(crate::models::NodeIndex::new(0), Vec2::new(11.0, 13.0))
            }),
            Box::new(|_| {
                let mut style = ElementStyle::default();
                style.stroke.width = 9.0;
                EditCommand::style_node(crate::models::NodeIndex::new(0), style)
            }),
            Box::new(|_| {
                EditCommand::style_edge(
                    crate::models::EdgeIndex::new(0),
                    ElementStyle {
                        opacity: 0.25,
                        ..ElementStyle::default()
                    },
                )
            }),
            Box::new(|_| {
                EditCommand::SetEdgeRouting(vec![(
                    crate::models::EdgeIndex::new(0),
                    crate::models::EdgeRouting::Step,
                )])
            }),
            Box::new(|_| {
                EditCommand::label_node(crate::models::NodeIndex::new(0), Some("hello".into()))
            }),
            Box::new(|_| {
                EditCommand::SetEdgeLabels(vec![(
                    crate::models::EdgeIndex::new(0),
                    Some("edge".into()),
                )])
            }),
            Box::new(|_| EditCommand::remove(vec![crate::models::NodeIndex::new(0)], Vec::new())),
            Box::new(|_| EditCommand::disconnect(vec![crate::models::EdgeIndex::new(0)])),
            Box::new(|world| {
                EditCommand::Connect(vec![
                    crate::runtime::EdgeSpec::new(
                        ElementId::NONE,
                        EdgeEnd::node(crate::models::NodeIndex::new(1)),
                        EdgeEnd::node(crate::models::NodeIndex::new(0)),
                    )
                    .with_routing(world.edges().routing(crate::models::EdgeIndex::new(0))),
                ])
            }),
        ];

        for build in cases {
            let mut world = world_with_two_connected_nodes();
            let before = elements(&world.to_document());

            let command = build(&world);
            let name = command.kind();
            let outcome = apply(&mut world, command).expect("the case is applicable");
            assert!(outcome.changed, "{name} claimed to change nothing");
            assert_ne!(
                elements(&world.to_document()),
                before,
                "{name} changed nothing"
            );

            apply(&mut world, outcome.inverse).expect("the inverse is applicable");
            assert_eq!(
                elements(&world.to_document()),
                before,
                "{name} did not round-trip"
            );
        }
    }

    /// Applying the inverse of the inverse must reproduce the original edit —
    /// this is what makes redo `apply` of what undo returned rather than a
    /// third code path.
    #[test]
    fn the_inverse_of_an_inverse_redoes_the_edit() {
        let mut world = world_with_two_connected_nodes();
        let after_edit = {
            let mut probe = world_with_two_connected_nodes();
            apply(
                &mut probe,
                EditCommand::remove(vec![crate::models::NodeIndex::new(0)], Vec::new()),
            )
            .unwrap();
            elements(&probe.to_document())
        };

        let undo = apply(
            &mut world,
            EditCommand::remove(vec![crate::models::NodeIndex::new(0)], Vec::new()),
        )
        .unwrap()
        .inverse;
        let redo = apply(&mut world, undo).unwrap().inverse;
        apply(&mut world, redo).unwrap();

        assert_eq!(elements(&world.to_document()), after_edit);
    }

    /// Removing a node takes its edges, and restoring it brings back exactly
    /// those — recorded rather than recomputed, so that an edge deleted first
    /// stays deleted.
    #[test]
    fn a_removed_node_takes_its_edges_and_gives_them_back() {
        let mut world = world_with_two_connected_nodes();
        let node = crate::models::NodeIndex::new(0);
        let edge = crate::models::EdgeIndex::new(0);

        let inverse = apply(&mut world, EditCommand::remove(vec![node], Vec::new()))
            .unwrap()
            .inverse;

        assert!(!world.node_is_live(node));
        assert!(
            !world.edge_is_live(edge),
            "the edge did not follow its node"
        );
        assert_eq!(inverse, EditCommand::restore(vec![node], vec![edge]));

        apply(&mut world, inverse).unwrap();
        assert!(world.node_is_live(node) && world.edge_is_live(edge));
    }

    /// The case the recorded cascade exists for: delete the edge, then the
    /// node, then undo the node. The edge must stay deleted, because the author
    /// deleted it separately and never asked for it back.
    #[test]
    fn restoring_a_node_does_not_resurrect_an_edge_deleted_before_it() {
        let mut world = world_with_two_connected_nodes();
        let node = crate::models::NodeIndex::new(0);
        let edge = crate::models::EdgeIndex::new(0);

        apply(&mut world, EditCommand::disconnect(vec![edge])).unwrap();
        let undo_removal = apply(&mut world, EditCommand::remove(vec![node], Vec::new()))
            .unwrap()
            .inverse;

        assert_eq!(
            undo_removal,
            EditCommand::restore(vec![node], Vec::new()),
            "the cascade claimed an edge that was already gone"
        );

        apply(&mut world, undo_removal).unwrap();
        assert!(world.node_is_live(node));
        assert!(!world.edge_is_live(edge));
    }

    /// A no-op is not an undo step. Every arm has to be able to say so, or a
    /// user pressing undo would get a keystroke that does nothing.
    #[test]
    fn a_command_that_changes_nothing_reports_it() {
        let mut world = world_with_two_connected_nodes();
        let node = crate::models::NodeIndex::new(0);
        let size = world.nodes().size(node);

        for command in [
            EditCommand::move_node(node, Vec2::ZERO),
            EditCommand::resize_node(node, size),
            EditCommand::style_node(node, world.nodes().style(node).clone()),
            EditCommand::label_node(node, None),
            EditCommand::remove(Vec::new(), Vec::new()),
            // Already live: restoring it flips nothing.
            EditCommand::restore(vec![node], Vec::new()),
            // Not there at all: a stale index from a history entry recorded
            // against a different document must not panic.
            EditCommand::move_node(crate::models::NodeIndex::new(99), Vec2::new(1.0, 1.0)),
        ] {
            let name = command.kind();
            let outcome = apply(&mut world, command).expect("a no-op is not an error");
            assert!(!outcome.changed, "{name} claimed a change it did not make");
        }
    }

    /// The one arm that can fail part-way must leave nothing behind.
    #[test]
    fn a_refused_connection_rolls_back_the_edges_it_had_already_made() {
        let mut world = world_with_two_connected_nodes();
        world.set_rules(ConnectionRules {
            allow_self_connections: false,
            ..ConnectionRules::PERMISSIVE
        });
        let before = elements(&world.to_document());
        let a = crate::models::NodeIndex::new(0);
        let b = crate::models::NodeIndex::new(1);

        let error = apply(
            &mut world,
            EditCommand::Connect(vec![
                crate::runtime::EdgeSpec::new(ElementId::NONE, EdgeEnd::node(b), EdgeEnd::node(a)),
                crate::runtime::EdgeSpec::new(ElementId::NONE, EdgeEnd::node(a), EdgeEnd::node(a)),
            ]),
        )
        .expect_err("a self-connection is refused");

        assert_eq!(
            error,
            EditError::Connection(ConnectionError::SelfConnection(a))
        );
        assert_eq!(
            elements(&world.to_document()),
            before,
            "the first edge survived a failed batch"
        );
    }

    /// `AddNodes` reports the indices it created, because the caller that
    /// created them is the one that wants to select or drag them next.
    #[test]
    fn adding_reports_what_it_created_and_allocates_the_ids() {
        let mut world = GraphWorld::new();
        let outcome = apply(
            &mut world,
            EditCommand::AddNodes(vec![draft(0.0), draft(100.0)]),
        )
        .unwrap();

        assert_eq!(outcome.added_nodes.len(), 2);
        assert_eq!(outcome.added_edges, Vec::new());
        for node in &outcome.added_nodes {
            assert_ne!(world.nodes().id(*node), ElementId::NONE);
        }
        assert_ne!(
            world.nodes().id(outcome.added_nodes[0]),
            world.nodes().id(outcome.added_nodes[1]),
            "two drafts were given the same id"
        );
    }

    #[test]
    fn an_unchanged_outcome_carries_an_inverse_that_is_itself_a_no_op() {
        let outcome = EditOutcome::unchanged();
        assert!(!outcome.changed);
        assert!(outcome.inverse.is_trivially_empty());
    }
}
