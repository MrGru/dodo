//! The saved connections, which one is selected, and what each one's live
//! status is.
//!
//! Plain data over [`ConnectionDocument`]: the document is what
//! `connections.json` holds, and the status map is what the current session
//! knows on top of it. The live `Arc<dyn Driver>` handles are *not* here — the
//! view owns those — which is what keeps every rule below testable with no
//! server.

use std::collections::HashMap;

use crate::database::models::connection::{ConnectionDocument, ConnectionProfile};
use crate::database::models::engine::Engine;
use crate::database::models::error::DbError;
use crate::i18n::Str;

/// Where one connection is between disconnected and usable.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum Status {
    #[default]
    Disconnected,
    Connecting,
    Connected,
    /// The last attempt failed, and this is why. Kept rather than reduced to a
    /// flag: "could not connect" without the server's own message is the least
    /// useful error message there is.
    Error(DbError),
}

impl Status {
    /// The word beside the connection's name.
    pub fn label(&self) -> Str {
        match self {
            Status::Disconnected => Str::DbStatusDisconnected,
            Status::Connecting => Str::DbStatusConnecting,
            Status::Connected => Str::DbStatusConnected,
            Status::Error(_) => Str::DbStatusError,
        }
    }

    pub fn is_connected(&self) -> bool {
        matches!(self, Status::Connected)
    }

    pub fn is_busy(&self) -> bool {
        matches!(self, Status::Connecting)
    }
}

#[derive(Default)]
pub struct ConnectionsState {
    document: ConnectionDocument,
    status: HashMap<u64, Status>,
    /// Whether the saved document has been read yet. Until it has, the list is
    /// deliberately empty rather than showing the "no connections yet" empty
    /// state — which would flash on every launch and read as data loss.
    loaded: bool,
}

impl ConnectionsState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn loaded(&self) -> bool {
        self.loaded
    }

    /// Adopts what the store read at launch.
    pub fn adopt(&mut self, document: ConnectionDocument) {
        // A `selected` naming a connection that is no longer in the file (a
        // hand edit, a half-restored backup) selects nothing rather than
        // leaving the page pointed at an id it cannot resolve.
        let selected = document
            .selected
            .filter(|id| document.connections.iter().any(|profile| profile.id == *id));
        self.document = ConnectionDocument {
            selected,
            ..document
        };
        self.status.clear();
        self.loaded = true;
    }

    /// The document to write back. Everything persisted lives here, so the
    /// caller never has to assemble it.
    pub fn document(&self) -> &ConnectionDocument {
        &self.document
    }

    pub fn profiles(&self) -> &[ConnectionProfile] {
        &self.document.connections
    }

    pub fn is_empty(&self) -> bool {
        self.document.connections.is_empty()
    }

    pub fn selected_id(&self) -> Option<u64> {
        self.document.selected
    }

    pub fn selected(&self) -> Option<&ConnectionProfile> {
        self.document.selected.and_then(|id| self.document.find(id))
    }

    pub fn find(&self, id: u64) -> Option<&ConnectionProfile> {
        self.document.find(id)
    }

    pub fn select(&mut self, id: Option<u64>) {
        self.document.selected =
            id.filter(|id| self.document.connections.iter().any(|p| p.id == *id));
    }

    pub fn status(&self, id: u64) -> &Status {
        self.status.get(&id).unwrap_or(&Status::Disconnected)
    }

    pub fn set_status(&mut self, id: u64, status: Status) {
        self.status.insert(id, status);
    }

    /// A blank profile for a new connection, with an id nothing is using.
    pub fn draft(&self, engine: Engine) -> ConnectionProfile {
        ConnectionProfile::new(self.document.next_id(), engine)
    }

    /// Saves `profile`, replacing the one with its id or appending it, and
    /// selects it.
    ///
    /// An edit that changes where a connection points **disconnects it**: the
    /// live handle is still attached to the old host, and leaving it marked
    /// Connected would let the user browse one database while the form says
    /// another. The caller drops the handle when this returns `true`.
    pub fn save(&mut self, profile: ConnectionProfile) -> bool {
        let id = profile.id;
        let retarget = match self.document.find(id) {
            Some(existing) => points_elsewhere(existing, &profile),
            None => false,
        };

        match self
            .document
            .connections
            .iter_mut()
            .find(|existing| existing.id == id)
        {
            Some(existing) => *existing = profile,
            None => self.document.connections.push(profile),
        }
        self.document.selected = Some(id);

        if retarget {
            self.status.insert(id, Status::Disconnected);
        }
        retarget
    }

    /// A copy of `id` under a fresh id, named so the two are told apart.
    /// Deliberately **not** connected: it is a new connection that happens to
    /// have been typed for you.
    pub fn duplicate(&mut self, id: u64, suffix: &str) -> Option<u64> {
        let copy = self
            .document
            .find(id)?
            .duplicated(self.document.next_id(), suffix);
        let new_id = copy.id;
        self.document.connections.push(copy);
        self.document.selected = Some(new_id);
        Some(new_id)
    }

    /// Removes `id`. The selection moves to whatever took its place in the
    /// list — the next connection, or the previous one if it was last — rather
    /// than to nothing, which would empty the page for no reason.
    pub fn delete(&mut self, id: u64) {
        let Some(position) = self
            .document
            .connections
            .iter()
            .position(|profile| profile.id == id)
        else {
            return;
        };
        self.document.connections.remove(position);
        self.status.remove(&id);

        if self.document.selected == Some(id) {
            let next = position.min(self.document.connections.len().saturating_sub(1));
            self.document.selected = self.document.connections.get(next).map(|p| p.id);
        }
    }
}

/// Whether an edit moved a connection to a different database.
///
/// The name is deliberately not part of it: renaming a connection is not a
/// reason to drop a working session.
fn points_elsewhere(before: &ConnectionProfile, after: &ConnectionProfile) -> bool {
    before.engine != after.engine
        || before.host != after.host
        || before.port != after.port
        || before.database != after.database
        || before.user != after.user
        || before.password != after.password
        || before.ssl_mode != after.ssl_mode
        || before.file != after.file
}

#[cfg(test)]
mod tests {
    use super::{ConnectionsState, Status, points_elsewhere};
    use crate::database::models::connection::{ConnectionDocument, ConnectionProfile, SslMode};
    use crate::database::models::engine::Engine;
    use crate::database::models::error::DbError;
    use crate::i18n::Language;

    fn profile(id: u64, name: &str) -> ConnectionProfile {
        ConnectionProfile {
            name: name.into(),
            database: "shop".into(),
            ..ConnectionProfile::new(id, Engine::PostgreSql)
        }
    }

    fn state() -> ConnectionsState {
        let mut state = ConnectionsState::new();
        state.adopt(ConnectionDocument {
            connections: vec![profile(1, "One"), profile(2, "Two"), profile(3, "Three")],
            selected: Some(2),
            ..ConnectionDocument::default()
        });
        state
    }

    #[test]
    fn a_fresh_state_has_not_loaded_and_shows_nothing() {
        let state = ConnectionsState::new();
        assert!(!state.loaded());
        assert!(state.is_empty());
        assert_eq!(state.selected_id(), None);
    }

    #[test]
    fn adopting_the_saved_document_restores_the_selection() {
        let state = state();
        assert!(state.loaded());
        assert_eq!(state.selected_id(), Some(2));
        assert_eq!(state.selected().map(|p| p.name.as_str()), Some("Two"));
    }

    /// A hand-edited file can name a connection that is not there. Pointing the
    /// page at an id it cannot resolve is worse than selecting nothing.
    #[test]
    fn a_selection_naming_a_missing_connection_selects_nothing() {
        let mut state = ConnectionsState::new();
        state.adopt(ConnectionDocument {
            connections: vec![profile(1, "One")],
            selected: Some(99),
            ..ConnectionDocument::default()
        });
        assert_eq!(state.selected_id(), None);
    }

    #[test]
    fn selecting_a_connection_that_does_not_exist_is_ignored() {
        let mut state = state();
        state.select(Some(99));
        assert_eq!(state.selected_id(), None);

        state.select(Some(3));
        assert_eq!(state.selected_id(), Some(3));

        state.select(None);
        assert_eq!(state.selected_id(), None);
    }

    #[test]
    fn a_draft_takes_an_id_no_saved_connection_is_using() {
        let state = state();
        let draft = state.draft(Engine::Sqlite);
        assert_eq!(draft.id, 4);
        assert_eq!(draft.engine, Engine::Sqlite);
    }

    #[test]
    fn saving_a_new_profile_appends_it_and_selects_it() {
        let mut state = state();
        let draft = state.draft(Engine::Sqlite);
        let retarget = state.save(draft);

        assert!(!retarget, "a new connection was never connected");
        assert_eq!(state.profiles().len(), 4);
        assert_eq!(state.selected_id(), Some(4));
    }

    #[test]
    fn saving_over_an_existing_profile_replaces_it_in_place() {
        let mut state = state();
        let mut edited = profile(2, "Renamed");
        edited.name = "Renamed".into();
        state.save(edited);

        assert_eq!(state.profiles().len(), 3);
        assert_eq!(state.find(2).map(|p| p.name.as_str()), Some("Renamed"));
        assert_eq!(
            state.profiles()[1].id,
            2,
            "the order the user arranged is preserved"
        );
    }

    /// The failure this prevents: browsing one database through a live handle
    /// while the form says another.
    #[test]
    fn editing_where_a_connection_points_disconnects_it() {
        let mut state = state();
        state.set_status(2, Status::Connected);

        let mut moved = profile(2, "Two");
        moved.host = "other-host".into();
        assert!(state.save(moved), "the caller must drop the live handle");
        assert_eq!(state.status(2), &Status::Disconnected);
    }

    #[test]
    fn renaming_a_connection_does_not_drop_a_working_session() {
        let mut state = state();
        state.set_status(2, Status::Connected);

        let mut renamed = profile(2, "Two");
        renamed.name = "Still the same server".into();
        assert!(!state.save(renamed));
        assert_eq!(state.status(2), &Status::Connected);
    }

    #[test]
    fn every_field_that_changes_the_target_counts_and_the_name_does_not() {
        let base = profile(1, "One");

        let mut renamed = base.clone();
        renamed.name = "Other".into();
        assert!(!points_elsewhere(&base, &renamed));

        for change in [
            |p: &mut ConnectionProfile| p.host = "elsewhere".into(),
            |p: &mut ConnectionProfile| p.port = 6543,
            |p: &mut ConnectionProfile| p.database = "other".into(),
            |p: &mut ConnectionProfile| p.user = "other".into(),
            |p: &mut ConnectionProfile| p.password = "other".into(),
            |p: &mut ConnectionProfile| p.ssl_mode = SslMode::Require,
            |p: &mut ConnectionProfile| p.file = "/other.db".into(),
            |p: &mut ConnectionProfile| p.engine = Engine::Sqlite,
        ] {
            let mut moved = base.clone();
            change(&mut moved);
            assert!(
                points_elsewhere(&base, &moved),
                "an edit that changes the target must disconnect"
            );
        }
    }

    #[test]
    fn a_duplicate_is_a_new_disconnected_connection_with_a_distinguishable_name() {
        let mut state = state();
        state.set_status(2, Status::Connected);

        let new_id = state.duplicate(2, "copy").expect("duplicates");
        assert_eq!(new_id, 4);
        assert_eq!(state.find(4).map(|p| p.name.as_str()), Some("Two copy"));
        assert_eq!(state.selected_id(), Some(4));
        assert_eq!(
            state.status(4),
            &Status::Disconnected,
            "a copy has no session of its own"
        );
        assert_eq!(state.status(2), &Status::Connected);
    }

    #[test]
    fn duplicating_something_that_is_not_there_does_nothing() {
        let mut state = state();
        assert_eq!(state.duplicate(99, "copy"), None);
        assert_eq!(state.profiles().len(), 3);
    }

    #[test]
    fn deleting_the_selected_connection_selects_the_one_that_took_its_place() {
        let mut state = state();
        state.delete(2);
        assert_eq!(state.profiles().len(), 2);
        assert_eq!(
            state.selected_id(),
            Some(3),
            "the next connection, not nothing"
        );
    }

    #[test]
    fn deleting_the_last_connection_selects_the_previous_one() {
        let mut state = state();
        state.select(Some(3));
        state.delete(3);
        assert_eq!(state.selected_id(), Some(2));
    }

    #[test]
    fn deleting_the_only_connection_selects_nothing() {
        let mut state = ConnectionsState::new();
        state.adopt(ConnectionDocument {
            connections: vec![profile(1, "One")],
            selected: Some(1),
            ..ConnectionDocument::default()
        });
        state.delete(1);
        assert!(state.is_empty());
        assert_eq!(state.selected_id(), None);
    }

    #[test]
    fn deleting_a_connection_that_is_not_selected_leaves_the_selection_alone() {
        let mut state = state();
        state.delete(1);
        assert_eq!(state.selected_id(), Some(2));
    }

    #[test]
    fn deleting_something_that_is_not_there_does_nothing() {
        let mut state = state();
        state.delete(99);
        assert_eq!(state.profiles().len(), 3);
        assert_eq!(state.selected_id(), Some(2));
    }

    #[test]
    fn a_status_reads_in_every_language_and_knows_what_it_means() {
        for status in [
            Status::Disconnected,
            Status::Connecting,
            Status::Connected,
            Status::Error(DbError::server("boom")),
        ] {
            for language in Language::ALL {
                assert!(!status.label().text(language).trim().is_empty());
            }
        }

        assert!(Status::Connected.is_connected());
        assert!(!Status::Connecting.is_connected());
        assert!(Status::Connecting.is_busy());
        assert!(!Status::Connected.is_busy());
    }

    #[test]
    fn the_document_handed_to_the_store_carries_the_selection() {
        let mut state = state();
        state.select(Some(3));
        assert_eq!(state.document().selected, Some(3));
        assert_eq!(state.document().connections.len(), 3);
    }
}
