//! The page's environments: the document, which one is active, and the last
//! store failure.
//!
//! Owned by `views::explorer::ApiExplorer` alongside `history` and
//! `collections`, and the same shape as
//! [`CollectionState`](crate::state::collection::CollectionState):
//! a plain-data model, a mutable accessor the view edits through, and an error
//! held as a [`Str`] rather than as rendered text so a banner already on screen
//! re-translates when the language changes.
//!
//! Every method here is pure state manipulation, so all of it is unit testable
//! without a `Window`. Persisting is the view's job, because it is the view
//! that owns the background executor and the store handle.

use crate::i18n::Str;
use crate::models::script::{VariableWrite, WriteScope};
use crate::models::variables::{Environment, Variable, VariableDocument, VariableSet};

#[derive(Default)]
pub struct EnvironmentState {
    document: VariableDocument,
    /// The next environment id to hand out. Monotonic and never reused, so a
    /// deleted environment's id cannot be resurrected by a stale click.
    next_id: u64,
    /// Whether the disk load has landed, either with a document or with an
    /// error.
    ///
    /// This exists to make one narrow race impossible rather than merely
    /// unlikely. The document is read on the background executor, so between
    /// app launch and that read completing this holds an *empty* document. An
    /// editor opened in that window would show no variables and, on the first
    /// edit, write that emptiness back over the file. Nothing may be saved
    /// until the load has been accounted for; see
    /// [`EnvironmentState::is_loaded`].
    loaded: bool,
    error: Option<Str>,
}

impl EnvironmentState {
    pub fn document(&self) -> &VariableDocument {
        &self.document
    }

    /// Installs a freshly loaded document, restarting id allocation above
    /// whatever it already contains.
    pub fn set_document(&mut self, document: VariableDocument) {
        self.next_id = document.next_id();
        self.document = document;
        self.loaded = true;
    }

    /// Whether what is held reflects the disk — true once the load has
    /// completed, whether it produced a document or an error.
    ///
    /// A *failed* load counts: the failure is shown, the held document is
    /// knowingly empty, and refusing to save for the rest of the session would
    /// be worse than letting the user start again.
    pub fn is_loaded(&self) -> bool {
        self.loaded
    }

    pub fn error(&self) -> Option<&Str> {
        self.error.as_ref()
    }

    pub fn set_error(&mut self, error: Option<Str>) {
        self.error = error;
    }

    /// Marks the load as accounted for after it failed. See
    /// [`EnvironmentState::is_loaded`].
    pub fn mark_load_failed(&mut self, error: Str) {
        self.error = Some(error);
        self.loaded = true;
    }

    pub fn environments(&self) -> &[Environment] {
        &self.document.environments
    }

    pub fn active(&self) -> Option<&Environment> {
        self.document.active()
    }

    pub fn active_id(&self) -> Option<u64> {
        self.document.active().map(|environment| environment.id)
    }

    /// Switches environment, or to "no environment" with `None`.
    pub fn set_active(&mut self, id: Option<u64>) {
        self.document.active_environment = id.filter(|id| self.document.environment(*id).is_some());
    }

    /// The layers a request resolves against right now.
    pub fn variable_set(&self) -> VariableSet {
        self.document.variable_set()
    }

    /// The variables of one environment, or the collection scope when `id` is
    /// `None` — the two things the editor edits, addressed the same way.
    pub fn variables(&self, id: Option<u64>) -> &[Variable] {
        match id {
            None => &self.document.collection_variables,
            Some(id) => self
                .document
                .environment(id)
                .map(|environment| environment.variables.as_slice())
                .unwrap_or_default(),
        }
    }

    /// Replaces one scope's variables wholesale. The editor harvests its rows
    /// and writes them through here, so there is one place a scope changes.
    pub fn set_variables(&mut self, id: Option<u64>, variables: Vec<Variable>) {
        match id {
            None => self.document.collection_variables = variables,
            Some(id) => {
                if let Some(environment) = self.document.environment_mut(id) {
                    environment.variables = variables;
                }
            }
        }
    }

    /// Applies one variable a pre-request script wrote, and says whether
    /// anything changed.
    ///
    /// A write to the environment scope with **no environment active** is
    /// dropped: creating one as a side effect of a script would be a surprise
    /// nobody asked for, and picking an arbitrary existing one would be worse.
    /// The collection scope always exists, so it always takes the write.
    ///
    /// A value written here is never marked `secret` and never un-marks an
    /// existing one: `secret` is the user's classification of a name, and a
    /// script overwriting the value of a token must not quietly reveal it.
    pub fn apply_script_write(&mut self, write: &VariableWrite) -> bool {
        let scope = match write.scope {
            WriteScope::Collection => None,
            WriteScope::Environment => match self.active_id() {
                Some(id) => Some(id),
                None => return false,
            },
        };

        let key = write.key.trim();
        if key.is_empty() {
            return false;
        }

        let mut variables = self.variables(scope).to_vec();
        match &write.value {
            Some(value) => {
                match variables
                    .iter_mut()
                    .find(|variable| variable.key.trim() == key)
                {
                    Some(variable) => variable.value = value.clone(),
                    None => variables.push(Variable {
                        key: key.to_string(),
                        value: value.clone(),
                        enabled: true,
                        secret: false,
                    }),
                }
            }
            None => variables.retain(|variable| variable.key.trim() != key),
        }

        self.set_variables(scope, variables);
        true
    }

    /// Adds an environment and returns its id. A new environment does *not*
    /// become active on its own: switching is the user's decision, and doing it
    /// for them would silently re-resolve every open request.
    pub fn create(&mut self, name: impl Into<String>) -> u64 {
        let id = self.take_id();
        self.document.environments.push(Environment::new(id, name));
        id
    }

    pub fn rename(&mut self, id: u64, name: impl Into<String>) {
        if let Some(environment) = self.document.environment_mut(id) {
            environment.name = name.into();
        }
    }

    /// Copies an environment, including its variables, and returns the copy's
    /// id. `suffix` is the translated " copy" wording the caller supplies, so
    /// this stays free of `cx`.
    pub fn duplicate(&mut self, id: u64, suffix: &str) -> Option<u64> {
        let source = self.document.environment(id)?.clone();
        let new_id = self.take_id();
        self.document.environments.push(Environment {
            id: new_id,
            name: format!("{} {suffix}", source.name),
            variables: source.variables,
        });
        Some(new_id)
    }

    /// Removes an environment. If it was the active one, the page falls back to
    /// "no environment" rather than silently picking a neighbour.
    pub fn remove(&mut self, id: u64) {
        self.document
            .environments
            .retain(|environment| environment.id != id);
        if self.document.active_environment == Some(id) {
            self.document.active_environment = None;
        }
    }

    /// Merges imported environments in, renumbering them so their file's ids
    /// cannot collide with what is already open. Returns the id of the last one
    /// merged, which is what the editor selects.
    pub fn import(&mut self, environments: Vec<Environment>) -> Option<u64> {
        let mut last = None;
        for environment in environments {
            let id = self.take_id();
            self.document.environments.push(Environment {
                id,
                name: environment.name,
                variables: environment.variables,
            });
            last = Some(id);
        }
        last
    }

    /// Merges a collection's own `variable` array into the collection scope.
    ///
    /// A name already present is **overwritten** rather than duplicated: two
    /// rows with one name would leave which one wins to list order, and the
    /// file just imported is the more recent statement of intent.
    pub fn import_collection_variables(&mut self, variables: Vec<Variable>) {
        for incoming in variables {
            let existing = self
                .document
                .collection_variables
                .iter_mut()
                .find(|held| held.key.trim() == incoming.key.trim());
            match existing {
                Some(held) => *held = incoming,
                None => self.document.collection_variables.push(incoming),
            }
        }
    }

    fn take_id(&mut self) -> u64 {
        // A document loaded from disk sets this; a default one starts at 0, and
        // ids are 1-based so that 0 stays available as "no id" in tests.
        self.next_id = self.next_id.max(1);
        let id = self.next_id;
        self.next_id += 1;
        id
    }
}

#[cfg(test)]
mod tests {
    use super::EnvironmentState;
    use crate::models::script::{VariableWrite, WriteScope};

    use crate::models::variables::{Environment, Variable, VariableDocument, VariableScope};

    fn state() -> EnvironmentState {
        let mut state = EnvironmentState::default();
        let staging = state.create("Staging");
        state.set_variables(
            Some(staging),
            vec![Variable::new("host", "staging.example.com")],
        );
        state.set_variables(None, vec![Variable::new("version", "v1")]);
        state.set_active(Some(staging));
        state
    }

    #[test]
    fn nothing_counts_as_loaded_until_the_disk_read_lands() {
        // The editor refuses to write anything back while this is false, which
        // is what stops a dialog opened during startup from saving its empty
        // placeholder rows over the file still being read.
        let mut state = EnvironmentState::default();
        assert!(!state.is_loaded());

        state.set_document(VariableDocument::default());
        assert!(state.is_loaded());
    }

    #[test]
    fn a_failed_load_still_counts_as_landed() {
        // Otherwise a read failure would leave the editor unable to save for
        // the rest of the session.
        let mut state = EnvironmentState::default();
        state.mark_load_failed(crate::i18n::api_variables::Text::StoreMissingVersion.into());
        assert!(state.is_loaded());
        assert!(state.error().is_some());
    }

    #[test]
    fn a_new_environment_does_not_steal_the_active_slot() {
        let mut state = state();
        let before = state.active_id();
        state.create("Prod");
        assert_eq!(state.active_id(), before);
    }

    #[test]
    fn ids_are_never_reused_after_a_delete() {
        let mut state = EnvironmentState::default();
        let first = state.create("A");
        state.remove(first);
        let second = state.create("B");
        assert_ne!(first, second);
    }

    #[test]
    fn a_loaded_document_keeps_allocating_above_its_own_ids() {
        let mut state = EnvironmentState::default();
        state.set_document(VariableDocument {
            environments: vec![Environment::new(42, "Loaded")],
            ..VariableDocument::default()
        });
        assert_eq!(state.create("New"), 43);
    }

    #[test]
    fn deleting_the_active_environment_falls_back_to_none() {
        let mut state = state();
        let active = state.active_id().expect("one is active");
        state.remove(active);
        assert_eq!(state.active_id(), None);
        // The collection layer still resolves; only the environment layer went.
        assert_eq!(
            state.variable_set().lookup("version"),
            Some((VariableScope::Collection, "v1"))
        );
    }

    #[test]
    fn setting_an_unknown_environment_active_is_ignored() {
        let mut state = state();
        state.set_active(Some(9_999));
        assert_eq!(state.active_id(), None);
    }

    #[test]
    fn duplicating_copies_the_variables_under_a_new_name_and_id() {
        let mut state = state();
        let source = state.active_id().expect("one is active");
        let copy = state.duplicate(source, "copy").expect("duplicates");

        assert_ne!(copy, source);
        assert_eq!(state.environments().len(), 2);
        assert_eq!(
            state
                .environments()
                .iter()
                .find(|environment| environment.id == copy)
                .map(|environment| environment.name.as_str()),
            Some("Staging copy")
        );
        assert_eq!(state.variables(Some(copy)), state.variables(Some(source)));
        // The copy is not made active by being made.
        assert_eq!(state.active_id(), Some(source));
    }

    #[test]
    fn imported_environments_are_renumbered_so_they_cannot_collide() {
        let mut state = state();
        let existing = state.active_id().expect("one is active");

        // Both carry the id the open environment already uses.
        let selected = state.import(vec![
            Environment::new(existing, "Imported A"),
            Environment::new(existing, "Imported B"),
        ]);

        assert_eq!(state.environments().len(), 3);
        let ids: Vec<u64> = state
            .environments()
            .iter()
            .map(|environment| environment.id)
            .collect();
        let mut unique = ids.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(ids.len(), unique.len(), "an imported id collided: {ids:?}");
        assert_eq!(selected, ids.last().copied());
    }

    #[test]
    fn imported_collection_variables_overwrite_a_name_rather_than_doubling_it() {
        let mut state = state();
        state.import_collection_variables(vec![
            Variable::new("version", "v2"),
            Variable::new("apiKey", "abc"),
        ]);
        assert_eq!(state.variables(None).len(), 2);
        assert_eq!(
            state.variable_set().lookup("version").map(|(_, v)| v),
            Some("v2")
        );
    }

    // ---- what a pre-request script wrote ------------------------------------

    fn write(scope: WriteScope, key: &str, value: Option<&str>) -> VariableWrite {
        VariableWrite {
            scope,
            key: key.into(),
            value: value.map(str::to_string),
        }
    }

    #[test]
    fn a_script_write_updates_the_active_environment_in_place() {
        let mut state = state();
        assert!(state.apply_script_write(&write(
            WriteScope::Environment,
            "host",
            Some("prod.example.com")
        )));
        assert_eq!(
            state.variable_set().lookup("host"),
            Some((VariableScope::Environment, "prod.example.com"))
        );
        assert_eq!(
            state.variables(state.active_id()).len(),
            1,
            "the write duplicated the row instead of updating it"
        );
    }

    #[test]
    fn a_script_write_of_a_new_name_adds_an_enabled_row() {
        let mut state = state();
        assert!(state.apply_script_write(&write(WriteScope::Environment, "token", Some("abc"))));
        let added = state
            .variables(state.active_id())
            .iter()
            .find(|variable| variable.key == "token")
            .expect("the variable was added");
        assert!(added.enabled);
        assert!(!added.secret);
    }

    #[test]
    fn a_script_write_does_not_reveal_a_secret_it_overwrites() {
        let mut state = state();
        let active = state.active_id().expect("one is active");
        state.set_variables(Some(active), vec![Variable::secret("token", "old")]);

        state.apply_script_write(&write(WriteScope::Environment, "token", Some("new")));

        let token = &state.variables(Some(active))[0];
        assert_eq!(token.value, "new");
        assert!(token.secret, "a script write unmasked a secret variable");
    }

    #[test]
    fn a_script_unset_removes_the_row() {
        let mut state = state();
        assert!(state.apply_script_write(&write(WriteScope::Environment, "host", None)));
        assert!(state.variables(state.active_id()).is_empty());
    }

    #[test]
    fn an_environment_write_with_nothing_active_is_dropped() {
        // Not "create an environment": that would be a side effect of running
        // somebody else's script.
        let mut state = state();
        state.set_active(None);
        assert!(!state.apply_script_write(&write(WriteScope::Environment, "token", Some("abc"))));
        assert_eq!(state.environments()[0].variables.len(), 1);
    }

    #[test]
    fn a_collection_write_always_lands_because_that_scope_always_exists() {
        let mut state = state();
        state.set_active(None);
        assert!(state.apply_script_write(&write(WriteScope::Collection, "version", Some("v9"))));
        assert_eq!(
            state.variable_set().lookup("version"),
            Some((VariableScope::Collection, "v9"))
        );
    }

    #[test]
    fn a_write_with_no_name_changes_nothing() {
        let mut state = state();
        assert!(!state.apply_script_write(&write(WriteScope::Collection, "  ", Some("x"))));
    }

    #[test]
    fn the_editor_addresses_the_collection_scope_as_none() {
        let mut state = state();
        state.set_variables(None, vec![Variable::new("only", "one")]);
        assert_eq!(state.variables(None).len(), 1);
        assert_eq!(state.variables(Some(9_999)).len(), 0);
    }
}
