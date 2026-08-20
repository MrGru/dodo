//! The Vietnamese column of the Flow Canvas.

use std::borrow::Cow;

use super::Text;

pub(crate) fn text(text: Text) -> Cow<'static, str> {
    match text {
        Text::ToolSelect => "Chọn".into(),
        Text::ToolHand => "Bàn tay".into(),
        Text::ToolRectangle => "Hình chữ nhật".into(),
        Text::ToolDiamond => "Hình thoi".into(),
        Text::ToolEllipse => "Hình elip".into(),
        Text::ToolArrow => "Mũi tên".into(),
        Text::ToolLine => "Đường thẳng".into(),
        Text::ToolGraphNode => "Nút đồ thị".into(),
        Text::Delete => "Xoá phần đang chọn".into(),
        Text::KeepToolActive => "Giữ nguyên công cụ sau khi vẽ".into(),
    }
}
