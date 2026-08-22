//! One sample per [`Text`] variant, for the language tests.
//!
//! `samples!` also emits an exhaustive `match` over [`Text`], so a variant
//! with no entry here is a compile error.

use crate::tests::{DETAIL, Sample, plain, term, with};

use super::Text;

samples! {
    plain ToolSelect;
    plain ToolHand;
    plain ToolRectangle;
    plain ToolDiamond;
    plain ToolEllipse;
    plain ToolArrow;
    plain ToolLine;
    plain ToolGraphNode;
    plain ToolText;
    plain TextPlaceholder;
    plain Delete;
    plain KeepToolActive;
    plain HandDrawnStyle;

    plain SectionStroke;
    plain SectionBackground;
    plain SectionFill;
    plain SectionStrokeWidth;
    plain SectionStrokeStyle;
    plain SectionSloppiness;
    plain SectionEdges;
    plain SectionArrowType;
    plain SectionArrowheads;
    plain SectionFontFamily;
    plain SectionFontSize;
    plain SectionTextAlign;
    plain SectionOpacity;
    plain SectionLayers;
    plain SectionActions;
    plain FillHachure;
    plain FillCrossHatch;
    plain FillSolid;
    plain StrokeWidthThin;
    plain StrokeWidthBold;
    plain StrokeWidthExtraBold;
    plain StrokeStyleSolid;
    plain StrokeStyleDashed;
    plain StrokeStyleDotted;
    plain SloppinessArchitect;
    plain SloppinessArtist;
    plain SloppinessCartoonist;
    plain SloppinessNeedsSketch;
    plain EdgesSharp;
    plain EdgesRound;
    plain ArrowStraight;
    plain ArrowCurved;
    plain ArrowElbow;
    plain ArrowheadStart;
    plain ArrowheadEnd;
    plain FontHandDrawn;
    plain FontNormal;
    plain FontCode;
    plain AlignLeft;
    plain AlignCenter;
    plain AlignRight;
    plain AlignTop;
    plain AlignMiddle;
    plain AlignBottom;
    plain LayerSendToBack;
    plain LayerSendBackward;
    plain LayerBringForward;
    plain LayerBringToFront;
    plain ActionDuplicate;
    plain ActionInsertImage;
    plain ActionCropToFrame;
    plain ActionCropWhole;
    plain CropNeedsFrame;
    plain ImageNotReadable;
    plain ActionLink;
    plain LinkPlaceholder;
    plain ColorPlaceholder;
    plain ColorFromTheme;
    with StorageProblem(DETAIL.into()) [DETAIL];
    plain StorageLoadConflict;

    // The four size glyphs are the same one or two Latin letters in every
    // language — they are the picture as much as the label.
    term FontSizeSmall;
    term FontSizeMedium;
    term FontSizeLarge;
    term FontSizeExtraLarge;
}
