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
        Text::ZoomOutLabel => "−".into(),
        Text::ZoomInLabel => "+".into(),
        Text::FitLabel => "Fit".into(),
        Text::TemplatesTooltip => "Insert a template".into(),
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
        Text::ThemeTooltip => "Diagram theme".into(),
        Text::ThemePanelTitle => "Theme for this diagram".into(),
        Text::ThemeClosePanelTooltip => "Close the theme panel".into(),
        Text::ThemePresetLabel => "Preset".into(),
        Text::ThemePresetAutomatic => "Automatic (app appearance)".into(),
        Text::ThemePresetModern => "Modern".into(),
        Text::ThemePresetDefault => "Mermaid default".into(),
        Text::ThemePresetDark => "Dark".into(),
        Text::ThemePresetForest => "Forest".into(),
        Text::ThemePresetNeutral => "Neutral".into(),
        Text::ThemeFieldFontFamily => "Font family".into(),
        Text::ThemeFieldFontSize => "Font size".into(),
        Text::ThemeFieldShapeFill => "Shape fill".into(),
        Text::ThemeFieldShapeText => "Shape text".into(),
        Text::ThemeFieldShapeBorder => "Shape border".into(),
        Text::ThemeFieldLines => "Lines and arrows".into(),
        Text::ThemeFieldBackground => "Background".into(),
        Text::ThemeFieldEdgeLabel => "Edge label background".into(),
        Text::ThemeFieldSubgraphFill => "Subgraph fill".into(),
        Text::ThemeFieldSubgraphBorder => "Subgraph border".into(),
        Text::ThemeFontSizeSmaller => "−".into(),
        Text::ThemeFontSizeLarger => "+".into(),
        Text::ThemeDefaultValue { value } => format!("Default: {value}").into(),
        Text::ThemeChangedTooltip => "Changed from the preset".into(),
        Text::ThemeResetFieldTooltip => "Back to the preset default".into(),
        Text::ThemeResetAll => "Reset every field".into(),
        Text::ThemeDiagramSpecificNote => {
            "Sequence, Git and pie styles follow the preset and are not edited here.".into()
        }
    }
}
