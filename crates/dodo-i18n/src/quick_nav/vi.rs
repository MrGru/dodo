//! The Vietnamese column of quick navigation.

use std::borrow::Cow;

use super::Text;

pub(crate) fn text(text: Text) -> Cow<'static, str> {
    match text {
        Text::CurlPattern => "Mẫu cURL".into(),
        Text::DatabasePattern => "Mẫu URI cơ sở dữ liệu".into(),
        Text::JwtPattern => "Mẫu JWT".into(),
        Text::JsonPattern => "Mẫu JSON".into(),
        Text::Base64Pattern => "Mẫu Base64".into(),
        Text::PatternInvalid(detail) => {
            format!("Mẫu này không hợp lệ nên dodo đang dùng mẫu dựng sẵn: {detail}").into()
        }
        Text::PatternTooLong { length, limit } => format!(
            "Mẫu này dài {length} ký tự, vượt giới hạn {limit}. dodo đang dùng mẫu dựng sẵn."
        )
        .into(),
        Text::StoreError(detail) => {
            format!("Không đọc hoặc ghi được quick-nav.json: {detail}").into()
        }
        Text::StoreMissingVersion => {
            "quick-nav.json không có trường version nên không phải do dodo ghi. dodo giữ \
                 nguyên tệp và dùng giá trị mặc định."
                .into()
        }
        Text::StoreUnsupportedVersion { found, understood } => format!(
            "quick-nav.json là phiên bản {found}; bản dodo này hiểu phiên bản {understood}. \
                 dodo dùng giá trị mặc định và giữ nguyên tệp."
        )
        .into(),
    }
}
