//! The Vietnamese column of the shared strings.

use std::borrow::Cow;

use super::Text;

pub(crate) fn text(text: Text) -> Cow<'static, str> {
    match text {
        Text::FormatButton => "Định dạng".into(),
        Text::Delete => "Xoá".into(),
    }
}
