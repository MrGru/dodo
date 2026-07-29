//! Variables and the environments that hold them.
//!
//! A variable is a name, a value, an enabled flag and a `secret` flag. Nothing
//! here knows what a request is: this module owns the data and the *resolution
//! order*, and [`interpolate`](crate::api_explorer::models::interpolate) owns
//! the substitution that reads it.
//!
//! # Scopes and precedence
//!
//! Two scopes ship in this round, in increasing precedence:
//!
//! 1. [`VariableScope::Collection`] — one shared list, the home for the
//!    `variable` array a Postman collection carries.
//! 2. [`VariableScope::Environment`] — the variables of whichever environment
//!    is active. `None` active is a legitimate state and simply drops the layer.
//!
//! Resolution walks a [`VariableSet`]'s layers from the last one back, so the
//! *highest* precedence layer wins. That is the whole reason a later round can
//! add script-set values without touching the store: a script layer is one more
//! [`VariableScope`] variant appended after `Environment` and one more
//! `push_layer` call at send time. The persisted document
//! ([`VariableDocument`]) does not change shape, because a script value is not
//! saved.
//!
//! # `secret` is a display flag, not encryption
//!
//! A variable marked `secret` is stored in the same file, in plain text, like
//! every other one — the captain's decision, recorded in
//! `decision-secret-variable-storage`. What the flag buys is that the editor
//! masks the value until it is explicitly revealed, and that the editor says on
//! screen that the value is stored unencrypted on this machine. dodo already
//! persists Basic-auth passwords this way ([`AuthDraft`]), so this adds no
//! exposure the app did not already have; it does make it more prominent, which
//! is why the wording is in the UI and not only in the docs.
//!
//! [`AuthDraft`]: crate::api_explorer::models::auth::AuthDraft

use serde::{Deserialize, Serialize};

/// The schema version written into every environments file.
///
/// It exists from the very first write on purpose. `#[serde(default)]` — the
/// only versioning [`RequestSnapshot`] has — copes with *added* fields and
/// nothing else; a renamed field, a changed meaning or a different container
/// shape would be read as silently wrong data. A file whose `version` is
/// **greater** than this is refused with a message rather than misread (see
/// `services::variable_store::parse_document`); a file whose version is lower
/// is read with serde's defaults, which is the ordinary forward path.
///
/// [`RequestSnapshot`]: crate::api_explorer::models::snapshot::RequestSnapshot
pub const SCHEMA_VERSION: u32 = 1;

/// `enabled` defaults to true when a file does not carry it.
///
/// Unlike [`KeyValue`], where a row with no flag is a bug in our own writer, an
/// environments file is small enough to hand-edit and a hand-written variable
/// with no `enabled` key plainly means "use it".
///
/// [`KeyValue`]: crate::api_explorer::models::key_value::KeyValue
fn enabled_by_default() -> bool {
    true
}

/// One name/value pair in one scope.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Variable {
    pub key: String,
    pub value: String,
    #[serde(default = "enabled_by_default")]
    pub enabled: bool,
    /// Masked in the editor until revealed. Stored in plain text regardless —
    /// see this module's doc.
    #[serde(default)]
    pub secret: bool,
}

impl Default for Variable {
    fn default() -> Self {
        Self {
            key: String::new(),
            value: String::new(),
            enabled: true,
            secret: false,
        }
    }
}

impl Variable {
    /// An enabled, non-secret variable.
    ///
    /// `#[cfg(test)]` like `DiskCollectionStore::at`: the shipping paths all
    /// build a variable from a file or from an editor row and set every field,
    /// so this exists only to keep the test tables readable.
    #[cfg(test)]
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
            ..Self::default()
        }
    }

    /// An enabled variable whose value the editor masks. Test constructor, for
    /// the same reason [`Variable::new`] is one.
    #[cfg(test)]
    pub fn secret(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            secret: true,
            ..Self::new(key, value)
        }
    }

    /// Whether this variable can resolve a reference: switched on, with a name.
    ///
    /// A blank name is "not filled in yet" rather than an error, for the same
    /// reason a blank key/value row is: the editor always shows one empty row.
    pub fn is_effective(&self) -> bool {
        self.enabled && !self.key.trim().is_empty()
    }
}

/// Which layer a value came from. Ordered by precedence, lowest first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum VariableScope {
    Collection,
    Environment,
}

impl VariableScope {
    /// The word the UI uses for this scope.
    pub fn label(self) -> crate::i18n::Str {
        match self {
            VariableScope::Collection => crate::i18n::Str::CollectionVariables,
            VariableScope::Environment => crate::i18n::Str::EnvironmentVariables,
        }
    }
}

/// A named set of variables the user switches between.
///
/// `id` rather than the name is the identity, so renaming an environment does
/// not orphan the "which one is active" pointer, and two environments may
/// briefly share a name while one is being typed.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Environment {
    pub id: u64,
    pub name: String,
    #[serde(default)]
    pub variables: Vec<Variable>,
}

impl Environment {
    pub fn new(id: u64, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            variables: Vec::new(),
        }
    }
}

/// Everything the environments file holds.
///
/// The `version` field is first and is mandatory — see [`SCHEMA_VERSION`].
/// Every other field is `#[serde(default)]` so that a file written by an older
/// build of *this* version still loads.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VariableDocument {
    pub version: u32,
    #[serde(default)]
    pub environments: Vec<Environment>,
    /// The collection scope. One shared list rather than one per collection:
    /// dodo's collection tree has no per-node variable storage yet, and a
    /// [`Variable`] names no owner, so growing to per-collection later is a
    /// field on `Node` rather than a reshape of this type.
    #[serde(default)]
    pub collection_variables: Vec<Variable>,
    /// Which environment is active, by [`Environment::id`]. `None` is the
    /// "no environment" state and is not an error.
    #[serde(default)]
    pub active_environment: Option<u64>,
}

impl Default for VariableDocument {
    fn default() -> Self {
        Self {
            version: SCHEMA_VERSION,
            environments: Vec::new(),
            collection_variables: Vec::new(),
            active_environment: None,
        }
    }
}

impl VariableDocument {
    pub fn environment(&self, id: u64) -> Option<&Environment> {
        self.environments.iter().find(|env| env.id == id)
    }

    pub fn environment_mut(&mut self, id: u64) -> Option<&mut Environment> {
        self.environments.iter_mut().find(|env| env.id == id)
    }

    /// The active environment, if the pointer still names one that exists.
    pub fn active(&self) -> Option<&Environment> {
        self.active_environment.and_then(|id| self.environment(id))
    }

    /// One more than the largest id in the document, so a freshly loaded
    /// document keeps handing out ids nothing already uses.
    pub fn next_id(&self) -> u64 {
        self.environments
            .iter()
            .map(|env| env.id)
            .max()
            .map_or(1, |max| max + 1)
    }

    /// The layers a request resolves against right now, in precedence order.
    pub fn variable_set(&self) -> VariableSet {
        let mut set = VariableSet::default();
        set.push_layer(VariableScope::Collection, self.collection_variables.clone());
        if let Some(active) = self.active() {
            set.push_layer(VariableScope::Environment, active.variables.clone());
        }
        set
    }
}

/// The layers one request resolves `{{name}}` against.
///
/// Owned plain data, so it crosses onto the background executor with the draft
/// it belongs to. Built once per send from [`VariableDocument::variable_set`];
/// a later round adds a script layer with one more [`push_layer`] call.
///
/// [`push_layer`]: VariableSet::push_layer
#[derive(Clone, Debug, Default)]
pub struct VariableSet {
    /// In increasing precedence — the last layer wins.
    layers: Vec<(VariableScope, Vec<Variable>)>,
}

impl VariableSet {
    /// Adds a layer above every layer already present.
    pub fn push_layer(&mut self, scope: VariableScope, variables: Vec<Variable>) {
        self.layers.push((scope, variables));
    }

    /// The value `name` resolves to, and which scope it came from.
    ///
    /// Later layers win; within a layer the first matching row wins, so an
    /// environment that somehow holds the same name twice behaves like the
    /// key/value tables do. Names are matched trimmed but case-sensitively:
    /// `{{baseUrl}}` and `{{baseurl}}` are different variables, as they are in
    /// Postman.
    pub fn lookup(&self, name: &str) -> Option<(VariableScope, &str)> {
        let name = name.trim();
        self.layers.iter().rev().find_map(|(scope, variables)| {
            variables
                .iter()
                .find(|variable| variable.is_effective() && variable.key.trim() == name)
                .map(|variable| (*scope, variable.value.as_str()))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Environment, SCHEMA_VERSION, Variable, VariableDocument, VariableScope, VariableSet,
    };

    fn document() -> VariableDocument {
        VariableDocument {
            environments: vec![Environment {
                id: 7,
                name: "Staging".into(),
                variables: vec![Variable::new("host", "staging.example.com")],
            }],
            collection_variables: vec![
                Variable::new("host", "example.com"),
                Variable::new("version", "v1"),
            ],
            active_environment: Some(7),
            ..VariableDocument::default()
        }
    }

    #[test]
    fn the_environment_layer_wins_over_the_collection_layer() {
        let set = document().variable_set();
        assert_eq!(
            set.lookup("host"),
            Some((VariableScope::Environment, "staging.example.com"))
        );
        // …and a name only the collection defines still resolves.
        assert_eq!(
            set.lookup("version"),
            Some((VariableScope::Collection, "v1"))
        );
    }

    #[test]
    fn no_active_environment_leaves_the_collection_layer_alone() {
        let mut document = document();
        document.active_environment = None;
        let set = document.variable_set();
        assert_eq!(
            set.lookup("host"),
            Some((VariableScope::Collection, "example.com"))
        );
    }

    #[test]
    fn an_active_pointer_at_a_deleted_environment_is_not_an_error() {
        let mut document = document();
        document.environments.clear();
        assert!(document.active().is_none());
        assert_eq!(
            document.variable_set().lookup("host"),
            Some((VariableScope::Collection, "example.com"))
        );
    }

    #[test]
    fn a_disabled_or_unnamed_variable_resolves_nothing() {
        let mut set = VariableSet::default();
        set.push_layer(
            VariableScope::Environment,
            vec![
                Variable {
                    enabled: false,
                    ..Variable::new("off", "x")
                },
                Variable::new("   ", "y"),
            ],
        );
        assert_eq!(set.lookup("off"), None);
        assert_eq!(set.lookup(""), None);
    }

    #[test]
    fn a_higher_layer_that_disables_a_name_falls_through_to_the_lower_one() {
        // A switched-off environment row is not a shadow: the collection value
        // is still the answer, which is what "enabled" means row by row.
        let mut set = VariableSet::default();
        set.push_layer(VariableScope::Collection, vec![Variable::new("k", "low")]);
        set.push_layer(
            VariableScope::Environment,
            vec![Variable {
                enabled: false,
                ..Variable::new("k", "high")
            }],
        );
        assert_eq!(set.lookup("k"), Some((VariableScope::Collection, "low")));
    }

    #[test]
    fn lookup_trims_the_reference_but_not_the_case() {
        let set = document().variable_set();
        assert_eq!(
            set.lookup("  host  ").map(|(_, value)| value),
            Some("staging.example.com")
        );
        assert_eq!(set.lookup("HOST"), None);
    }

    #[test]
    fn a_document_round_trips_through_json_with_its_version_and_secret_flag() {
        let mut document = document();
        document.environments.push(Environment {
            id: 9,
            name: "Prod".into(),
            variables: vec![Variable::secret("token", "s3cr3t")],
        });

        let json = serde_json::to_string(&document).expect("serializes");
        assert!(
            json.contains(&format!("\"version\":{SCHEMA_VERSION}")),
            "the version field is not in the written document: {json}"
        );
        assert!(json.contains("\"secret\":true"));

        let back: VariableDocument = serde_json::from_str(&json).expect("deserializes");
        assert_eq!(back, document);
        assert!(back.environment(9).expect("Prod").variables[0].secret);
    }

    #[test]
    fn a_hand_written_variable_without_enabled_is_on() {
        let variable: Variable =
            serde_json::from_str(r#"{"key":"a","value":"1"}"#).expect("deserializes");
        assert!(variable.enabled);
        assert!(!variable.secret);
    }

    #[test]
    fn next_id_never_reuses_one_already_in_the_file() {
        assert_eq!(document().next_id(), 8);
        assert_eq!(VariableDocument::default().next_id(), 1);
    }
}
