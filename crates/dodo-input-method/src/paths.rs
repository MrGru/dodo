//! Which platform this build is for, and where dodo keeps the files it writes.
//!
//! Every *rule* is [`dodo_paths`]'; what is here is the seam that supplies the
//! one impure input those rules take. dodo's own `main.rs` has the same seam
//! and reads the platform out of the target triple `build.rs` embedded into
//! `build_info::VERSION_INFO.target`. A library crate is handed no such
//! variable — and a build script of this crate's own, re-deriving one string,
//! would be a real cost for no gain — so [`current`] names the same fact with
//! `cfg!`. The two spellings are one answer, and `main.rs`'s `paths` module
//! carries the test that keeps them one; this is the same seam `dodo-cleaner`,
//! `dodo-docker`, `dodo-database`, `dodo-api-explorer` and `dodo-updater`
//! already have.
//!
//! The file this guards is `input-method.json`
//! ([`services::store`](crate::services::store)). A disagreement with the
//! binary would make every engine setting and selected language appear lost on
//! the next launch.

use std::path::PathBuf;

use dodo_paths::resolve;
pub use dodo_paths::{Environment, HostOs};

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
