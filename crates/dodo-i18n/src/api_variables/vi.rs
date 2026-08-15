//! The Vietnamese column of the API Explorer's variables.

use std::borrow::Cow;

use super::Text;

pub(crate) fn text(text: Text) -> Cow<'static, str> {
    match text {
        Text::ColumnKey => "KHOÁ".into(),
        Text::ColumnValue => "GIÁ TRỊ".into(),
        Text::DeleteRow => "Xoá dòng".into(),
        Text::NamePlaceholder => "Tên".into(),
        Text::NoEnvironment => "Không dùng môi trường".into(),
        Text::SelectEnvironment => "Chọn môi trường đang dùng".into(),
        Text::ManageEnvironments => "Quản lý môi trường…".into(),
        Text::Environments => "Môi trường".into(),
        Text::NewEnvironment => "Môi trường mới".into(),
        Text::DefaultEnvironmentName => "Môi trường mới".into(),
        Text::EnvironmentCopySuffix => "bản sao".into(),
        Text::DuplicateEnvironment => "Nhân bản".into(),
        Text::DeleteEnvironment => "Xoá".into(),
        Text::ImportEnvironment => "Nhập".into(),
        Text::CollectionVariables => "Biến bộ sưu tập".into(),
        Text::EnvironmentVariables => "Biến môi trường".into(),
        Text::CollectionVariablesNote => {
            "Dùng chung cho mọi yêu cầu, bất kể môi trường nào đang bật. Bộ sưu tập được \
                 nhập vào sẽ đặt biến của nó ở đây."
                .into()
        }
        Text::NoEnvironmentsYet => "Chưa có môi trường nào".into(),
        Text::NoEnvironmentsYetHint => {
            "Hãy tạo một môi trường để giữ tên máy chủ, mã thông báo hay khoá API ở một \
                 chỗ và gọi lại bằng {{name}}."
                .into()
        }
        Text::ColumnSecret => "BÍ MẬT".into(),
        Text::AddVariable => "Thêm biến".into(),
        Text::NoActiveVariables => "Chưa có biến nào".into(),
        Text::ActiveVariables(count) => format!("{count} đang bật").into(),
        Text::KeyPlaceholder => "baseUrl".into(),
        Text::ValuePlaceholder => "Giá trị".into(),
        Text::MarkSecret => "Che giá trị này trong trình sửa".into(),
        Text::RevealSecret => "Hiện giá trị".into(),
        Text::HideSecret => "Ẩn giá trị".into(),
        Text::SecretStorageWarning => {
            "Giá trị bí mật được che ở đây, nhưng vẫn lưu trên máy này dưới dạng văn bản \
                 thuần, không mã hoá, như mọi biến khác."
                .into()
        }
        Text::ResolvedUrlLabel => "Kết quả thay thế".into(),
        Text::UnresolvedVariablePreview(name) => format!("{name} chưa được định nghĩa").into(),
        Text::ResolvesFrom { name, scope } => format!("{name} — lấy từ {scope}").into(),
        Text::StoreError(detail) => format!("Không lưu hoặc đọc được môi trường: {detail}").into(),
        Text::StoreMissingVersion => {
            "Tệp môi trường này không ghi phiên bản lược đồ nên không thể đọc an toàn.".into()
        }
        Text::StoreUnsupportedVersion { found, supported } => format!(
            "Tệp môi trường này dùng lược đồ {found}; bản dodo này chỉ đọc {supported}. Hãy \
                 cập nhật dodo thay vì đọc sai tệp."
        )
        .into(),
        Text::EnvironmentImportError(detail) => {
            format!("Không nhập được môi trường đó: {detail}").into()
        }
        Text::ScriptVariables => "Kịch bản".into(),
    }
}
