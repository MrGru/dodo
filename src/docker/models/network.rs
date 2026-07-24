//! One row of the Networks table: an engine network reduced to the columns the
//! page renders and the fields the search box filters on.
//!
//! The service builds these from `bollard`'s `Network`. [`Network::is_predefined`]
//! is the pure rule the Delete action keys off: the `bridge`, `host` and `none`
//! networks are created by the engine and cannot be removed, so their Delete is
//! disabled rather than sent and refused. "Containers (attached)" is derived from
//! the container network attachments at render time, not stored here.

/// The engine's own networks, which exist for the lifetime of the daemon and
/// cannot be removed.
const PREDEFINED: [&str; 3] = ["bridge", "host", "none"];

/// A network as the table renders it.
#[derive(Clone, PartialEq, Debug)]
pub struct Network {
    /// The network id — the stable row key and what a Delete targets.
    pub id: String,
    /// The network name, matched against a container's attachments for the
    /// "containers" count and checked against [`PREDEFINED`].
    pub name: String,
    pub driver: String,
    pub scope: String,
    /// Creation time as Unix seconds, or `None` when the engine did not report a
    /// parseable timestamp.
    pub created: Option<i64>,
}

impl Network {
    /// Whether this is one of the engine's built-in networks, which the Engine
    /// API refuses to remove. The Delete action disables itself on these.
    pub fn is_predefined(&self) -> bool {
        PREDEFINED.contains(&self.name.as_str())
    }

    /// Whether this row matches a search query, case-insensitively over the name
    /// and the driver. An empty query matches everything.
    pub fn matches(&self, query: &str) -> bool {
        let query = query.trim().to_lowercase();
        if query.is_empty() {
            return true;
        }
        self.name.to_lowercase().contains(&query) || self.driver.to_lowercase().contains(&query)
    }
}

#[cfg(test)]
mod tests {
    use super::Network;

    fn network(name: &str, driver: &str) -> Network {
        Network {
            id: format!("id-{name}"),
            name: name.into(),
            driver: driver.into(),
            scope: "local".into(),
            created: None,
        }
    }

    #[test]
    fn the_engine_networks_are_predefined() {
        assert!(network("bridge", "bridge").is_predefined());
        assert!(network("host", "host").is_predefined());
        assert!(network("none", "null").is_predefined());
    }

    #[test]
    fn a_user_network_is_not_predefined() {
        assert!(!network("app_default", "bridge").is_predefined());
    }

    #[test]
    fn search_is_case_insensitive_over_name_and_driver() {
        let network = network("app_default", "bridge");
        assert!(network.matches("APP"));
        assert!(network.matches("Bridge"));
        assert!(network.matches(""));
        assert!(!network.matches("overlay"));
    }
}
