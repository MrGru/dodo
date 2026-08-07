//! Read-only check for whether any of a list of candidate bundle identifiers
//! is currently running.
//!
//! Generalized from `macos::platform::xcode::is_xcode_running` (which now
//! delegates here) because `macos::scanners::ai_apps` needs the exact same
//! one-line `NSRunningApplication` check for two more bundle identifiers
//! (Ollama, LM Studio), and unlike most "should this be a shared
//! abstraction?" calls in this codebase, this one really is trivial: the
//! whole thing is a single Cocoa class method, already a typed, read-only,
//! thread-safe-for-reading call (see `xcode`'s doc comment for why it is
//! safe to call from the background executor scans run on). There is no
//! third, more general "is any process running" concept hiding behind this
//! — just the same one-liner, callable with more than one candidate id so a
//! caller unsure of an app's exact bundle identifier can hedge with several.

use objc2_app_kit::NSRunningApplication;
use objc2_foundation::NSString;

/// `true` if at least one of `bundle_ids` names a currently-running
/// application. Every candidate is checked; the first match short-circuits.
/// An empty slice always returns `false`.
pub fn is_any_bundle_running(bundle_ids: &[&str]) -> bool {
    bundle_ids.iter().any(|bundle_id| {
        let bundle_id = NSString::from_str(bundle_id);
        !NSRunningApplication::runningApplicationsWithBundleIdentifier(&bundle_id).is_empty()
    })
}
