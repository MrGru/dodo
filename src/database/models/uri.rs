//! Turning a pasted connection URI into a [`ConnectionProfile`].
//!
//! The user pastes what their host, their `docker run` line or their teammate
//! gave them and the add-connection form fills itself in. This module is the
//! whole of that: a pure `&str` → profile function with no GPUI and no driver
//! crate, so every form a real URI takes is a unit test rather than something
//! checked by clicking.
//!
//! # Where the accepted forms come from
//!
//! Each engine's URI shape is taken from the crate that actually dials it, not
//! from memory:
//!
//! - **PostgreSQL** — `tokio-postgres`'s `Config` doc (`src/config.rs`, "# Url")
//!   names both `postgres://` and `postgresql://`, says every component is
//!   optional, allows comma-separated host/port pairs, and accepts every
//!   key-value connection parameter as a query parameter. Its `sslmode` takes
//!   `disable` / `prefer` / `require`, which is where [`SslMode`]'s vocabulary
//!   came from in the first place.
//! - **MySQL / MariaDB** — the `mysql` crate's `Opts::from_url` hands the query
//!   pairs to `from_hash_map`, whose documented keys include `user`,
//!   `password`, `host`, `port` and `db_name`. It models **no** TLS parameter,
//!   so the `ssl-mode` spelling accepted here is the server's own (what the
//!   `mysql` client and the Connectors use), mapped onto dodo's three modes.
//! - **Redis** — the `redis` crate's `ConnectionInfo` doc lists
//!   `redis://user:password@host:port/db`, `redis://:password@host` and
//!   `rediss://` for TLS, and its scheme match also accepts `valkey` /
//!   `valkeys`. `db` as a query parameter is that crate's unix-socket spelling,
//!   accepted here for TCP too because it costs nothing and is what a user who
//!   knows it will type.
//! - **SQLite** — there is no dialled URI: the file engines' conventional form
//!   is the scheme plus a path, which is exactly what
//!   [`ConnectionProfile::url`] already writes (`sqlite:///tmp/app.db`), so
//!   parsing is defined to round-trip it.
//!
//! # The readings that are choices rather than facts
//!
//! Written down because each of them is a place a reasonable person would have
//! chosen differently:
//!
//! - **A `sqlite:` path is whatever follows the scheme**, with one `//`
//!   stripped. `sqlite:///tmp/app.db` is `/tmp/app.db` and `sqlite://app.db` is
//!   the relative `app.db`. That is the common reading and it round-trips
//!   dodo's own URL; SQLAlchemy's "four slashes mean absolute" is not.
//! - **Query parameters win over the same value in the authority**, matching
//!   what `tokio-postgres` does when it applies the query after the URL.
//! - **An empty user or an absent one both leave the engine's default user.**
//!   An empty password and an absent one are likewise the same thing here,
//!   because they are the same thing to every driver dodo has: each one only
//!   sends a password when it is non-empty.
//! - **The first host of a comma-separated list is used** and the rest are
//!   reported as not applied. dodo dials one server.
//! - **A stray `%` is passed through** rather than refused, the way the
//!   percent-encoding crate does it, so a password typed with a literal `%`
//!   still arrives. Only an escape that decodes to invalid UTF-8 is an error.
//!
//! # Nothing is dropped in silence
//!
//! Anything the URI carried that dodo does not model — an unknown query
//! parameter, a second host, a fragment, extra path segments — is named in
//! [`ParsedUri::ignored`] so the form can say so. A `rediss://` URI is parsed
//! and its `ssl_mode` recorded, but [`ParsedUri::tls_unsupported`] is set,
//! because dodo's Redis client dials plaintext and a silent downgrade is the
//! one thing worse than an unsupported scheme.

use percent_encoding::percent_decode_str;

use crate::i18n::Str;

use super::connection::{ConnectionProfile, SslMode};
use super::engine::{Address, Engine};

/// A URI that parsed, and everything the form needs to say about it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedUri {
    /// The profile as the URI describes it. Every field the URI did not carry
    /// is left at [`ConnectionProfile::new`]'s value for that engine, so an
    /// omitted port is the engine's default port and never `0`.
    pub profile: ConnectionProfile,
    /// The parts of the URI that were understood but not applied, in the order
    /// they were met. Data — a parameter name, a host — shown inside a
    /// translated frame.
    pub ignored: Vec<String>,
    /// The URI asked for TLS and this engine's client in dodo cannot give it.
    /// Only `rediss://` / `valkeys://` set it.
    pub tls_unsupported: bool,
}

impl ParsedUri {
    /// A name for a connection the user has not named yet.
    ///
    /// The database is what tells two connections to the same server apart, so
    /// it comes first — except on Redis, where it is a small integer and the
    /// host is the only informative half. A file connection is named after its
    /// file rather than its whole path.
    pub fn suggested_name(&self) -> String {
        match self.profile.engine.address() {
            Address::File => file_name(&self.profile.file),
            Address::Network => {
                let database = self.profile.database.trim();
                if self.profile.engine == Engine::Redis || database.is_empty() {
                    self.profile.host.trim().to_string()
                } else {
                    database.to_string()
                }
            }
        }
    }
}

/// Why a pasted URI could not be read.
///
/// Every variant names what was wrong with the text, because the form shows one
/// message and the user's next act is to fix the string they pasted. **A parse
/// either succeeds or changes nothing**: there is no partial result, so no user
/// is ever left guessing which fields came from their paste.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UriError {
    /// Nothing was pasted.
    Empty,
    /// No `scheme:` at the front, so there is nothing to identify the engine.
    NoScheme,
    /// A scheme that names no engine dodo supports. Carries the scheme itself,
    /// because "unsupported" without the word is useless.
    UnknownScheme(String),
    /// The port is not a number in `1..=65535`. Carries the text as pasted.
    InvalidPort(String),
    /// A `sqlite:` URI with no path.
    MissingFile,
    /// A `%xx` escape that does not decode to UTF-8.
    InvalidEscape,
}

impl UriError {
    pub fn message(&self) -> Str {
        match self {
            UriError::Empty => Str::DbUriEmpty,
            UriError::NoScheme => Str::DbUriNoScheme,
            UriError::UnknownScheme(scheme) => Str::DbUriUnknownScheme(scheme.clone()),
            UriError::InvalidPort(port) => Str::DbUriInvalidPort(port.clone()),
            UriError::MissingFile => Str::DbUriMissingFile,
            UriError::InvalidEscape => Str::DbUriInvalidEscape,
        }
    }
}

/// Reads `input` as a connection URI, giving `id` to the profile it builds.
///
/// The engine comes from the scheme, so the user does not pick it first; a
/// scheme that names no supported engine is [`UriError::UnknownScheme`] and
/// never a silent PostgreSQL.
pub fn parse(input: &str, id: u64) -> Result<ParsedUri, UriError> {
    let input = input.trim();
    if input.is_empty() {
        return Err(UriError::Empty);
    }

    let (scheme, rest) = split_scheme(input)?;
    let (engine, tls_scheme) =
        engine_for_scheme(&scheme).ok_or_else(|| UriError::UnknownScheme(scheme.clone()))?;

    let mut parsed = ParsedUri {
        profile: ConnectionProfile::new(id, engine),
        ignored: Vec::new(),
        tls_unsupported: false,
    };

    // The fragment ends the URI, so it is taken off before the query.
    let (rest, fragment) = split_first(rest, '#');
    let (rest, query) = split_first(rest, '?');
    if let Some(fragment) = fragment.filter(|fragment| !fragment.is_empty()) {
        parsed.ignore(format!("#{fragment}"));
    }

    match engine.address() {
        Address::Network => read_authority(rest, &mut parsed)?,
        Address::File => parsed.profile.file = read_file_path(rest)?,
    }

    if let Some(query) = query {
        read_query(query, &mut parsed)?;
    }

    if tls_scheme {
        // Recorded even though this build cannot honour it: the field is
        // already persisted, and a future TLS-capable Redis client should find
        // the user's intent rather than a default.
        parsed.profile.ssl_mode = SslMode::Require;
        parsed.tls_unsupported = !engine.supports_tls();
    }

    if engine.address() == Address::File && parsed.profile.file.trim().is_empty() {
        return Err(UriError::MissingFile);
    }

    Ok(parsed)
}

impl ParsedUri {
    /// Records a part of the URI that was read but not applied, once.
    fn ignore(&mut self, part: impl Into<String>) {
        let part = part.into();
        if !self.ignored.contains(&part) {
            self.ignored.push(part);
        }
    }
}

/// Splits `scheme:` off the front.
///
/// The scheme charset is RFC 3986's — a letter, then letters, digits, `+`, `-`
/// or `.` — so a bare `host:5432/db` fails the *engine* lookup with its own
/// name in the message, while `/tmp/app.db` fails here as having no scheme at
/// all.
fn split_scheme(input: &str) -> Result<(String, &str), UriError> {
    let (scheme, rest) = input.split_once(':').ok_or(UriError::NoScheme)?;
    let mut characters = scheme.chars();
    let starts_well = characters
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic());
    let rest_well = characters.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    if !starts_well || !rest_well {
        return Err(UriError::NoScheme);
    }
    Ok((scheme.to_ascii_lowercase(), rest))
}

/// The engine a scheme names, and whether the scheme itself asks for TLS.
fn engine_for_scheme(scheme: &str) -> Option<(Engine, bool)> {
    match scheme {
        "postgres" | "postgresql" => Some((Engine::PostgreSql, false)),
        "mysql" | "mariadb" => Some((Engine::MySql, false)),
        "redis" | "valkey" => Some((Engine::Redis, false)),
        "rediss" | "valkeys" => Some((Engine::Redis, true)),
        // `file:` is SQLite's own URI-filename scheme, and dodo's only
        // file-addressed engine is SQLite.
        "sqlite" | "sqlite3" | "file" => Some((Engine::Sqlite, false)),
        _ => None,
    }
}

/// Splits at the first `separator`, if there is one.
fn split_first(text: &str, separator: char) -> (&str, Option<&str>) {
    match text.split_once(separator) {
        Some((before, after)) => (before, Some(after)),
        None => (text, None),
    }
}

/// Reads `//user:password@host:port/database` into `parsed`.
fn read_authority(rest: &str, parsed: &mut ParsedUri) -> Result<(), UriError> {
    let rest = rest.strip_prefix("//").unwrap_or(rest);
    let (authority, path) = split_first(rest, '/');

    // `rfind`, not `find`: a password pasted with an unescaped `@` in it is the
    // normal case, and a host cannot contain one.
    let (userinfo, hostport) = match authority.rfind('@') {
        Some(at) => (Some(&authority[..at]), &authority[at + 1..]),
        None => (None, authority),
    };

    if let Some(userinfo) = userinfo {
        let (user, password) = split_first(userinfo, ':');
        let user = decode(user)?;
        // An empty user keeps the engine's default rather than blanking it: an
        // empty `user:` segment says nothing, and the default is what the form
        // would have shown.
        if !user.is_empty() {
            parsed.profile.user = user;
        }
        if let Some(password) = password {
            parsed.profile.password = decode(password)?;
        }
    }

    let mut hosts = hostport.split(',');
    let first = hosts.next().unwrap_or_default();
    for extra in hosts.filter(|host| !host.is_empty()) {
        parsed.ignore(extra.to_string());
    }

    let (host, port) = split_host_port(first)?;
    let host = decode(&host)?;
    if !host.is_empty() {
        parsed.profile.host = host;
    }
    if let Some(port) = port {
        parsed.profile.port = port;
    }

    let mut segments = path
        .unwrap_or("")
        .split('/')
        .filter(|segment| !segment.is_empty());
    if let Some(database) = segments.next() {
        parsed.profile.database = decode(database)?;
    }
    for extra in segments {
        parsed.ignore(format!("/{extra}"));
    }

    Ok(())
}

/// Splits `host:port`, understanding a bracketed IPv6 literal.
///
/// An unbracketed address with more than one colon is taken as a whole host
/// rather than guessed at: `[::1]:6379` has a port and `::1` does not.
fn split_host_port(hostport: &str) -> Result<(String, Option<u16>), UriError> {
    if let Some(rest) = hostport.strip_prefix('[') {
        let (host, after) = rest.split_once(']').ok_or_else(|| {
            // An unclosed bracket is not a port problem, but it is the only
            // thing it can be reported as without inventing a variant for a
            // string nobody pastes twice.
            UriError::InvalidPort(hostport.to_string())
        })?;
        let port = match after.strip_prefix(':') {
            Some(port) => parse_port(port)?,
            None => None,
        };
        return Ok((host.to_string(), port));
    }

    if hostport.matches(':').count() > 1 {
        return Ok((hostport.to_string(), None));
    }

    match hostport.rsplit_once(':') {
        Some((host, port)) => Ok((host.to_string(), parse_port(port)?)),
        None => Ok((hostport.to_string(), None)),
    }
}

/// An empty port is an absent one — `port=1234,,5678` is a form libpq itself
/// writes — and anything else must be a real port number.
fn parse_port(port: &str) -> Result<Option<u16>, UriError> {
    if port.is_empty() {
        return Ok(None);
    }
    match port.parse::<u16>() {
        Ok(0) | Err(_) => Err(UriError::InvalidPort(port.to_string())),
        Ok(port) => Ok(Some(port)),
    }
}

/// Reads a `sqlite:` URI's path: everything after the scheme, with one `//`
/// stripped. `sqlite::memory:` therefore yields `:memory:`.
fn read_file_path(rest: &str) -> Result<String, UriError> {
    decode(rest.strip_prefix("//").unwrap_or(rest))
}

/// Applies the query parameters this engine models and names the rest.
///
/// `+` is left alone rather than read as a space: this is a connection URI, not
/// an HTML form, and a password containing a `+` is far likelier than one
/// containing a space.
fn read_query(query: &str, parsed: &mut ParsedUri) -> Result<(), UriError> {
    for pair in query.split('&').filter(|pair| !pair.is_empty()) {
        let (key, value) = split_first(pair, '=');
        let key = decode(key)?;
        let value = decode(value.unwrap_or(""))?;
        let key = key.to_ascii_lowercase();

        let applied = match parsed.profile.engine {
            Engine::PostgreSql | Engine::MySql => apply_sql_parameter(&key, &value, parsed)?,
            Engine::Redis => apply_redis_parameter(&key, &value, parsed),
            // SQLite's URI parameters (`mode`, `cache`, `immutable`) address
            // how the file is opened, which dodo's SQLite driver decides for
            // itself. None of them is modelled, so all of them are named.
            Engine::Sqlite => false,
        };
        if !applied {
            parsed.ignore(key);
        }
    }
    Ok(())
}

/// The parameters both SQL engines share a spelling for, plus each one's TLS
/// parameter. Returns whether the parameter was applied.
fn apply_sql_parameter(key: &str, value: &str, parsed: &mut ParsedUri) -> Result<bool, UriError> {
    match key {
        "user" | "username" => parsed.profile.user = value.to_string(),
        "password" => parsed.profile.password = value.to_string(),
        "dbname" | "database" | "db_name" => parsed.profile.database = value.to_string(),
        "host" => parsed.profile.host = value.to_string(),
        "port" => {
            parsed.profile.port =
                parse_port(value)?.ok_or_else(|| UriError::InvalidPort(value.to_string()))?;
        }
        // PostgreSQL spells it `sslmode`; MySQL's own clients spell it
        // `ssl-mode`. Both are accepted for both, because a user pasting one
        // at the other is not making an interesting mistake.
        "sslmode" | "ssl-mode" | "ssl_mode" => {
            return Ok(match ssl_mode_for(value) {
                Some(mode) => {
                    parsed.profile.ssl_mode = mode;
                    true
                }
                // An unrecognised value leaves the default and is named,
                // rather than being read as the nearest thing and quietly
                // changing how dodo dials.
                None => false,
            });
        }
        _ => return Ok(false),
    }
    Ok(true)
}

/// A `sslmode` / `ssl-mode` value in either engine's vocabulary.
///
/// libpq's `allow` prefers plaintext but will negotiate TLS, so [`SslMode`]'s
/// `Prefer` is the closest of the three dodo has. `verify-ca` / `verify-full`
/// and MySQL's `VERIFY_*` fold into `Require` without losing anything: dodo's
/// connectors verify against the platform trust store in every mode and ship no
/// way to turn that off, so "require TLS" is already "require a verified TLS".
fn ssl_mode_for(value: &str) -> Option<SslMode> {
    match value.to_ascii_lowercase().as_str() {
        "disable" | "disabled" | "false" | "0" => Some(SslMode::Disable),
        "allow" | "prefer" | "preferred" => Some(SslMode::Prefer),
        "require" | "required" | "true" | "1" | "verify-ca" | "verify_ca" | "verify-full"
        | "verify_ca_identity" | "verify_identity" | "verify-identity" => Some(SslMode::Require),
        _ => None,
    }
}

/// Redis's own query parameters. Its logical database is a path segment, but
/// the `redis` crate accepts `db` in the query for unix sockets and users type
/// it for TCP too.
fn apply_redis_parameter(key: &str, value: &str, parsed: &mut ParsedUri) -> bool {
    match key {
        "db" | "database" => parsed.profile.database = value.to_string(),
        "user" | "username" => parsed.profile.user = value.to_string(),
        "password" => parsed.profile.password = value.to_string(),
        _ => return false,
    }
    true
}

/// Percent-decodes one component.
///
/// A `%` that is not followed by two hex digits is passed through, which is
/// what the percent-encoding crate does and what makes a password typed with a
/// literal `%` survive. Only bytes that are not UTF-8 once decoded are refused.
fn decode(raw: &str) -> Result<String, UriError> {
    percent_decode_str(raw)
        .decode_utf8()
        .map(|decoded| decoded.into_owned())
        .map_err(|_| UriError::InvalidEscape)
}

/// The last path component of `path`, for naming a file connection. Both
/// separators, because a Windows path is a perfectly ordinary thing to paste.
fn file_name(path: &str) -> String {
    let path = path.trim().trim_end_matches(['/', '\\']);
    path.rsplit(['/', '\\'])
        .find(|component| !component.is_empty())
        .unwrap_or(path)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::{ParsedUri, UriError, parse};
    use crate::database::models::connection::SslMode;
    use crate::database::models::engine::Engine;
    use crate::i18n::Language;

    fn ok(input: &str) -> ParsedUri {
        parse(input, 7).unwrap_or_else(|error| panic!("{input} did not parse: {error:?}"))
    }

    // ---- the four engines and their scheme aliases -----------------------

    #[test]
    fn a_postgresql_uri_fills_every_field_it_carries() {
        let parsed = ok("postgresql://alice:s3cret@db.example.com:6543/shop");
        assert_eq!(parsed.profile.engine, Engine::PostgreSql);
        assert_eq!(parsed.profile.host, "db.example.com");
        assert_eq!(parsed.profile.port, 6543);
        assert_eq!(parsed.profile.database, "shop");
        assert_eq!(parsed.profile.user, "alice");
        assert_eq!(parsed.profile.password, "s3cret");
        assert_eq!(parsed.profile.id, 7, "the caller's id is kept");
        assert!(parsed.ignored.is_empty());
    }

    /// `postgres://` is what half the world pastes and `postgresql://` is what
    /// the other half does; `tokio-postgres` accepts both and so must this.
    #[test]
    fn both_postgresql_schemes_mean_the_same_engine() {
        assert_eq!(
            ok("postgres://alice@host/shop").profile,
            ok("postgresql://alice@host/shop").profile
        );
    }

    #[test]
    fn a_mysql_uri_parses_and_mariadb_is_the_same_engine() {
        let parsed = ok("mysql://root:pw@127.0.0.1:3307/app");
        assert_eq!(parsed.profile.engine, Engine::MySql);
        assert_eq!(parsed.profile.port, 3307);
        assert_eq!(parsed.profile.database, "app");
        assert_eq!(parsed.profile.user, "root");

        assert_eq!(
            ok("mariadb://root:pw@127.0.0.1:3307/app").profile,
            parsed.profile
        );
    }

    #[test]
    fn a_redis_uri_takes_its_logical_database_from_the_path() {
        let parsed = ok("redis://:pw@cache.internal:6380/3");
        assert_eq!(parsed.profile.engine, Engine::Redis);
        assert_eq!(parsed.profile.host, "cache.internal");
        assert_eq!(parsed.profile.port, 6380);
        assert_eq!(parsed.profile.database, "3");
        assert_eq!(parsed.profile.password, "pw");
        assert_eq!(
            parsed.profile.user, "",
            "an empty user segment leaves Redis's own default"
        );
        assert!(!parsed.tls_unsupported);
    }

    /// The `redis` crate's scheme match accepts these, so a Valkey user's URI
    /// is a Redis connection rather than an unsupported scheme.
    #[test]
    fn valkey_schemes_are_redis() {
        assert_eq!(ok("valkey://cache/1").profile.engine, Engine::Redis);
        assert_eq!(ok("valkeys://cache/1").profile.engine, Engine::Redis);
    }

    /// dodo's Redis client dials plaintext. The URI is still parsed — every
    /// other field is right — but the downgrade is reported rather than
    /// silent, and the intent is kept in `ssl_mode` for a client that can.
    #[test]
    fn a_rediss_uri_parses_and_says_the_tls_is_not_applied() {
        let parsed = ok("rediss://cache.internal:6380/2");
        assert_eq!(parsed.profile.host, "cache.internal");
        assert_eq!(parsed.profile.ssl_mode, SslMode::Require);
        assert!(parsed.tls_unsupported);
    }

    #[test]
    fn every_sqlite_form_names_the_same_file() {
        for input in [
            "sqlite:///tmp/app.db",
            "sqlite:/tmp/app.db",
            "sqlite3:///tmp/app.db",
            "file:///tmp/app.db",
        ] {
            let parsed = ok(input);
            assert_eq!(parsed.profile.engine, Engine::Sqlite, "{input}");
            assert_eq!(parsed.profile.file, "/tmp/app.db", "{input}");
            assert_eq!(parsed.profile.port, 0, "a file engine suggests no port");
        }
    }

    /// The relative reading, and the one that round-trips
    /// `ConnectionProfile::url` for a relative file.
    #[test]
    fn a_sqlite_uri_with_a_relative_path_keeps_it_relative() {
        assert_eq!(ok("sqlite://app.db").profile.file, "app.db");
        assert_eq!(ok("sqlite:app.db").profile.file, "app.db");
        assert_eq!(ok("sqlite::memory:").profile.file, ":memory:");
    }

    /// What `ConnectionProfile::url` writes must be what this reads back.
    #[test]
    fn dodos_own_url_round_trips() {
        use crate::database::models::connection::ConnectionProfile;

        let mut profile = ConnectionProfile::new(1, Engine::PostgreSql);
        profile.database = "shop".into();
        let parsed = ok(&profile.url());
        assert_eq!(parsed.profile.host, profile.host);
        assert_eq!(parsed.profile.port, profile.port);
        assert_eq!(parsed.profile.database, profile.database);
        assert_eq!(parsed.profile.user, profile.user);

        let mut file = ConnectionProfile::new(1, Engine::Sqlite);
        file.file = "/tmp/app.db".into();
        assert_eq!(ok(&file.url()).profile.file, "/tmp/app.db");
    }

    // ---- the parts that bite ---------------------------------------------

    /// A password with `@` or `/` in it is the normal case, not an edge case.
    #[test]
    fn percent_encoded_credentials_are_decoded() {
        let parsed = ok("postgresql://a%40corp:p%40ss%2Fw%3Ard%21@db:5432/shop");
        assert_eq!(parsed.profile.user, "a@corp");
        assert_eq!(parsed.profile.password, "p@ss/w:rd!");
        assert_eq!(parsed.profile.host, "db");
    }

    /// Unescaped, too: `rfind` on the `@` is what makes this work, and it is
    /// what a user who typed their password into a URI by hand will produce.
    #[test]
    fn an_unescaped_at_sign_in_a_password_still_finds_the_host() {
        let parsed = ok("postgresql://alice:p@ss@db.example.com:5432/shop");
        assert_eq!(parsed.profile.user, "alice");
        assert_eq!(parsed.profile.password, "p@ss");
        assert_eq!(parsed.profile.host, "db.example.com");
    }

    /// A `%` that is not an escape is text, not a failure.
    #[test]
    fn a_literal_percent_in_a_password_survives() {
        assert_eq!(
            ok("mysql://root:100%pure@db/app").profile.password,
            "100%pure"
        );
    }

    #[test]
    fn an_omitted_port_lands_on_the_engines_default_and_never_zero() {
        assert_eq!(ok("postgresql://alice@db/shop").profile.port, 5432);
        assert_eq!(ok("mysql://root@db/app").profile.port, 3306);
        assert_eq!(ok("redis://cache/0").profile.port, 6379);
    }

    /// `port=1234,,5678` is a shape libpq writes itself, so an empty port is
    /// an absent one rather than a zero.
    #[test]
    fn an_empty_port_is_an_absent_one() {
        assert_eq!(ok("postgresql://alice@db:/shop").profile.port, 5432);
    }

    #[test]
    fn an_omitted_database_leaves_the_engines_default() {
        assert_eq!(ok("postgresql://alice@db").profile.database, "");
        assert_eq!(ok("postgresql://alice@db/").profile.database, "");
        assert_eq!(
            ok("redis://cache").profile.database,
            "0",
            "Redis's default logical database, not a blank"
        );
    }

    #[test]
    fn an_omitted_user_leaves_the_engines_default() {
        assert_eq!(ok("postgresql://db/shop").profile.user, "postgres");
        assert_eq!(ok("mysql://db/app").profile.user, "root");
        assert_eq!(
            ok("postgresql://:pw@db/shop").profile.user,
            "postgres",
            "an empty user segment is not a request for a blank user"
        );
    }

    /// An absent password and an empty one are the same thing to every driver
    /// dodo has: each sends one only when it is non-empty.
    #[test]
    fn an_absent_and_an_empty_password_both_leave_the_field_blank() {
        assert_eq!(ok("postgresql://alice@db/shop").profile.password, "");
        assert_eq!(ok("postgresql://alice:@db/shop").profile.password, "");
    }

    #[test]
    fn an_ipv6_literal_keeps_its_address_and_its_port() {
        let parsed = ok("postgresql://alice@[2001:db8::1]:6543/shop");
        assert_eq!(parsed.profile.host, "2001:db8::1");
        assert_eq!(parsed.profile.port, 6543);

        let no_port = ok("redis://[::1]/2");
        assert_eq!(no_port.profile.host, "::1");
        assert_eq!(no_port.profile.port, 6379);
    }

    /// Unbracketed, an address with several colons is a host and nothing else
    /// — guessing a port out of it would silently truncate the address.
    #[test]
    fn an_unbracketed_ipv6_address_is_taken_whole() {
        let parsed = ok("redis://::1/2");
        assert_eq!(parsed.profile.host, "::1");
        assert_eq!(parsed.profile.port, 6379);
    }

    // ---- query parameters -------------------------------------------------

    #[test]
    fn postgresqls_sslmode_reaches_the_profile() {
        assert_eq!(
            ok("postgresql://a@db/shop?sslmode=disable")
                .profile
                .ssl_mode,
            SslMode::Disable
        );
        assert_eq!(
            ok("postgresql://a@db/shop?sslmode=require")
                .profile
                .ssl_mode,
            SslMode::Require
        );
        assert_eq!(
            ok("postgresql://a@db/shop?sslmode=verify-full")
                .profile
                .ssl_mode,
            SslMode::Require,
            "dodo verifies in every mode, so verify-full is require"
        );
        assert_eq!(
            ok("postgresql://a@db/shop?sslmode=allow").profile.ssl_mode,
            SslMode::Prefer
        );
    }

    /// MySQL's own clients spell it `ssl-mode` with upper-case values.
    #[test]
    fn mysqls_ssl_mode_reaches_the_profile() {
        assert_eq!(
            ok("mysql://root@db/app?ssl-mode=REQUIRED").profile.ssl_mode,
            SslMode::Require
        );
        assert_eq!(
            ok("mysql://root@db/app?ssl-mode=DISABLED").profile.ssl_mode,
            SslMode::Disable
        );
        assert_eq!(
            ok("mysql://root@db/app?ssl_mode=VERIFY_IDENTITY")
                .profile
                .ssl_mode,
            SslMode::Require
        );
    }

    /// `tokio-postgres` applies the query after the URL, so the query wins.
    #[test]
    fn a_query_parameter_overrides_the_same_value_in_the_authority() {
        let parsed = ok("postgresql://alice@db/shop?user=bob&password=pw&dbname=other&port=6000");
        assert_eq!(parsed.profile.user, "bob");
        assert_eq!(parsed.profile.password, "pw");
        assert_eq!(parsed.profile.database, "other");
        assert_eq!(parsed.profile.port, 6000);
        assert!(parsed.ignored.is_empty());
    }

    #[test]
    fn redis_accepts_its_database_as_a_query_parameter() {
        assert_eq!(ok("redis://cache?db=4").profile.database, "4");
    }

    /// The rule the module opens with: nothing is dropped in silence.
    #[test]
    fn a_parameter_dodo_does_not_model_is_named_rather_than_dropped() {
        let parsed =
            ok("postgresql://a@db/shop?application_name=psql&connect_timeout=10&sslmode=nonsense");
        assert_eq!(
            parsed.ignored,
            vec![
                "application_name".to_string(),
                "connect_timeout".to_string(),
                "sslmode".to_string()
            ]
        );
        assert_eq!(
            parsed.profile.ssl_mode,
            SslMode::Prefer,
            "an unreadable sslmode leaves the default rather than guessing"
        );
    }

    #[test]
    fn extra_hosts_extra_segments_and_a_fragment_are_named() {
        let parsed = ok("postgresql://a@host1:1234,host2,host3:5678/shop/extra#note");
        assert_eq!(parsed.profile.host, "host1");
        assert_eq!(parsed.profile.port, 1234);
        assert_eq!(parsed.profile.database, "shop");
        assert_eq!(
            parsed.ignored,
            vec![
                "#note".to_string(),
                "host2".to_string(),
                "host3:5678".to_string(),
                "/extra".to_string()
            ]
        );
    }

    #[test]
    fn a_sqlite_uri_names_its_parameters_rather_than_applying_them() {
        let parsed = ok("sqlite:///tmp/app.db?mode=ro&cache=shared");
        assert_eq!(parsed.profile.file, "/tmp/app.db");
        assert_eq!(
            parsed.ignored,
            vec!["mode".to_string(), "cache".to_string()]
        );
    }

    // ---- malformed --------------------------------------------------------

    #[test]
    fn nothing_pasted_is_its_own_message() {
        assert_eq!(parse("", 1), Err(UriError::Empty));
        assert_eq!(parse("   \n ", 1), Err(UriError::Empty));
    }

    #[test]
    fn a_string_with_no_scheme_says_so() {
        assert_eq!(parse("db.example.com/shop", 1), Err(UriError::NoScheme));
        assert_eq!(parse("/tmp/app.db", 1), Err(UriError::NoScheme));
        assert_eq!(parse("//db/shop", 1), Err(UriError::NoScheme));
    }

    /// Never a silent default to PostgreSQL, and the message carries the
    /// scheme, because "unsupported" without the word is useless.
    #[test]
    fn an_unsupported_scheme_is_refused_by_name() {
        assert_eq!(
            parse("mongodb://db/app", 1),
            Err(UriError::UnknownScheme("mongodb".into()))
        );
        assert_eq!(
            parse("http://db/app", 1),
            Err(UriError::UnknownScheme("http".into()))
        );
        // A missing scheme that happens to look like one still names itself.
        assert_eq!(
            parse("localhost:5432/shop", 1),
            Err(UriError::UnknownScheme("localhost".into()))
        );
    }

    #[test]
    fn a_port_that_is_not_a_port_is_refused_with_the_text_as_pasted() {
        assert_eq!(
            parse("postgresql://a@db:pgbouncer/shop", 1),
            Err(UriError::InvalidPort("pgbouncer".into()))
        );
        assert_eq!(
            parse("postgresql://a@db:99999/shop", 1),
            Err(UriError::InvalidPort("99999".into()))
        );
        assert_eq!(
            parse("postgresql://a@db:0/shop", 1),
            Err(UriError::InvalidPort("0".into()))
        );
        assert_eq!(
            parse("postgresql://a@db/shop?port=nope", 1),
            Err(UriError::InvalidPort("nope".into()))
        );
    }

    #[test]
    fn a_sqlite_uri_with_no_path_says_which_thing_is_missing() {
        assert_eq!(parse("sqlite://", 1), Err(UriError::MissingFile));
        assert_eq!(parse("sqlite:", 1), Err(UriError::MissingFile));
        assert_eq!(parse("sqlite:   ", 1), Err(UriError::MissingFile));
    }

    #[test]
    fn an_escape_that_is_not_utf8_is_refused() {
        assert_eq!(
            parse("postgresql://alice:%FF%FE@db/shop", 1),
            Err(UriError::InvalidEscape)
        );
    }

    #[test]
    fn every_failure_says_something_in_every_language() {
        for error in [
            UriError::Empty,
            UriError::NoScheme,
            UriError::UnknownScheme("mongodb".into()),
            UriError::InvalidPort("nope".into()),
            UriError::MissingFile,
            UriError::InvalidEscape,
        ] {
            for language in Language::ALL {
                let text = error.message().text(language).into_owned();
                assert!(
                    !text.trim().is_empty(),
                    "{:?} has no wording in {}",
                    error,
                    language.code()
                );
            }
        }
    }

    /// The pasted scheme and port reach the user, so they can see what dodo
    /// read rather than guess.
    #[test]
    fn a_failure_message_repeats_what_was_pasted() {
        for language in Language::ALL {
            assert!(
                UriError::UnknownScheme("mongodb".into())
                    .message()
                    .text(language)
                    .contains("mongodb")
            );
            assert!(
                UriError::InvalidPort("pgbouncer".into())
                    .message()
                    .text(language)
                    .contains("pgbouncer")
            );
        }
    }

    // ---- the derived name -------------------------------------------------

    #[test]
    fn the_suggested_name_is_the_most_identifying_part() {
        assert_eq!(
            ok("postgresql://alice@db.example.com/shop").suggested_name(),
            "shop"
        );
        assert_eq!(
            ok("postgresql://alice@db.example.com").suggested_name(),
            "db.example.com",
            "with no database, the host is what identifies it"
        );
        assert_eq!(
            ok("redis://cache.internal/3").suggested_name(),
            "cache.internal",
            "a Redis logical database is a digit, not a name"
        );
        assert_eq!(ok("sqlite:///var/data/app.db").suggested_name(), "app.db");
        assert_eq!(
            ok("sqlite:C:\\data\\app.db").suggested_name(),
            "app.db",
            "a Windows path is an ordinary thing to paste"
        );
    }

    /// Surrounding whitespace is what a copy out of a terminal carries.
    #[test]
    fn a_pasted_uri_is_trimmed() {
        assert_eq!(ok("  postgresql://alice@db/shop\n").profile.host, "db");
    }
}
