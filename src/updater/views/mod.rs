//! Rendering, and the tasks that feed it.
//!
//! - [`dialog`] — [`UpdateDialog`](dialog::UpdateDialog), the whole of the
//!   updater's user interface: one `window.open_dialog` whose body is an
//!   entity, opened either by the sidebar's **Check for updates** button
//!   ([`dialog::open`]) or by a background check that found something
//!   ([`dialog::open_with`]).
//!
//! One module, because the updater has one surface. The sidebar affordance that
//! opens it lives in `src/layout.rs` with the other sidebar chrome.

pub mod dialog;
