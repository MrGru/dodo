//! Which database product a connection speaks to.
//!
//! One enum, and every per-product fact hangs off it: the product's name, how
//! its connection is addressed, its default port, and the language id its query
//! editor is opened with. Adding a backend is a variant plus one arm in each
//! match here, one file under `services/`, and one arm in `services::connect` —
//! nothing under `views/` changes shape, which is what the design report means
//! by "add a database type without changing the UI".
//!
//! There is deliberately **no icon method here**: an icon is
//! [`AppIcon`](crate::app_icon::AppIcon), which is GPUI, and `models/` names no
//! GPUI so that every one of these facts stays testable without a `Window`. The
//! `Engine` → icon mapping lives with the views that draw it.

use serde::{Deserialize, Serialize};

/// A database product dodo can connect to.
///
/// The serialized form is the `serde(rename)` below, not the variant name, so
/// renaming a variant cannot orphan somebody's saved connections. There is no
/// `id()`/`from_id()` pair beside it: serde owns that mapping, and a second
/// spelling of it would be one more thing to keep in step.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Engine {
    #[default]
    #[serde(rename = "postgresql")]
    PostgreSql,
    #[serde(rename = "sqlite")]
    Sqlite,
}

/// How a connection to an engine is addressed. This is the whole reason the
/// connection form has two shapes and the reason a future key/value store fits
/// without a third: a store is either something you dial or something you open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Address {
    /// Host, port, user, password, database — a server on the network.
    Network,
    /// One path on this machine.
    File,
}

impl Engine {
    /// Every engine this build can connect to, in the order the picker lists
    /// them.
    pub const ALL: [Engine; 2] = [Engine::PostgreSql, Engine::Sqlite];

    /// The product's own name. A proper noun in every language, so it is a
    /// `&'static str` rather than a [`Str`](crate::i18n::Str) — the same
    /// treatment "Dodo" gets.
    pub fn display_name(self) -> &'static str {
        match self {
            Engine::PostgreSql => "PostgreSQL",
            Engine::Sqlite => "SQLite",
        }
    }

    /// Whether this engine is dialled or opened.
    pub fn address(self) -> Address {
        match self {
            Engine::PostgreSql => Address::Network,
            Engine::Sqlite => Address::File,
        }
    }

    /// The port a fresh connection form starts on, or `None` for an engine that
    /// has no port.
    pub fn default_port(self) -> Option<u16> {
        match self {
            Engine::PostgreSql => Some(5432),
            Engine::Sqlite => None,
        }
    }

    /// The user a fresh connection form starts on. The conventional superuser
    /// for the product, which is what a locally-run container or install has.
    pub fn default_user(self) -> &'static str {
        match self {
            Engine::PostgreSql => "postgres",
            Engine::Sqlite => "",
        }
    }

    /// The language id handed to the query editor's `code_editor`, so a backend
    /// whose console is not SQL is never coloured as though it were. Both of
    /// this round's engines speak SQL; the method exists because the next one
    /// might not, and because it is the honest place for that answer.
    ///
    /// `"sql"` only colours anything when the `sql-highlighting` cargo feature
    /// is on — see `Cargo.toml`. Without it the editor falls back to plain
    /// text, which is the library's own graceful default.
    pub fn editor_language(self) -> &'static str {
        match self {
            Engine::PostgreSql | Engine::Sqlite => "sql",
        }
    }

    /// The scheme of the connection URL shown on hover. The product's own
    /// conventional one, so the string is something a user could paste into
    /// `psql` or a driver config and recognise.
    pub fn url_scheme(self) -> &'static str {
        match self {
            Engine::PostgreSql => "postgresql",
            Engine::Sqlite => "sqlite",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Address, Engine};

    /// The serialized names are what `connections.json` holds. Changing one
    /// orphans every saved connection of that engine, so they are pinned by
    /// literal here and every engine round trips.
    #[test]
    fn the_serialized_names_are_pinned_and_round_trip() {
        assert_eq!(
            serde_json::to_string(&Engine::PostgreSql).expect("serializes"),
            "\"postgresql\""
        );
        assert_eq!(
            serde_json::to_string(&Engine::Sqlite).expect("serializes"),
            "\"sqlite\""
        );

        for engine in Engine::ALL {
            let json = serde_json::to_string(&engine).expect("serializes");
            let back: Engine = serde_json::from_str(&json).expect("deserializes");
            assert_eq!(back, engine);
        }

        assert!(serde_json::from_str::<Engine>("\"mysql\"").is_err());
    }

    #[test]
    fn a_file_engine_has_no_port_and_a_network_engine_does() {
        assert_eq!(Engine::PostgreSql.address(), Address::Network);
        assert_eq!(Engine::PostgreSql.default_port(), Some(5432));

        assert_eq!(Engine::Sqlite.address(), Address::File);
        assert_eq!(
            Engine::Sqlite.default_port(),
            None,
            "a file-addressed engine must not suggest a port"
        );
        assert_eq!(Engine::Sqlite.default_user(), "");
    }

    #[test]
    fn every_engine_names_an_editor_language() {
        for engine in Engine::ALL {
            assert!(!engine.editor_language().is_empty());
            assert!(!engine.display_name().is_empty());
        }
    }
}
