//! The English column of the API Explorer's collections.

use std::borrow::Cow;

use super::Text;

pub(crate) fn text(text: Text) -> Cow<'static, str> {
    match text {
        Text::Collections => "Collections".into(),
        Text::NoCollections => "No collections yet".into(),
        Text::NoCollectionsHint => "Saved requests will be grouped here.".into(),
        Text::ImportCollection => "Import a collection".into(),
        Text::NewCollection => "New collection".into(),
        Text::NewFolder => "New folder".into(),
        Text::Rename => "Rename".into(),
        Text::Duplicate => "Duplicate".into(),
        Text::Open => "Open".into(),
        Text::MoreActions => "Actions".into(),
        Text::CollectionStoreError(detail) => {
            format!("Could not save collections: {detail}").into()
        }
        Text::CollectionImportError(detail) => {
            format!("Could not import that file: {detail}").into()
        }
        Text::History => "History".into(),
        Text::NoHistory => "No requests yet".into(),
        Text::NoHistoryHint => "Requests you send appear here, newest first.".into(),
        Text::HistoryReopen => "Reopen in a new tab".into(),
        Text::HistoryResend => "Resend".into(),
        Text::HistoryClearAll => "Clear all".into(),
        Text::HistoryJustNow => "just now".into(),
        Text::HistoryMinutesAgo(minutes) => format!("{minutes}m ago").into(),
        Text::HistoryHoursAgo(hours) => format!("{hours}h ago").into(),
        Text::HistoryDaysAgo(days) => format!("{days}d ago").into(),
    }
}
