//! The document model. **No file here names a UI framework**, and none may.
//!
//! This is the persistent half of requirements §31's split:
//!
//! ```text
//! Persistent document          Runtime derived state
//! (this module, serde)     vs  (runtime/, spatial/, geometry caches — Phases 2-4)
//! ```
//!
//! - [`ids`] — [`ids::ElementId`], the stable identity that is
//!   serialized, and [`ids::NodeIndex`] /
//!   [`ids::EdgeIndex`] / [`ids::HandleIndex`], the
//!   compact `u32` runtime indices that are **not**. That asymmetry is
//!   enforced by the derives: the indices do not implement `Serialize`, so a
//!   struct that reaches for one in a persisted field does not compile.
//! - [`image`] — §10's [`image::ImageResource`], the
//!   [`image::ImageHandle`] that shares one copy of it between elements, and
//!   the [`image::ImageCrop`] that is metadata rather than a rewrite.
//! - [`kind`] — [`kind::ElementKind`], the taxonomy from
//!   requirements §3. Extensible by construction; see its module doc for what
//!   "not hard-coded around rectangles" cost.
//! - [`style`] — the shared style structs of §32, including
//!   [`style::RenderQuality`], which carries the flattening
//!   tolerance that measurement promoted to a first-class field.
//! - [`document`] — [`document::FlowDocument`] and the node,
//!   handle and edge records it holds.
//! - [`serialization`] — the versioned envelope and the migration ladder,
//!   present from version 1 rather than retrofitted.
//!
//! **Nothing here is a store.** `NodeStore` / `EdgeStore` and the SoA layout of
//! the SoA layout arrive in `runtime/` later; `FlowDocument` is the thing that is
//! written to disk and read back, and it is deliberately an ordinary
//! `Vec`-of-structs because it is touched once per load and once per save, not
//! once per frame.

pub mod document;
pub mod ids;
pub mod image;
pub mod kind;
pub mod serialization;
pub mod style;

pub use document::{
    DocumentSettings, Endpoint, FlowDocument, FlowEdge, FlowNode, Handle, HandleDirection,
    HandlePlacement, Metadata, handle_world_position,
};
pub use ids::{EdgeIndex, ElementId, HandleId, HandleIndex, IdAllocator, NodeIndex};
pub use image::{
    ImageCrop, ImageFormat, ImageHandle, ImageResource, NodeImage, decode_base64, encode_base64,
};
pub use kind::{CustomKind, ElementKind, GraphNodeKind, LinearKind, ShapeKind};
pub use serialization::{CURRENT_VERSION, LoadError, SaveError};
pub use style::{
    ArrowMarker, Color, DashPattern, EdgeRouting, ElementStyle, FillStyle, FontFamily, FontSize,
    FontStyle, RenderQuality, RenderStyle, SketchStyle, Sloppiness, StrokeStyle, TextAlign,
};
