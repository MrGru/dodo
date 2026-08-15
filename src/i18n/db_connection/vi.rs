//! The Vietnamese column of the Database Explorer's connection form.

use std::borrow::Cow;

use super::Text;

pub(crate) fn text(text: Text) -> Cow<'static, str> {
    match text {
        Text::NewConnection => "Kết nối mới".into(),
        Text::EditConnectionTitle => "Sửa kết nối".into(),
        Text::Cancel => "Huỷ".into(),
        Text::Save => "Lưu".into(),
        Text::FieldName => "Tên".into(),
        Text::FieldNamePlaceholder => "Không bắt buộc".into(),
        Text::FieldEngine => "Loại".into(),
        Text::FieldHost => "Máy chủ".into(),
        Text::FieldPort => "Cổng".into(),
        Text::FieldDatabase => "Cơ sở dữ liệu".into(),
        Text::FieldUser => "Người dùng".into(),
        Text::FieldUrl => "URL".into(),
        Text::FieldPassword => "Mật khẩu".into(),
        Text::FieldFile => "Tệp".into(),
        Text::FieldFilePlaceholder => "Đường dẫn tới tệp cơ sở dữ liệu".into(),
        Text::FieldSsl => "TLS".into(),
        Text::SslDisable => "Tắt".into(),
        Text::SslPrefer => "Ưu tiên".into(),
        Text::SslRequire => "Bắt buộc".into(),
        Text::PasswordStorageNotice => {
            "Mật khẩu đã lưu được giữ ở dạng không mã hoá trong thư mục dữ liệu của dodo, \
                 giống các biến bí mật của API Explorer. Ai đọc được thư mục đó thì đọc được \
                 mật khẩu."
                .into()
        }
        Text::RevealPassword => "Hiện mật khẩu".into(),
        Text::HidePassword => "Ẩn mật khẩu".into(),
        Text::TestConnection => "Thử kết nối".into(),
        Text::Testing => "Đang thử…".into(),
        Text::TestSucceeded => "Kết nối hoạt động tốt.".into(),
        Text::ProfileHostMissing => "Hãy nhập máy chủ.".into(),
        Text::ProfilePortMissing => "Hãy nhập cổng.".into(),
        Text::ProfileDatabaseMissing => "Hãy nhập tên cơ sở dữ liệu.".into(),
        Text::ProfileFileMissing => "Hãy chọn tệp cơ sở dữ liệu.".into(),
        Text::ConnectionStoreError(detail) => format!("Không thể lưu các kết nối: {detail}").into(),
        Text::ConnectionStoreMissingVersion => {
            "Tệp kết nối đã lưu không có phiên bản lược đồ nên không thể đọc được.".into()
        }
        Text::ConnectionStoreUnsupportedVersion { found, supported } => format!(
            "Các kết nối đã lưu được ghi bởi một bản dodo mới hơn (phiên bản {found}; bản này \
                 hiểu {supported}). Hãy cập nhật dodo để mở chúng."
        )
        .into(),
        Text::ProfileRedisDatabaseInvalid => "Hãy nhập số cơ sở dữ liệu logic không âm.".into(),
        Text::FieldUri => "URI kết nối".into(),
        Text::FieldUriPlaceholder => "postgresql://user:password@host:5432/database".into(),
        Text::FillFromUri => "Điền từ URI".into(),
        Text::UriFilled => "Đã điền từ URI. Hãy kiểm tra các trường trước khi lưu.".into(),
        Text::UriIgnored(parts) => format!("Đã đọc nhưng không áp dụng: {parts}").into(),
        Text::UriTlsNotApplied => {
            "URI này yêu cầu TLS, nhưng ứng dụng khách Redis của dodo kết nối mà không dùng \
                 TLS."
                .into()
        }
        Text::UriEmpty => "Hãy dán một URI kết nối trước.".into(),
        Text::UriNoScheme => {
            "Chuỗi này không có lược đồ nên không biết đây là cơ sở dữ liệu nào. Hãy bắt đầu \
                 bằng postgresql://, mysql://, sqlite:// hoặc redis://."
                .into()
        }
        Text::UriUnknownScheme(scheme) => format!(
            "dodo không kết nối được tới \"{scheme}\". Hãy dùng postgresql, mysql, sqlite \
                 hoặc redis."
        )
        .into(),
        Text::UriInvalidPort(port) => format!("\"{port}\" không phải là số cổng.").into(),
        Text::UriMissingFile => "URI này không nêu tệp cơ sở dữ liệu nào.".into(),
        Text::UriInvalidEscape => {
            "Một chuỗi thoát phần trăm trong URI này không phải UTF-8 hợp lệ.".into()
        }
    }
}
