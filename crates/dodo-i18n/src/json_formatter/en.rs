//! The English column of the JSON formatter.

use std::borrow::Cow;

use super::Text;

pub(crate) fn text(text: Text) -> Cow<'static, str> {
    match text {
        Text::JsonPlaceholder => "Paste JSON here, then click Format.".into(),
        Text::IndentLabel => "Indent:".into(),
        Text::IndentSpaces(count) => format!("{count} spaces").into(),
        Text::InvalidJson {
            line,
            column,
            detail,
        } => format!("Invalid JSON at line {line}, column {column}: {detail}").into(),
    }
}
