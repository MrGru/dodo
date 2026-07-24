//! The request history: every exchange this session sent, newest first.
//!
//! Phase 1 left this as a described seam — "fed from the one place a request
//! completes, `tab::RequestTabState::receive`". That is exactly how it is fed:
//! the tab emits a [`HistoryRecord`] when it finishes (whether a response
//! arrived or the request failed), and the page records it here.
//!
//! It is in-memory only, by design: history is a within-session convenience, not
//! saved user data like a collection. Each entry keeps a full
//! [`RequestSnapshot`], so Reopen, Duplicate and Resend rebuild the exact
//! request that ran.

use std::time::{Duration, SystemTime};

use crate::api_explorer::models::exchange::StatusClass;
use crate::api_explorer::models::method::HttpMethod;
use crate::api_explorer::models::snapshot::RequestSnapshot;

/// The most entries kept. Old ones fall off the end so a long session cannot
/// grow history without bound.
const MAX_ENTRIES: usize = 200;

/// What the tab hands the page when a request finishes.
#[derive(Clone, Debug)]
pub struct HistoryRecord {
    pub snapshot: RequestSnapshot,
    /// The HTTP status, or `None` when the request never got a response.
    pub status: Option<u16>,
    /// How long the round trip took, or `None` on failure.
    pub elapsed: Option<Duration>,
}

/// One recorded request.
pub struct HistoryEntry {
    pub id: u64,
    pub method: HttpMethod,
    pub url: String,
    pub status: Option<u16>,
    pub elapsed: Option<Duration>,
    pub at: SystemTime,
    /// The full request, kept so the entry can be reopened or resent exactly.
    pub snapshot: RequestSnapshot,
}

impl HistoryEntry {
    /// The status class, for colouring the badge — `None` when the request
    /// failed before any status arrived.
    pub fn status_class(&self) -> Option<StatusClass> {
        self.status.map(StatusClass::of)
    }
}

#[derive(Default)]
pub struct History {
    /// Newest first, so the list renders in the order it is stored.
    entries: Vec<HistoryEntry>,
    next_id: u64,
}

impl History {
    /// Records a finished request at the front of the list.
    pub fn record(&mut self, record: HistoryRecord) {
        let id = self.next_id;
        self.next_id += 1;
        self.entries.insert(
            0,
            HistoryEntry {
                id,
                method: record.snapshot.method,
                url: record.snapshot.url.clone(),
                status: record.status,
                elapsed: record.elapsed,
                at: SystemTime::now(),
                snapshot: record.snapshot,
            },
        );
        self.entries.truncate(MAX_ENTRIES);
    }

    pub fn entries(&self) -> &[HistoryEntry] {
        &self.entries
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The saved request behind an entry, for Reopen / Duplicate / Resend.
    pub fn snapshot(&self, id: u64) -> Option<&RequestSnapshot> {
        self.entries
            .iter()
            .find(|entry| entry.id == id)
            .map(|entry| &entry.snapshot)
    }

    /// Removes one entry. Unknown ids are ignored — a stale click is not an
    /// error.
    pub fn remove(&mut self, id: u64) {
        self.entries.retain(|entry| entry.id != id);
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::{History, HistoryRecord, MAX_ENTRIES};
    use crate::api_explorer::models::method::HttpMethod;
    use crate::api_explorer::models::snapshot::RequestSnapshot;
    use std::time::Duration;

    fn record(url: &str, status: Option<u16>) -> HistoryRecord {
        HistoryRecord {
            snapshot: RequestSnapshot {
                method: HttpMethod::Get,
                url: url.into(),
                ..RequestSnapshot::default()
            },
            status,
            elapsed: Some(Duration::from_millis(10)),
        }
    }

    #[test]
    fn the_newest_request_is_first() {
        let mut history = History::default();
        history.record(record("https://a", Some(200)));
        history.record(record("https://b", Some(404)));
        assert_eq!(history.entries()[0].url, "https://b");
        assert_eq!(history.entries()[1].url, "https://a");
    }

    #[test]
    fn a_failed_request_records_with_no_status() {
        let mut history = History::default();
        history.record(record("https://x", None));
        assert!(history.entries()[0].status.is_none());
        assert!(history.entries()[0].status_class().is_none());
    }

    #[test]
    fn an_entry_can_be_removed_and_reopened_by_id() {
        let mut history = History::default();
        history.record(record("https://a", Some(200)));
        let id = history.entries()[0].id;
        assert_eq!(
            history.snapshot(id).map(|s| s.url.as_str()),
            Some("https://a")
        );
        history.remove(id);
        assert!(history.is_empty());
        assert!(history.snapshot(id).is_none());
    }

    #[test]
    fn clear_empties_the_history() {
        let mut history = History::default();
        history.record(record("https://a", Some(200)));
        history.clear();
        assert!(history.is_empty());
    }

    #[test]
    fn history_is_capped_and_ids_stay_unique() {
        let mut history = History::default();
        for n in 0..(MAX_ENTRIES + 50) {
            history.record(record(&format!("https://x/{n}"), Some(200)));
        }
        assert_eq!(history.entries().len(), MAX_ENTRIES);
        // The most recent id is preserved at the front despite the truncation.
        let ids: std::collections::HashSet<u64> =
            history.entries().iter().map(|entry| entry.id).collect();
        assert_eq!(ids.len(), MAX_ENTRIES, "ids must stay unique");
    }
}
