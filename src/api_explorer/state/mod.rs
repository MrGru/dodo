//! The API Explorer's state, split by concern rather than gathered into one
//! struct.
//!
//! - [`request`] — what is being asked, per open tab.
//! - [`response`] — what came back, per open tab.
//! - [`tab`] — the pair of those two plus the in-flight task; one entity per
//!   open request, which is what makes tabs independent.
//! - [`collection`] — the Collections panel's runtime state, over the plain-data
//!   [`collection::CollectionState`] tree the model owns.
//! - [`history`] — the in-memory request history, fed from where an exchange
//!   completes: `tab::RequestTabState` emits a record and the page records it.
//! - [`ui`] — panel sizes, collapse flags, which left panel is shown, active tab.

pub mod collection;
pub mod history;
pub mod request;
pub mod response;
pub mod tab;
pub mod ui;
