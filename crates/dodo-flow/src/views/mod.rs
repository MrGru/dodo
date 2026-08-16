//! The Flow Canvas's rendering. **These are the only files in the crate that
//! may name `gpui` or `gpui-component`**, and that restriction is the crate's
//! main design constraint — `lib.rs` says why.
//!
//! [`flow::FlowView`] is the entity the standalone launcher mounts and the one
//! the app's `tools!` row will eventually point at. Today it paints an empty themed pane
//! and owns the [`Viewport`](crate::geometry::Viewport) and the
//! [`FlowDocument`](crate::models::FlowDocument) the later phases will draw.
//!
//! The `canvas()` element arrives under it next. The shape is already settled:
//! `canvas()` is sufficient and no custom `Element` is needed, but its `id()`
//! returns `None` in the pinned gpui, so the focus handle, key context and
//! cursor style live on a wrapping `div` and the canvas sits inside it. That wrapper is the reason
//! this view exists at all today rather than alongside the first painter.

pub mod flow;

pub use flow::FlowView;
