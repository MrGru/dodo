//! The Vietnamese column of the JSON formatter.

use std::borrow::Cow;

use super::Text;

pub(crate) fn text(text: Text) -> Cow<'static, str> {
    match text {
        Text::JsonPlaceholder => "Dán JSON vào đây rồi bấm Định dạng.".into(),
        Text::IndentLabel => "Thụt lề:".into(),
        Text::IndentSpaces(count) => format!("{count} khoảng trắng").into(),
        Text::InvalidJson {
            line,
            column,
            detail,
        } => format!("JSON không hợp lệ tại dòng {line}, cột {column}: {detail}").into(),
    }
}
