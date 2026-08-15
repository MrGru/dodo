//! Linux Cleaner implementations: four filesystem categories (System Junk,
//! User Cache, Trash Bins, Large & Old Files), Installed Apps, plus the shared
//! AI Apps, Node Tooling Cache and Docker Cache scanners. Installed Apps uses
//! visible desktop entries with dpkg metadata on Debian/Ubuntu, RPM metadata
//! on Fedora-family systems, or pacman metadata on Arch-family systems, plus
//! separately-scoped Flatpak inventory, Snap and bounded AppImages. Native
//! packages, system Flatpaks and Snaps are scan-only; only user Flatpaks and
//! bounded AppImages have actions. Language Files stays hidden because
//! package-owned and immutable localization assets are not a safe deletion
//! unit. Orphaned Files may return only with conservative,
//! package-manager-aware detection.
//!
//! Unlike the Windows module, `cargo check`'s Linux row is one of the two
//! this Mac genuinely cannot cross-compile at all (`aws-lc-sys`'s C build
//! script needs a cross C toolchain — see "Two of the four `cargo check`
//! targets…" in the project's root doc). So this has not even been
//! *type-checked* against the real target, let alone run on one; every claim
//! below is unverified until a captain does both.

#[cfg(target_os = "linux")]
pub mod cleanup;
#[cfg(target_os = "linux")]
pub mod platform;
pub mod scanners;

#[cfg(target_os = "linux")]
use std::sync::Arc;

#[cfg(target_os = "linux")]
use crate::core::scanner::CleanerScanner;

#[cfg(target_os = "linux")]
pub fn default_scanners() -> Vec<Arc<dyn CleanerScanner>> {
    scanners::default_scanners()
}
