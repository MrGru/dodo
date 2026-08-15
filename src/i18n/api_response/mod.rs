//! The API Explorer's response viewer.
//!
//! `en` and `vi` each render every variant below; the compiler names any
//! string a language has not been given.

pub(crate) mod en;
pub(crate) mod vi;

#[cfg(test)]
pub(crate) mod samples;

/// The strings this area owns.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Text {
    // API Explorer — response viewer.
    ResponseTabBody,
    ResponseTabHeaders,
    ResponseTabCookies,
    ResponseTabTests,
    ResponseTabConsole,
    NoResponseYet,
    NoResponseHint,
    Sending,
    RequestFailed,
    CollapseResponse,
    ExpandResponse,
    BodyPretty,
    BodyRaw,
    LoadMoreLines,
    /// "{shown} of {total} lines" — the response body footer.
    LineRange {
        shown: usize,
        total: usize,
    },

    // API Explorer — status classes.
    StatusClassInfo,
    StatusClassSuccess,
    StatusClassRedirect,
    StatusClassClientError,
    StatusClassServerError,
    StatusClassUnknown,

    // API Explorer — response viewer polish (phase 3).
    BodyPreview,
    BodyTree,
    SaveToFile,
    /// "Showing the first {count} nodes — collapse some to see the rest."
    JsonTreeTruncated(usize),
    HtmlPreviewNote,
    NoCookies,
    NoCookiesHint,
}
