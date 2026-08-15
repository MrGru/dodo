//! The Vietnamese column of the API Explorer's request side.

use std::borrow::Cow;

use super::Text;

pub(crate) fn text(text: Text) -> Cow<'static, str> {
    match text {
        Text::UrlPlaceholder => "Nhập URL rồi bấm Gửi.".into(),
        Text::Send => "Gửi".into(),
        Text::NewRequest => "Yêu cầu mới".into(),
        Text::CloseRequest => "Đóng yêu cầu".into(),
        Text::NameRequest => "Đặt tên yêu cầu này".into(),
        Text::NameRequestPlaceholder => "Tên yêu cầu".into(),
        Text::SaveName => "Lưu tên".into(),
        Text::GenerateCode => "Sinh mã".into(),
        Text::RequestTabParams => "Tham số".into(),
        Text::RequestTabHeaders => "Header".into(),
        Text::RequestTabBody => "Nội dung".into(),
        Text::RequestTabAuth => "Xác thực".into(),
        Text::RequestTabScripts => "Kịch bản".into(),
        Text::Add => "Thêm".into(),
        Text::AddParameter => "Thêm tham số".into(),
        Text::AddHeader => "Thêm header".into(),
        Text::NoActiveParams => "Không có tham số nào bật".into(),
        Text::ActiveParams(count) => format!("{count} tham số đang bật").into(),
        Text::NoActiveHeaders => "Không có header nào bật".into(),
        Text::ActiveHeaders(count) => format!("{count} header đang bật").into(),
        Text::ParamKeyPlaceholder => "Tham số".into(),
        Text::ParamValuePlaceholder => "Giá trị".into(),
        Text::HeaderKeyPlaceholder => "Tên header".into(),
        Text::HeaderValuePlaceholder => "Giá trị".into(),
        Text::ColumnDescription => "MÔ TẢ".into(),
        Text::DescriptionPlaceholder => "Mô tả".into(),
        Text::DuplicateRow => "Nhân đôi dòng".into(),
        Text::MoveRowUp => "Chuyển dòng lên".into(),
        Text::MoveRowDown => "Chuyển dòng xuống".into(),
        Text::AddField => "Thêm trường".into(),
        Text::NoActiveFields => "Không có trường nào đang bật".into(),
        Text::ActiveFields(count) => format!("{count} trường đang bật").into(),
        Text::FieldKeyPlaceholder => "Trường".into(),
        Text::FieldValuePlaceholder => "Giá trị".into(),
        Text::BodyTypeNone => "Không có".into(),
        Text::BodyTypeJson => "JSON".into(),
        Text::BodyTypeText => "Văn bản thô".into(),
        Text::BodyTypeXml => "XML".into(),
        Text::BodyTypeHtml => "HTML".into(),
        Text::BodyTypeFormData => "Dữ liệu biểu mẫu".into(),
        Text::BodyTypeUrlEncoded => "x-www-form-urlencoded".into(),
        Text::BodyTypeBinary => "Nhị phân".into(),
        Text::BodyPlaceholder => "Nhập hoặc dán nội dung yêu cầu vào đây.".into(),
        Text::NoBodyTitle => "Không có nội dung".into(),
        Text::NoBodyHint => {
            "Yêu cầu này được gửi mà không có nội dung. Chọn một loại ở trên để thêm.".into()
        }
        Text::BinaryBodyHint => "Chọn một tệp để gửi làm nội dung thô của yêu cầu.".into(),
        Text::MethodSendsNoBody(method) => {
            format!("Yêu cầu {method} được gửi mà không có nội dung.").into()
        }
        Text::AuthTypeLabel => "Kiểu xác thực".into(),
        Text::AuthTypeNone => "Không xác thực".into(),
        Text::AuthTypeBearer => "Bearer token".into(),
        Text::AuthTypeBasic => "Basic auth".into(),
        Text::AuthTypeApiKey => "API key".into(),
        Text::AuthTypeOAuth2 => "OAuth 2.0".into(),
        Text::OAuth2Later => {
            "OAuth 2.0 cần chuyển hướng trình duyệt và nơi lưu token; phần này sẽ có ở bước sau."
                .into()
        }
        Text::NoAuthTitle => "Không có xác thực".into(),
        Text::NoAuthHint => {
            "Yêu cầu này không mang header Authorization. Chọn một cách ở trên để thêm.".into()
        }
        Text::AuthTokenLabel => "Token".into(),
        Text::AuthTokenPlaceholder => "Dán bearer token vào đây".into(),
        Text::AuthUsernameLabel => "Tên đăng nhập".into(),
        Text::AuthUsernamePlaceholder => "Tên đăng nhập của bạn".into(),
        Text::AuthPasswordLabel => "Mật khẩu".into(),
        Text::AuthPasswordPlaceholder => "Mật khẩu của bạn".into(),
        Text::ApiKeyNameLabel => "Khoá".into(),
        Text::ApiKeyNamePlaceholder => "Ví dụ X-Api-Key".into(),
        Text::ApiKeyValueLabel => "Giá trị".into(),
        Text::ApiKeyValuePlaceholder => "Giá trị của khoá".into(),
        Text::ApiKeySendAs => "Gửi dưới dạng".into(),
        Text::ApiKeyInHeader => "Header".into(),
        Text::ApiKeyInQuery => "Tham số truy vấn".into(),
        Text::PreRequestScriptPlaceholder => "Chạy trước khi yêu cầu được gửi.".into(),
        Text::PostResponseScriptPlaceholder => "Chạy sau khi phản hồi về.".into(),
        Text::InvalidUrl(detail) => {
            if detail.is_empty() {
                "Hãy nhập URL trước khi gửi.".into()
            } else {
                format!("Không đọc được URL đó: {detail}").into()
            }
        }
        Text::UnsupportedScheme(scheme) => {
            format!("Công cụ này chỉ gọi được http và https, không phải {scheme}.").into()
        }
        Text::InvalidHeader(name) => {
            format!("Header \"{name}\" không gửi được như đang viết.").into()
        }
        Text::Timeout(seconds) => format!("Không có phản hồi trong {seconds} giây.").into(),
        Text::DnsFailure(host) => format!("Không tìm thấy địa chỉ \"{host}\".").into(),
        Text::ConnectFailure(detail) => format!("Không kết nối được: {detail}").into(),
        Text::TlsFailure(detail) => format!("Kết nối bảo mật bị từ chối: {detail}").into(),
        Text::BodyNotText(detail) => {
            format!("Phản hồi không phải văn bản có thể hiển thị ({detail}).").into()
        }
        Text::Unexpected(detail) => format!("Yêu cầu thất bại: {detail}").into(),
        Text::SearchCollectionsPlaceholder => "Tìm bộ sưu tập".into(),
        Text::DefaultCollectionName => "Bộ sưu tập mới".into(),
        Text::DefaultFolderName => "Thư mục mới".into(),
        Text::SaveToCollectionNote => "Đã lưu vào bộ sưu tập của bạn.".into(),
        Text::ToggleAllRows => "Bật hoặc tắt tất cả các dòng".into(),
        Text::EditModeTable => "Bảng".into(),
        Text::EditModeBulk => "Sửa hàng loạt".into(),
        Text::BulkEditPlaceholder => {
            "Mỗi dòng một mục dạng Key: Value. Bắt đầu dòng bằng # để tắt mục đó.".into()
        }
        Text::UntitledRequest => "Chưa đặt tên".into(),
        Text::ColumnType => "LOẠI".into(),
        Text::FieldKindText => "Văn bản".into(),
        Text::FieldKindFile => "Tệp".into(),
        Text::ChooseFile => "Chọn tệp…".into(),
        Text::ReplaceFile => "Chọn tệp khác".into(),
        Text::ClearFile => "Bỏ tệp đã chọn".into(),
        Text::NoFileSelected => "Chưa chọn tệp".into(),
        Text::IncompleteFileFields(count) => {
            format!("{count} trường tệp chưa chọn tệp nên sẽ không được gửi.").into()
        }
        Text::FileUnreadable { path, detail } => format!("Không đọc được {path}: {detail}").into(),
        Text::FileTooLarge { path, limit_mb } => {
            format!("{path} lớn hơn mức {limit_mb} MB mà bản dựng này gửi được.").into()
        }
        Text::UnresolvedVariable(name) => format!(
            "Chưa có biến nào tên {name}. Hãy thêm nó vào một môi trường hoặc vào \
                         biến bộ sưu tập rồi gửi lại."
        )
        .into(),
        Text::RecursiveVariable(name) => {
            format!("Biến {name} tham chiếu lại chính nó nên không thể thay thế được.").into()
        }
        Text::ScriptFinished { millis } => {
            format!("Kịch bản trước yêu cầu chạy xong trong {millis} ms.").into()
        }
        Text::ScriptWroteVariables(count) => format!("Kịch bản đã ghi {count} biến.").into(),
        Text::ScriptUnknownMethod(method) => format!(
            "Kịch bản yêu cầu phương thức {method} mà dodo không có; phương thức trong \
                 trình soạn thảo được giữ nguyên."
        )
        .into(),
        Text::ConsoleRunTruncated(count) => {
            format!("{count} dòng của lần chạy này đã bị bỏ.").into()
        }
        Text::ScriptSyntaxError(detail) => format!("Lỗi cú pháp: {detail}").into(),
        Text::TestScriptFinished { millis } => {
            format!("Kịch bản sau phản hồi chạy xong trong {millis} ms.").into()
        }
        Text::CodeTargetCurl => "cURL".into(),
        Text::CodeTargetFetch => "fetch".into(),
        Text::CodeTargetAxios => "axios".into(),
        Text::CodeTargetXhr => "XMLHttpRequest".into(),
        Text::GenerateCodeCarriesValues => {
            "Đoạn mã này mang đúng các giá trị thật của yêu cầu, kể cả token hay mật \
                 khẩu mà nó dùng."
                .into()
        }
        Text::GenerateCodeSecretsWithheld(names) => format!(
            "Được giữ nguyên dạng {{{{chỗ trống}}}}: {names}. Mọi thứ còn lại — kể cả \
                 token hay mật khẩu gõ trực tiếp vào yêu cầu này — đều nằm trong đoạn mã \
                 bên dưới."
        )
        .into(),
        Text::GenerateCodeSecretsRevealed => {
            "Đoạn mã này chứa giá trị thật của mọi biến bí mật mà nó dùng, ở dạng văn \
                 bản thuần. Bất cứ nơi nào bạn dán vào cũng giữ lại giá trị đó."
                .into()
        }
        Text::GenerateCodeRevealSecrets => "Thay thế cả biến bí mật".into(),
    }
}
