//! One row of the Volumes table: an engine volume reduced to the columns the
//! page renders and the fields the search box filters on.
//!
//! The service builds these from `bollard`'s `Volume`. Size is optional because
//! the Engine API only fills `UsageData.Size` when explicitly asked for it (and
//! many drivers never report it), so the column shows `N/A` rather than blocking
//! the page on a size scan. "Containers using" is derived from the container
//! mounts at render time, not stored here.

/// A volume as the table renders it.
#[derive(Clone, PartialEq, Debug)]
pub struct Volume {
    /// The volume name — the stable row key and what a container mount is matched
    /// against for the "containers using" count.
    pub name: String,
    pub driver: String,
    pub mountpoint: String,
    /// Size in bytes when the engine reported it, `None` when it did not (`N/A`).
    pub size: Option<i64>,
}

impl Volume {
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
    use super::Volume;

    fn volume(name: &str, driver: &str) -> Volume {
        Volume {
            name: name.into(),
            driver: driver.into(),
            mountpoint: "/var/lib/…".into(),
            size: None,
        }
    }

    #[test]
    fn search_is_case_insensitive_over_name_and_driver() {
        let volume = volume("pgdata", "local");
        assert!(volume.matches("PGDATA"));
        assert!(volume.matches("Local"));
        assert!(volume.matches(""));
        assert!(!volume.matches("nfs"));
    }
}
