//! macOS-specific Cleaner implementations.
//!
//! Real filesystem scanners, Full Disk Access checks, Finder/Trash integrations
//! and app-bundle analysis belong under this module. The phase-1 UI uses shared
//! mock scanners only, with no destructive operations.

pub mod applications;
pub mod cleanup;
pub mod permissions;
pub mod platform;
pub mod scanners;

use std::sync::Arc;

use crate::core::scanner::CleanerScanner;

pub fn default_scanners() -> Vec<Arc<dyn CleanerScanner>> {
    scanners::default_scanners()
}
