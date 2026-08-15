//! One sample per [`Text`] variant, for the language tests.
//!
//! `samples!` also emits an exhaustive `match` over [`Text`], so a variant
//! with no entry here is a compile error.

use crate::tests::{NUMBER, NUMBER_TEXT, Sample, plain, with};

use super::Text;

samples! {
    plain ResponseTabBody;
    plain ResponseTabHeaders;
    plain ResponseTabCookies;
    plain ResponseTabTests;
    plain ResponseTabConsole;
    plain NoResponseYet;
    plain NoResponseHint;
    plain Sending;
    plain RequestFailed;
    plain CollapseResponse;
    plain ExpandResponse;
    plain BodyPretty;
    plain BodyRaw;
    plain LoadMoreLines;
    with LineRange { shown: NUMBER, total: 77 } [NUMBER_TEXT, "77"];
    plain StatusClassInfo;
    plain StatusClassSuccess;
    plain StatusClassRedirect;
    plain StatusClassClientError;
    plain StatusClassServerError;
    plain StatusClassUnknown;
    plain BodyPreview;
    plain BodyTree;
    plain SaveToFile;
    with JsonTreeTruncated(NUMBER) [NUMBER_TEXT];
    plain HtmlPreviewNote;
    plain NoCookies;
    plain NoCookiesHint;
}
