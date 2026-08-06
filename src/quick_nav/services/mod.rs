//! Quick navigation's one outside-world seam: where `quick-nav.json` is read
//! and written.
//!
//! Behind a trait like every other store in dodo, so the settings can be
//! round-tripped in a test with no filesystem, and blocking by contract — the
//! caller runs it on the background executor, never on the UI thread.

pub mod config_store;
