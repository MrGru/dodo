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
//! grid::generate ─┐
//! shapes::…       ├─> PaintPlan ─ enforce_vertex_ceiling ─> paint_into(WindowPainter)
//! selection rect ─┘   (quads | paths | text)                all quads, all paths, all text
//! ```
//!
//! [`plan`] is where the two contracts Phase 0 made structural live: paint
//! order batched by primitive kind, and painted-vertex accounting. Read its doc
//! before adding a painter.

pub mod grid;
pub mod painter;
pub mod plan;
pub mod shapes;

pub use grid::{GridLevel, GridLimits, GridSettings, GridStyle};
pub use painter::WindowPainter;
pub use plan::{PaintPlan, PaintStats, PathPaint, PathPrimitive, PrimitiveSink, QuadPrimitive};
pub use shapes::Outline;
