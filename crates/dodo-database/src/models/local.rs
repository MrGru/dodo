//! Recognising a database server running in a local container, and turning it
//! into a connection profile.
//!
//! # Why this crate never asks Docker itself
//!
//! The design report's "detect running database containers" prefill was cut
//! in every earlier round for one reason: it would put a compile-time edge
//! between two tools. It is still kept off. This crate states what it needs
//! as plain data — [`LocalContainer`] — and the binary hands over a function
//! that produces it (see [`crate::observe_local_containers`]); `main.rs` is
//! the only file that names both crates. So
//! `grep -rn '^use crate::' crates/dodo-database/src/` still finds no other
//! tool, and the Database Explorer still builds with Docker absent.
//!
//! # What counts as a local SQL database
//!
//! A *running* container whose image is a PostgreSQL, MySQL or MariaDB
//! server, with that server's port published on the host. Redis is
//! deliberately not offered: the button is "use my local SQL database".
//! An unpublished port is skipped rather than guessed at — on macOS and
//! Windows the container's own address lives inside a VM and is not
//! reachable from dodo at all, so a profile pointing there would only fail.
//!
//! The image is matched by its *repository name*, the last path segment
//! before any tag or digest, so `docker.io/library/postgres:16-alpine`,
//! `postgis/postgis` and `bitnami/mysql` are all recognised.
//!
//! # Credentials
//!
//! Read from the container's environment, by each official image's own
//! documented variables (`POSTGRES_*`, `MYSQL_*`, `MARIADB_*`). A variable the
//! image did not set falls back to that image's documented default — the
//! `postgres` superuser, or MySQL's `root`. When an image sets both an
//! application user and a root password, the application user wins: that is
//! the account the container was configured for.
//!
//! When several containers qualify, the first one in the order the engine
//! listed them is used.

use crate::models::connection::ConnectionProfile;
use crate::models::engine::Engine;

/// One container as the binary reports it. Only what recognising a database
/// needs: no ids, no state — the source lists running containers only.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LocalContainer {
    /// The container's name, shown on the button and in the notice.
    pub name: String,
    /// The image reference the container runs, as the engine reports it.
    pub image: String,
    /// The container's environment, as `KEY=value` entries.
    pub env: Vec<String>,
    /// Every port published on the host.
    pub ports: Vec<PublishedPort>,
}

/// A container port published on the host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PublishedPort {
    pub container: u16,
    pub host: u16,
}

/// A container recognised as a database, with the profile that reaches it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalDatabase {
    pub container: String,
    pub profile: ConnectionProfile,
}

/// The first container in `containers` that is a reachable SQL database, as a
/// profile carrying `id`. `None` when there is none.
pub fn first_database(containers: &[LocalContainer], id: u64) -> Option<LocalDatabase> {
    containers
        .iter()
        .find_map(|container| recognise(container, id))
}

fn recognise(container: &LocalContainer, id: u64) -> Option<LocalDatabase> {
    let engine = engine_for_image(&container.image).or_else(|| engine_for_env(&container.env))?;
    let server_port = engine.default_port()?;
    let host_port = container
        .ports
        .iter()
        .find(|port| port.container == server_port)?
        .host;

    let env = |key: &str| lookup(&container.env, key);
    let mut profile = ConnectionProfile::new(id, engine);
    profile.port = host_port;
    profile.name = container.name.clone();

    match engine {
        Engine::PostgreSql => {
            let user = env("POSTGRES_USER").unwrap_or_else(|| "postgres".into());
            // The official image creates a database named after the user when
            // `POSTGRES_DB` is not set.
            profile.database = env("POSTGRES_DB").unwrap_or_else(|| user.clone());
            profile.password = env("POSTGRES_PASSWORD").unwrap_or_default();
            profile.user = user;
        }
        Engine::MySql | Engine::MariaDb => {
            // MariaDB's image reads `MARIADB_*` first and accepts `MYSQL_*` as
            // the older spelling; MySQL's reads `MYSQL_*` only.
            let var = |suffix: &str| {
                let mariadb = (engine == Engine::MariaDb)
                    .then(|| env(&format!("MARIADB_{suffix}")))
                    .flatten();
                mariadb.or_else(|| env(&format!("MYSQL_{suffix}")))
            };
            match (var("USER"), var("PASSWORD")) {
                (Some(user), Some(password)) => {
                    profile.user = user;
                    profile.password = password;
                }
                _ => {
                    profile.user = "root".into();
                    profile.password = var("ROOT_PASSWORD").unwrap_or_default();
                }
            }
            profile.database = var("DATABASE").unwrap_or_default();
        }
        Engine::Sqlite | Engine::Redis => return None,
    }

    Some(LocalDatabase {
        container: container.name.clone(),
        profile,
    })
}

/// The SQL engine an image reference runs, judged by its repository name.
fn engine_for_image(image: &str) -> Option<Engine> {
    // Drop a digest, then a tag — a tag's colon is after the last slash, a
    // registry port's is not — then keep the last path segment.
    let image = image.split('@').next().unwrap_or(image);
    let last = image.rsplit('/').next().unwrap_or(image);
    let repository = last.split(':').next().unwrap_or(last).to_ascii_lowercase();

    match repository.as_str() {
        "postgres" | "postgresql" | "postgis" | "pgvector" | "timescaledb" | "timescaledb-ha" => {
            Some(Engine::PostgreSql)
        }
        "mysql" | "mysql-server" | "percona-server" => Some(Engine::MySql),
        "mariadb" => Some(Engine::MariaDb),
        _ => None,
    }
}

/// The SQL engine an environment belongs to, for an image whose name says
/// nothing. An image built `FROM` an official one inherits that image's own
/// `ENV` lines — `PG_MAJOR`, `MYSQL_MAJOR`, `MARIADB_VERSION` — and a user
/// configuring it sets the `POSTGRES_*` / `MYSQL_*` / `MARIADB_*` variables.
/// MariaDB is asked first: its image also accepts the `MYSQL_*` spelling.
fn engine_for_env(env: &[String]) -> Option<Engine> {
    let has = |prefix: &str| {
        env.iter().any(|entry| {
            entry
                .split('=')
                .next()
                .is_some_and(|name| name.starts_with(prefix))
        })
    };
    if has("MARIADB_") {
        Some(Engine::MariaDb)
    } else if has("POSTGRES_") || has("PG_MAJOR") || has("PG_VERSION") {
        Some(Engine::PostgreSql)
    } else if has("MYSQL_") {
        Some(Engine::MySql)
    } else {
        None
    }
}

/// The value of `key` in a `KEY=value` environment, when it is set and not
/// empty.
fn lookup(env: &[String], key: &str) -> Option<String> {
    env.iter().find_map(|entry| {
        let (name, value) = entry.split_once('=')?;
        (name == key && !value.is_empty()).then(|| value.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::{LocalContainer, PublishedPort, engine_for_image, first_database};
    use crate::models::engine::Engine;

    fn container(name: &str, image: &str, env: &[&str], ports: &[(u16, u16)]) -> LocalContainer {
        LocalContainer {
            name: name.into(),
            image: image.into(),
            env: env.iter().map(|entry| entry.to_string()).collect(),
            ports: ports
                .iter()
                .map(|&(container, host)| PublishedPort { container, host })
                .collect(),
        }
    }

    #[test]
    fn recognises_images_by_repository_name() {
        assert_eq!(
            engine_for_image("docker.io/library/postgres:16-alpine"),
            Some(Engine::PostgreSql)
        );
        assert_eq!(
            engine_for_image("postgis/postgis"),
            Some(Engine::PostgreSql)
        );
        assert_eq!(
            engine_for_image("localhost:5000/mysql:8@sha256:abc"),
            Some(Engine::MySql)
        );
        assert_eq!(engine_for_image("bitnami/mariadb"), Some(Engine::MariaDb));
        assert_eq!(engine_for_image("redis:7"), None);
        assert_eq!(engine_for_image("nginx"), None);
    }

    #[test]
    fn recognises_a_derived_image_by_its_inherited_environment() {
        // `pgvector/pgvector` is built `FROM postgres` and is matched by name;
        // an image with an unrelated name is matched by the `ENV` it inherited.
        assert_eq!(
            engine_for_image("docker.io/pgvector/pgvector:pg17"),
            Some(Engine::PostgreSql)
        );
        let found = first_database(
            &[container(
                "custom",
                "acme/warehouse-db:3",
                &["PG_MAJOR=17", "POSTGRES_PASSWORD=pw"],
                &[(5432, 5432)],
            )],
            1,
        )
        .unwrap();
        assert_eq!(found.profile.engine, Engine::PostgreSql);
        assert_eq!(found.profile.password, "pw");
        assert!(
            first_database(
                &[container(
                    "web",
                    "acme/web",
                    &["PATH=/bin"],
                    &[(5432, 5432)]
                )],
                1
            )
            .is_none()
        );
    }

    #[test]
    fn postgres_reads_the_official_image_variables() {
        let found = first_database(
            &[container(
                "pg",
                "postgres:16",
                &[
                    "POSTGRES_USER=app",
                    "POSTGRES_PASSWORD=secret",
                    "POSTGRES_DB=shop",
                ],
                &[(5432, 15432)],
            )],
            7,
        )
        .unwrap();
        assert_eq!(found.container, "pg");
        let profile = found.profile;
        assert_eq!(profile.id, 7);
        assert_eq!(profile.engine, Engine::PostgreSql);
        assert_eq!(profile.host, "127.0.0.1");
        assert_eq!(profile.port, 15432);
        assert_eq!(profile.user, "app");
        assert_eq!(profile.password, "secret");
        assert_eq!(profile.database, "shop");
        assert_eq!(profile.name, "pg");
    }

    #[test]
    fn postgres_defaults_follow_the_image() {
        let profile = first_database(
            &[container(
                "pg",
                "postgres",
                &["POSTGRES_PASSWORD=pw"],
                &[(5432, 5432)],
            )],
            1,
        )
        .unwrap()
        .profile;
        assert_eq!(profile.user, "postgres");
        assert_eq!(profile.database, "postgres");
    }

    #[test]
    fn mysql_prefers_the_application_user_over_root() {
        let profile = first_database(
            &[container(
                "db",
                "mysql:8",
                &[
                    "MYSQL_ROOT_PASSWORD=root-pw",
                    "MYSQL_USER=app",
                    "MYSQL_PASSWORD=app-pw",
                    "MYSQL_DATABASE=shop",
                ],
                &[(3306, 3307)],
            )],
            1,
        )
        .unwrap()
        .profile;
        assert_eq!(profile.engine, Engine::MySql);
        assert_eq!(profile.port, 3307);
        assert_eq!(
            (profile.user.as_str(), profile.password.as_str()),
            ("app", "app-pw")
        );
        assert_eq!(profile.database, "shop");
    }

    #[test]
    fn mysql_falls_back_to_root() {
        let profile = first_database(
            &[container(
                "db",
                "mysql",
                &["MYSQL_ROOT_PASSWORD=root-pw"],
                &[(3306, 3306)],
            )],
            1,
        )
        .unwrap()
        .profile;
        assert_eq!(
            (profile.user.as_str(), profile.password.as_str()),
            ("root", "root-pw")
        );
    }

    #[test]
    fn mariadb_reads_its_own_spelling_before_mysqls() {
        let profile = first_database(
            &[container(
                "maria",
                "mariadb:11",
                &["MARIADB_ROOT_PASSWORD=new", "MYSQL_ROOT_PASSWORD=old"],
                &[(3306, 3306)],
            )],
            1,
        )
        .unwrap()
        .profile;
        assert_eq!(profile.engine, Engine::MariaDb);
        assert_eq!(profile.password, "new");
    }

    #[test]
    fn skips_a_database_whose_port_is_not_published() {
        assert!(first_database(&[container("pg", "postgres", &[], &[])], 1).is_none());
        assert!(first_database(&[container("pg", "postgres", &[], &[(8080, 8080)])], 1).is_none());
    }

    #[test]
    fn takes_the_first_qualifying_container_in_order() {
        let found = first_database(
            &[
                container("cache", "redis", &[], &[(6379, 6379)]),
                container("web", "nginx", &[], &[(80, 8080)]),
                container("first", "mysql", &[], &[(3306, 3306)]),
                container("second", "postgres", &[], &[(5432, 5432)]),
            ],
            1,
        )
        .unwrap();
        assert_eq!(found.container, "first");
    }

    #[test]
    fn finds_nothing_in_an_empty_list() {
        assert!(first_database(&[], 1).is_none());
    }
}
