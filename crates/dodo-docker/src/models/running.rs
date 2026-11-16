//! A running container as another tool asks for it: the one shape this crate
//! hands out across its boundary.
//!
//! The Database Explorer's "use local database" button needs the image, the
//! environment and the published ports of every running container. It cannot
//! name this crate — the two tools stay independent — so `main.rs` calls
//! [`crate::running_containers`] and converts this into that crate's own
//! plain type. Keeping it this small is what keeps that conversion trivial.

use crate::models::port::PortMapping;

/// One running container, with what the engine's list and inspect report.
#[derive(Clone, Debug, Default)]
pub struct RunningContainer {
    /// The name without the engine's leading `/`.
    pub name: String,
    /// The image reference the container was created from.
    pub image: String,
    /// The container's environment, as `KEY=value` entries. It can carry
    /// secrets; it is handed to the caller and never displayed here.
    pub env: Vec<String>,
    /// Every port mapping, published or not.
    pub ports: Vec<PortMapping>,
}
