//! The Docker module's rendering.
//!
//! [`docker::DockerView`] is the entity `Layout` holds; it owns the four pages
//! and shows the selected one. [`containers::ContainersView`] is the round-1
//! Containers page. Images, Volumes and Networks are placeholder pages rendered
//! by `DockerView` itself until their real views land.

pub mod containers;
pub mod docker;

pub use docker::{DockerPage, DockerView};
