//! World-space geometry. **No file here names a UI framework**, and none may.
//!
//! Everything in this module is plain `f32` in *world* coordinates — the
//! document's own, infinite, zoom-independent space. GPUI's `Point<Pixels>`
//! and `Bounds<Pixels>` live on the other side of [`crate::views`], and
//! `lib.rs` records why that line matters more than it looks.
//!
//! - [`vec`](mod@vec) — [`Vec2`], the two-component vector every position, size and
//!   delta is expressed in.
//! - [`bounds`] — [`Rect`], an axis-aligned world rectangle, and the union /
//!   intersection / containment maths that culling and zoom-to-fit are built
//!   from.
//! - [`transform`] — [`Viewport`], **the single owner of world↔screen**.
//!   Requirements §22 is explicit that these formulas must not be scattered
//!   across renderers, and this is the file that keeps that promise.
//!
//! Later phases add `bezier`, `orthogonal`, `arrow`, `shape` and `sketch` here,
//! plus the tessellation-tolerance and re-tessellation policy, which measurement
//! made a module boundary rather than a tuning constant. The tolerance
//! itself already exists, as
//! [`RenderQuality`](crate::models::style::RenderQuality) — it is a style/quality
//! field because it is a 2× budget multiplier the user can trade, not a
//! constant the geometry layer picks.

pub mod bounds;
pub mod transform;
pub mod vec;

pub use bounds::Rect;
pub use transform::Viewport;
pub use vec::Vec2;
