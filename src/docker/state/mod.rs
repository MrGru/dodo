//! The Docker module's runtime state, plain data over the [`models`], split by
//! concern like `api_explorer::state`.
//!
//! - [`selection`] — the multi-select set the row checkboxes drive, and the bulk
//!   toolbar that consumes it (round 2).
//! - [`filters`] — the multiple-simultaneous filter set the Filter popover drives
//!   and its per-row predicate (round 2).
//! - [`grouping`] — the pure partition of the list into Compose-project groups
//!   plus the Ungrouped bucket, with each group's rolled-up status (round 2).
//! - [`containers`] — the Containers page store: the rows, the load status, the
//!   search query, the filters, the group expansion set, and the selection over
//!   them. Kept free of GPUI so its sorting, filtering, grouping and single-row
//!   CPU update are unit tested directly.
//!
//! Round 3's Images/Volumes/Networks pages share one store — [`resource`]'s
//! generic [`ResourceState<T>`](resource::ResourceState) — because they need only
//! the container store's spine (load status, rows, search query, the derived
//! usage and an action-error banner), not its grouping/filter/selection
//! machinery. The [`LoadStatus`](containers::LoadStatus) is reused across both.

pub mod containers;
pub mod filters;
pub mod grouping;
pub mod resource;
pub mod selection;
