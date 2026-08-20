//! The Vietnamese column of the shell.

use std::borrow::Cow;

use super::Text;

pub(crate) fn text(text: Text) -> Cow<'static, str> {
    match text {
        Text::Settings => "Cài đặt".into(),
        Text::General => "Chung".into(),
        Text::Appearance => "Giao diện".into(),
        Text::Language => "Ngôn ngữ".into(),
        Text::LanguageDescription => {
                "Ngôn ngữ dùng cho các nhãn của ứng dụng.".into()
            }
        Text::Theme => "Chủ đề".into(),
        Text::ThemeDescription => "Bảng màu của toàn bộ ứng dụng.".into(),
        Text::FontSize => "Cỡ chữ".into(),
        Text::FontSizeDescription => {
                "Cỡ chữ cơ bản của ứng dụng.".into()
            }
        Text::BorderRadius => "Bo góc".into(),
        Text::BorderRadiusDescription => {
                "Độ bo góc của nút, ô nhập và khung.".into()
            }
        Text::Large => "Lớn".into(),
        Text::Medium => "Vừa".into(),
        Text::Small => "Nhỏ".into(),
        Text::SearchSettingsPlaceholder => {
                "Tìm cài đặt, rồi nhấn Enter để chuyển tới".into()
            }
        Text::NoSettingsMatch => {
                "Không có cài đặt nào khớp với tìm kiếm đó.".into()
            }
        Text::Tools => "Công cụ".into(),
        Text::JsonFormatterTitle => "Định dạng JSON".into(),
        Text::EncoderDecoderTitle => "Mã hoá / Giải mã".into(),
        Text::ApiExplorerTitle => "Khám phá API".into(),
        Text::CleanerTitle => "Dọn dẹp".into(),
        Text::DiagramTitle => "Sơ đồ".into(),
        Text::RunScripts => "Chạy kịch bản".into(),
        Text::RunScriptsDescription => {
                "API Explorer có chạy kịch bản đi kèm yêu cầu hay không. Kịch bản đến từ bộ \
                 sưu tập nhập vào là mã của người khác."
                    .into()
            }
        Text::CheckForUpdates => "Kiểm tra cập nhật".into(),
        Text::DatabaseTitle => "Cơ sở dữ liệu".into(),
        Text::QuickNavigation => "Điều hướng nhanh".into(),
        Text::QuickNavEnabled => "Dán để điều hướng".into(),
        Text::QuickNavEnabledDescription => {
                "Khi không có ô nhập nào đang được chọn, Cmd+V, Ctrl+V hoặc p sẽ đọc bảng nhớ tạm \
                 và mở công cụ xử lý được nội dung đó. Nhấn Esc trong ô nhập để rời khỏi nó trước."
                    .into()
            }
        Text::QuickNavGateDescription => {
                "Tùy chọn. dodo đã có bộ phân tích thật cho định dạng này và luôn dùng nó; mẫu ở \
                 đây chỉ thu hẹp phần được đưa vào bộ phân tích. Để trống để thử bộ phân tích với \
                 mọi nội dung."
                    .into()
            }
        Text::QuickNavShapeDescription => {
                "Hình dạng mà một ứng viên phải có. Để trống để dùng mẫu dựng sẵn; dù thế nào thì \
                 nội dung vẫn phải giải mã được thì dodo mới chuyển sang công cụ."
                    .into()
            }
        Text::QuickNavStorageProblem => "Cài đặt đã lưu".into(),
        Text::SessionStorageProblem => "Phiên đã lưu".into(),
        Text::Features => "Tính năng".into(),
        Text::FeaturesDescription => {
                "Chọn những công cụ hiện trong thanh bên và thứ tự của chúng. Kéo một dòng bằng \
                 tay cầm, hoặc dùng các mũi tên."
                    .into()
            }
        Text::FeatureShowInSidebar => "Hiện trong thanh bên".into(),
        Text::FeatureDragToReorder => "Kéo để sắp xếp lại".into(),
        Text::FeatureMoveUp => "Chuyển lên".into(),
        Text::FeatureMoveDown => "Chuyển xuống".into(),
        Text::InputMethod => "Bộ gõ".into(),
        Text::StartWithOs => "Khởi động cùng hệ điều hành".into(),
        Text::StartWithOsDescription => {
                "Khởi động Dodo trong khay khi bạn đăng nhập. macOS cần macOS 13 trở lên và Dodo.app đã đóng gói; Windows thêm mục Khởi động cho người dùng hiện tại.".into()
            }
        Text::StartWithOsChecking => "Đang kiểm tra trạng thái…".into(),
        Text::StartWithOsStatusUnknown => {
                "Không có trạng thái".into()
            }
    }
}
