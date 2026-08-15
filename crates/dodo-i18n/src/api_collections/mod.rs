//! The API Explorer's collections and history panels.
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
    // API Explorer — collections panel.
    Collections,
    NoCollections,
    NoCollectionsHint,

    // API Explorer — collections panel (phase 3).
    ImportCollection,
    NewCollection,
    NewFolder,
    Rename,
    Duplicate,
    Open,
    MoreActions,
    /// The store's own IO/serde message is third-party English, kept verbatim.
    CollectionStoreError(String),
    CollectionImportError(String),

    // API Explorer — request history (phase 3).
    History,
    NoHistory,
    NoHistoryHint,
    HistoryReopen,
    HistoryResend,
    HistoryClearAll,
    HistoryJustNow,
    /// "{minutes}m ago" — how long ago a request in the history ran.
    HistoryMinutesAgo(u64),
    HistoryHoursAgo(u64),
    HistoryDaysAgo(u64),
}
