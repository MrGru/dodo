//! The Vietnamese column of session restoration.

use std::borrow::Cow;

use super::Text;

pub(crate) fn text(text: Text) -> Cow<'static, str> {
    match text {
        Text::StoreError(detail) => {
            format!("Không đọc hoặc ghi được session.json: {detail}").into()
        }
        Text::StoreMissingVersion => {
            "session.json không có trường version nên không phải do dodo ghi. dodo giữ nguyên \
                 tệp và không lưu gì trong lần chạy này."
                .into()
        }
        Text::StoreUnsupportedVersion { found, understood } => format!(
            "session.json là phiên bản {found}; bản dodo này hiểu phiên bản {understood}. \
                     dodo giữ nguyên tệp và không lưu gì trong lần chạy này."
        )
        .into(),
        Text::FeatureLastVisibleTool => "Thanh bên phải giữ lại ít nhất một công cụ.".into(),
    }
}
