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
//! # Where the theme is, and is not
//!
//! [`MermaidTheme`] used to be a two-variant `Light`/`Dark` enum living here,
//! and it is now [`crate::theme`]'s: a preset plus the general fields the user
//! has moved off it. The split is the same boundary this file already draws,
//! one step further out — [`crate::theme`] holds the *rules* (which fields are
//! editable, what a valid colour is, what resetting means) with no dependency
//! on `mermaid_rs_renderer` at all, and this file holds the only translation
//! between those rules and the renderer's own 40-field `Theme`. Three
//! functions make up that translation and each is an exhaustive `match`, so a
//! field added to [`ThemeField`] cannot be forgotten in any of them:
//! [`preset_defaults`] reads a preset out, [`write_field`] merges an override
//! in, and [`renderer_theme`] is the pair applied in order.
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

use crate::theme::{
    MermaidTheme, MermaidThemePreset, ThemeDefaults, ThemeField, format_font_size, parse_font_size,
};

/// The five preset constructors, as one exhaustive `match`.
///
/// `Theme::from_name(&str)` exists upstream and is deliberately not used: a
/// preset that upstream renames or drops must break the build here rather than
/// return `None` at runtime and silently fall back.
fn renderer_preset(preset: MermaidThemePreset) -> Theme {
    match preset {
        MermaidThemePreset::Modern => Theme::modern(),
        MermaidThemePreset::MermaidDefault => Theme::mermaid_default(),
        MermaidThemePreset::Dark => Theme::dark(),
        MermaidThemePreset::Forest => Theme::forest(),
        MermaidThemePreset::Neutral => Theme::neutral(),
    }
}

/// The ten general fields `preset` ships with, for the theme panel to show
/// and for [`crate::theme::TabTheme::set`] to compare against.
///
/// This is the whole reason [`ThemeDefaults`] is a table of `String`s rather
/// than the renderer's own `Theme`: it is the one direction in which the
/// renderer's type crosses out of this file, and it crosses as data.
pub(crate) fn preset_defaults(preset: MermaidThemePreset) -> ThemeDefaults {
    let theme = renderer_preset(preset);
    ThemeDefaults::new(ThemeField::ALL.map(|field| read_field(&theme, field)))
}

/// One general field's value out of a renderer theme.
///
/// Paired with [`write_field`], and the pairing is asserted below
/// (`every_field_reads_back_what_it_wrote`): a `read`/`write` mismatch would
/// show up as a row whose control edits one thing and whose value displays
/// another, which is exactly the "a style field no painter reads" failure
/// `dodo-flow`'s `properties.rs` has met repeatedly.
fn read_field(theme: &Theme, field: ThemeField) -> String {
    match field {
        ThemeField::FontFamily => theme.font_family.clone(),
        ThemeField::FontSize => format_font_size(theme.font_size),
        ThemeField::PrimaryColor => theme.primary_color.clone(),
        ThemeField::PrimaryTextColor => theme.primary_text_color.clone(),
        ThemeField::PrimaryBorderColor => theme.primary_border_color.clone(),
        ThemeField::LineColor => theme.line_color.clone(),
        ThemeField::Background => theme.background.clone(),
        ThemeField::EdgeLabelBackground => theme.edge_label_background.clone(),
        ThemeField::ClusterBackground => theme.cluster_background.clone(),
        ThemeField::ClusterBorder => theme.cluster_border.clone(),
    }
}

/// One general field's value into a renderer theme.
///
/// A size that will not parse is *dropped* rather than written: values reach
/// here only through [`ThemeField::canonical`], so an unparsable one is a bug
/// in this crate rather than user input, and leaving the preset's own size in
/// place renders a diagram while a `0.0` or a NaN would render none.
///
/// [`ThemeField::canonical`]: crate::theme::ThemeField::canonical
fn write_field(theme: &mut Theme, field: ThemeField, value: &str) {
    match field {
        ThemeField::FontFamily => theme.font_family = value.to_string(),
        ThemeField::FontSize => {
            if let Some(size) = parse_font_size(value) {
                theme.font_size = size;
            }
        }
        ThemeField::PrimaryColor => theme.primary_color = value.to_string(),
        ThemeField::PrimaryTextColor => theme.primary_text_color = value.to_string(),
        ThemeField::PrimaryBorderColor => theme.primary_border_color = value.to_string(),
        ThemeField::LineColor => theme.line_color = value.to_string(),
        ThemeField::Background => theme.background = value.to_string(),
        ThemeField::EdgeLabelBackground => theme.edge_label_background = value.to_string(),
        ThemeField::ClusterBackground => theme.cluster_background = value.to_string(),
        ThemeField::ClusterBorder => theme.cluster_border = value.to_string(),
    }
}

/// A [`MermaidTheme`] as the renderer's own `Theme`: the preset, then the
/// user's overrides merged on top.
///
/// **Merged, not rebuilt.** The preset's remaining ~30 fields — every
/// `sequence_*`, `git_*` and `pie_*` value — are carried through untouched,
/// which is what "the diagram-specific groups follow the preset" means in
/// code. There is no second table of defaults anywhere in this crate.
fn renderer_theme(theme: &MermaidTheme) -> Theme {
    let mut resolved = renderer_preset(theme.base());
    for (field, value) in theme.overrides() {
        write_field(&mut resolved, field, value);
    }
    resolved
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
            theme: renderer_theme(&theme),
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
    use crate::theme::{TabTheme, ThemeFieldKind};

    fn renderer() -> DefaultMermaidRenderer {
        DefaultMermaidRenderer
    }

    fn render_ok(source: &str) -> MermaidRenderOutput {
        renderer()
            .render(source, MermaidTheme::default())
            .unwrap_or_else(|error| panic!("expected a render, got {error}: {source}"))
    }

    fn themed(preset: MermaidThemePreset, overrides: &[(ThemeField, &str)]) -> MermaidTheme {
        let defaults = preset_defaults(preset);
        let mut tab = TabTheme::default();
        tab.choose_preset(preset);
        for (field, value) in overrides {
            tab.set(*field, value, preset, &defaults)
                .unwrap_or_else(|error| panic!("{field:?} rejected {value:?}: {error:?}"));
        }
        tab.resolve(preset)
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
        assert!(renderer().render("", MermaidTheme::default()).is_err());
    }

    #[test]
    fn whitespace_only_source_is_a_controlled_error_not_a_panic() {
        assert!(
            renderer()
                .render("   \n\t  \n", MermaidTheme::default())
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
        let result = renderer().render("flowchart LR\n  --> --> -->\n", MermaidTheme::default());
        assert!(result.is_err());
    }

    #[test]
    fn unrecognised_diagram_keyword_is_a_controlled_error() {
        let result = renderer().render("notADiagramType\n  A --> B\n", MermaidTheme::default());
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

    /// The whole point of the appearance mapping: the two presets dodo's
    /// light and dark appearances resolve to must actually produce visibly
    /// different SVGs, or "fits dodo's appearance" is a no-op.
    #[test]
    fn light_and_dark_appearances_render_different_svgs() {
        let source = "flowchart LR\n  A --> B\n";
        let light = renderer()
            .render(
                source,
                MermaidTheme::preset(MermaidThemePreset::for_appearance(false)),
            )
            .unwrap();
        let dark = renderer()
            .render(
                source,
                MermaidTheme::preset(MermaidThemePreset::for_appearance(true)),
            )
            .unwrap();
        assert_ne!(light.svg, dark.svg);
    }

    /// Every preset has to reach the renderer as itself. Rendering the same
    /// source five times and finding two identical SVGs would mean a preset
    /// arm pointing at the wrong constructor — a mistake nothing else here
    /// could see.
    #[test]
    fn every_preset_renders_its_own_svg() {
        let source = "flowchart LR\n  A[Start] --> B[End]\n";
        let mut rendered: Vec<(MermaidThemePreset, String)> = Vec::new();
        for preset in MermaidThemePreset::ALL {
            let svg = renderer()
                .render(source, MermaidTheme::preset(preset))
                .unwrap_or_else(|error| panic!("{preset:?} failed to render: {error}"))
                .svg;
            for (other, other_svg) in &rendered {
                assert_ne!(&svg, other_svg, "{preset:?} renders exactly like {other:?}");
            }
            rendered.push((preset, svg));
        }
    }

    /// The panel's promise, checked at the only place it can be: a field the
    /// user changed must reach the renderer, and the SVG must differ from the
    /// untouched preset's.
    #[test]
    fn every_editable_field_changes_the_rendered_svg() {
        // One source exercising a node, its label, an edge with a label, a
        // subgraph and the page behind them, so every general field has
        // something to colour.
        let source = "flowchart LR\n  subgraph Group\n    A[Start] -->|go| B[End]\n  end\n";
        let base = renderer()
            .render(source, MermaidTheme::preset(MermaidThemePreset::Modern))
            .unwrap()
            .svg;

        for field in ThemeField::ALL {
            let value = match field.kind() {
                ThemeFieldKind::FontStack => "Courier New, monospace",
                ThemeFieldKind::Size => "26",
                ThemeFieldKind::Colour => "#FF00FF",
            };
            let themed = themed(MermaidThemePreset::Modern, &[(field, value)]);
            let svg = renderer()
                .render(source, themed)
                .unwrap_or_else(|error| panic!("{field:?} failed to render: {error}"))
                .svg;
            assert_ne!(
                svg, base,
                "{field:?} is offered in the theme panel but changes nothing the renderer draws"
            );
        }
    }

    /// The other half of the panel's promise: the groups it says it does not
    /// touch really do follow the preset. `pie_colors` is the clearest probe —
    /// it is derived from the preset's own base colours while the preset is
    /// being *constructed*, so an override merged on afterwards must not move
    /// it.
    #[test]
    fn overriding_a_general_field_leaves_the_diagram_specific_groups_alone() {
        let base = renderer_preset(MermaidThemePreset::Modern);
        let merged = renderer_theme(&themed(
            MermaidThemePreset::Modern,
            &[
                (ThemeField::PrimaryColor, "#FF00FF"),
                (ThemeField::ClusterBackground, "#00FF00"),
            ],
        ));

        assert_eq!(merged.pie_colors, base.pie_colors);
        assert_eq!(merged.git_colors, base.git_colors);
        assert_eq!(merged.sequence_actor_fill, base.sequence_actor_fill);
        assert_eq!(merged.sequence_note_fill, base.sequence_note_fill);
        // …while the two fields that *were* overridden did move.
        assert_eq!(merged.primary_color, "#FF00FF");
        assert_eq!(merged.cluster_background, "#00FF00");
    }

    /// [`read_field`] and [`write_field`] are two exhaustive matches over one
    /// enum, which is exactly the shape in which one arm quietly points at the
    /// wrong renderer field.
    #[test]
    fn every_field_reads_back_what_it_wrote() {
        for field in ThemeField::ALL {
            let written = match field.kind() {
                ThemeFieldKind::FontStack => "Sentinel Sans",
                ThemeFieldKind::Size => "23",
                ThemeFieldKind::Colour => "#0F0F0F",
            };
            let mut theme = renderer_preset(MermaidThemePreset::Modern);
            write_field(&mut theme, field, written);
            assert_eq!(read_field(&theme, field), written, "{field:?}");

            // …and it wrote *only* that field: every other one still reads the
            // preset's own value.
            let pristine = renderer_preset(MermaidThemePreset::Modern);
            for other in ThemeField::ALL {
                if other == field {
                    continue;
                }
                assert_eq!(
                    read_field(&theme, other),
                    read_field(&pristine, other),
                    "writing {field:?} also moved {other:?}"
                );
            }
        }
    }

    /// The theme panel paints a swatch for every colour row, including the
    /// untouched ones, so every preset's own spelling of every colour field
    /// has to be one [`crate::theme::canonical_colour`] understands — the
    /// presets use bare names, `rgba(…)`, `hsl(…)` and 3-digit hex as well as
    /// full hex.
    #[test]
    fn every_preset_default_parses() {
        for preset in MermaidThemePreset::ALL {
            let defaults = preset_defaults(preset);
            for field in ThemeField::ALL {
                let value = defaults.get(field);
                assert!(
                    field.canonical(value).is_ok(),
                    "{preset:?}'s {field:?} is {value:?}, which the theme panel cannot show"
                );
            }
        }
    }
}
