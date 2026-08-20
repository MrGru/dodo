//! The Vietnamese column of the Input method tool.

use std::borrow::Cow;

use super::Text;

pub(crate) fn text(text: Text) -> Cow<'static, str> {
    match text {
        Text::Description => {
            "Cấu hình gõ tiếng Việt. Chặn sự kiện cần quyền Trợ năng của macOS và hoạt động khi Dodo đang mở.".into()
        }
        Text::WindowsDescription => {
            "Cấu hình gõ tiếng Việt. Keyboard Hook hoạt động khi Dodo đang mở.".into()
        }
        Text::StorageProblem => "Tệp thiết lập".into(),
        Text::StoreError(detail) => {
            format!("Không thể đọc hoặc lưu thiết lập bộ gõ: {detail}").into()
        }
        Text::StoreMissingVersion => {
            "Tệp thiết lập bộ gõ không ghi phiên bản lược đồ nên không thể đọc an toàn.".into()
        }
        Text::StoreUnsupportedVersion { found, supported } => format!(
            "Tệp thiết lập bộ gõ dùng lược đồ {found}; bản dodo này chỉ đọc {supported}. Hãy cập nhật dodo thay vì đọc sai tệp."
        ).into(),
        Text::EventTapStatus => "Trạng thái chặn sự kiện".into(),
        Text::EventTapInactive => "Chặn sự kiện chưa hoạt động.".into(),
        Text::EventTapNeedsAccessibility => {
            "macOS cần bạn bật Dodo trong Cài đặt hệ thống → Quyền riêng tư và bảo mật → Trợ năng. Các phím được chuyển qua không thay đổi.".into()
        }
        Text::EventTapRunning => {
            "Chặn sự kiện hoạt động khi Dodo đang mở. Dodo không bao giờ lưu hoặc gửi nội dung bạn gõ.".into()
        }
        Text::EventTapFailed => {
            "Không thể khởi động chặn sự kiện. Các phím được chuyển qua không thay đổi.".into()
        }
        Text::KeyboardHookStatus => "Trạng thái Keyboard Hook".into(),
        Text::KeyboardHookInactive => "Keyboard Hook chưa hoạt động.".into(),
        Text::KeyboardHookRunning => {
            "Keyboard Hook chỉ hoạt động khi Dodo đang mở. Dodo không bao giờ lưu hoặc gửi nội dung bạn gõ.".into()
        }
        Text::KeyboardHookFailed => {
            "Không thể khởi động Keyboard Hook. Các phím được chuyển qua không thay đổi.".into()
        }
        Text::Scheme => "Kiểu gõ".into(),
        Text::SchemeDescription => {
            "Telex bỏ dấu bằng chữ (aa, ow, s, f); VNI bỏ dấu bằng số (a6, o7, 1, 2).".into()
        }
        Text::Telex => "Telex".into(),
        Text::Vni => "VNI".into(),
        Text::TonePlacement => "Vị trí dấu thanh".into(),
        Text::TonePlacementDescription => {
            "Kiểu mới đặt dấu trên nguyên âm chính (hoà); kiểu cũ đặt trên nguyên âm đầu (hòa).".into()
        }
        Text::ToneModern => "Kiểu mới".into(),
        Text::ToneTraditional => "Kiểu cũ".into(),
        Text::SpellCheck => "Kiểm tra chính tả".into(),
        Text::SpellCheckDescription => {
            "Trả lại đúng các ký tự đã gõ khi kết quả không phải âm tiết tiếng Việt, để từ tiếng Anh không bị đổi.".into()
        }
        Text::BracketShortcuts => "Phím ngoặc".into(),
        Text::BracketShortcutsDescription => {
            "Trong Telex, [ và ] gõ ơ và ư — cách duy nhất để gõ uơ (thuở, huơ).".into()
        }
        Text::ActiveLanguages => "Ngôn ngữ đang bật".into(),
        Text::ActiveLanguagesDescription => {
            "Chọn các ngôn ngữ có trong menu và được phím tắt chuyển đổi sử dụng.".into()
        }
        Text::LanguageDescription => "Chọn ngôn ngữ nhập hiện tại.".into(),
        Text::LanguageSwitch => "Chuyển ngôn ngữ".into(),
        Text::LanguageSwitchDescription => {
            "Luân chuyển các ngôn ngữ đang bật. Nhấp vào phím tắt rồi nhấn tổ hợp phím bạn muốn.".into()
        }
        Text::ShortcutBeep => "Âm báo".into(),
        Text::ShortcutSpace => "Phím cách".into(),
        Text::ShortcutEnter => "Phím Enter".into(),
        Text::ShortcutTab => "Phím Tab".into(),
        Text::ShortcutEscape => "Phím Esc".into(),
        Text::ShortcutRecording => "Nhấn tổ hợp phím…".into(),
        Text::ShortcutUnsupportedKey => {
            "Không ghi được phím đó. Hãy giữ một phím bổ trợ rồi nhấn một phím không gõ ra chữ, hoặc giữ riêng hai phím bổ trợ.".into()
        }
        Text::ShortcutBackspace => "Phím xóa lùi".into(),
        Text::ShortcutDelete => "Phím xóa tới".into(),
        Text::ShortcutHome => "Phím Home".into(),
        Text::ShortcutEnd => "Phím End".into(),
        Text::ShortcutPageUp => "Phím lên trang".into(),
        Text::ShortcutPageDown => "Phím xuống trang".into(),
        Text::ShortcutArrowLeft => "Mũi tên trái".into(),
        Text::ShortcutArrowRight => "Mũi tên phải".into(),
        Text::ShortcutArrowUp => "Mũi tên lên".into(),
        Text::ShortcutArrowDown => "Mũi tên xuống".into(),
        Text::BrowserFix => "Thanh địa chỉ trình duyệt".into(),
        Text::BrowserFixDescription => {
            "Xử lý các trình duyệt vẫn bôi đen gợi ý tự động trong lúc bạn gõ, nếu không dấu thanh sẽ rơi vào sai chữ.".into()
        }
    }
}
