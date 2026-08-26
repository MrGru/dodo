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
        Text::ZoomOutLabel => "−".into(),
        Text::ZoomInLabel => "+".into(),
        Text::FitLabel => "Vừa khung".into(),
        Text::TemplatesTooltip => "Chèn mẫu".into(),
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
        Text::ThemeTooltip => "Giao diện sơ đồ".into(),
        Text::ThemePanelTitle => "Giao diện của sơ đồ này".into(),
        Text::ThemeClosePanelTooltip => "Đóng bảng giao diện".into(),
        Text::ThemePresetLabel => "Bộ dựng sẵn".into(),
        Text::ThemePresetAutomatic => "Tự động (theo giao diện ứng dụng)".into(),
        Text::ThemePresetModern => "Hiện đại".into(),
        Text::ThemePresetDefault => "Mermaid mặc định".into(),
        Text::ThemePresetDark => "Tối".into(),
        Text::ThemePresetForest => "Rừng".into(),
        Text::ThemePresetNeutral => "Trung tính".into(),
        Text::ThemeFieldFontFamily => "Phông chữ".into(),
        Text::ThemeFieldFontSize => "Cỡ chữ".into(),
        Text::ThemeFieldShapeFill => "Nền hình khối".into(),
        Text::ThemeFieldShapeText => "Chữ trong hình khối".into(),
        Text::ThemeFieldShapeBorder => "Viền hình khối".into(),
        Text::ThemeFieldLines => "Đường nối và mũi tên".into(),
        Text::ThemeFieldBackground => "Nền".into(),
        Text::ThemeFieldEdgeLabel => "Nền nhãn đường nối".into(),
        Text::ThemeFieldSubgraphFill => "Nền nhóm con".into(),
        Text::ThemeFieldSubgraphBorder => "Viền nhóm con".into(),
        Text::ThemeFontSizeSmaller => "−".into(),
        Text::ThemeFontSizeLarger => "+".into(),
        Text::ThemeDefaultValue { value } => format!("Mặc định: {value}").into(),
        Text::ThemeChangedTooltip => "Đã đổi khác bộ dựng sẵn".into(),
        Text::ThemeResetFieldTooltip => "Trở về giá trị mặc định".into(),
        Text::ThemeResetAll => "Đặt lại mọi mục".into(),
        Text::ThemeDiagramSpecificNote => {
            "Kiểu của sơ đồ trình tự, Git và hình quạt theo bộ dựng sẵn, không chỉnh ở đây.".into()
        }
    }
}
