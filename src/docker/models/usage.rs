//! "Containers using" derivation for the Images, Volumes and Networks pages.
//!
//! Each of those columns is a count of the running container set that references
//! a resource, and the round-3 brief says to derive it *from the container
//! list* rather than trust the engine's own per-resource counters (Podman often
//! reports `-1`, "not calculated"). The service reduces every container summary
//! to one [`ContainerUsageEntry`] — the resolved image id it runs, the named
//! volumes it mounts, the networks it is attached to — and the pages count
//! against that. Pure data, no GPUI and no `bollard`, so the counting is unit
//! tested directly.

/// One container's references, as the three usage columns need them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ContainerUsageEntry {
    /// The resolved image id the container runs (`ContainerSummary.image_id`),
    /// which matches an [`Image`](crate::docker::models::image::Image)'s id
    /// exactly — unlike the human image reference, which may be a tag or a
    /// digest.
    pub image_id: String,
    /// The names of the named volumes the container mounts. Bind mounts have no
    /// name and so never appear here.
    pub volume_names: Vec<String>,
    /// The names of the networks the container is attached to.
    pub network_names: Vec<String>,
}

/// The whole container set reduced to what the usage columns count against.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ContainerUsage {
    entries: Vec<ContainerUsageEntry>,
}

impl ContainerUsage {
    pub fn new(entries: Vec<ContainerUsageEntry>) -> Self {
        Self { entries }
    }

    /// How many containers run the image with this id. An empty id (an image the
    /// engine could not identify) matches nothing rather than every container
    /// that also lacks one.
    pub fn images_using(&self, image_id: &str) -> usize {
        if image_id.is_empty() {
            return 0;
        }
        self.entries
            .iter()
            .filter(|entry| entry.image_id == image_id)
            .count()
    }

    /// How many containers mount the named volume.
    pub fn volumes_using(&self, name: &str) -> usize {
        self.entries
            .iter()
            .filter(|entry| entry.volume_names.iter().any(|volume| volume == name))
            .count()
    }

    /// How many containers are attached to the network.
    pub fn networks_using(&self, name: &str) -> usize {
        self.entries
            .iter()
            .filter(|entry| entry.network_names.iter().any(|network| network == name))
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::{ContainerUsage, ContainerUsageEntry};

    fn entry(image: &str, volumes: &[&str], networks: &[&str]) -> ContainerUsageEntry {
        ContainerUsageEntry {
            image_id: image.into(),
            volume_names: volumes.iter().map(|s| s.to_string()).collect(),
            network_names: networks.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn sample() -> ContainerUsage {
        ContainerUsage::new(vec![
            entry("sha256:web", &["data"], &["app", "bridge"]),
            entry("sha256:web", &[], &["app"]),
            entry("sha256:db", &["data", "pgdata"], &["app"]),
        ])
    }

    #[test]
    fn images_using_counts_containers_by_resolved_id() {
        let usage = sample();
        assert_eq!(usage.images_using("sha256:web"), 2);
        assert_eq!(usage.images_using("sha256:db"), 1);
        assert_eq!(usage.images_using("sha256:absent"), 0);
    }

    #[test]
    fn an_empty_image_id_matches_nothing() {
        let usage = ContainerUsage::new(vec![entry("", &[], &[])]);
        assert_eq!(usage.images_using(""), 0);
    }

    #[test]
    fn volumes_using_counts_each_mounting_container_once() {
        let usage = sample();
        assert_eq!(usage.volumes_using("data"), 2);
        assert_eq!(usage.volumes_using("pgdata"), 1);
        assert_eq!(usage.volumes_using("unused"), 0);
    }

    #[test]
    fn networks_using_counts_attached_containers() {
        let usage = sample();
        assert_eq!(usage.networks_using("app"), 3);
        assert_eq!(usage.networks_using("bridge"), 1);
        assert_eq!(usage.networks_using("none"), 0);
    }
}
