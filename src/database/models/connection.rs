//! A saved connection, and the document `connections.json` holds.
//!
//! # The password is stored in plain text, and dodo says so
//!
//! There is no OS keychain here and no `keyring` dependency on any platform.
//! A database password is stored exactly the way the API Explorer already
//! stores a secret variable: plain text in dodo's own data directory, flagged,
//! masked in the UI behind a reveal toggle, and accompanied by a notice that is
//! **never absent** ([`Str::DbPasswordStorageNotice`]). That was a deliberate
//! decision, not an omission, and the design report's `CredentialStore` trait
//! is deliberately **not** built: with one storage behaviour it would be
//! machinery for a choice that no longer exists.
//!
//! [`Str::DbPasswordStorageNotice`]: crate::i18n::Str::DbPasswordStorageNotice
//!
//! # Versioning
//!
//! [`ConnectionDocument`] carries an explicit `version` from its very first
//! write and its parser refuses a file from a newer dodo, copying
//! `environments.json` / `script-consent.json` / `updater.json` rather than
//! `collections.json`, whose `#[serde(default)]`-only scheme copes with added
//! fields and nothing else.

use serde::{Deserialize, Serialize};

use super::engine::{Address, Engine};

/// The schema version written into `connections.json`.
///
/// Bump this when a change to [`ConnectionProfile`] cannot be expressed by
/// adding a `#[serde(default)]` field — an older dodo must then refuse the file
/// rather than half-read it. Version 2 adds MySQL and Redis enum values, which
/// a version-1 reader cannot deserialize.
pub const SCHEMA_VERSION: u32 = 2;

/// Everything `connections.json` holds.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionDocument {
    /// Always written. `parse_document` reads it *first* and refuses anything
    /// higher than [`SCHEMA_VERSION`].
    pub version: u32,
    #[serde(default)]
    pub connections: Vec<ConnectionProfile>,
    /// The id of the connection selected when the app last saved. Restored on
    /// launch so a single-connection user does not have to re-pick it. Not a
    /// session restore: no tab, query or result is persisted.
    #[serde(default)]
    pub selected: Option<u64>,
}

impl Default for ConnectionDocument {
    fn default() -> Self {
        Self {
            version: SCHEMA_VERSION,
            connections: Vec::new(),
            selected: None,
        }
    }
}

impl ConnectionDocument {
    /// An id no saved connection is using. Monotonic over the saved set rather
    /// than a counter, so it survives a hand-edited file.
    pub fn next_id(&self) -> u64 {
        self.connections
            .iter()
            .map(|profile| profile.id)
            .max()
            .map_or(1, |highest| highest + 1)
    }

    pub fn find(&self, id: u64) -> Option<&ConnectionProfile> {
        self.connections.iter().find(|profile| profile.id == id)
    }
}

/// How much dodo insists on TLS when dialling a server.
///
/// The three values use PostgreSQL's familiar `sslmode` vocabulary, reduced to
/// the behaviours shared with MySQL. Certificate validation is never disabled:
/// connectors verify against their root store and dodo ships no insecure mode.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SslMode {
    /// Never negotiate TLS. The right answer for a container on `127.0.0.1`.
    #[serde(rename = "disable")]
    Disable,
    /// Use TLS if the server offers it, plain otherwise. The default, and what
    /// `psql` does.
    #[default]
    #[serde(rename = "prefer")]
    Prefer,
    /// Refuse to connect without TLS.
    #[serde(rename = "require")]
    Require,
}

impl SslMode {
    /// Every mode, in the order the picker shows them.
    pub const ALL: [SslMode; 3] = [SslMode::Disable, SslMode::Prefer, SslMode::Require];
}

/// One saved connection.
///
/// Every field but `id` and `engine` is `#[serde(default)]`, so a file written
/// by an older dodo loads with sensible blanks — the ordinary forward path the
/// version field exists to distinguish from the backward one.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionProfile {
    pub id: u64,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub engine: Engine,

    // ---- Network engines ---------------------------------------------------
    #[serde(default)]
    pub host: String,
    #[serde(default)]
    pub port: u16,
    #[serde(default)]
    pub database: String,
    #[serde(default)]
    pub user: String,
    /// **Stored unencrypted.** See this module's doc. Masked in the UI, and the
    /// form's notice about it is never hidden.
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub ssl_mode: SslMode,

    // ---- File engines ------------------------------------------------------
    /// The database file's path, for a file-addressed engine.
    #[serde(default)]
    pub file: String,
}

impl ConnectionProfile {
    /// A blank profile for `engine`, prefilled with that engine's conventions.
    pub fn new(id: u64, engine: Engine) -> Self {
        Self {
            id,
            name: String::new(),
            engine,
            host: "127.0.0.1".into(),
            port: engine.default_port().unwrap_or_default(),
            database: engine.default_database().into(),
            user: engine.default_user().into(),
            password: String::new(),
            ssl_mode: SslMode::default(),
            file: String::new(),
        }
    }

    /// Re-points a profile at another engine, replacing only the fields whose
    /// old values would be meaningless — so switching PostgreSQL → SQLite and
    /// back does not silently keep a port of 0.
    pub fn set_engine(&mut self, engine: Engine) {
        if self.engine == engine {
            return;
        }
        let previous = self.engine;
        self.engine = engine;
        self.port = engine.default_port().unwrap_or_default();
        if self.user.is_empty() || self.user == previous.default_user() {
            self.user = engine.default_user().into();
        }
        let database_fits = engine != Engine::Redis
            || self
                .database
                .trim()
                .parse::<i64>()
                .is_ok_and(|database| database >= 0);
        if self.database.is_empty()
            || self.database == previous.default_database()
            || !database_fits
        {
            self.database = engine.default_database().into();
        }
    }

    /// What the connection list shows under the name: enough to tell two
    /// connections to the same server apart. Data, never translated.
    pub fn target(&self) -> String {
        match self.engine.address() {
            Address::Network => {
                let mut target = String::new();
                if !self.user.is_empty() {
                    target.push_str(&self.user);
                    target.push('@');
                }
                target.push_str(&self.host);
                if self.port != 0 {
                    target.push(':');
                    target.push_str(&self.port.to_string());
                }
                if !self.database.is_empty() {
                    target.push('/');
                    target.push_str(&self.database);
                }
                target
            }
            Address::File => self.file.clone(),
        }
    }

    /// The name to show when the user never typed one. Falls back to the target
    /// rather than to "Untitled": a connection with no name but a host is
    /// perfectly identifiable, and the host is what the user would have typed.
    pub fn display_name(&self) -> String {
        let name = self.name.trim();
        if !name.is_empty() {
            return name.to_string();
        }
        let target = self.target();
        if target.is_empty() {
            self.engine.display_name().to_string()
        } else {
            target
        }
    }

    /// What is missing before this profile can be connected. `None` means it is
    /// ready. Checked before dialling so an obviously incomplete form fails at
    /// the form rather than as a driver error a second later.
    pub fn problem(&self) -> Option<ProfileProblem> {
        match self.engine.address() {
            Address::Network => {
                if self.host.trim().is_empty() {
                    return Some(ProfileProblem::HostMissing);
                }
                if self.port == 0 {
                    return Some(ProfileProblem::PortMissing);
                }
                if self.database.trim().is_empty() {
                    return Some(ProfileProblem::DatabaseMissing);
                }
                if self.engine == Engine::Redis
                    && self
                        .database
                        .trim()
                        .parse::<i64>()
                        .ok()
                        .is_none_or(|database| database < 0)
                {
                    return Some(ProfileProblem::RedisDatabaseInvalid);
                }
                None
            }
            Address::File => self
                .file
                .trim()
                .is_empty()
                .then_some(ProfileProblem::FileMissing),
        }
    }

    /// The connection as one URL — what the hover card's `URL` row shows.
    ///
    /// **The password is not in it**, in any form, not even masked. A URL is
    /// the one string a user is most likely to copy out of a client and paste
    /// somewhere else, so the rule this module opens with matters here more
    /// than anywhere: the password lives in `connections.json` and in the form
    /// behind a reveal toggle, and nowhere a glance or a screenshot reaches.
    pub fn url(&self) -> String {
        let scheme = self.engine.url_scheme();
        match self.engine.address() {
            Address::Network => format!("{scheme}://{}", self.target()),
            // Three slashes: the target is already an absolute path, so
            // `sqlite://` plus `/tmp/app.db` is the conventional form.
            Address::File => format!("{scheme}://{}", self.file),
        }
    }

    /// The connection as label/value rows, for the hover card on its tree row.
    ///
    /// Only the fields that mean something for this engine, and only the ones
    /// that are filled in — a blank row says nothing and costs a line. The
    /// labels are [`DetailField`]s rather than text, because the view is what
    /// translates; the values are data and are never translated.
    ///
    /// The password is not a [`DetailField`] at all. That is the point, and
    /// there is a test that says so.
    pub fn details(&self) -> Vec<(DetailField, String)> {
        let mut rows = vec![
            (DetailField::Name, self.display_name()),
            (DetailField::Url, self.url()),
        ];
        match self.engine.address() {
            Address::Network => {
                rows.push((DetailField::Host, self.host.clone()));
                if self.port != 0 {
                    rows.push((DetailField::Port, self.port.to_string()));
                }
                rows.push((DetailField::Database, self.database.clone()));
                rows.push((DetailField::User, self.user.clone()));
            }
            Address::File => rows.push((DetailField::File, self.file.clone())),
        }
        rows.push((DetailField::Type, self.engine.display_name().to_string()));
        rows.retain(|(_, value)| !value.trim().is_empty());
        rows
    }

    /// A copy of this profile under a new id, named so the two are told apart
    /// in the list. Mirrors the API Explorer's environment duplication.
    pub fn duplicated(&self, id: u64, suffix: &str) -> Self {
        Self {
            id,
            name: format!("{} {suffix}", self.display_name()),
            ..self.clone()
        }
    }
}

/// One row of a connection's hover card.
///
/// There is deliberately **no `Password` variant**. The card is shown on hover
/// over a tree row — a glance, a screenshot, a shared screen — and a masked
/// password there would say "there is one" while teaching nobody anything. See
/// [`ConnectionProfile::details`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DetailField {
    Name,
    Url,
    Host,
    Port,
    Database,
    User,
    File,
    Type,
}

impl DetailField {
    /// Every field, in the order the card lists them. Nothing draws from it —
    /// the card draws what [`ConnectionProfile::details`] gives it — so it
    /// exists for the tests that prove the set is what it claims to be, the
    /// password one above all.
    #[cfg(test)]
    pub const ALL: [DetailField; 8] = [
        DetailField::Name,
        DetailField::Url,
        DetailField::Host,
        DetailField::Port,
        DetailField::Database,
        DetailField::User,
        DetailField::File,
        DetailField::Type,
    ];

    /// The word beside the value. These are the connection form's own labels:
    /// the card and the form name the same thing the same way.
    pub fn label(self) -> crate::i18n::Str {
        use crate::i18n::Str;
        match self {
            DetailField::Name => Str::DbFieldName,
            DetailField::Url => Str::DbFieldUrl,
            DetailField::Host => Str::DbFieldHost,
            DetailField::Port => Str::DbFieldPort,
            DetailField::Database => Str::DbFieldDatabase,
            DetailField::User => Str::DbFieldUser,
            DetailField::File => Str::DbFieldFile,
            DetailField::Type => Str::DbFieldEngine,
        }
    }
}

/// Why a profile cannot be connected yet.
///
/// Every variant names the field that is absent, so the shared `Missing`
/// suffix is the point rather than an accident: the form shows one message and
/// it has to say *which* field.
#[allow(
    clippy::enum_variant_names,
    reason = "the shared suffix is the meaning"
)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfileProblem {
    HostMissing,
    PortMissing,
    DatabaseMissing,
    RedisDatabaseInvalid,
    FileMissing,
}

impl ProfileProblem {
    pub fn message(self) -> crate::i18n::Str {
        use crate::i18n::Str;
        match self {
            ProfileProblem::HostMissing => Str::DbProfileHostMissing,
            ProfileProblem::PortMissing => Str::DbProfilePortMissing,
            ProfileProblem::DatabaseMissing => Str::DbProfileDatabaseMissing,
            ProfileProblem::RedisDatabaseInvalid => Str::DbProfileRedisDatabaseInvalid,
            ProfileProblem::FileMissing => Str::DbProfileFileMissing,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ConnectionDocument, ConnectionProfile, DetailField, ProfileProblem, SCHEMA_VERSION, SslMode,
    };
    use crate::database::models::engine::Engine;

    fn postgres() -> ConnectionProfile {
        ConnectionProfile {
            database: "shop".into(),
            name: "Local".into(),
            ..ConnectionProfile::new(1, Engine::PostgreSql)
        }
    }

    #[test]
    fn a_fresh_profile_carries_its_engines_conventions() {
        let profile = ConnectionProfile::new(7, Engine::PostgreSql);
        assert_eq!(profile.port, 5432);
        assert_eq!(profile.user, "postgres");
        assert_eq!(profile.host, "127.0.0.1");
        assert_eq!(profile.ssl_mode, SslMode::Prefer);

        let file = ConnectionProfile::new(8, Engine::Sqlite);
        assert_eq!(file.port, 0, "a file engine suggests no port");
        assert_eq!(file.user, "");

        let mysql = ConnectionProfile::new(9, Engine::MySql);
        assert_eq!(mysql.port, 3306);
        assert_eq!(mysql.user, "root");

        let redis = ConnectionProfile::new(10, Engine::Redis);
        assert_eq!(redis.port, 6379);
        assert_eq!(redis.database, "0");
        assert_eq!(redis.user, "");
    }

    /// `127.0.0.1`, not `localhost`: `localhost` is IPv4/IPv6-ambiguous, and
    /// some clients read it as "use the unix socket", which is exactly wrong
    /// for a published container port.
    #[test]
    fn the_default_host_is_the_loopback_address_not_localhost() {
        assert_eq!(
            ConnectionProfile::new(1, Engine::PostgreSql).host,
            "127.0.0.1"
        );
    }

    #[test]
    fn switching_engine_replaces_the_facts_that_stopped_making_sense() {
        let mut profile = postgres();
        profile.set_engine(Engine::Sqlite);
        assert_eq!(profile.port, 0);

        profile.set_engine(Engine::PostgreSql);
        assert_eq!(profile.port, 5432);
        assert_eq!(profile.database, "shop", "the database name is still valid");

        profile.user = "postgres".into();
        profile.database = "shop".into();
        profile.set_engine(Engine::Redis);
        assert_eq!(profile.port, 6379);
        assert_eq!(profile.user, "");
        assert_eq!(profile.database, "0");
    }

    #[test]
    fn switching_to_the_same_engine_changes_nothing() {
        let mut profile = postgres();
        profile.port = 6543;
        profile.set_engine(Engine::PostgreSql);
        assert_eq!(profile.port, 6543, "an edited port must not be reset");
    }

    #[test]
    fn the_target_line_identifies_a_network_connection() {
        assert_eq!(postgres().target(), "postgres@127.0.0.1:5432/shop");
    }

    #[test]
    fn the_target_line_of_a_file_connection_is_the_file() {
        let mut profile = ConnectionProfile::new(2, Engine::Sqlite);
        profile.file = "/tmp/app.db".into();
        assert_eq!(profile.target(), "/tmp/app.db");
    }

    #[test]
    fn an_unnamed_connection_shows_its_target_rather_than_untitled() {
        let mut profile = postgres();
        profile.name = "   ".into();
        assert_eq!(profile.display_name(), "postgres@127.0.0.1:5432/shop");
    }

    #[test]
    fn an_unnamed_untargeted_connection_falls_back_to_the_product_name() {
        let profile = ConnectionProfile::new(3, Engine::Sqlite);
        assert_eq!(profile.display_name(), "SQLite");
    }

    #[test]
    fn a_complete_profile_has_no_problem_and_an_incomplete_one_names_it() {
        assert_eq!(postgres().problem(), None);

        let mut blank = ConnectionProfile::new(1, Engine::PostgreSql);
        assert_eq!(blank.problem(), Some(ProfileProblem::DatabaseMissing));

        blank.database = "shop".into();
        blank.host = "  ".into();
        assert_eq!(blank.problem(), Some(ProfileProblem::HostMissing));

        blank.host = "db".into();
        blank.port = 0;
        assert_eq!(blank.problem(), Some(ProfileProblem::PortMissing));

        let file = ConnectionProfile::new(1, Engine::Sqlite);
        assert_eq!(file.problem(), Some(ProfileProblem::FileMissing));

        let mut redis = ConnectionProfile::new(2, Engine::Redis);
        assert_eq!(redis.problem(), None);
        redis.database = "-1".into();
        assert_eq!(redis.problem(), Some(ProfileProblem::RedisDatabaseInvalid));
        redis.database = "not-a-number".into();
        assert_eq!(redis.problem(), Some(ProfileProblem::RedisDatabaseInvalid));
    }

    #[test]
    fn a_network_url_is_the_scheme_and_the_target() {
        assert_eq!(
            postgres().url(),
            "postgresql://postgres@127.0.0.1:5432/shop"
        );
    }

    #[test]
    fn a_file_url_names_the_file() {
        let mut profile = ConnectionProfile::new(2, Engine::Sqlite);
        profile.file = "/tmp/app.db".into();
        assert_eq!(profile.url(), "sqlite:///tmp/app.db");
    }

    /// The rule this module opens with, checked where it is easiest to break:
    /// the hover card is a glance, a screenshot, a shared screen.
    #[test]
    fn no_detail_row_carries_the_password_in_any_form() {
        let mut profile = postgres();
        profile.password = "hunter2-do-not-leak".into();

        for (field, value) in profile.details() {
            assert!(
                !value.contains("hunter2"),
                "{field:?} leaked the password: {value}"
            );
        }
        assert!(!profile.url().contains("hunter2"));
        assert!(
            !DetailField::ALL
                .iter()
                .any(|field| matches!(field.label(), crate::i18n::Str::DbFieldPassword)),
            "no card row may even be labelled Password"
        );
    }

    #[test]
    fn a_network_connections_card_names_the_server_and_a_file_ones_names_the_file() {
        let fields: Vec<DetailField> = postgres().details().into_iter().map(|(f, _)| f).collect();
        assert_eq!(
            fields,
            vec![
                DetailField::Name,
                DetailField::Url,
                DetailField::Host,
                DetailField::Port,
                DetailField::Database,
                DetailField::User,
                DetailField::Type,
            ]
        );

        let mut file = ConnectionProfile::new(2, Engine::Sqlite);
        file.file = "/tmp/app.db".into();
        let fields: Vec<DetailField> = file.details().into_iter().map(|(f, _)| f).collect();
        assert_eq!(
            fields,
            vec![
                DetailField::Name,
                DetailField::Url,
                DetailField::File,
                DetailField::Type,
            ],
            "a file connection has no host, port or user to show"
        );
    }

    /// A blank row says nothing and costs a line, so it is not drawn.
    #[test]
    fn a_field_the_user_left_empty_is_left_out() {
        let mut profile = postgres();
        profile.user = "  ".into();
        let fields: Vec<DetailField> = profile.details().into_iter().map(|(f, _)| f).collect();
        assert!(!fields.contains(&DetailField::User));
        assert!(fields.contains(&DetailField::Host));
    }

    #[test]
    fn every_card_row_has_a_label_in_every_language() {
        for field in DetailField::ALL {
            for language in crate::i18n::Language::ALL {
                assert!(!field.label().text(language).trim().is_empty());
            }
        }
    }

    #[test]
    fn a_duplicate_takes_a_new_id_and_a_distinguishable_name() {
        let copy = postgres().duplicated(9, "copy");
        assert_eq!(copy.id, 9);
        assert_eq!(copy.name, "Local copy");
        assert_eq!(copy.database, "shop");
        assert_eq!(copy.password, postgres().password);
    }

    #[test]
    fn the_next_id_never_collides_with_a_saved_one() {
        let mut document = ConnectionDocument::default();
        assert_eq!(document.next_id(), 1);

        document
            .connections
            .push(ConnectionProfile::new(4, Engine::Sqlite));
        document
            .connections
            .push(ConnectionProfile::new(2, Engine::Sqlite));
        assert_eq!(document.next_id(), 5);
    }

    #[test]
    fn a_default_document_carries_the_schema_version() {
        assert_eq!(ConnectionDocument::default().version, SCHEMA_VERSION);
    }

    /// Like the engine's, these names are in `connections.json` and must not
    /// drift.
    #[test]
    fn ssl_modes_serialize_to_pinned_names_and_default_to_prefer() {
        assert_eq!(
            serde_json::to_string(&SslMode::Disable).expect("serializes"),
            "\"disable\""
        );
        assert_eq!(
            serde_json::to_string(&SslMode::Require).expect("serializes"),
            "\"require\""
        );
        for mode in SslMode::ALL {
            let json = serde_json::to_string(&mode).expect("serializes");
            let back: SslMode = serde_json::from_str(&json).expect("deserializes");
            assert_eq!(back, mode);
        }
        assert_eq!(SslMode::default(), SslMode::Prefer);
    }

    /// A file from an older dodo is the ordinary forward path and must load
    /// with blanks rather than fail.
    #[test]
    fn a_profile_missing_every_optional_field_still_deserializes() {
        let profile: ConnectionProfile =
            serde_json::from_str(r#"{"id":3,"engine":"sqlite"}"#).expect("loads");
        assert_eq!(profile.id, 3);
        assert_eq!(profile.engine, Engine::Sqlite);
        assert_eq!(profile.ssl_mode, SslMode::Prefer);
        assert!(profile.file.is_empty());
    }
}
