//! [`EditCommand`] — §30's delta, and **the only vocabulary in which the
//! document changes**.
//!
//! # Why the enum is both directions
//!
//! §30 sketches one enum of commands and says to undo by delta rather than by
//! cloning the document. The obvious next step is a second enum of inverses —
//! and it is the wrong one. Every inverse of an edit is itself an edit: undoing
//! a move is a move, undoing a removal is a restore, undoing a style change is
//! another style change. So [`apply`](mod@super::apply) takes an `EditCommand` and
//! **returns the `EditCommand` that undoes it**, undo is `apply` of that, and
//! redo is `apply` of what undo returned. One enum, one applier, and no second
//! path that can drift from the first.
//!
//! That is also why the removal variant is [`EditCommand::SetPresence`] with a
//! `present` flag rather than a `Remove` and a `Restore`: the two are each
//! other's inverse, they run the same code, and a bug in one is a bug in both
//! rather than a mismatch between them.
//!
//! # What is here and what is honestly missing
//!
//! §30's list is `AddElements`, `RemoveElements`, `MoveElements`,
//! `ResizeElements`, `RotateElements`, `UpdateStyle`, `Connect`, `Disconnect`,
//! `EditText`, `Group`, `Ungroup`. Everything on it that the engine can perform
//! today is here. Three are **not**, and are left out rather than stubbed:
//!
//! - **`Rotate`** — nothing in the engine has an angle. A node is a
//!   [`Rect`](crate::geometry::Rect), the hit test is a rectangle containment,
//!   the spatial bounds are axis-aligned, and rotation is on the plan's
//!   deferred list. A `RotateElements` variant would be a variant nobody could
//!   apply.
//! - **`Group` / `Ungroup`** — [`NodeCold::parent`](crate::runtime::NodeCold)
//!   records the relationship and nothing resolves it; §11's hierarchy is a
//!   later cycle. Groups as a user-facing feature are explicitly deferred.
//!
//! Adding either later is a variant and a match arm. Adding either **now**
//! would be a command that exists so the enum looks complete, which is the
//! failure mode the phase brief names.
//!
//! # Deltas, not snapshots
//!
//! Every variant carries only what it touched. A drag of one node is a
//! `Vec<NodeIndex>` of length one and a [`Vec2`] — sixteen bytes of payload —
//! and the coalescing in [`super::history`] keeps it at one entry however many
//! mouse moves the drag emits. The variants that cannot be inverted from the
//! delta alone ([`SetNodeStyles`](EditCommand::SetNodeStyles) and its
//! siblings) carry the *new* value, and the applier reads the old one out of
//! the store on its way past — so the history stores one style per styled
//! element, never a document.
//!
//! **This file names no UI framework.**

use crate::{
    geometry::Vec2,
    models::{EdgeIndex, EdgeRouting, ElementStyle, NodeIndex},
    runtime::{ConnectionError, EdgeSpec, HandleSpec, NodeSpec},
};

/// A node to be created, with the handles it is born with.
///
/// [`NodeSpec::id`] may be [`ElementId::NONE`](crate::models::ElementId::NONE),
/// in which case the applier allocates one from the world's allocator. That is
/// the normal case: a caller creating a node has no business knowing the id
/// space.
#[derive(Debug, Clone, PartialEq)]
pub struct NodeDraft {
    pub spec: NodeSpec,
    pub handles: Vec<HandleSpec>,
}

impl NodeDraft {
    pub fn new(spec: NodeSpec) -> NodeDraft {
        NodeDraft {
            spec,
            handles: Vec::new(),
        }
    }

    pub fn with_handles(mut self, handles: Vec<HandleSpec>) -> NodeDraft {
        self.handles = handles;
        self
    }
}

/// **§30's delta.** See the module doc for why one enum covers both directions.
#[derive(Debug, Clone, PartialEq)]
pub enum EditCommand {
    /// §30's `AddElements`, for nodes. Its inverse is a
    /// [`SetPresence`](EditCommand::SetPresence) over the indices it created —
    /// **not** another `AddNodes`, so that a redo puts the node back at its own
    /// index instead of allocating a new one beside it.
    AddNodes(Vec<NodeDraft>),

    /// §30's `Connect`. Same story: its inverse names indices.
    Connect(Vec<EdgeSpec>),

    /// §30's `RemoveElements` and `Disconnect` when `present` is false, and
    /// their undo when it is true.
    ///
    /// Removing a node takes its live edges with it — the applier records the
    /// ones it actually took, so restoring puts back exactly those and not an
    /// edge the author had already deleted.
    SetPresence {
        nodes: Vec<NodeIndex>,
        edges: Vec<EdgeIndex>,
        present: bool,
    },

    /// §30's `MoveElements`, as a translation — what a drag emits, sixty times
    /// a second, and the only variant whose payload does not grow with the
    /// number of nodes it moves.
    ///
    /// **Its inverse is [`SetNodePositions`](EditCommand::SetNodePositions),
    /// not a negated delta.** See that variant for the arithmetic that decided
    /// it.
    MoveNodes { nodes: Vec<NodeIndex>, delta: Vec2 },

    /// Absolute positions, and **the reason a coalesced drag can be undone
    /// exactly**.
    ///
    /// The obvious inverse of a translation is the opposite translation, and in
    /// `f32` it is not one. A drag of sixty two-unit steps and one hundred-and-
    /// twenty-unit step back do not land on the same number: sixty additions
    /// round sixty times, the subtraction rounds once, and the node comes home
    /// two ten-millionths out. That is invisible on screen and **not** invisible
    /// to this phase's own contract — the node's painted bounds differ, so its
    /// spatial cell may differ, so the frame after the undo is not the frame
    /// before the edit, and the property test says so.
    ///
    /// So the applier records where the nodes *were*, and the undo puts them
    /// back rather than moving them back. The cost is one [`Vec2`] per moved
    /// node instead of one per command — still a delta over the elements the
    /// edit touched, which is what §30 asks for, and still one entry per drag
    /// however long the drag is.
    SetNodePositions(Vec<(NodeIndex, Vec2)>),

    /// §30's `ResizeElements`, as the size each node should end up with.
    ResizeNodes(Vec<(NodeIndex, Vec2)>),

    /// §30's `UpdateStyle`, for nodes.
    SetNodeStyles(Vec<(NodeIndex, ElementStyle)>),

    /// §30's `UpdateStyle`, for edges.
    SetEdgeStyles(Vec<(EdgeIndex, ElementStyle)>),

    /// §8's routing choice. Not on §30's list by name, but it is document data
    /// that a user changes, so it is an edit and it is undoable.
    SetEdgeRouting(Vec<(EdgeIndex, EdgeRouting)>),

    /// §30's `EditText`, as far as the engine has text: a node's label.
    SetNodeLabels(Vec<(NodeIndex, Option<String>)>),

    /// §30's `EditText`, for an edge's label.
    SetEdgeLabels(Vec<(EdgeIndex, Option<String>)>),

    /// **§32's z, as the property panel's Layers row writes it.**
    ///
    /// Not on §30's list under any name, and it is an edit for the same reason
    /// [`SetEdgeRouting`](EditCommand::SetEdgeRouting) is: it is document data a
    /// user changes, it survives a save, and it has to survive an undo.
    /// Absolute rather than a nudge, so the inverse is the same command with the
    /// old depths — a "send backward" applied twice and undone once must not
    /// leave the element one step out.
    SetNodeZ(Vec<(NodeIndex, i32)>),

    /// The same, for edges.
    SetEdgeZ(Vec<(EdgeIndex, i32)>),

    /// **A hyperlink on an element.** `None` clears it.
    SetNodeLinks(Vec<(NodeIndex, Option<String>)>),

    /// The same, for edges.
    SetEdgeLinks(Vec<(EdgeIndex, Option<String>)>),
}

impl EditCommand {
    /// Removes nodes (and their live edges) and edges.
    pub fn remove(nodes: Vec<NodeIndex>, edges: Vec<EdgeIndex>) -> EditCommand {
        EditCommand::SetPresence {
            nodes,
            edges,
            present: false,
        }
    }

    /// §30's `Disconnect`.
    pub fn disconnect(edges: Vec<EdgeIndex>) -> EditCommand {
        EditCommand::remove(Vec::new(), edges)
    }

    /// Puts removed elements back at their own indices.
    pub fn restore(nodes: Vec<NodeIndex>, edges: Vec<EdgeIndex>) -> EditCommand {
        EditCommand::SetPresence {
            nodes,
            edges,
            present: true,
        }
    }

    pub fn move_node(node: NodeIndex, delta: Vec2) -> EditCommand {
        EditCommand::MoveNodes {
            nodes: vec![node],
            delta,
        }
    }

    pub fn resize_node(node: NodeIndex, size: Vec2) -> EditCommand {
        EditCommand::ResizeNodes(vec![(node, size)])
    }

    pub fn style_node(node: NodeIndex, style: ElementStyle) -> EditCommand {
        EditCommand::SetNodeStyles(vec![(node, style)])
    }

    pub fn style_edge(edge: EdgeIndex, style: ElementStyle) -> EditCommand {
        EditCommand::SetEdgeStyles(vec![(edge, style)])
    }

    pub fn label_node(node: NodeIndex, label: Option<String>) -> EditCommand {
        EditCommand::SetNodeLabels(vec![(node, label)])
    }

    pub fn depth_node(node: NodeIndex, z: i32) -> EditCommand {
        EditCommand::SetNodeZ(vec![(node, z)])
    }

    pub fn link_node(node: NodeIndex, link: Option<String>) -> EditCommand {
        EditCommand::SetNodeLinks(vec![(node, link)])
    }

    /// A short, stable name for this kind of edit. For tests and for a debug
    /// overlay; **not** a user-facing string — nothing here goes through
    /// `dodo-i18n`, and nothing here reaches a user until Phase 8 gives it a
    /// translated one.
    pub fn kind(&self) -> &'static str {
        match self {
            EditCommand::AddNodes(_) => "add-nodes",
            EditCommand::Connect(_) => "connect",
            EditCommand::SetPresence { present: false, .. } => "remove",
            EditCommand::SetPresence { present: true, .. } => "restore",
            EditCommand::MoveNodes { .. } => "move-nodes",
            EditCommand::SetNodePositions(_) => "place-nodes",
            EditCommand::ResizeNodes(_) => "resize-nodes",
            EditCommand::SetNodeStyles(_) => "style-nodes",
            EditCommand::SetEdgeStyles(_) => "style-edges",
            EditCommand::SetEdgeRouting(_) => "route-edges",
            EditCommand::SetNodeLabels(_) => "label-nodes",
            EditCommand::SetEdgeLabels(_) => "label-edges",
            EditCommand::SetNodeZ(_) => "depth-nodes",
            EditCommand::SetEdgeZ(_) => "depth-edges",
            EditCommand::SetNodeLinks(_) => "link-nodes",
            EditCommand::SetEdgeLinks(_) => "link-edges",
        }
    }

    /// Whether this command would change nothing at all, decided without
    /// touching the world. A cheap pre-filter; the applier decides the real
    /// answer, because a move of a node that is not there also changes nothing.
    pub fn is_trivially_empty(&self) -> bool {
        match self {
            EditCommand::AddNodes(drafts) => drafts.is_empty(),
            EditCommand::Connect(specs) => specs.is_empty(),
            EditCommand::SetPresence { nodes, edges, .. } => nodes.is_empty() && edges.is_empty(),
            EditCommand::MoveNodes { nodes, delta } => {
                nodes.is_empty() || (delta.x == 0.0 && delta.y == 0.0)
            }
            EditCommand::SetNodePositions(items) => items.is_empty(),
            EditCommand::ResizeNodes(items) => items.is_empty(),
            EditCommand::SetNodeStyles(items) => items.is_empty(),
            EditCommand::SetEdgeStyles(items) => items.is_empty(),
            EditCommand::SetEdgeRouting(items) => items.is_empty(),
            EditCommand::SetNodeLabels(items) => items.is_empty(),
            EditCommand::SetEdgeLabels(items) => items.is_empty(),
            EditCommand::SetNodeZ(items) => items.is_empty(),
            EditCommand::SetEdgeZ(items) => items.is_empty(),
            EditCommand::SetNodeLinks(items) => items.is_empty(),
            EditCommand::SetEdgeLinks(items) => items.is_empty(),
        }
    }

    /// **§30's coalescing, as a pure function.** Folds `next` into `self` and
    /// answers whether it could; on `false`, `self` is untouched.
    ///
    /// **The two halves of a coalesced drag merge differently, and they have
    /// to.** `self` is the earlier command; `next` is the one arriving.
    ///
    /// - [`MoveNodes`](EditCommand::MoveNodes) — the *redo* half — accumulates:
    ///   two translations of the same nodes are their sum.
    /// - [`SetNodePositions`](EditCommand::SetNodePositions) — the *undo* half —
    ///   **keeps the earlier positions and discards the later ones**, because
    ///   where the drag started is where undo has to put it back. Folding these
    ///   the way the deltas fold would undo one mouse move.
    ///
    /// Everything else answers `false` rather than guessing. Two style changes
    /// in one gesture stay two entries, and [`super::history`]'s gesture
    /// grouping is what still makes them one undo step — merging is how the
    /// stack stays small, grouping is how the *step* stays whole, and they are
    /// deliberately different mechanisms.
    pub fn merge(&mut self, next: &EditCommand) -> bool {
        match (self, next) {
            (
                EditCommand::MoveNodes { nodes, delta },
                EditCommand::MoveNodes {
                    nodes: next_nodes,
                    delta: next_delta,
                },
            ) if nodes == next_nodes => {
                *delta += *next_delta;
                true
            }
            (EditCommand::SetNodePositions(first), EditCommand::SetNodePositions(later))
                if same_nodes(first, later) =>
            {
                // Deliberately a no-op on the payload: the earliest recorded
                // position is the one the whole gesture undoes to.
                true
            }
            _ => false,
        }
    }
}

impl EditCommand {
    /// **The second coalescing rule, for a control that is dragged rather than
    /// clicked** — the opacity slider, and any continuous property after it.
    ///
    /// [`merge`](EditCommand::merge) cannot serve this and the reason is worth
    /// stating, because it is not obvious and it is where an hour goes.
    /// [`super::history`] folds *both* halves of an entry with `merge`, so one
    /// rule has to be right for the forward command and for its inverse. A
    /// coalesced drag wants the **latest** forward value and the **earliest**
    /// before-value, which are opposite folds of the same variant — a
    /// contradiction `MoveNodes`/`SetNodePositions` avoids only by being two
    /// variants. A style change's inverse genuinely *is* a style change, so
    /// that trick is not available.
    ///
    /// So this is a different question, asked only of the forward half:
    /// *"does `next` say the same thing as I do, about the same elements, more
    /// recently?"* When it does, the history **replaces** the forward command
    /// and leaves the inverse exactly as it was — which is the earliest
    /// before-state, which is where one undo has to land.
    ///
    /// Only absolute per-element assignments answer `true`, and only when they
    /// name the same elements in the same order. A command that *creates*,
    /// *removes* or *translates* is not idempotent and must not be replaced.
    /// [`SetNodePositions`](EditCommand::SetNodePositions) is deliberately
    /// excluded even though it qualifies:
    /// it already has a `merge` rule, `merge` is tried first, and two rules for
    /// one variant is how they drift apart.
    pub fn supersedes(&self, next: &EditCommand) -> bool {
        match (self, next) {
            (EditCommand::ResizeNodes(first), EditCommand::ResizeNodes(later)) => {
                same_keys(first, later)
            }
            (EditCommand::SetNodeStyles(first), EditCommand::SetNodeStyles(later)) => {
                same_keys(first, later)
            }
            (EditCommand::SetEdgeStyles(first), EditCommand::SetEdgeStyles(later)) => {
                same_keys(first, later)
            }
            (EditCommand::SetEdgeRouting(first), EditCommand::SetEdgeRouting(later)) => {
                same_keys(first, later)
            }
            (EditCommand::SetNodeZ(first), EditCommand::SetNodeZ(later)) => same_keys(first, later),
            (EditCommand::SetEdgeZ(first), EditCommand::SetEdgeZ(later)) => same_keys(first, later),
            (EditCommand::SetNodeLabels(first), EditCommand::SetNodeLabels(later)) => {
                same_keys(first, later)
            }
            (EditCommand::SetEdgeLabels(first), EditCommand::SetEdgeLabels(later)) => {
                same_keys(first, later)
            }
            (EditCommand::SetNodeLinks(first), EditCommand::SetNodeLinks(later)) => {
                same_keys(first, later)
            }
            (EditCommand::SetEdgeLinks(first), EditCommand::SetEdgeLinks(later)) => {
                same_keys(first, later)
            }
            _ => false,
        }
    }
}

/// Whether two per-element payloads name the same elements in the same order.
/// Only the keys are compared — the values are exactly what differs.
fn same_keys<K: PartialEq, V>(first: &[(K, V)], later: &[(K, V)]) -> bool {
    !first.is_empty()
        && first.len() == later.len()
        && first
            .iter()
            .zip(later)
            .all(|((key, _), (other, _))| key == other)
}

/// Whether two positional payloads name the same nodes in the same order.
/// Only the indices are compared — the positions are exactly what differs.
fn same_nodes(first: &[(NodeIndex, Vec2)], later: &[(NodeIndex, Vec2)]) -> bool {
    first.len() == later.len()
        && first
            .iter()
            .zip(later)
            .all(|((node, _), (other, _))| node == other)
}

/// Why an edit could not be applied.
///
/// Returned rather than logged — dodo installs no logger — and returned as a
/// value rather than a panic, because a refused connection is an ordinary
/// answer to an ordinary gesture (§4's rules exist to refuse things).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditError {
    /// A command named a node index that is not there, or was deleted.
    UnknownNode(NodeIndex),
    /// A command named an edge index that is not there, or was deleted.
    UnknownEdge(EdgeIndex),
    /// §4's rules refused a connection. The world is unchanged: a `Connect`
    /// that fails part-way rolls back the edges it had already made.
    Connection(ConnectionError),
}

impl From<ConnectionError> for EditError {
    fn from(error: ConnectionError) -> EditError {
        EditError::Connection(error)
    }
}

impl std::fmt::Display for EditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EditError::UnknownNode(node) => write!(f, "no such node: {node}"),
            EditError::UnknownEdge(edge) => write!(f, "no such edge: {edge}"),
            EditError::Connection(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for EditError {}

#[cfg(test)]
mod tests {
    use super::{EditCommand, NodeDraft};
    use crate::{
        geometry::Vec2,
        models::{ElementId, ElementKind, NodeIndex, ShapeKind},
        runtime::NodeSpec,
    };

    fn draft() -> NodeDraft {
        NodeDraft::new(NodeSpec::new(
            ElementId::NONE,
            ElementKind::Shape(ShapeKind::Rectangle),
            Vec2::new(0.0, 0.0),
            Vec2::new(10.0, 10.0),
        ))
    }

    #[test]
    fn two_moves_of_the_same_nodes_coalesce_by_summing_their_deltas() {
        let mut first = EditCommand::move_node(NodeIndex::new(0), Vec2::new(3.0, 4.0));
        let second = EditCommand::move_node(NodeIndex::new(0), Vec2::new(1.0, -2.0));

        assert!(first.merge(&second));
        assert_eq!(
            first,
            EditCommand::move_node(NodeIndex::new(0), Vec2::new(4.0, 2.0))
        );
    }

    /// **The undo half of a coalesced drag keeps the earliest position.**
    /// Folding it the way the deltas fold would leave one undo press putting
    /// the node back one mouse move, which is the bug this variant exists to
    /// prevent.
    #[test]
    fn coalescing_positions_keeps_where_the_gesture_started() {
        let node = NodeIndex::new(0);
        let mut first = EditCommand::SetNodePositions(vec![(node, Vec2::new(10.0, 10.0))]);
        let later = EditCommand::SetNodePositions(vec![(node, Vec2::new(12.0, 11.0))]);

        assert!(first.merge(&later));
        assert_eq!(
            first,
            EditCommand::SetNodePositions(vec![(node, Vec2::new(10.0, 10.0))])
        );
    }

    /// The arithmetic that made [`EditCommand::SetNodePositions`] necessary,
    /// pinned so that nobody "simplifies" the inverse back to a negated delta.
    /// Sixty two-unit steps and one hundred-and-twenty-unit step back do not
    /// meet in `f32`.
    #[test]
    fn a_summed_delta_does_not_invert_a_drag_exactly_in_f32() {
        let start = 25.032717_f32;
        let mut walked = start;
        for _ in 0..60 {
            walked += 2.0;
        }

        assert_ne!(
            walked - 120.0,
            start,
            "if this ever holds, the float model changed, not the design"
        );
        assert!((walked - 120.0 - start).abs() < 1e-5);
    }

    #[test]
    fn positions_of_different_nodes_do_not_coalesce() {
        let mut first = EditCommand::SetNodePositions(vec![(NodeIndex::new(0), Vec2::ZERO)]);
        assert!(!first.merge(&EditCommand::SetNodePositions(vec![(
            NodeIndex::new(1),
            Vec2::ZERO
        )])));
    }

    #[test]
    fn moves_of_different_nodes_do_not_coalesce() {
        let mut first = EditCommand::move_node(NodeIndex::new(0), Vec2::new(1.0, 0.0));
        assert!(!first.merge(&EditCommand::move_node(
            NodeIndex::new(1),
            Vec2::new(1.0, 0.0)
        )));
        assert_eq!(
            first,
            EditCommand::move_node(NodeIndex::new(0), Vec2::new(1.0, 0.0))
        );
    }

    /// Nothing but a move coalesces, and a failed merge must leave the target
    /// exactly as it was — the history keeps the untouched entry.
    #[test]
    fn nothing_else_coalesces_and_a_refusal_changes_nothing() {
        let mut add = EditCommand::AddNodes(vec![draft()]);
        let before = add.clone();
        assert!(!add.merge(&EditCommand::AddNodes(vec![draft()])));
        assert_eq!(add, before);

        let mut remove = EditCommand::remove(vec![NodeIndex::new(0)], Vec::new());
        assert!(!remove.merge(&EditCommand::remove(vec![NodeIndex::new(1)], Vec::new())));
    }

    #[test]
    fn a_zero_delta_move_and_an_empty_list_are_trivially_empty() {
        assert!(EditCommand::move_node(NodeIndex::new(0), Vec2::ZERO).is_trivially_empty());
        assert!(
            EditCommand::MoveNodes {
                nodes: Vec::new(),
                delta: Vec2::new(1.0, 1.0)
            }
            .is_trivially_empty()
        );
        assert!(
            !EditCommand::move_node(NodeIndex::new(0), Vec2::new(1.0, 0.0)).is_trivially_empty()
        );
        assert!(EditCommand::AddNodes(Vec::new()).is_trivially_empty());
        assert!(EditCommand::disconnect(Vec::new()).is_trivially_empty());
    }

    /// The names are used by tests and by a debug overlay, so a variant that
    /// silently shares another's name would make both unreadable.
    #[test]
    fn presence_reads_as_remove_or_restore_depending_on_its_direction() {
        assert_eq!(
            EditCommand::remove(vec![NodeIndex::new(0)], Vec::new()).kind(),
            "remove"
        );
        assert_eq!(
            EditCommand::restore(vec![NodeIndex::new(0)], Vec::new()).kind(),
            "restore"
        );
    }
}
