//! Read-only check for whether Xcode is currently running.
//!
//! `macos::scanners::xcode_junk` uses this to attach a warning to DerivedData
//! results rather than blocking the scan (the ticket's "Warn when Xcode is
//! running", not "refuse to scan"). Implemented with
//! `NSRunningApplication::runningApplicationsWithBundleIdentifier` rather than
//! shelling out to `/bin/ps` and parsing its output: it is already a typed,
//! read-only Cocoa call, and turning on objc2-app-kit's `NSRunningApplication`
//! feature costs nothing new in the dependency graph — see the comment above
//! `[target.'cfg(target_os = "macos")'.dependencies]` in `Cargo.toml`.
//!
//! `NSRunningApplication` is documented thread-safe for reading its
//! properties, and the class method used here takes no main-thread marker, so
//! this is safe to call from the background executor scans run on (see
//! `docs/cleaner/architecture.md`'s "Concurrency strategy"). The actual check
//! now lives in `macos::platform::running_apps::is_any_bundle_running`,
//! generalized for shared `cleaner::ai_apps`'s Ollama/LM Studio checks; this
//! function is kept as a named, single-bundle-id wrapper so `xcode_junk`'s
//! call site stays exactly as readable as before.

use crate::cleaner::macos::platform::running_apps::is_any_bundle_running;

/// Xcode's bundle identifier, stable since Xcode 4.
const XCODE_BUNDLE_ID: &str = "com.apple.dt.Xcode";

pub fn is_xcode_running() -> bool {
    is_any_bundle_running(&[XCODE_BUNDLE_ID])
}
