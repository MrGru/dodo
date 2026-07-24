//! The Docker module's rendering.
//!
//! [`docker::DockerView`] is the entity `Layout` holds; it owns the four pages
//! and shows the selected one. [`containers::ContainersView`] is the round-1
//! Containers page; [`images::ImagesView`], [`volumes::VolumesView`] and
//! [`networks::NetworksView`] are round 3's list pages. [`widgets`] holds the
//! small render helpers those three share.
//!
//! [`detail::DetailPanel`] is round 5's read-only overlay — Inspect on all four
//! pages, Logs on Containers. Every page owns one and renders it over its table;
//! its module doc explains why it is an owned struct rather than an entity or a
//! dialog.

pub mod containers;
pub mod detail;
pub mod docker;
pub mod images;
pub mod networks;
pub mod volumes;
pub mod widgets;

pub use docker::{DockerPage, DockerView};
