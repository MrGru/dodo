//! Where dodo keeps the two files this crate writes.
//!
//! Every *rule* is [`dodo_paths`]'; what is here is the seam that supplies the
//! one impure input those rules take. dodo's own `main.rs` has the same seam
//! and reads the platform out of the target triple `build.rs` embedded into
//! `build_info::VERSION_INFO.target`. A library crate is handed no such
//! variable — and a build script of this crate's own, re-deriving one string,
//! would be a real cost for no gain — so [`current`] names the same fact with
//! `cfg!`. The two spellings are one answer, and `main.rs`'s `paths` module
//! carries the test that keeps them one; this is the same seam `dodo-cleaner`
//! and `dodo-docker` already have, and here it matters most — a `data_dir()`
//! that disagreed with the binary's would leave every saved connection and
//! every saved query behind on the next launch.
//!
//! [`services::connection_store`](crate::services::connection_store) and
//! [`services::query_store`](crate::services::query_store) are the two callers.

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
/// first.
pub fn data_dir() -> PathBuf {
    resolve(current(), &Environment::from_env())
}
