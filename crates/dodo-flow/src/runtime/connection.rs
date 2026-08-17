//! §4's connection validation: **the rules, and the reasons a connection is
//! refused**.
//!
//! The rules are a value rather than a set of `if`s in the connect path,
//! because §4 lists them as capabilities an application configures — "validation
//! rules", "connection limits", "whole-node connection mode where useful" — and
//! because a rule expressed as data can be asserted from a test without
//! standing up an interaction.
//!
//! # Why a rejection is an enum and not a `bool`
//!
//! A connection tool has to *show* the user why a drop was refused: an
//! already-full input handle and a wrong-direction handle look identical if all
//! the engine says is "no". [`ConnectionError`] is the reason, and Phase 5's
//! handle rendering colours the target from it.
//!
//! **This file names no UI framework.**

use crate::models::{EdgeIndex, HandleIndex, NodeIndex};

/// What this world allows to be connected (§4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ConnectionRules {
    /// Whether a node may connect to itself. Off by default: a self-loop is
    /// almost always a slip of the pointer, and the applications that want one
    /// (a state machine's self-transition) know they do.
    pub allow_self_connections: bool,
    /// Whether the same pair of endpoints may be connected twice. Off by
    /// default for the same reason — the second edge lands exactly on the first
    /// and looks like nothing happened.
    pub allow_duplicate_edges: bool,
    /// Whether both ends must name a handle. Off by default, which leaves §4's
    /// whole-node connection mode available; an application whose nodes have
    /// meaningful ports turns it on and gets the check for free.
    pub require_handles: bool,
    /// Whether a kind that does not take a whole-node connection — a drawn
    /// shape, a frame — may be an endpoint anyway.
    ///
    /// Off for an interactive connection, so dragging an edge onto a rectangle
    /// is refused rather than producing an edge that nothing routes. **On while
    /// loading**, where the file is the authority: a document that says two
    /// elements are connected must come back with them connected, whatever this
    /// build thinks of the kinds involved.
    pub allow_unconnectable_nodes: bool,
}

impl ConnectionRules {
    pub const DEFAULT: ConnectionRules = ConnectionRules {
        allow_self_connections: false,
        allow_duplicate_edges: false,
        require_handles: false,
        allow_unconnectable_nodes: false,
    };

    /// Everything permitted. For a document being **loaded**, where the file is
    /// the authority and refusing an edge would silently discard the author's
    /// data.
    pub const PERMISSIVE: ConnectionRules = ConnectionRules {
        allow_self_connections: true,
        allow_duplicate_edges: true,
        require_handles: false,
        allow_unconnectable_nodes: true,
    };
}

impl Default for ConnectionRules {
    fn default() -> ConnectionRules {
        ConnectionRules::DEFAULT
    }
}

/// Why a connection was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConnectionError {
    /// The node index does not exist in this world.
    UnknownNode(NodeIndex),
    /// The handle index does not exist in this world.
    UnknownHandle(HandleIndex),
    /// The handle exists but belongs to a different node than the endpoint
    /// names. Almost always a bug in the caller rather than a user action, and
    /// worth its own variant for exactly that reason.
    HandleNotOnNode {
        node: NodeIndex,
        handle: HandleIndex,
    },
    /// The node has no handles and is not a kind that accepts a whole-node
    /// connection.
    NodeNotConnectable(NodeIndex),
    /// An endpoint named no handle and [`ConnectionRules::require_handles`] is
    /// set.
    HandleRequired,
    /// The source end is a handle that only accepts incoming connections, or
    /// the target end one that only produces outgoing ones.
    DirectionMismatch { handle: HandleIndex },
    /// Both ends are the same node and
    /// [`ConnectionRules::allow_self_connections`] is not set.
    SelfConnection(NodeIndex),
    /// These two endpoints are already connected, and
    /// [`ConnectionRules::allow_duplicate_edges`] is not set. Carries the edge
    /// that is already there, so a caller can select it instead of complaining.
    Duplicate(EdgeIndex),
    /// The handle is at its [`max_connections`](crate::runtime::HandleSpec::max_connections).
    HandleAtLimit { handle: HandleIndex, limit: u32 },
}

impl ConnectionError {
    /// Whether this refusal is about the *user's* action rather than about the
    /// caller's arguments. A connection tool shows the first kind and logs the
    /// second.
    pub fn is_user_facing(&self) -> bool {
        matches!(
            self,
            ConnectionError::DirectionMismatch { .. }
                | ConnectionError::SelfConnection(_)
                | ConnectionError::Duplicate(_)
                | ConnectionError::HandleAtLimit { .. }
                | ConnectionError::HandleRequired
                | ConnectionError::NodeNotConnectable(_)
        )
    }
}

impl std::fmt::Display for ConnectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // English, and deliberately not routed through `dodo-i18n`: nothing
        // here reaches a user's screen. These strings are for a test failure
        // and a developer's `Debug` print, and the canvas has no user-visible
        // string at all until the sidebar row lands in Phase 8.
        match self {
            ConnectionError::UnknownNode(node) => write!(f, "no node at index {node}"),
            ConnectionError::UnknownHandle(handle) => write!(f, "no handle at index {handle}"),
            ConnectionError::HandleNotOnNode { node, handle } => {
                write!(f, "handle {handle} does not belong to node {node}")
            }
            ConnectionError::NodeNotConnectable(node) => {
                write!(f, "node {node} does not accept connections")
            }
            ConnectionError::HandleRequired => f.write_str("both ends must name a handle"),
            ConnectionError::DirectionMismatch { handle } => {
                write!(f, "handle {handle} does not accept that direction")
            }
            ConnectionError::SelfConnection(node) => {
                write!(f, "node {node} may not connect to itself")
            }
            ConnectionError::Duplicate(edge) => {
                write!(f, "already connected, by edge {edge}")
            }
            ConnectionError::HandleAtLimit { handle, limit } => {
                write!(f, "handle {handle} already has its {limit} connection(s)")
            }
        }
    }
}

impl std::error::Error for ConnectionError {}

#[cfg(test)]
mod tests {
    use super::{ConnectionError, ConnectionRules};
    use crate::models::{EdgeIndex, HandleIndex, NodeIndex};

    #[test]
    fn the_default_rules_are_the_conservative_ones() {
        let rules = ConnectionRules::default();

        assert!(!rules.allow_self_connections);
        assert!(!rules.allow_duplicate_edges);
        assert!(!rules.require_handles);
        assert!(!rules.allow_unconnectable_nodes);
    }

    /// Loading a document must not refuse what the file says, or opening a
    /// document would silently delete edges from it.
    #[test]
    fn the_permissive_rules_refuse_nothing_structural() {
        let rules = ConnectionRules::PERMISSIVE;

        assert!(rules.allow_self_connections);
        assert!(rules.allow_duplicate_edges);
        assert!(rules.allow_unconnectable_nodes);
        assert!(!rules.require_handles);
    }

    #[test]
    fn a_users_mistake_and_a_callers_mistake_are_told_apart() {
        assert!(
            ConnectionError::SelfConnection(NodeIndex::new(0)).is_user_facing(),
            "the user dropped on the node they started from"
        );
        assert!(ConnectionError::Duplicate(EdgeIndex::new(3)).is_user_facing());
        assert!(
            !ConnectionError::UnknownNode(NodeIndex::new(9)).is_user_facing(),
            "an index that does not exist is a bug, not a gesture"
        );
        assert!(
            !ConnectionError::HandleNotOnNode {
                node: NodeIndex::new(0),
                handle: HandleIndex::new(1)
            }
            .is_user_facing()
        );
    }

    #[test]
    fn every_refusal_says_something_a_person_can_read() {
        let rendered = ConnectionError::HandleAtLimit {
            handle: HandleIndex::new(4),
            limit: 1,
        }
        .to_string();

        assert!(rendered.contains('4'), "{rendered}");
        assert!(rendered.contains('1'), "{rendered}");
    }
}
