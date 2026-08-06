use std::sync::Arc;

use crate::cleaner::core::scanner::CleanerScanner;

#[cfg(target_os = "macos")]
pub fn default_scanners() -> Vec<Arc<dyn CleanerScanner>> {
    crate::cleaner::macos::default_scanners()
}

#[cfg(not(target_os = "macos"))]
pub fn default_scanners() -> Vec<Arc<dyn CleanerScanner>> {
    Vec::new()
}
