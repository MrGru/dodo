//! The Flow Canvas's copy of dodo's platform seam.
//!
//! A feature crate is not handed the target triple embedded by dodo's build
//! script, so it chooses with `cfg!`; `src/main.rs` asserts this resolves to the
//! same directory as the binary's own spelling.

use std::path::PathBuf;

use dodo_paths::{Environment, HostOs, resolve};

pub fn current() -> HostOs {
    if cfg!(target_os = "macos") {
        HostOs::MacOs
    } else if cfg!(target_os = "windows") {
        HostOs::Windows
    } else {
        HostOs::Unix
    }
}

pub fn data_dir() -> PathBuf {
    resolve(current(), &Environment::from_env())
}
