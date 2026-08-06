//! The one place that knows `session.json` is a file.
//!
//! Same seam as every other store in dodo: a trait the state layer holds behind
//! an `Arc`, so nothing above it learns a path, and the disk implementation
//! beside it.

pub mod session_store;
