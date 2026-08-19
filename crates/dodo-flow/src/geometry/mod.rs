//! World-space geometry. **No file here names a UI framework**, and none may.
//!
//! Everything in this module is plain `f32` in *world* coordinates — the
//! document's own, infinite, zoom-independent space. GPUI's `Point<Pixels>`
//! and `Bounds<Pixels>` live on the other side of [`crate::views`], and
//! `lib.rs` records why that line matters more than it looks.
//!
//! - [`vec`](mod@vec) — [`Vec2`], the two-component vector every position, size and
//!   delta is expressed in.
//! - [`bounds`] — [`Rect`], an axis-aligned world rectangle, the union /
//!   intersection / containment maths that culling and zoom-to-fit are built
//!   from, and [`bounds::segment_intersects_rect`], the exact test §28's box
//!   selection resolves an edge with.
//! - [`curve`] — cubic evaluation and flattening. One step-count formula, used
//!   by the vertex estimate in `render::shapes` and by the world-space
//!   narrow phase that asks whether a route crosses a rectangle.
//! - [`route`] — [`EdgeRoute`], the five edge routings of §8 as derived
//!   world-space geometry, kept strictly apart from the logical edge.
//! - [`arrow`] — §8's endpoint decorations, allocation-free, with the dot
//!   expressed as a circle so it can be painted as a quad.
//! - [`transform`] — [`Viewport`], **the single owner of world↔screen**.
//!   Requirements §22 is explicit that these formulas must not be scattered
//!   across renderers, and this is the file that keeps that promise.
//!
//! Later phases add `shape` and `sketch` here, plus the tessellation-tolerance
//! and re-tessellation policy, which measurement
//! made a module boundary rather than a tuning constant. The tolerance
//! itself already exists, as
//! [`RenderQuality`](crate::models::style::RenderQuality) — it is a style/quality
//! field because it is a 2× budget multiplier the user can trade, not a
//! constant the geometry layer picks.

pub mod arrow;
pub mod bounds;
pub mod curve;
pub mod route;
pub mod transform;
pub mod vec;

pub use arrow::{ArrowGeometry, ArrowPolygon};
pub use bounds::{Rect, segment_intersects_rect};
pub use curve::{cubic_point, cubic_segments, flatten_cubic};
pub use route::{Attachment, EdgeRoute, RouteOptions, RouteSegment, Side};
pub use transform::Viewport;
pub use vec::Vec2;

/// The circular magic constant, `4/3 * (sqrt(2) - 1)`: the control-point offset
/// that makes a cubic Bézier approximate a quarter circle to within about
/// 0.02 % of the radius.
///
/// Here rather than beside either of its two users — `render::shapes`' rounded
/// rectangle and ellipse, and [`route`]'s smooth-step corners — because it is a
/// property of circles rather than of shapes or of routes, and because two
/// copies of a fitted-looking constant is how one of them ends up subtly
/// different from the other.
pub const CIRCLE_KAPPA: f32 = 0.552_284_8;
