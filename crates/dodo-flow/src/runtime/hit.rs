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
//! ## And Phase 10.5 gave the gesture in front of it
//!
//! Phase 10 recorded that a press on an edge started a rubber band, so the only
//! way to select one was to band over it and `Delete` could reach one no other
//! way. That is closed:
//! [`InteractionEffect::SelectEdge`](crate::interaction::InteractionEffect::SelectEdge)
//! is what a press on this variant now raises, additively under shift, and the
//! band is left to [`PointerTarget::Empty`] alone.
//!
//! Phase 10 argued the gesture should wait for whatever makes an edge
//! *draggable*, so the two would be designed together. **That was the wrong
//! call and it is worth saying why**: the two are not one design. Selecting is
//! a press that ends where it started; dragging is what the *following* moves
//! mean, and an edge has no drag gesture at all — so the press stays in
//! [`InteractionState::Idle`](crate::interaction::InteractionState::Idle), the
//! moves after it mean nothing, and a later phase that gives an edge a drag
//! adds a state without touching the arm that selects. Waiting cost a user the
//! only way to delete an edge for a whole phase, and bought nothing.
//!
//! The one thing a press on an edge does **not** do is start a band, and that
//! is a real trade rather than a free win: everywhere within
//! [`HitTolerance::EDGE_SCREEN_RADIUS`] of a route is canvas a rubber band can
//! no longer be started in. Six screen pixels is the number that keeps it small
//! — see that constant.
//!
//! **This file names no UI framework.**

use crate::{
    geometry::{ResizeCorner, Vec2},
    models::{ConnectorAttachment, ConnectorEnd, EdgeIndex, HandleIndex, NodeIndex},
};

/// Snap/highlight feedback for a straight connector endpoint.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ConnectorSnap {
    pub target: NodeIndex,
    pub point: Vec2,
    pub attachment: ConnectorAttachment,
}

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
    /// **A press on one selects it** (Phase 10.5), additively under shift, and
    /// starts no gesture — an edge has no drag. A double-click on one opens its
    /// label. Neither is a rubber band: the band belongs to
    /// [`Empty`](PointerTarget::Empty) alone.
    Edge(EdgeIndex),
    /// **A resize grip on the selected element** (Phase 12), and the only
    /// target that is not a document element at all.
    ///
    /// Ranked **above** everything, which is the opposite end of the order from
    /// [`Edge`](PointerTarget::Edge) and for the same kind of reason: a grip is
    /// drawn on top of the element it belongs to, it is small, and a press
    /// inside its dot means the resize rather than the drag underneath it. Get
    /// this ranking wrong and the grips are simply dead — every press on one
    /// starts a move instead, and nothing reports it.
    ///
    /// It exists only for the element the selection ring is drawn around, so a
    /// document with nothing selected can never produce one.
    ResizeGrip {
        node: NodeIndex,
        corner: ResizeCorner,
    },
    /// One of exactly two ordered endpoint handles on a selected straight
    /// connector.
    ConnectorEndpoint { node: NodeIndex, end: ConnectorEnd },
}

impl PointerTarget {
    pub fn node(self) -> Option<NodeIndex> {
        match self {
            PointerTarget::Empty | PointerTarget::Edge(_) => None,
            PointerTarget::Node(node) => Some(node),
            PointerTarget::Handle { node, .. } => Some(node),
            PointerTarget::ResizeGrip { node, .. }
            | PointerTarget::ConnectorEndpoint { node, .. } => Some(node),
        }
    }

    /// The grip this press landed on, if it landed on one.
    pub fn resize_grip(self) -> Option<(NodeIndex, ResizeCorner)> {
        match self {
            PointerTarget::ResizeGrip { node, corner } => Some((node, corner)),
            _ => None,
        }
    }

    pub fn connector_endpoint(self) -> Option<(NodeIndex, ConnectorEnd)> {
        match self {
            PointerTarget::ConnectorEndpoint { node, end } => Some((node, end)),
            _ => None,
        }
    }

    pub fn edge(self) -> Option<EdgeIndex> {
        match self {
            PointerTarget::Edge(edge) => Some(edge),
            _ => None,
        }
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
    /// How far from a resize grip's centre still counts as hitting it
    /// (Phase 12).
    ///
    /// Its own field for the same reason `edge_radius` is one: a grip sits at a
    /// corner of the selection ring, a handle sits on the middle of a side, and
    /// on a small element the two are a few pixels apart. Sharing a radius
    /// would make whichever was tested first swallow the other.
    pub grip_radius: f32,
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

    /// The screen-pixel radius of a resize grip's target. Matched to the grip
    /// `views::nodes` draws, plus a little margin — a corner is aimed at
    /// deliberately, so it needs less generosity than a handle.
    pub const GRIP_SCREEN_RADIUS: f32 = 7.0;

    /// **The tolerance at a zoom level** — §29's *"tolerance in screen-space
    /// pixels"* as one function of one number.
    ///
    /// The conversion used to be two lines in `views::flow`, which put the only
    /// statement of "a thin edge stays clickable at any zoom" in the one file
    /// that needs a `Window` to build. Here it is a pure function, and
    /// `an_edges_target_is_the_same_width_on_screen_at_every_zoom` asserts the
    /// property at seven zoom levels with no window anywhere.
    ///
    /// A zoom at or below zero is not a camera anybody can be looking through;
    /// it answers the unzoomed tolerance rather than an infinity that would
    /// make every element on the canvas the nearest one.
    pub fn at_zoom(zoom: f32) -> HitTolerance {
        let zoom = if zoom > 0.0 { zoom } else { 1.0 };
        HitTolerance::new(HitTolerance::HANDLE_SCREEN_RADIUS / zoom)
    }

    /// Both radii from one world-space handle radius, keeping their screen
    /// proportion. The view has a zoom and converts once.
    pub fn new(handle_radius: f32) -> HitTolerance {
        HitTolerance {
            handle_radius,
            edge_radius: handle_radius
                * (HitTolerance::EDGE_SCREEN_RADIUS / HitTolerance::HANDLE_SCREEN_RADIUS),
            grip_radius: handle_radius
                * (HitTolerance::GRIP_SCREEN_RADIUS / HitTolerance::HANDLE_SCREEN_RADIUS),
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
    use crate::{
        geometry::ResizeCorner,
        models::{EdgeIndex, HandleIndex, NodeIndex},
    };

    /// A grip belongs to its element, so every question a caller asks about
    /// "what node is this press about?" answers the same for it as for a body.
    #[test]
    fn a_grip_reports_its_node_and_its_corner() {
        let node = NodeIndex::new(7);
        let target = PointerTarget::ResizeGrip {
            node,
            corner: ResizeCorner::TopRight,
        };

        assert_eq!(target.node(), Some(node));
        assert_eq!(
            target.resize_grip(),
            Some((node, ResizeCorner::TopRight)),
            "a grip that cannot say which corner it is resizes nothing"
        );
        assert_eq!(PointerTarget::Node(node).resize_grip(), None);
        assert!(!target.is_empty());
    }

    /// A grip's target is smaller than a handle's and larger than an edge's,
    /// and every one of them is a *screen* distance — so the three keep their
    /// proportions at any zoom.
    #[test]
    fn the_three_radii_keep_their_order_at_every_zoom() {
        for zoom in [0.05f32, 0.5, 1.0, 4.0, 20.0] {
            let tolerance = HitTolerance::at_zoom(zoom);
            assert!(tolerance.edge_radius < tolerance.grip_radius, "at {zoom}");
            assert!(tolerance.grip_radius < tolerance.handle_radius, "at {zoom}");
            assert!(
                (tolerance.grip_radius * zoom - HitTolerance::GRIP_SCREEN_RADIUS).abs() < 1e-3,
                "a grip is not the same size on screen at {zoom}"
            );
        }
    }

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

    /// An edge is a target in its own right: it belongs to no node, and it is
    /// not the canvas.
    ///
    /// `is_empty` is the question the band is started from (Phase 10.5), so an
    /// edge answering `true` to it would put the band back over every route.
    #[test]
    fn an_edge_is_neither_a_node_nor_the_canvas() {
        let edge = PointerTarget::Edge(EdgeIndex::new(2));

        // An edge belongs to no node, so §44's hover and every "which node?"
        // question answer `None` rather than picking one of its ends.
        assert_eq!(edge.node(), None);
        assert_eq!(edge.handle(), None);
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

    /// **§29's requirement, stated as a number at seven cameras.**
    ///
    /// A hit tolerance in world units would shrink to nothing when zoomed in
    /// and swallow the canvas when zoomed out; the whole point of converting
    /// from screen pixels is that the target a person aims at is the same size
    /// on the glass however far the camera is. `1e-3` is float slop on a
    /// division and a multiplication, not a tolerance on the tolerance.
    #[test]
    fn an_edges_target_is_the_same_width_on_screen_at_every_zoom() {
        for zoom in [0.05_f32, 0.25, 0.5, 1.0, 2.0, 8.0, 40.0] {
            let tolerance = HitTolerance::at_zoom(zoom);

            assert!(
                (tolerance.edge_radius * zoom - HitTolerance::EDGE_SCREEN_RADIUS).abs() < 1e-3,
                "at zoom {zoom} an edge's band is {} screen pixels",
                tolerance.edge_radius * zoom
            );
            assert!(
                (tolerance.handle_radius * zoom - HitTolerance::HANDLE_SCREEN_RADIUS).abs() < 1e-3,
                "at zoom {zoom} a handle's radius is {} screen pixels",
                tolerance.handle_radius * zoom
            );
        }

        // A camera nobody can look through answers the unzoomed tolerance
        // rather than an infinite one, which would make every edge in the
        // document a candidate for the nearest.
        assert_eq!(HitTolerance::at_zoom(0.0), HitTolerance::default());
        assert_eq!(HitTolerance::at_zoom(-1.0), HitTolerance::default());
    }

    #[test]
    fn the_default_tolerance_is_the_screen_radius_the_painter_draws() {
        assert_eq!(
            HitTolerance::default().handle_radius,
            HitTolerance::HANDLE_SCREEN_RADIUS
        );
    }
}
