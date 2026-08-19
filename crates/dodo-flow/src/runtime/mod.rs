//! The graph engine (§17–§20). **No file here names a UI framework**, and none
//! may.
//!
//! This is the *runtime* half of §31's split — everything derived from the
//! document, rebuilt on load, and never written to disk:
//!
//! ```text
//! Persistent document          Runtime derived state
//! models/, serde           vs  this module, spatial/, the caches
//! ```
//!
//! - [`nodes`] / [`edges`] / [`handles`] — §17's SoA stores, split by what the
//!   paint loop touches rather than by what reads nicely.
//! - [`adjacency`] — §20's index: a node's incident edges in time proportional
//!   to its degree, benchmarked rather than asserted.
//! - [`dirty`] — §19's change tracking: bitflags per element **and** a queue of
//!   what was touched, because flags alone can only be read by scanning them.
//! - [`routes`] — the derived edge geometry, and the rebuild counter §19's
//!   property test measures.
//! - [`connection`] — §4's validation rules and the reasons a drop is refused.
//! - [`hit`] — §29's narrow phase, behind [`crate::spatial`]'s broad phase.
//! - [`selection`] — §28's selection: a bitset for "is this selected?" and a
//!   list for "what is selected?", holding compact ids and never elements.
//! - [`world`] — [`GraphWorld`], which owns all of the above and is the one
//!   place the propagation rule is written down.
//!
//! # Read [`world`] first
//!
//! Every other file here is storage or bookkeeping; `world.rs` is the module's
//! argument. Its [`GraphWorld::move_node`] is §19's diagram in code, and the
//! test at the bottom of that file — one node moved in a 100,000-node,
//! 500,000-edge graph, four rebuilds and no more — is what all of this is for.

pub mod adjacency;
pub mod compact;
pub mod connection;
pub mod dirty;
pub mod edges;
pub mod handles;
pub mod hit;
pub mod nodes;
pub mod routes;
pub mod selection;
pub mod world;

pub use adjacency::AdjacencyIndex;
pub use compact::CompactList;
pub use connection::{ConnectionError, ConnectionRules};
pub use dirty::{DirtyState, EdgeDirty, NodeDirty};
pub use edges::{EdgeEnd, EdgeFlags, EdgeSpec, EdgeStore, OptionalHandle};
pub use handles::{HandleFlags, HandleSpec, HandleStore};
pub use hit::{HitTolerance, PointerTarget};
pub use nodes::{NodeCold, NodeFlags, NodeShape, NodeSpec, NodeStore};
pub use routes::EdgeGeometryStore;
pub use selection::{BoxQuery, BoxSelectMode, SelectionSet};
pub use world::{GraphWorld, LoadReport};
