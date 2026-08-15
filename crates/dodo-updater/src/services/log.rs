//! The updater's one diagnostic channel.
//!
//! Every line the updater writes for a developer goes through here, and here is
//! two `eprintln!`s. That is the whole point: dodo has **no logging framework**
//! and adding one for this round was rejected deliberately — `tracing` plus a
//! subscriber is a real dependency tree on a binary whose size is measured and
//! recorded per round (`docs/build-optimization.md`), for output nobody
//! currently collects.
//!
//! What this buys instead is that adding one later is a **one-file change**: no
//! call site names `eprintln!`, so [`note`] and [`problem`] can become
//! `tracing::info!` / `tracing::warn!` without touching anything else.
//!
//! # Not user-facing
//!
//! Nothing here reaches the UI, so nothing here goes through
//! [`Str`](crate::i18n::Str) — these are developer messages, the same category
//! as the `eprintln!`s in `settings::init`. Anything a *user* reads is a `Str`
//! on an error or a state, and the dialog renders it.
//!
//! # What is deliberately not logged
//!
//! The manifest URL and the archive URL are, because they are public. Nothing
//! else about the machine is: not the install path, not the home directory, not
//! the user name. A desktop developer tool has no business narrating a user's
//! filesystem to stderr, and a path is exactly the sort of thing that gets
//! pasted into a bug report.

/// A step happened. Ordinary progress: a check started, an install finished.
pub fn note(message: &str) {
    eprintln!("dodo/updater: {message}");
}

/// Something went wrong, or was refused. Still stderr — this is not an alert,
/// and the user-visible half is the dialog's error state.
pub fn problem(message: &str) {
    eprintln!("dodo/updater: {message}");
}
