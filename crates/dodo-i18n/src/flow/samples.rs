//! One sample per [`Text`] variant, for the language tests.
//!
//! `samples!` also emits an exhaustive `match` over [`Text`], so a variant
//! with no entry here is a compile error.

use crate::tests::{Sample, plain};

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
}
