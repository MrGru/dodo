//! The Flow Canvas: the tool palette, the actions beside it, and the
//! contextual property panel.
//!
//! **This area exists four phases before the canvas reaches the sidebar**, and
//! that is the decision worth recording. The sidebar row is deliberately last,
//! so the palette, the Delete action, the tool lock and the property panel that
//! follows them would otherwise be written as English literals and retrofitted
//! in one pass at the end — every string touched twice, and every pass an
//! invitation for a bare literal to slip past the two guards. The catalogue
//! comes first instead, and the canvas has had no untranslated string since.
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
    // ---- the tool palette (§45) ----
    //
    // One per `CanvasTool`. The palette draws each button's glyph from the
    // canvas's own outline builders, so these are its tooltips rather than its
    // labels — the shortcut beside them is rendered from the real binding
    // table and is not a string.
    ToolSelect,
    ToolHand,
    ToolRectangle,
    ToolDiamond,
    ToolEllipse,
    ToolArrow,
    ToolLine,
    ToolGraphNode,
    ToolText,

    // ---- §9's text editor ----
    /// The prompt inside an empty text editor, on a node, on an edge or on a
    /// standalone text element. One string for all three: it says what to do,
    /// and what to do is the same.
    TextPlaceholder,

    // ---- the actions beside the tools ----
    /// Removes the selection, from the toolbar or from the keyboard.
    Delete,
    /// The tool lock: with it on, finishing a drawing keeps the tool rather
    /// than returning to Select.
    KeepToolActive,

    // ---- the contextual property panel (Phase 11) ----
    //
    // Every control on the panel is iconographic — line samples, wobble
    // samples, corner and arrow glyphs — so with one exception these are
    // *section labels* and *tooltips* rather than button captions. The
    // exception is the font-size row, whose glyphs are the letters S, M, L and
    // XL: those are drawn from the size names below rather than hard-coded,
    // because a language that sizes its text differently is exactly the case a
    // catalogue exists for.

    // The section labels, in panel order.
    SectionStroke,
    SectionBackground,
    SectionFill,
    SectionStrokeWidth,
    SectionStrokeStyle,
    SectionSloppiness,
    SectionEdges,
    SectionArrowType,
    SectionArrowheads,
    SectionFontFamily,
    SectionFontSize,
    SectionTextAlign,
    SectionOpacity,
    SectionLayers,
    SectionActions,

    // Fill.
    FillHachure,
    FillCrossHatch,
    FillSolid,

    // Stroke width.
    StrokeWidthThin,
    StrokeWidthBold,
    StrokeWidthExtraBold,

    // Stroke style.
    StrokeStyleSolid,
    StrokeStyleDashed,
    StrokeStyleDotted,

    // Sloppiness. Excalidraw's three names, because they say what the hand is
    // rather than how rough the number is.
    SloppinessArchitect,
    SloppinessArtist,
    SloppinessCartoonist,
    /// Why the Sloppiness row is muted: it edits a real property that a clean
    /// drawing cannot show.
    SloppinessNeedsSketch,

    // Edges (the corner style).
    EdgesSharp,
    EdgesRound,

    // Arrow type.
    ArrowStraight,
    ArrowCurved,
    ArrowElbow,

    // Arrowheads, as two toggles.
    ArrowheadStart,
    ArrowheadEnd,

    // Font family.
    FontHandDrawn,
    FontNormal,
    FontCode,

    /// The four discrete sizes, drawn *as* the button's glyph. Short by
    /// design — one or two characters — because they are the picture as well as
    /// the label.
    FontSizeSmall,
    FontSizeMedium,
    FontSizeLarge,
    FontSizeExtraLarge,

    // Text alignment.
    AlignLeft,
    AlignCenter,
    AlignRight,

    /// Vertical alignment — the panel's fourth text row. It is drawn with no
    /// heading (the reference screenshot's), so these three are the only words
    /// a user ever sees for it and they carry the whole meaning of the row.
    AlignTop,
    AlignMiddle,
    AlignBottom,

    // Layers.
    LayerSendToBack,
    LayerSendBackward,
    LayerBringForward,
    LayerBringToFront,

    // Actions. `Delete` is Phase 9's and is reused rather than duplicated.
    ActionDuplicate,
    /// **Inserts §10's picture** — an action beside the tools rather than a
    /// tool, because it opens a file picker instead of changing what the next
    /// press means. See `dodo_flow::views::palette`.
    ActionInsertImage,
    /// The Crop button, when the frame is a different shape from the picture in
    /// it: the press trims the picture to the frame.
    ActionCropToFrame,
    /// The same button when the picture is already cropped to its frame: the
    /// press shows the whole of it again.
    ActionCropWhole,
    /// Why the Crop button is muted: there is nothing to crop to yet, and the
    /// gesture that makes something is a shift-drag on a corner.
    CropNeedsFrame,
    /// The picker chose a file no decoder here would take.
    ImageNotReadable,
    /// Opens the link editor.
    ActionLink,
    /// The link editor's placeholder.
    LinkPlaceholder,
    /// The colour editor's placeholder, on the swatch past the separator.
    ColorPlaceholder,
    /// The tooltip on that swatch when there is no colour of the element's own
    /// to show — the theme is answering.
    ColorFromTheme,

    // Phase 8: the app-owned persisted document.
    StorageProblem(String),
    StorageLoadConflict,
}
