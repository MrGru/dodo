//! The Docker module's rendering.
//!
//! [`docker::DockerView`] is the entity `Layout` holds; it owns the four pages
//! and shows the selected one. [`containers::ContainersView`] is the round-1
//! Containers page; [`images::ImagesView`], [`volumes::VolumesView`] and
//! [`networks::NetworksView`] are round 3's list pages. [`widgets`] holds the
//! small render helpers those three share.

pub mod containers;
pub mod docker;
pub mod images;
pub mod networks;
pub mod volumes;
pub mod widgets;

pub use docker::{DockerPage, DockerView};
