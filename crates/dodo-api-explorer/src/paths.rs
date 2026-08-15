//! Where dodo keeps the three files this crate writes.
//!
//! Every *rule* is [`dodo_paths`]'; what is here is the seam that supplies the
//! one impure input those rules take. dodo's own `main.rs` has the same seam
//! and reads the platform out of the target triple `build.rs` embedded into
//! `build_info::VERSION_INFO.target`. A library crate is handed no such
//! variable — and a build script of this crate's own, re-deriving one string,
//! would be a real cost for no gain — so [`current`] names the same fact with
//! `cfg!`. The two spellings are one answer, and `main.rs`'s `paths` module
//! carries the test that keeps them one; this is the same seam `dodo-cleaner`,
//! `dodo-docker` and `dodo-database` already have, and the consequence of a
//! disagreement is the same one `dodo-database` states: a `data_dir()` that did
//! not match the binary's would leave every saved collection, every environment
//! and every approved script behind on the next launch.
//!
//! [`services::collection_store`](crate::services::collection_store),
//! [`services::variable_store`](crate::services::variable_store) and
//! [`services::consent_store`](crate::services::consent_store) are the three
//! writers; `views::explorer` reads it once, to show the user where they live.

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
