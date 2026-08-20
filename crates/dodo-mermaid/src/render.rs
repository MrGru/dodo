//! Mermaid source in, SVG out — the whole surface between dodo and
//! `mermaid-rs-renderer`, and nothing else.
//!
//! Nothing outside this file names `mermaid_rs_renderer` directly. [`view`]'s
//! render path calls [`MermaidRenderer::render`], not the upstream crate,
//! because the two questions "how does dodo talk to a Mermaid workspace" and
//! "which Rust crate turns Mermaid text into SVG" are answered in different
//! places on purpose: swapping the renderer, or adding a second one for a
//! diagram family upstream does not cover, changes this file and nothing that
//! calls it.
//!
//! **No GPUI here, on purpose.** This module is parse-and-render only — a
//! `&str` in, a [`MermaidRenderOutput`] or a [`MermaidError`] out, with no
//! window, no view and no background executor. That is what makes every test
//! below a plain `#[test]`: the debounce, the render-generation bookkeeping
//! and the "off the UI thread" requirement all belong to [`view`], which owns
//! a tab's state, not to the service that renders one string. Keeping them
//! apart is what lets this module's tests run in milliseconds and the view's
//! tests run with a fake clock instead of a real renderer.
//!
//! [`view`]: crate::view
//!
//! # Error isolation
//!
//! [`DefaultMermaidRenderer::render`] never panics on malformed input — Mermaid
//! source is untrusted text, exactly like a pasted database URI or a pasted
//! cURL command, and a syntax error must produce a [`MermaidError`] the
//! workspace can show beside the last good preview, not a crash that takes the
//! rest of dodo with it.

use std::fmt;

use mermaid_rs_renderer::{RenderOptions, Theme, render_with_timing};

/// Which of the renderer's built-in colour themes to draw with.
///
/// Deliberately just these two — the workspace plan's "start simple" rule for
/// theming (§20): dodo's own `Dodo`/`System` appearance already resolves to
/// one of light or dark before it reaches here (see [`view`]'s call site), so
/// there is nothing for a third variant to mean.
///
/// [`view`]: crate::view
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MermaidTheme {
    #[default]
    Light,
    Dark,
}

impl MermaidTheme {
    fn into_renderer_theme(self) -> Theme {
        match self {
            MermaidTheme::Light => Theme::modern(),
            MermaidTheme::Dark => Theme::dark(),
        }
    }
}

/// One successful render: the SVG text, plus the three timings the upstream
/// crate already measures. The timings are `pub` so a view can feed them to
/// `tracing` or a debug overlay during development (see this crate's `AGENTS`
/// note in the workspace plan, §9) — they are not meant to become permanent
/// user-facing UI.
#[derive(Debug, Clone, PartialEq)]
pub struct MermaidRenderOutput {
    pub svg: String,
    pub parse_us: u128,
    pub layout_us: u128,
    pub render_us: u128,
}

/// Why a render did not produce an [`MermaidRenderOutput`].
///
/// One variant, not one per pipeline stage: `mermaid-rs-renderer`'s own error
/// type carries a message but not which stage produced it, so a `Parse` versus
/// `Render` split here would be a distinction this crate cannot actually make —
/// every call site would still see one message and have to guess. If a future
/// upstream release exposes the stage, split this then.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MermaidError(String);

impl fmt::Display for MermaidError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Mermaid render error: {}", self.0)
    }
}

impl std::error::Error for MermaidError {}

/// The interface a GPUI view renders against.
///
/// A trait, not a bare function, so a view under test can hand itself a fake
/// that returns a fixed [`MermaidRenderOutput`] or [`MermaidError`] without
/// linking the real renderer or paying for font/layout initialisation per
/// [`DefaultMermaidRenderer`]'s laziness note below.
pub trait MermaidRenderer {
    fn render(
        &self,
        source: &str,
        theme: MermaidTheme,
    ) -> Result<MermaidRenderOutput, MermaidError>;
}

/// The renderer dodo ships: `mermaid-rs-renderer` behind [`MermaidRenderer`].
///
/// Holds nothing today — `mermaid-rs-renderer`'s API is a free function, so
/// there is no font cache or parser state to own yet. The struct exists so a
/// view can hold one field of a stable type regardless of what upstream's API
/// looks like release to release, and so *lazy construction* is a real
/// decision a view makes (`OnceCell`, or built on first use) rather than
/// something this crate has an opinion about. Root `AGENTS.md`'s "no eager
/// startup work" rule is a call site's job, not this type's.
#[derive(Debug, Default, Clone, Copy)]
pub struct DefaultMermaidRenderer;

impl MermaidRenderer for DefaultMermaidRenderer {
    fn render(
        &self,
        source: &str,
        theme: MermaidTheme,
    ) -> Result<MermaidRenderOutput, MermaidError> {
        let options = RenderOptions {
            theme: theme.into_renderer_theme(),
            ..RenderOptions::default()
        };
        let timed =
            render_with_timing(source, options).map_err(|error| MermaidError(error.to_string()))?;

        Ok(MermaidRenderOutput {
            svg: timed.svg,
            parse_us: timed.parse_us,
            layout_us: timed.layout_us,
            render_us: timed.render_us,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn renderer() -> DefaultMermaidRenderer {
        DefaultMermaidRenderer
    }

    fn render_ok(source: &str) -> MermaidRenderOutput {
        renderer()
            .render(source, MermaidTheme::Light)
            .unwrap_or_else(|error| panic!("expected a render, got {error}: {source}"))
    }

    #[test]
    fn flowchart_renders_to_svg() {
        let output = render_ok("flowchart LR\n  A[Request] --> B{Auth}\n  B --> C[API]\n");
        assert!(output.svg.contains("<svg"), "{}", output.svg);
    }

    #[test]
    fn sequence_diagram_renders_to_svg() {
        let output = render_ok("sequenceDiagram\n  Alice->>Bob: Hello\n  Bob-->>Alice: Hi\n");
        assert!(output.svg.contains("<svg"), "{}", output.svg);
    }

    #[test]
    fn class_diagram_renders_to_svg() {
        let output = render_ok("classDiagram\n  Animal <|-- Duck\n  Animal : +String name\n");
        assert!(output.svg.contains("<svg"), "{}", output.svg);
    }

    #[test]
    fn state_diagram_renders_to_svg() {
        let output = render_ok("stateDiagram-v2\n  [*] --> Idle\n  Idle --> Running\n");
        assert!(output.svg.contains("<svg"), "{}", output.svg);
    }

    #[test]
    fn er_diagram_renders_to_svg() {
        let output = render_ok("erDiagram\n  USER ||--o{ ORDER : places\n");
        assert!(output.svg.contains("<svg"), "{}", output.svg);
    }

    /// dodo has multilingual ambitions (root `AGENTS.md`) — a label the
    /// renderer cannot lay out is a regression this crate has to catch before
    /// any view does.
    #[test]
    fn vietnamese_labels_render_to_svg() {
        let output =
            render_ok("flowchart TD\n  A[Xin chào] --> B[Đăng nhập]\n  B --> C[Cơ sở dữ liệu]\n");
        assert!(output.svg.contains("<svg"), "{}", output.svg);
    }

    #[test]
    fn a_long_label_renders_to_svg() {
        let long_label = "word ".repeat(40);
        let source = format!("flowchart LR\n  A[{long_label}] --> B[End]\n");
        let output = render_ok(&source);
        assert!(output.svg.contains("<svg"), "{}", output.svg);
    }

    #[test]
    fn empty_source_is_a_controlled_error_not_a_panic() {
        assert!(renderer().render("", MermaidTheme::Light).is_err());
    }

    #[test]
    fn whitespace_only_source_is_a_controlled_error_not_a_panic() {
        assert!(
            renderer()
                .render("   \n\t  \n", MermaidTheme::Light)
                .is_err()
        );
    }

    /// The upstream parser is deliberately forgiving — most garbled text still
    /// parses as *some* diagram rather than erroring, which is a property of
    /// the renderer this crate does not get to change. Bare arrows with no
    /// node on either side is one shape it does reject, and stands in here for
    /// "malformed syntax produces `MermaidError`, not a panic".
    #[test]
    fn malformed_syntax_is_a_controlled_error_not_a_panic() {
        let result = renderer().render("flowchart LR\n  --> --> -->\n", MermaidTheme::Light);
        assert!(result.is_err());
    }

    #[test]
    fn unrecognised_diagram_keyword_is_a_controlled_error() {
        let result = renderer().render("notADiagramType\n  A --> B\n", MermaidTheme::Light);
        assert!(result.is_err());
    }

    /// Timing fields are always present on success — a view feeding them to
    /// `tracing` should never see a render with no numbers behind it.
    #[test]
    fn a_successful_render_carries_timing() {
        let output = render_ok("flowchart LR\n  A --> B\n");
        // Timings are `u128` (unsigned) and therefore never negative; the only
        // meaningful assertion is that the field is populated at all, so this
        // just proves the struct construction path is exercised.
        let _ = (output.parse_us, output.layout_us, output.render_us);
    }

    /// The whole point of [`MermaidTheme`]: the two variants must actually
    /// produce visibly different SVGs, or "fits dodo's appearance" is a no-op.
    #[test]
    fn light_and_dark_themes_render_different_svgs() {
        let source = "flowchart LR\n  A --> B\n";
        let light = renderer().render(source, MermaidTheme::Light).unwrap();
        let dark = renderer().render(source, MermaidTheme::Dark).unwrap();
        assert_ne!(light.svg, dark.svg);
    }
}
