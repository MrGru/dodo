//! The English column of the API Explorer's response viewer.

use std::borrow::Cow;

use super::Text;

pub(crate) fn text(text: Text) -> Cow<'static, str> {
    match text {
        Text::ResponseTabBody => "Body".into(),
        Text::ResponseTabHeaders => "Headers".into(),
        Text::ResponseTabCookies => "Cookies".into(),
        Text::ResponseTabTests => "Tests".into(),
        Text::ResponseTabConsole => "Console".into(),
        Text::NoResponseYet => "No response yet".into(),
        Text::NoResponseHint => "Send the request to see the response here.".into(),
        Text::Sending => "Sending…".into(),
        Text::RequestFailed => "FAILED".into(),
        Text::CollapseResponse => "Collapse response".into(),
        Text::ExpandResponse => "Expand response".into(),
        Text::BodyPretty => "Pretty".into(),
        Text::BodyRaw => "Raw".into(),
        Text::LoadMoreLines => "Load more lines".into(),
        Text::LineRange { shown, total } => format!("{shown} of {total} lines").into(),
        Text::StatusClassInfo => "INFO".into(),
        Text::StatusClassSuccess => "SUCCESS".into(),
        Text::StatusClassRedirect => "REDIRECT".into(),
        Text::StatusClassClientError => "CLIENT ERR".into(),
        Text::StatusClassServerError => "SERVER ERR".into(),
        Text::StatusClassUnknown => "UNKNOWN".into(),
        Text::BodyPreview => "Preview".into(),
        Text::BodyTree => "Tree".into(),
        Text::SaveToFile => "Save to file".into(),
        Text::JsonTreeTruncated(count) => {
            format!("Showing the first {count} nodes — collapse some to see the rest.").into()
        }
        Text::HtmlPreviewNote => {
            "Text preview — markup is shown as readable text, not rendered.".into()
        }
        Text::NoCookies => "No cookies set".into(),
        Text::NoCookiesHint => "This response sent no Set-Cookie headers.".into(),
    }
}
