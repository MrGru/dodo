//! The English column of the shared strings.

use std::borrow::Cow;

use super::Text;

pub(crate) fn text(text: Text) -> Cow<'static, str> {
    match text {
        Text::FormatButton => "Format".into(),
        Text::Delete => "Delete".into(),
    }
}
