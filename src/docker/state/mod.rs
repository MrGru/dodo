//! The Docker module's runtime state, plain data over the [`models`], split by
//! concern like `api_explorer::state`.
//!
//! - [`selection`] — the multi-select set the row checkboxes drive. The bulk
//!   toolbar actions that consume it arrive in round 2; the model ships now.
//! - [`containers`] — the Containers page store: the rows, the load status, the
//!   search query, and the selection over them. Kept free of GPUI so its
//!   sorting, filtering and single-row CPU update are unit tested directly.
//!
//! Round 2's Images/Volumes/Networks stores are siblings here, over the model
//! types added alongside `models::container`.

pub mod containers;
pub mod selection;
