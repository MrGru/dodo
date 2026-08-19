//! The Flow Canvas: the tool palette and the actions beside it.
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
}
