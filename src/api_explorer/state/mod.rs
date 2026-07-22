//! The API Explorer's state, split by concern rather than gathered into one
//! struct.
//!
//! - [`request`] — what is being asked, per open tab.
//! - [`response`] — what came back, per open tab.
//! - [`tab`] — the pair of those two plus the in-flight task; one entity per
//!   open request, which is what makes tabs independent.
//! - [`collection`] — the Collections panel's data (phase 3 fills it).
//! - [`ui`] — panel sizes, collapse flags, active tab.
//!
//! History is deliberately absent in phase 1: with no UI reading it, every
//! field would be dead code. Its seam is a `history` module beside these, fed
//! from the one place a request completes — `tab::RequestTabState::receive`.

pub mod collection;
pub mod request;
pub mod response;
pub mod tab;
pub mod ui;
