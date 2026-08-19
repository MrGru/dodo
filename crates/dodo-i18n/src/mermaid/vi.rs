//! The Vietnamese column of the Mermaid workspace.

use std::borrow::Cow;

use super::Text;

pub(crate) fn text(text: Text) -> Cow<'static, str> {
    match text {
        Text::UntitledTab(number) => format!("Chưa đặt tên {number}").into(),
        Text::NewTabTooltip => "Thẻ mới".into(),
        Text::CloseTabTooltip => "Đóng thẻ".into(),
        Text::ModeEditor => "Trình soạn thảo".into(),
        Text::ModeSplit => "Chia đôi".into(),
        Text::ModePreview => "Xem trước".into(),
        Text::EditorPlaceholder => "Nhập mã nguồn Mermaid vào đây.".into(),
        Text::Rendering => "Đang dựng hình…".into(),
        Text::RenderError { detail } => format!("Lỗi cú pháp Mermaid: {detail}").into(),
        Text::EmptyPreviewHint => "Nhập mã nguồn Mermaid để xem trước.".into(),
        Text::StatusLabel => "Mermaid".into(),
        Text::ZoomOutLabel => "−".into(),
        Text::ZoomInLabel => "+".into(),
        Text::FitLabel => "Vừa khung".into(),
        Text::TemplateBlank => "Trống".into(),
        Text::TemplateFlowchart => "Lưu đồ".into(),
        Text::TemplateSequence => "Trình tự".into(),
        Text::TemplateClass => "Lớp".into(),
        Text::TemplateState => "Trạng thái".into(),
        Text::TemplateEr => "ER".into(),
        Text::TemplateArchitecture => "Kiến trúc".into(),
        Text::CopySourceTooltip => "Sao chép mã nguồn Mermaid".into(),
        Text::CopySvgTooltip => "Sao chép SVG".into(),
        Text::SaveSourceTooltip => "Lưu .mmd".into(),
        Text::SaveSvgTooltip => "Lưu SVG".into(),
    }
}
