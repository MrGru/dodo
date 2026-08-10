//! Linux Cleaner implementations: the four generic categories (System Junk,
//! User Cache, Trash Bins, Large & Old Files) that have a meaningful, honest
//! equivalent on Linux. Every macOS-only category — Mail, Xcode, Homebrew,
//! Installed Apps/uninstall review, Orphaned Files, AI Apps, Universal
//! Binaries, Node Tooling Cache, Docker Cache, Language Files — has no
//! scanner here at all; `CleanerView::pending_result` already turns a
//! missing scanner into "planned but not implemented yet" rather than a
//! silent gap, so nothing here needs to say so again.
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
