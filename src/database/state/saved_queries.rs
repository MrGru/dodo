//! Saved query CRUD, independent of its JSON store and dialog.

use crate::database::models::library::SavedQuery;

#[derive(Default)]
pub struct SavedQueries {
    entries: Vec<SavedQuery>,
}

impl SavedQueries {
    pub fn adopt(&mut self, entries: Vec<SavedQuery>) {
        self.entries = entries;
    }

    pub fn snapshot(&self) -> Vec<SavedQuery> {
        self.entries.clone()
    }

    /// Inserts a draft (`id == 0`) or replaces an existing item. Names are
    /// trimmed because a whitespace-only label is not useful search metadata;
    /// statement bytes are preserved exactly.
    pub fn save(&mut self, mut query: SavedQuery) -> bool {
        query.name = query.name.trim().to_string();
        if query.name.is_empty() || query.statement.trim().is_empty() {
            return false;
        }

        if query.id == 0 {
            let Some(id) = self.next_id() else {
                return false;
            };
            query.id = id;
            self.entries.insert(0, query);
            return true;
        }

        let Some(existing) = self.entries.iter_mut().find(|entry| entry.id == query.id) else {
            return false;
        };
        *existing = query;
        true
    }

    pub fn delete(&mut self, id: u64) -> bool {
        let before = self.entries.len();
        self.entries.retain(|entry| entry.id != id);
        self.entries.len() != before
    }

    fn next_id(&self) -> Option<u64> {
        self.entries
            .iter()
            .map(|entry| entry.id)
            .max()
            .unwrap_or(0)
            .checked_add(1)
    }
}

#[cfg(test)]
mod tests {
    use super::SavedQueries;
    use crate::database::models::connection::ConnectionProfile;
    use crate::database::models::engine::Engine;
    use crate::database::models::library::{QueryScope, SavedQuery};

    fn query(id: u64, name: &str, statement: &str) -> SavedQuery {
        SavedQuery {
            id,
            name: name.into(),
            statement: statement.into(),
            scope: QueryScope::from_profile(&ConnectionProfile::new(7, Engine::PostgreSql)),
        }
    }

    #[test]
    fn snippet_crud_preserves_query_text_and_search_metadata() {
        let mut saved = SavedQueries::default();
        assert!(saved.save(query(0, "  Users  ", "SELECT *\nFROM users")));
        let inserted = saved.snapshot()[0].clone();
        assert_eq!(inserted.id, 1);
        assert_eq!(inserted.name, "Users");
        assert_eq!(inserted.statement, "SELECT *\nFROM users");
        assert!(inserted.matches("users"));

        assert!(saved.save(query(inserted.id, "Active users", "SELECT 2")));
        assert_eq!(saved.snapshot()[0].name, "Active users");
        assert_eq!(saved.snapshot()[0].statement, "SELECT 2");

        assert!(saved.delete(inserted.id));
        assert!(saved.snapshot().is_empty());
        assert!(!saved.delete(inserted.id));
    }

    #[test]
    fn empty_names_and_statements_are_not_saved() {
        let mut saved = SavedQueries::default();
        assert!(!saved.save(query(0, " ", "SELECT 1")));
        // A comment is valid query text; only literal blank input is refused.
        assert!(saved.save(query(0, "comment", "-- note")));
        assert!(!saved.save(query(0, "name", " \n\t")));
    }
}
