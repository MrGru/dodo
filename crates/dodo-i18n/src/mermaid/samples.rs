//! One sample per [`Text`] variant, for the language tests.
//!
//! `samples!` also emits an exhaustive `match` over [`Text`], so a variant
//! with no entry here is a compile error.

use crate::tests::{DETAIL, NUMBER, NUMBER_TEXT, Sample, plain, term, with};

use super::Text;

samples! {
    with UntitledTab(NUMBER) [NUMBER_TEXT];
    plain NewTabTooltip;
    plain CloseTabTooltip;
    plain ModeEditor;
    plain ModeSplit;
    plain ModePreview;
    plain EditorPlaceholder;
    plain Rendering;
    with RenderError { detail: DETAIL.into() } [DETAIL];
    plain EmptyPreviewHint;
    term StatusLabel;
    term ZoomOutLabel;
    term ZoomInLabel;
    plain FitLabel;
    plain TemplateBlank;
    plain TemplateFlowchart;
    plain TemplateSequence;
    plain TemplateClass;
    plain TemplateState;
    term TemplateEr;
    plain TemplateArchitecture;
    plain CopySourceTooltip;
    plain CopySvgTooltip;
    plain SaveSourceTooltip;
    plain SaveSvgTooltip;
}
