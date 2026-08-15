//! Persisted searchable query history, newest first.
//!
//! Round 2's in-session list remains the model; round 6 gives that same list a
//! versioned JSON store. Retention is deterministic: at most 200 entries and at
//! most 4 MiB of query/connection text, always keeping the newest entries that
//! fit. A single query larger than the byte budget is not recorded rather than
//! being truncated into dangerous text that could later be reopened.

use std::time::{SystemTime, UNIX_EPOCH};

use crate::models::library::{HistoryEntry, HistoryOutcome, QueryScope};
use crate::models::split::split_statements;

pub const MAX_ENTRIES: usize = 200;
pub const MAX_BYTES: usize = 4 * 1024 * 1024;

#[derive(Default)]
pub struct History {
    entries: Vec<HistoryEntry>,
}

impl History {
    pub fn adopt(&mut self, entries: Vec<HistoryEntry>) {
        self.entries = retained(entries);
    }

    /// Records one editor buffer that reached execution. Empty and comment-only
    /// SQL buffers never reached a driver and therefore are not history. A
    /// non-SQL driver is checked by the caller before it reaches this method.
    pub fn record(
        &mut self,
        scope: QueryScope,
        statement: String,
        outcome: HistoryOutcome,
        duration_ms: Option<u64>,
    ) -> bool {
        if split_statements(&statement).is_empty() {
            return false;
        }
        let entry = HistoryEntry {
            scope,
            statement,
            recorded_at: now(),
            outcome,
            duration_ms,
        };
        if entry.stored_bytes() > MAX_BYTES {
            return false;
        }
        self.entries.insert(0, entry);
        self.entries = retained(std::mem::take(&mut self.entries));
        true
    }

    pub fn clear(&mut self) -> bool {
        let changed = !self.entries.is_empty();
        self.entries.clear();
        changed
    }

    pub fn snapshot(&self) -> Vec<HistoryEntry> {
        self.entries.clone()
    }
}

fn retained(entries: Vec<HistoryEntry>) -> Vec<HistoryEntry> {
    let mut bytes = 0usize;
    entries
        .into_iter()
        .take(MAX_ENTRIES)
        .take_while(|entry| {
            let size = entry.stored_bytes();
            if size > MAX_BYTES.saturating_sub(bytes) {
                return false;
            }
            bytes += size;
            true
        })
        .collect()
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::{History, MAX_BYTES, MAX_ENTRIES, retained};
    use crate::models::connection::ConnectionProfile;
    use crate::models::engine::Engine;
    use crate::models::library::{HistoryEntry, HistoryOutcome, QueryScope};

    fn scope(name: &str) -> QueryScope {
        let mut profile = ConnectionProfile::new(1, Engine::PostgreSql);
        profile.name = name.into();
        QueryScope::from_profile(&profile)
    }

    fn entry(number: usize, size: usize) -> HistoryEntry {
        HistoryEntry {
            scope: scope("local"),
            statement: format!("SELECT {number} -- {}", "x".repeat(size)),
            recorded_at: number as u64,
            outcome: HistoryOutcome::Succeeded,
            duration_ms: Some(1),
        }
    }

    #[test]
    fn history_is_newest_first_searchable_and_session_bounded() {
        let mut history = History::default();
        history.record(
            scope("local"),
            "SELECT 1".into(),
            HistoryOutcome::Succeeded,
            Some(1),
        );
        history.record(
            scope("staging"),
            "SELECT * FROM users".into(),
            HistoryOutcome::Failed,
            None,
        );

        let entries = history.snapshot();
        assert_eq!(entries[0].scope.connection_name, "staging");
        assert!(entries[0].matches("USERS"));
        assert!(entries[1].matches("LOCAL"));
        assert!(!entries[1].matches("missing"));

        for number in 0..(MAX_ENTRIES + 20) {
            history.record(
                scope("local"),
                format!("SELECT {number}"),
                HistoryOutcome::Cancelled,
                None,
            );
        }
        assert_eq!(history.snapshot().len(), MAX_ENTRIES);
    }

    #[test]
    fn retention_keeps_the_newest_entries_that_fit_the_disk_budget() {
        let entries = vec![
            entry(3, MAX_BYTES / 2),
            entry(2, MAX_BYTES / 2),
            entry(1, 20),
        ];
        let kept = retained(entries);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].recorded_at, 3);
        assert!(kept.iter().map(HistoryEntry::stored_bytes).sum::<usize>() <= MAX_BYTES);

        let too_large = retained(vec![entry(4, MAX_BYTES + 1)]);
        assert!(too_large.is_empty(), "oversized text must not be truncated");

        let mut history = History::default();
        assert!(history.record(
            scope("local"),
            "SELECT 1".into(),
            HistoryOutcome::Succeeded,
            None,
        ));
        assert!(!history.record(
            scope("local"),
            "x".repeat(MAX_BYTES + 1),
            HistoryOutcome::Failed,
            None,
        ));
        assert_eq!(history.snapshot().len(), 1, "old history must survive");
    }

    #[test]
    fn adopting_old_history_applies_the_same_bounds_as_new_entries() {
        let entries = (0..(MAX_ENTRIES + 5))
            .map(|number| entry(number, 1))
            .collect();
        let mut history = History::default();
        history.adopt(entries);
        assert_eq!(history.snapshot().len(), MAX_ENTRIES);
    }

    #[test]
    fn text_that_never_reaches_a_driver_is_not_history_and_clear_is_explicit() {
        let mut history = History::default();
        assert!(!history.record(
            scope("local"),
            "-- only a comment".into(),
            HistoryOutcome::Succeeded,
            None,
        ));
        assert!(history.snapshot().is_empty());

        history.record(
            scope("local"),
            "SELECT 1".into(),
            HistoryOutcome::Succeeded,
            None,
        );
        assert!(history.clear());
        assert!(!history.clear());
    }
}
