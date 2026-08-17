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
//! **This file names no UI framework.**

use crate::models::{HandleIndex, NodeIndex};

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
}

impl PointerTarget {
    pub fn node(self) -> Option<NodeIndex> {
        match self {
            PointerTarget::Empty => None,
            PointerTarget::Node(node) => Some(node),
            PointerTarget::Handle { node, .. } => Some(node),
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
}

impl HitTolerance {
    /// The screen-pixel radius the view converts into world units. Matched to
    /// the handle dot the painter draws, plus enough margin to be grabbable
    /// without aiming.
    pub const HANDLE_SCREEN_RADIUS: f32 = 9.0;

    pub fn new(handle_radius: f32) -> HitTolerance {
        HitTolerance { handle_radius }
    }
}

impl Default for HitTolerance {
    fn default() -> HitTolerance {
        HitTolerance {
            handle_radius: HitTolerance::HANDLE_SCREEN_RADIUS,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{HitTolerance, PointerTarget};
    use crate::models::{HandleIndex, NodeIndex};

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

    #[test]
    fn the_default_tolerance_is_the_screen_radius_the_painter_draws() {
        assert_eq!(
            HitTolerance::default().handle_radius,
            HitTolerance::HANDLE_SCREEN_RADIUS
        );
    }
}
