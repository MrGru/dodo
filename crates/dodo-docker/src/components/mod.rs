//! Small, generic GPUI elements the Docker pages share.
//!
//! Each is deliberately container-agnostic so that round 2's Images, Volumes and
//! Networks pages reuse them unchanged: they take already-translated strings and
//! plain values, never a `Container`. Anything the widget library already
//! provides — buttons, checkboxes, inputs, tags — is used directly.
//!
//! - [`status_badge`] — a coloured lifecycle badge.
//! - [`search_bar`] — an icon + text input for instant filtering.
//! - [`toolbar`] — the pre-styled row a page's controls sit in.
//! - [`skeleton`] — the non-blocking loading placeholder.
//! - [`states`] — the empty and error+retry placeholders.

pub mod search_bar;
pub mod skeleton;
pub mod states;
pub mod status_badge;
pub mod toolbar;
