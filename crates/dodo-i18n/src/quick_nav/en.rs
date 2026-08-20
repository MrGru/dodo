//! The English column of quick navigation.

use std::borrow::Cow;

use super::Text;

pub(crate) fn text(text: Text) -> Cow<'static, str> {
    match text {
        Text::CurlPattern => "cURL pattern".into(),
        Text::DatabasePattern => "Database URI pattern".into(),
        Text::JwtPattern => "JWT pattern".into(),
        Text::JsonPattern => "JSON pattern".into(),
        Text::Base64Pattern => "Base64 pattern".into(),
        Text::MermaidPattern => "Mermaid pattern".into(),
        Text::PatternInvalid(detail) => {
            format!("This pattern is not valid, so the built-in one is being used: {detail}").into()
        }
        Text::PatternTooLong { length, limit } => format!(
            "This pattern is {length} characters long; the limit is {limit}. The built-in one \
                 is being used."
        )
        .into(),
        Text::StoreError(detail) => {
            format!("quick-nav.json could not be read or written: {detail}").into()
        }
        Text::StoreMissingVersion => {
            "quick-nav.json carries no version, so it was not written by dodo. It is being \
                 left alone and the defaults are in use."
                .into()
        }
        Text::StoreUnsupportedVersion { found, understood } => format!(
            "quick-nav.json is version {found}; this dodo understands {understood}. The \
                 defaults are in use and the file is being left alone."
        )
        .into(),
    }
}
