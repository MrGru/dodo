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
//! `docs/cleaner/architecture.md`'s "Concurrency strategy").

use objc2_app_kit::NSRunningApplication;
use objc2_foundation::NSString;

/// Xcode's bundle identifier, stable since Xcode 4.
const XCODE_BUNDLE_ID: &str = "com.apple.dt.Xcode";

pub fn is_xcode_running() -> bool {
    let bundle_id = NSString::from_str(XCODE_BUNDLE_ID);
    !NSRunningApplication::runningApplicationsWithBundleIdentifier(&bundle_id).is_empty()
}
