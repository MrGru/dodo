//! Query history for this process, newest first.
//!
//! Deliberately in memory, following the API Explorer's precedent: history is
//! a within-session convenience, not saved user data. Query tabs are likewise
//! not restored after restart.

use crate::database::models::split::split_statements;

const MAX_ENTRIES: usize = 200;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryEntry {
    pub connection: String,
    pub statement: String,
}

impl HistoryEntry {
    pub fn matches(&self, query: &str) -> bool {
        let query = query.trim().to_lowercase();
        query.is_empty()
            || self.statement.to_lowercase().contains(&query)
            || self.connection.to_lowercase().contains(&query)
    }
}

#[derive(Default)]
pub struct History {
    entries: Vec<HistoryEntry>,
}

impl History {
    /// Records one editor buffer that reached execution. Empty and comment-only
    /// buffers never reached a driver and therefore are not history.
    pub fn record(&mut self, connection: String, statement: String) {
        if split_statements(&statement).is_empty() {
            return;
        }
        self.entries.insert(
            0,
            HistoryEntry {
                connection,
                statement,
            },
        );
        self.entries.truncate(MAX_ENTRIES);
    }

    pub fn snapshot(&self) -> Vec<HistoryEntry> {
        self.entries.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::{History, MAX_ENTRIES};

    #[test]
    fn history_is_newest_first_searchable_and_session_bounded() {
        let mut history = History::default();
        history.record("local".into(), "SELECT 1".into());
        history.record("staging".into(), "SELECT * FROM users".into());

        let entries = history.snapshot();
        assert_eq!(entries[0].connection, "staging");
        assert!(entries[0].matches("USERS"));
        assert!(entries[1].matches("LOCAL"));
        assert!(!entries[1].matches("missing"));

        for number in 0..(MAX_ENTRIES + 20) {
            history.record("local".into(), format!("SELECT {number}"));
        }
        assert_eq!(history.snapshot().len(), MAX_ENTRIES);
    }

    #[test]
    fn text_that_never_reaches_a_driver_is_not_history() {
        let mut history = History::default();
        history.record("local".into(), "-- only a comment".into());
        assert!(history.snapshot().is_empty());
    }
}
