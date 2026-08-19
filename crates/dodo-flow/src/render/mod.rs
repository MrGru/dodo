//! Turning a document into a frame.
//!
//! **The one module in the crate whose layering is per file rather than per
//! directory**, and deliberately so: [`plan`], [`shapes`] and [`grid`] name no
//! UI framework and are unit tested with no window, while [`painter`] is the
//! GPUI end and the only file in the crate that paints anything. The crate doc
//! explains what that boundary is worth; here it buys the ability to assert the
//! paint-order contract, the grid's bounded output and the vertex estimate as
//! ordinary unit tests.
//!
//! The frame is built in three steps and nothing skips them:
//!
//! ```text
//! VisibleSet ─> scene::plan_scene ─┐
//! selection rect / preview ────────┴─> PaintPlan ─ enforce_vertex_ceiling ─>
//!                                      (quads | paths | text)   paint_into(WindowPainter)
//! ```
//!
//! [`scene`] is the step in front of all of it: it takes the
//! [`VisibleSet`](crate::spatial::VisibleSet) and turns *only that* into
//! primitives. It moved out of `views::flow` in Phase 4 precisely so the
//! "no offscreen path reaches the painter" property could be a unit test.
//!
//! [`edges`] joins them for a graph edge: it is the one place a world-space
//! [`EdgeRoute`](crate::geometry::EdgeRoute) becomes screen-space primitives,
//! markers included.
//!
//! [`plan`] is where the two contracts Phase 0 made structural live: paint
//! order batched by primitive kind, and painted-vertex accounting. Read its doc
//! before adding a painter.

pub mod edges;
pub mod grid;
pub mod lod;
pub mod painter;
pub mod plan;
pub mod registry;
pub mod scene;
pub mod shapes;

pub use edges::{EdgePaint, plan_connection_preview, plan_edge};
pub use grid::{GridLevel, GridLimits, GridSettings, GridStyle};
pub use lod::{EdgeDetail, HandleDetail, LodPlan, SceneLoad};
pub use painter::WindowPainter;
pub use plan::{PaintPlan, PaintStats, PathPaint, PathPrimitive, PrimitiveSink, QuadPrimitive};
pub use registry::{
    AccentRole, GenericKind, NodeGlyph, NodeRef, NodeRenderer, NodeRendererRegistry, NodeVisual,
};
pub use scene::{SceneInk, SceneOptions, SceneStats, plan_scene};
pub use shapes::Outline;
