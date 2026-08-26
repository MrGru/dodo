//! The Mermaid workspace: the tab bar, the editor, the preview, and the
//! controls floating inside each pane.
//!
//! `en` and `vi` each render every variant below; the compiler names any
//! string a language has not been given.

pub(crate) mod en;
pub(crate) mod vi;

#[cfg(test)]
pub(crate) mod samples;

/// The strings this area owns.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Text {
    // The tab bar.
    /// "Untitled {n}" — a new tab's default title before the user names it.
    UntitledTab(usize),
    NewTabTooltip,
    CloseTabTooltip,

    // The three workspace modes.
    ModeEditor,
    ModeSplit,
    ModePreview,

    // The editor.
    EditorPlaceholder,

    // The preview, and the two signals that float in its top-left corner.
    // Both used to sit on a status bar across the bottom of the workspace;
    // that bar is gone and they are not, because the preview deliberately
    // keeps the last good render when the source stops parsing.
    /// Shown only once rendering has run long enough to be visible — see
    /// `dodo-flow`'s sibling rule for why a sub-threshold render shows nothing.
    Rendering,
    /// The frame around the renderer's own message, which is third-party and
    /// English — the same treatment `json_formatter::Text::InvalidJson` gives
    /// serde_json's text.
    RenderError {
        detail: String,
    },
    EmptyPreviewHint,

    // The zoom cluster floating in the preview's bottom-right corner
    // (workspace plan phase 4). `ZoomOutLabel` and `ZoomInLabel` are the
    // glyphs on the two step buttons; both languages draw the same symbol.
    // `FitLabel` resets zoom and pan (also Cmd-0).
    ZoomOutLabel,
    ZoomInLabel,
    FitLabel,

    // The editor's floating template button (workspace plan phase 6): a
    // small, fixed template set, not a library. Each *appends* its example
    // source to the document being edited, which is why there is no `Blank`
    // entry — a row that appends nothing would do nothing.
    TemplatesTooltip,
    TemplateFlowchart,
    TemplateSequence,
    TemplateClass,
    TemplateState,
    TemplateEr,
    TemplateArchitecture,

    // Copy and save (workspace plan phase 6, "Required" scope). PNG/PDF are
    // explicitly out of scope — see that section's "Explicitly out of scope".
    CopySourceTooltip,
    CopySvgTooltip,
    SaveSourceTooltip,
    SaveSvgTooltip,
}
