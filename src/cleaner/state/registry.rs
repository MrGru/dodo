use std::sync::Arc;

use crate::cleaner::core::scanner::CleanerScanner;

#[cfg(target_os = "macos")]
pub fn default_scanners() -> Vec<Arc<dyn CleanerScanner>> {
    crate::cleaner::macos::default_scanners()
}

#[cfg(target_os = "windows")]
pub fn default_scanners() -> Vec<Arc<dyn CleanerScanner>> {
    crate::cleaner::windows::default_scanners()
}

#[cfg(target_os = "linux")]
pub fn default_scanners() -> Vec<Arc<dyn CleanerScanner>> {
    crate::cleaner::linux::default_scanners()
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
pub fn default_scanners() -> Vec<Arc<dyn CleanerScanner>> {
    Vec::new()
}
