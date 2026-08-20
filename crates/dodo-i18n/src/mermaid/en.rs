//! The English column of the Mermaid workspace.

use std::borrow::Cow;

use super::Text;

pub(crate) fn text(text: Text) -> Cow<'static, str> {
    match text {
        Text::UntitledTab(number) => format!("Untitled {number}").into(),
        Text::NewTabTooltip => "New tab".into(),
        Text::CloseTabTooltip => "Close tab".into(),
        Text::ModeEditor => "Editor".into(),
        Text::ModeSplit => "Split".into(),
        Text::ModePreview => "Preview".into(),
        Text::EditorPlaceholder => "Type Mermaid source here.".into(),
        Text::Rendering => "Rendering…".into(),
        Text::RenderError { detail } => format!("Mermaid syntax error: {detail}").into(),
        Text::EmptyPreviewHint => "Type Mermaid source to see a preview.".into(),
        Text::StatusLabel => "Mermaid".into(),
        Text::ZoomOutLabel => "−".into(),
        Text::ZoomInLabel => "+".into(),
        Text::FitLabel => "Fit".into(),
        Text::TemplateBlank => "Blank".into(),
        Text::TemplateFlowchart => "Flowchart".into(),
        Text::TemplateSequence => "Sequence".into(),
        Text::TemplateClass => "Class".into(),
        Text::TemplateState => "State".into(),
        Text::TemplateEr => "ER".into(),
        Text::TemplateArchitecture => "Architecture".into(),
        Text::CopySourceTooltip => "Copy Mermaid source".into(),
        Text::CopySvgTooltip => "Copy SVG".into(),
        Text::SaveSourceTooltip => "Save .mmd".into(),
        Text::SaveSvgTooltip => "Save SVG".into(),
    }
}
