//! Linux Cleaner implementations: four filesystem categories (System Junk,
//! User Cache, Trash Bins, Large & Old Files) plus the shared AI Apps, Node
//! Tooling Cache and Docker Cache scanners. Every other category has no scanner here
//! and is hidden by
//! `CleanerCategory::hidden_for`, so every listed row has a working
//! implementation. Language Files stays hidden because package-owned and
//! immutable localization assets are not a safe deletion unit. Orphaned Files
//! may return only with conservative, package-manager-aware detection.
//!
//! Unlike the Windows module, `cargo check`'s Linux row is one of the two
//! this Mac genuinely cannot cross-compile at all (`aws-lc-sys`'s C build
//! script needs a cross C toolchain — see "Two of the four `cargo check`
//! targets…" in the project's root doc). So this has not even been
//! *type-checked* against the real target, let alone run on one; every claim
//! below is unverified until a captain does both.

pub mod cleanup;
pub mod platform;
pub mod scanners;

use std::sync::Arc;

use crate::cleaner::core::scanner::CleanerScanner;

pub fn default_scanners() -> Vec<Arc<dyn CleanerScanner>> {
    scanners::default_scanners()
}
