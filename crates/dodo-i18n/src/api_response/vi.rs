//! The Vietnamese column of the API Explorer's response viewer.

use std::borrow::Cow;

use super::Text;

pub(crate) fn text(text: Text) -> Cow<'static, str> {
    match text {
        Text::ResponseTabBody => "Nội dung".into(),
        Text::ResponseTabHeaders => "Header".into(),
        Text::ResponseTabCookies => "Cookie".into(),
        Text::ResponseTabTests => "Kiểm thử".into(),
        Text::ResponseTabConsole => "Nhật ký".into(),
        Text::NoResponseYet => "Chưa có phản hồi".into(),
        Text::NoResponseHint => "Gửi yêu cầu để xem phản hồi ở đây.".into(),
        Text::Sending => "Đang gửi…".into(),
        Text::RequestFailed => "THẤT BẠI".into(),
        Text::CollapseResponse => "Thu gọn phản hồi".into(),
        Text::ExpandResponse => "Mở rộng phản hồi".into(),
        Text::BodyPretty => "Đẹp".into(),
        Text::BodyRaw => "Thô".into(),
        Text::LoadMoreLines => "Tải thêm dòng".into(),
        Text::LineRange { shown, total } => format!("{shown} trên {total} dòng").into(),
        Text::StatusClassInfo => "THÔNG TIN".into(),
        Text::StatusClassSuccess => "THÀNH CÔNG".into(),
        Text::StatusClassRedirect => "CHUYỂN HƯỚNG".into(),
        Text::StatusClassClientError => "LỖI PHÍA GỌI".into(),
        Text::StatusClassServerError => "LỖI MÁY CHỦ".into(),
        Text::StatusClassUnknown => "KHÔNG RÕ".into(),
        Text::BodyPreview => "Xem trước".into(),
        Text::BodyTree => "Cây".into(),
        Text::SaveToFile => "Lưu ra tệp".into(),
        Text::JsonTreeTruncated(count) => {
            format!("Đang hiện {count} nút đầu — thu gọn bớt để xem phần còn lại.").into()
        }
        Text::HtmlPreviewNote => {
            "Xem trước văn bản — mã đánh dấu hiển thị dạng chữ, không kết xuất.".into()
        }
        Text::NoCookies => "Không có cookie nào".into(),
        Text::NoCookiesHint => "Phản hồi này không gửi header Set-Cookie nào.".into(),
    }
}
