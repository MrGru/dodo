//! dodo's Mermaid workspace: write Mermaid source, see it as SVG immediately.
//!
//! Two files, two very different rules:
//!
//! - [`render`] is the pure service — `mermaid-rs-renderer` behind
//!   [`MermaidRenderer`] — and holds no `App`, no `Window`, no background
//!   executor. It is gpui-free the same way `dodo-flow`'s `models/` and
//!   `geometry/` are, and for the same reason: a debounce, a stale-render
//!   check or a render-generation race is a property of the *view*, and
//!   testing those does not need a real renderer if the view is the only
//!   thing under test.
//! - [`view`] is the GPUI workspace: tabs, the editor, the live SVG preview,
//!   and the debounced, generation-guarded pipeline that calls into
//!   [`render`] from a background task rather than from `Render::render`.
//!
//! Nothing outside [`view`] names `mermaid_rs_renderer` or a GPUI type from
//! [`render`]'s module; that boundary is the whole reason two files exist
//! instead of one.

use dodo_i18n as i18n;

mod render;
mod view;

pub use render::{
    DefaultMermaidRenderer, MermaidError, MermaidRenderOutput, MermaidRenderer, MermaidTheme,
};
pub use view::{MermaidView, init};
