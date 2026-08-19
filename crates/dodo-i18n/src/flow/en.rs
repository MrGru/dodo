//! The English column of the Flow Canvas.

use std::borrow::Cow;

use super::Text;

pub(crate) fn text(text: Text) -> Cow<'static, str> {
    match text {
        Text::ToolSelect => "Select".into(),
        Text::ToolHand => "Hand".into(),
        Text::ToolRectangle => "Rectangle".into(),
        Text::ToolDiamond => "Diamond".into(),
        Text::ToolEllipse => "Ellipse".into(),
        Text::ToolArrow => "Arrow".into(),
        Text::ToolLine => "Line".into(),
        Text::ToolGraphNode => "Graph node".into(),
        Text::ToolText => "Text".into(),
        Text::TextPlaceholder => "Type something".into(),
        Text::Delete => "Delete the selection".into(),
        Text::KeepToolActive => "Keep the tool active after drawing".into(),
    }
}
