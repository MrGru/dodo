//! dodo's Mermaid workspace: write Mermaid source, see it as SVG immediately.
//!
//! One GPUI file and four gpui-free ones, and the split is the whole point:
//!
//! - [`render`] is the pure service — `mermaid-rs-renderer` behind
//!   [`MermaidRenderer`] — and holds no `App`, no `Window`, no background
//!   executor.
//! - [`workspace`] is what a mode means and what closing a tab leaves behind,
//!   [`templates`] is the template set and the rule for appending one into a
//!   buffer, and [`zoom`] is the preview's transform arithmetic.
//! - [`view`] is the GPUI workspace: tabs, the editor, the live SVG preview,
//!   the floating controls in each pane, and the debounced,
//!   generation-guarded pipeline that calls into [`render`] from a background
//!   task rather than from `Render::render`.
//!
//! Everything but [`view`] is gpui-free the same way `dodo-flow`'s `models/`
//! and `geometry/` are, and for the same reason: a debounce, a stale-render
//! check or a render-generation race is a property of the *view*, and testing
//! the rules a gesture obeys does not need a window. In this crate that is
//! more than a preference — [`view`]'s module doc records that a
//! `#[gpui::test]` cannot be added here at all at the pinned `gpui` revision,
//! so a decision left inside [`view`] is a decision no test can reach.
//!
//! Nothing outside [`view`] names a GPUI type, and nothing outside [`render`]
//! names `mermaid_rs_renderer`; those two boundaries are why there are five
//! files instead of one.

use dodo_i18n as i18n;

mod render;
mod templates;
mod view;
mod workspace;
mod zoom;

pub use render::{
    DefaultMermaidRenderer, MermaidError, MermaidRenderOutput, MermaidRenderer, MermaidTheme,
};
pub use view::{MermaidView, init};
