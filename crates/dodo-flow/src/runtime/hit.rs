//! What is under the pointer (§29), and the seam where Phase 4's broad phase
//! plugs in.
//!
//! §29 asks for broad phase plus precise narrow phase, and **only the narrow
//! phase is here**. [`GraphWorld::hit_test`](crate::runtime::GraphWorld::hit_test)
//! takes the candidates it should test as an iterator, so the caller decides
//! where they came from; Phase 4's uniform grid answers that properly, and
//! until it does the launcher passes every node and says so at the call site.
//!
//! That is a considered line rather than a convenience. §40 rule 1 forbids
//! **scanning every element per frame to find the visible ones** — a per-frame
//! cost proportional to the document. A hit test runs once per press, on a
//! pointer event a human generated, and the candidate set is a parameter rather
//! than something this file goes and fetches. So there is nothing here for
//! Phase 4 to delete: it changes one argument.
//!
//! # A node's narrow phase is its rectangle, and one kind pays for that
//!
//! The narrow phase below tests **containment in the node's bounds**, which is
//! exact for every closed body the canvas draws — a rectangle, a rounded
//! rectangle and a graph node *are* their rectangles, and an ellipse or a
//! diamond inside one is close enough that no user notices the corners.
//!
//! It is **not** close for §7's free linear elements.
//! [`NodeShape::Line`](crate::runtime::NodeShape::Line) and
//! [`Arrow`](crate::runtime::NodeShape::Arrow) are the diagonal of their box,
//! so a 400×300 arrow is hit anywhere in 120,000 square units of which it
//! covers a few hundred — and the empty corners of a long diagonal are exactly
//! where a user reaches to click the thing *behind* it. Phase 7.5 added the
//! tools that create them; it did not add a segment-distance narrow phase for
//! them, and the two are separable because the broad phase already returns the
//! right candidates.
//!
//! The fix is one arm here rather than a new index:
//! [`segment_intersects_rect`](crate::geometry::segment_intersects_rect) and a
//! point-to-segment distance against [`HitTolerance`] already exist for edges,
//! which have the same shape of problem and solved it. Until then a linear
//! element is grabbed by its box, which is generous rather than wrong — nothing
//! is unreachable, some things are reachable from further away than they look.
//!
//! # An edge's narrow phase arrived with §9's text, and is a distance
//!
//! [`PointerTarget::Edge`] and
//! [`EdgeRoute::distance_to_point`](crate::geometry::EdgeRoute::distance_to_point)
//! are Phase 10's, and they were added for one gesture: a double-click has to
//! be able to say *which* edge it landed on, or an edge cannot be labelled.
//! It reuses exactly what the paragraph above predicted it would — the route's
//! own flattening, a point-to-segment distance, and a control-hull rejection
//! first — and it is a **second, ranked pass** rather than a branch inside the
//! node loop: bodies and handles are resolved first, and edges are asked only
//! if nothing else was hit. That ordering is the whole reason a labelled edge
//! passing under a node is still the node when you press it.
//!
//! The linear-element arm above is still missing. It is a different fix — a
//! node's *bounds* against a segment, not an edge's route — and this phase did
//! not need it.
//!
//! ## The limitation a user meets: an edge cannot be *clicked*
//!
//! [`PointerTarget::Edge`] is resolved, and then
//! [`InteractionMachine`](crate::interaction::InteractionMachine) treats it
//! exactly as empty canvas: a press starts a rubber band. So **the only way to
//! select an edge is to band over it**, and `Delete` cannot reach one any other
//! way. Double-clicking it works, because that is a different event.
//!
//! That is deliberate for one phase rather than an oversight, and the reason is
//! worth the next person's attention: making a press *select* an edge is not
//! one arm. A press that selects has to decide what a press-and-drag on the
//! same edge means — nothing, today, since an edge has no drag gesture — and
//! §28's selection is a set two other things already read. The narrow phase is
//! the expensive half and it is done; adding the gesture is
//! `interaction::state`'s work and it should arrive with whatever makes an
//! edge draggable, so the two are designed together rather than one being
//! retrofitted around the other.
//!
//! **This file names no UI framework.**

use crate::models::{EdgeIndex, HandleIndex, NodeIndex};

/// What a press landed on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PointerTarget {
    /// Empty canvas — a box selection or a pan, depending on the modifiers.
    #[default]
    Empty,
    /// A node's body: a drag.
    Node(NodeIndex),
    /// A handle: the start of a connection. Carries its node so the connection
    /// tool does not have to ask the store for the owner.
    Handle {
        node: NodeIndex,
        handle: HandleIndex,
    },
    /// **An edge's drawn line**, within [`HitTolerance::edge_radius`] of it.
    ///
    /// Ranked *below* every node and every handle, which is why it is last:
    /// an edge is a line a node's body is drawn over, and a press where the two
    /// overlap means the node. [`GraphWorld::hit_test`](crate::runtime::GraphWorld::hit_test)
    /// only asks about edges once it has decided nothing else was there.
    ///
    /// **A press on one behaves exactly like a press on empty canvas** in
    /// [`InteractionMachine`](crate::interaction::InteractionMachine) —
    /// see [`PointerTarget::starts_a_band`]. Edges are not draggable and not
    /// yet click-selectable; what this arm exists for is §9's double-click,
    /// which is the one gesture that has to tell an edge from the canvas
    /// behind it.
    Edge(EdgeIndex),
}

impl PointerTarget {
    pub fn node(self) -> Option<NodeIndex> {
        match self {
            PointerTarget::Empty | PointerTarget::Edge(_) => None,
            PointerTarget::Node(node) => Some(node),
            PointerTarget::Handle { node, .. } => Some(node),
        }
    }

    pub fn edge(self) -> Option<EdgeIndex> {
        match self {
            PointerTarget::Edge(edge) => Some(edge),
            _ => None,
        }
    }

    /// **Whether a press here starts a rubber band rather than grabbing
    /// something.**
    ///
    /// True for empty canvas *and* for an edge, and that is the whole reason
    /// this is a named question rather than `== PointerTarget::Empty` at three
    /// call sites. Adding [`Edge`](PointerTarget::Edge) made every one of those
    /// comparisons silently wrong — a press on an edge would have started
    /// nothing at all, which reads as a canvas that has stopped responding.
    pub fn starts_a_band(self) -> bool {
        matches!(self, PointerTarget::Empty | PointerTarget::Edge(_))
    }

    pub fn handle(self) -> Option<HandleIndex> {
        match self {
            PointerTarget::Handle { handle, .. } => Some(handle),
            _ => None,
        }
    }

    pub fn is_empty(self) -> bool {
        matches!(self, PointerTarget::Empty)
    }
}

/// How generous a hit test is, in **world** units.
///
/// §8 asks for edge hit testing with a tolerance independent of the visible
/// line thickness, and §29 for handles to be easy to grab; both are the same
/// idea, that the target a person aims at is bigger than the thing they see.
/// The view converts from screen pixels, because a fixed world tolerance would
/// grow to cover the whole node when zoomed out.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HitTolerance {
    /// How far from a handle's centre still counts as hitting it.
    pub handle_radius: f32,
    /// How far from an edge's drawn line still counts as hitting it — §8's
    /// *"tolerance independent of the visible line thickness"*, stated as a
    /// number.
    ///
    /// Its own field rather than a reuse of `handle_radius`, because the two
    /// answer different questions: a handle is a dot you aim at, and an edge is
    /// a line you aim *across*. A generous handle radius that also widened
    /// every edge would make an edge under a handle unhittable.
    pub edge_radius: f32,
}

impl HitTolerance {
    /// The screen-pixel radius the view converts into world units. Matched to
    /// the handle dot the painter draws, plus enough margin to be grabbable
    /// without aiming.
    pub const HANDLE_SCREEN_RADIUS: f32 = 9.0;

    /// The screen-pixel band around an edge's line that counts as the edge.
    ///
    /// Narrower than a handle's radius on purpose: an edge is long, so a
    /// generous band covers a great deal of canvas, and everything it covers is
    /// canvas a rubber band can no longer be started in.
    pub const EDGE_SCREEN_RADIUS: f32 = 6.0;

    /// Both radii from one world-space handle radius, keeping their screen
    /// proportion. The view has a zoom and converts once.
    pub fn new(handle_radius: f32) -> HitTolerance {
        HitTolerance {
            handle_radius,
            edge_radius: handle_radius
                * (HitTolerance::EDGE_SCREEN_RADIUS / HitTolerance::HANDLE_SCREEN_RADIUS),
        }
    }
}

impl Default for HitTolerance {
    fn default() -> HitTolerance {
        HitTolerance::new(HitTolerance::HANDLE_SCREEN_RADIUS)
    }
}

#[cfg(test)]
mod tests {
    use super::{HitTolerance, PointerTarget};
    use crate::models::{EdgeIndex, HandleIndex, NodeIndex};

    #[test]
    fn a_target_reports_the_node_it_belongs_to_however_it_was_hit() {
        let node = NodeIndex::new(3);

        assert_eq!(PointerTarget::Empty.node(), None);
        assert_eq!(PointerTarget::Node(node).node(), Some(node));
        assert_eq!(
            PointerTarget::Handle {
                node,
                handle: HandleIndex::new(1)
            }
            .node(),
            Some(node)
        );
    }

    #[test]
    fn only_a_handle_hit_reports_a_handle() {
        let node = NodeIndex::new(3);

        assert_eq!(PointerTarget::Node(node).handle(), None);
        assert_eq!(
            PointerTarget::Handle {
                node,
                handle: HandleIndex::new(1)
            }
            .handle(),
            Some(HandleIndex::new(1))
        );
        assert!(PointerTarget::Empty.is_empty());
    }

    /// **The arm every `== PointerTarget::Empty` in the machine had to become.**
    ///
    /// Adding an `Edge` variant made three comparisons silently wrong: a press
    /// on an edge would have matched none of them and started no gesture at
    /// all, which reads as a canvas that has stopped responding. The named
    /// question is what makes that a compile-time choice rather than a
    /// behaviour nobody notices.
    #[test]
    fn a_press_on_an_edge_starts_a_band_exactly_as_empty_canvas_does() {
        let edge = PointerTarget::Edge(EdgeIndex::new(2));

        assert!(PointerTarget::Empty.starts_a_band());
        assert!(edge.starts_a_band());
        assert!(!PointerTarget::Node(NodeIndex::new(0)).starts_a_band());
        assert!(
            !PointerTarget::Handle {
                node: NodeIndex::new(0),
                handle: HandleIndex::new(0)
            }
            .starts_a_band()
        );

        // An edge belongs to no node, so §44's hover and every "which node?"
        // question answer `None` rather than picking one of its ends.
        assert_eq!(edge.node(), None);
        assert_eq!(edge.edge(), Some(EdgeIndex::new(2)));
        assert_eq!(PointerTarget::Node(NodeIndex::new(0)).edge(), None);
        assert!(
            !edge.is_empty(),
            "an edge is something, it is just not a node"
        );
    }

    /// The two radii are separate fields on purpose, and this pins the reason:
    /// a handle is a dot you aim at, an edge is a line you aim across, and a
    /// generous handle radius that also widened every edge would make the edge
    /// under a handle unhittable.
    #[test]
    fn an_edge_is_a_narrower_target_than_a_handle() {
        let tolerance = HitTolerance::new(18.0);

        assert_eq!(tolerance.handle_radius, 18.0);
        assert!(tolerance.edge_radius < tolerance.handle_radius);
        assert_eq!(
            tolerance.edge_radius,
            18.0 * (HitTolerance::EDGE_SCREEN_RADIUS / HitTolerance::HANDLE_SCREEN_RADIUS)
        );
    }

    #[test]
    fn the_default_tolerance_is_the_screen_radius_the_painter_draws() {
        assert_eq!(
            HitTolerance::default().handle_radius,
            HitTolerance::HANDLE_SCREEN_RADIUS
        );
    }
}
