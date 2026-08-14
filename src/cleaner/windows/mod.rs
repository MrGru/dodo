//! Windows Cleaner implementations: Installed Apps, four filesystem categories
//! (System Junk, User Cache, Trash Bins, Large & Old Files), and the shared AI
//! Apps, Node Tooling Cache and Docker Cache scanners. Every other category has
//! no scanner here and is hidden by
//! `CleanerCategory::hidden_for`, so every listed row has a working
//! implementation. In particular, Language Files has no safe common
//! Windows deletion unit, and generic AppData leftovers are not trustworthy
//! evidence for Orphaned Files.
//!
//! Pure Windows inventory and policy are tested from every host, but the real
//! registry/MSIX/Explorer integrations still need captain testing on Windows.
//! A full Windows cross-check from this Mac remains blocked by `aws-lc-sys`'s
//! C build requiring Windows headers (see "Two of the four `cargo check`
//! targets…" in the project's root doc).

#[cfg(target_os = "windows")]
pub mod cleanup;
#[cfg(target_os = "windows")]
pub mod platform;
pub mod scanners;

#[cfg(target_os = "windows")]
use std::sync::Arc;

#[cfg(target_os = "windows")]
use crate::cleaner::core::scanner::CleanerScanner;

#[cfg(target_os = "windows")]
pub fn default_scanners() -> Vec<Arc<dyn CleanerScanner>> {
    scanners::default_scanners()
}
