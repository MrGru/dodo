//! Which platform this build is for.
//!
//! Every *rule* is [`dodo_paths`]'; what is here is the seam that supplies the
//! one impure input those rules take. dodo's own `main.rs` has the same seam
//! and reads the platform out of the target triple `build.rs` embedded into
//! `build_info::VERSION_INFO.target`. A library crate is handed no such
//! variable — and a build script of this crate's own, re-deriving one string,
//! would be a real cost for no gain — so [`current`] names the same fact with
//! `cfg!`. The two spellings are one answer, and `main.rs`'s `paths` module
//! carries the test that keeps them one; this is the same seam
//! `dodo-cleaner` already has.
//!
//! Nothing below is a decision a test would want to pin, which is the point:
//! everywhere else in this crate a [`HostOs`] is a **parameter** —
//! [`models::runtime`](crate::models::runtime) decides which container
//! runtimes exist and how to start them purely from one, so Windows' and
//! Linux' answers are asserted from whichever platform you happen to be on.
//! This module is the single place the compiled-for platform enters, and
//! [`services::runtime`](crate::services::runtime) is its only caller.

pub use dodo_paths::HostOs;

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
