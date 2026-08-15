//! Persisted query data: saved queries and execution history.
//!
//! This document deliberately contains no connection profile and therefore no
//! password. A [`QueryScope`] keeps only the facts needed to avoid opening a
//! query against a connection that has since been repointed. Query text itself
//! is expected user data and is stored as plain JSON, exactly as entered.

use serde::{Deserialize, Serialize};

use crate::models::connection::ConnectionProfile;
use crate::models::engine::Engine;

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueryDataDocument {
    pub version: u32,
    #[serde(default)]
    pub saved_queries: Vec<SavedQuery>,
    #[serde(default)]
    pub history: Vec<HistoryEntry>,
}

impl Default for QueryDataDocument {
    fn default() -> Self {
        Self {
            version: SCHEMA_VERSION,
            saved_queries: Vec::new(),
            history: Vec::new(),
        }
    }
}

/// Enough connection identity to reopen text safely, and nothing sensitive.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueryScope {
    pub connection_id: u64,
    pub connection_name: String,
    pub engine: Engine,
    /// Includes database/file and network address, but never the password.
    pub target: String,
}

impl QueryScope {
    pub fn from_profile(profile: &ConnectionProfile) -> Self {
        Self {
            connection_id: profile.id,
            connection_name: profile.display_name(),
            engine: profile.engine,
            target: profile.target(),
        }
    }

    /// An id alone is not enough: editing a saved profile may point it at a
    /// different server or database while retaining that id.
    pub fn matches_profile(&self, profile: &ConnectionProfile) -> bool {
        self.connection_id == profile.id
            && self.engine == profile.engine
            && self.target == profile.target()
    }

    pub fn matches(&self, query: &str) -> bool {
        let query = query.to_lowercase();
        self.connection_name.to_lowercase().contains(&query)
            || self.engine.display_name().to_lowercase().contains(&query)
            || self.target.to_lowercase().contains(&query)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedQuery {
    pub id: u64,
    pub name: String,
    pub statement: String,
    pub scope: QueryScope,
}

impl SavedQuery {
    pub fn matches(&self, query: &str) -> bool {
        let query = query.trim().to_lowercase();
        query.is_empty()
            || self.name.to_lowercase().contains(&query)
            || self.statement.to_lowercase().contains(&query)
            || self.scope.matches(&query)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoryOutcome {
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub statement: String,
    pub scope: QueryScope,
    /// Seconds since the Unix epoch. The store needs no clock crate and the UI
    /// renders this as a localized relative age.
    pub recorded_at: u64,
    pub outcome: HistoryOutcome,
    /// Present only where the current execution model reports a duration.
    #[serde(default)]
    pub duration_ms: Option<u64>,
}

impl HistoryEntry {
    pub fn matches(&self, query: &str) -> bool {
        let query = query.trim().to_lowercase();
        query.is_empty()
            || self.statement.to_lowercase().contains(&query)
            || self.scope.matches(&query)
    }

    pub fn stored_bytes(&self) -> usize {
        self.statement.len() + self.scope.connection_name.len() + self.scope.target.len()
    }
}

#[cfg(test)]
mod tests {
    use super::{HistoryEntry, HistoryOutcome, QueryDataDocument, QueryScope, SavedQuery};
    use crate::models::connection::ConnectionProfile;
    use crate::models::engine::Engine;

    fn profile() -> ConnectionProfile {
        ConnectionProfile {
            name: "Local shop".into(),
            host: "127.0.0.1".into(),
            port: 5432,
            database: "shop".into(),
            user: "postgres".into(),
            password: "must-not-leak".into(),
            ..ConnectionProfile::new(7, Engine::PostgreSql)
        }
    }

    #[test]
    fn query_scope_detects_a_repointed_connection_without_carrying_its_password() {
        let profile = profile();
        let scope = QueryScope::from_profile(&profile);
        assert!(scope.matches_profile(&profile));

        let mut moved = profile.clone();
        moved.database = "production".into();
        assert!(!scope.matches_profile(&moved));

        let bytes = serde_json::to_string(&scope).expect("serializes");
        assert!(!bytes.contains("must-not-leak"));
        assert!(bytes.contains("shop"));
    }

    #[test]
    fn saved_queries_and_history_search_text_and_connection_identity() {
        let scope = QueryScope::from_profile(&profile());
        let saved = SavedQuery {
            id: 1,
            name: "Recent orders".into(),
            statement: "SELECT * FROM orders".into(),
            scope: scope.clone(),
        };
        assert!(saved.matches("RECENT"));
        assert!(saved.matches("orders"));
        assert!(saved.matches("local shop"));
        assert!(!saved.matches("missing"));

        let history = HistoryEntry {
            statement: saved.statement,
            scope,
            recorded_at: 1,
            outcome: HistoryOutcome::Succeeded,
            duration_ms: Some(12),
        };
        assert!(history.matches("postgresql"));
    }

    #[test]
    fn a_default_document_starts_at_the_current_schema() {
        let document = QueryDataDocument::default();
        assert_eq!(document.version, super::SCHEMA_VERSION);
        assert!(document.saved_queries.is_empty());
        assert!(document.history.is_empty());
    }
}
