//! One sample per [`Text`] variant, for the language tests.
//!
//! `samples!` also emits an exhaustive `match` over [`Text`], so a variant
//! with no entry here is a compile error.

use crate::i18n::tests::{DETAIL, NUMBER, NUMBER_TEXT, Sample, plain, with};

use super::Text;

samples! {
    plain Collections;
    plain NoCollections;
    plain NoCollectionsHint;
    plain ImportCollection;
    plain NewCollection;
    plain NewFolder;
    plain Rename;
    plain Duplicate;
    plain Open;
    plain MoreActions;
    with CollectionStoreError(DETAIL.into()) [DETAIL];
    with CollectionImportError(DETAIL.into()) [DETAIL];
    plain History;
    plain NoHistory;
    plain NoHistoryHint;
    plain HistoryReopen;
    plain HistoryResend;
    plain HistoryClearAll;
    plain HistoryJustNow;
    with HistoryMinutesAgo(NUMBER as u64) [NUMBER_TEXT];
    with HistoryHoursAgo(NUMBER as u64) [NUMBER_TEXT];
    with HistoryDaysAgo(NUMBER as u64) [NUMBER_TEXT];
}
