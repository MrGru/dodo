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
//! Later rounds add `image`, `volume` and `network` siblings here; the
//! `Container` row is the template they copy.

pub mod container;
pub mod port;
pub mod stats;
pub mod status;
pub mod time;
