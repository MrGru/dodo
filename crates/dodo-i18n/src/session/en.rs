//! The English column of session restoration.

use std::borrow::Cow;

use super::Text;

pub(crate) fn text(text: Text) -> Cow<'static, str> {
    match text {
        Text::StoreError(detail) => {
            format!("session.json could not be read or written: {detail}").into()
        }
        Text::StoreMissingVersion => {
            "session.json carries no version, so it was not written by dodo. It is being left \
                 alone and nothing is being saved this run."
                .into()
        }
        Text::StoreUnsupportedVersion { found, understood } => format!(
            "session.json is version {found}; this dodo understands {understood}. It is \
                     being left alone and nothing is being saved this run."
        )
        .into(),
        Text::FeatureLastVisibleTool => "At least one tool has to stay in the sidebar.".into(),
    }
}
