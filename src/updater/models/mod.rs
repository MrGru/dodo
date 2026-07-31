//! Plain data for the updater: no GPUI, no `reqwest`, no filesystem, all of it
//! unit tested in-file.
//!
//! - [`manifest`] — the `update.json` document and the strict parse that
//!   refuses one this build cannot act on safely.
//! - [`platform`] — [`PlatformKey`](platform::PlatformKey), which `files` entry
//!   describes this binary, derived from the target triple `build.rs` embedded.
//! - [`version`] — SemVer parsing and precedence, the three channels, and the
//!   one pure function that decides whether a candidate is offered.
//! - [`sha256`] — an incremental SHA-256, so an archive is hashed as it streams
//!   and never held in memory.
//! - [`config`] — `updater.json`, dodo's first persisted *setting*.
//! - [`state`] — the [`UpdaterState`](state::UpdaterState) and
//!   [`UpdateEvent`](state::UpdateEvent) enums the machine moves between, plus
//!   the errors, all of which carry a [`Str`](crate::i18n::Str) rather than
//!   rendered English.
//! - [`install_target`] — what the running binary is on disk (a `.app` bundle
//!   or a loose executable) and the paths an install stages through.

pub mod config;
pub mod install_target;
pub mod manifest;
pub mod platform;
pub mod sha256;
pub mod state;
pub mod version;
