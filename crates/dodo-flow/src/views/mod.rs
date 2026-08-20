//! The Flow Canvas's rendering. **These are the only files in the crate that
//! may name `gpui` or `gpui-component`**, and that restriction is the crate's
//! main design constraint — `lib.rs` says why.
//!
//! [`flow::FlowView`] is the entity the standalone launcher mounts and the one
//! the app's `tools!` row will eventually point at. It owns the
//! [`Viewport`](crate::geometry::Viewport), the graph, the spatial index and
//! §24's snapshot, and it is where the two halves of the hybrid renderer meet.
//!
//! `canvas()` is sufficient and no custom `Element` is needed, but its `id()`
//! returns `None` in the pinned gpui, so the focus handle, key context and
//! cursor style live on a wrapping `div` and the canvas sits inside it.
//!
//! # The two halves, and which is which
//!
//! ```text
//! div  ── canvas()      the painted half: grid, edges, bodies, dots, labels
//!     ├─ absolute layer the rich half: node elements, handles, toolbar
//!     └─ absolute layer §45's tool palette
//! ```
//!
//! [`palette`] is a third thing again: chrome rather than content. It belongs
//! to the canvas rather than to the launcher because Phase 8's sidebar row
//! mounts the same [`flow::FlowView`], and a palette wired into the launcher
//! would be a control that vanished the day the tool shipped.
//!
//! [`nodes`] is the rich half. It draws **tens** of elements — the ones
//! [`RenderSnapshot`](crate::render::RenderSnapshot) marked rich, plus controls
//! for the one node being worked on — and it can never draw more, because the
//! snapshot never offers more. Everything else on screen is paint.
//!
//! Which half a node lands in is §15's and §16's answer and it is made in
//! `render::snapshot`, not here. This directory turns decisions into elements;
//! it does not take them.

pub mod flow;
pub mod keymap;
pub mod nodes;
pub mod palette;

pub use flow::FlowView;
pub use keymap::{Redo, SelectTool, Undo, init};
