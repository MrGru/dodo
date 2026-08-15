//! Which platform this build is for, and where dodo keeps the files it writes.
//!
//! Every *rule* is [`dodo_paths`]'; what is here is the seam that supplies the
//! one impure input those rules take. dodo's own `main.rs` has the same seam
//! and reads the platform out of the target triple `build.rs` embedded into
//! `build_info::VERSION_INFO.target`. A library crate is handed no such
//! variable — and a build script of this crate's own, re-deriving one string,
//! would be a real cost for no gain — so [`current`] names the same fact with
//! `cfg!`. The two spellings are one answer, and `main.rs`'s `paths` module
//! carries the test that keeps them one.
//!
//! Nothing below is a decision a test would want to pin, which is the point:
//! everywhere else in this crate a [`HostOs`] is a **parameter**
//! ([`CleanerCategory::hidden_for`](crate::core::category::CleanerCategory::hidden_for)
//! is the one that matters), so Windows' and Linux' answers are asserted from
//! whichever platform you happen to be on. This module is the single place the
//! compiled-for platform enters.

use std::path::PathBuf;

pub use dodo_paths::HostOs;
use dodo_paths::{Environment, resolve};

/// The platform this build is for.
pub fn current() -> HostOs {
    if cfg!(target_os = "macos") {
        HostOs::MacOs
    } else if cfg!(target_os = "windows") {
        HostOs::Windows
    } else {
        HostOs::Unix
    }
}

/// dodo's data directory on this machine, created by whichever store saves
/// first. [`services::ignore_store`](crate::services::ignore_store) is the
/// Cleaner's one user of it.
pub fn data_dir() -> PathBuf {
    resolve(current(), &Environment::from_env())
}
