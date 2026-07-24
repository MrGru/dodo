//! Plain data for the Docker module: no GPUI, no `bollard`, unit tested.
//!
//! Everything here is the shape the views and state stores work in, translated
//! out of the engine's wire types by [`services`](crate::docker::services) so
//! that nothing above the service layer ever names `bollard`. The split mirrors
//! `api_explorer::models`:
//!
//! - [`status`] — the container lifecycle state, its badge colour and which
//!   per-row actions it permits.
//! - [`port`] — a published port mapping and how it reads (`host → container`).
//! - [`time`] — parsing the engine's RFC 3339 timestamps and turning an instant
//!   into a human relative time ("2 minutes ago").
//! - [`stats`] — the CPU-percent computation from two stats samples.
//! - [`container`] — the [`Container`](container::Container) row itself, the
//!   compose-project extraction, and the search predicate.
//!
//! Round 3 adds the sibling rows the Images/Volumes/Networks pages render, all
//! copied from the `Container` template:
//!
//! - [`image`] — the [`Image`](image::Image) row, the repository/tag split and
//!   its `<none>` handling, and the short id.
//! - [`volume`] — the [`Volume`](volume::Volume) row, with its optional size.
//! - [`network`] — the [`Network`](network::Network) row and the predefined-name
//!   rule the Delete action keys off.
//! - [`size`] — the shared human-readable byte formatting.
//! - [`usage`] — [`ContainerUsage`](usage::ContainerUsage), the pure "containers
//!   using" derivation the three pages count against.

pub mod container;
pub mod image;
pub mod network;
pub mod port;
pub mod size;
pub mod stats;
pub mod status;
pub mod time;
pub mod usage;
pub mod volume;
