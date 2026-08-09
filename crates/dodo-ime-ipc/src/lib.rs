//! The contract between dodo and its platform-native Vietnamese hosts.
//!
//! Two processes that never link each other: dodo is a gpui application, the
//! input method is a faceless InputMethodKit agent macOS launches into every
//! text field on the machine. Both link this crate, and it is the only thing
//! they share besides the engine. It holds three kinds of agreement:
//!
//! - **The names macOS looks the bundle up by** — [`bundle`], and the Windows
//!   COM/profile identifiers — [`tsf`].
//! - **The two files they exchange** — [`settings`] and [`status`], under
//!   [`paths::support_dir`].
//! - **The notification that says a file changed** — [`SETTINGS_CHANGED`].
//!
//! # Why a third crate rather than a module in either process
//!
//! Neither process can reach the other's code. dodo does not link
//! `dodo-ime-macos` (it would drag InputMethodKit into a UI application for
//! four string constants), and the bundle must not link `dodo` (gpui in every
//! text field). `dodo-ime-core` cannot hold it either: its `purity_lint`
//! forbids serde by test, deliberately, so that the engine stays plain Rust for
//! the Windows and Linux hosts.
//!
//! So the alternative to this crate is *two copies of the schema*, kept in step
//! by nothing. A drifted field name here does not fail to compile and does not
//! error at runtime — it reads as absent, and the user's setting silently has
//! no effect. That is the same silent-failure class [`bundle`] documents for the
//! identifiers, which is why the identifiers moved in here with it.
//!
//! It is a workspace member for the reason `dodo-ime-core` is: dodo links it, so
//! a second `Cargo.lock` would resolve serde and the engine independently of
//! dodo's, and "the schema the tests prove" and "the schema the shipped bundle
//! reads" would be two resolutions nothing compares.
//!
//! # One writer per file, and no locking
//!
//! ```text
//!   Dodo.app  ──writes──▶  input-method.json         ──reads──▶  the bundle
//!   Dodo.app  ◀──reads──   input-method-status.json  ◀─writes──  the bundle
//! ```
//!
//! **Each file has exactly one writer**, which is the whole concurrency design:
//! there is no lock file, no advisory locking, no compare-and-swap, and no
//! moment at which two processes write the same path. A reader can only ever see
//! a complete file, because every write goes through
//! [`document::write_atomic`] — temp file beside the target, then `rename`,
//! which is atomic within a directory on APFS. A reader that arrives mid-write
//! sees the *previous* file, never a half of either.
//!
//! Two things follow, and both are deliberate:
//!
//! - **Neither side may edit the other's file.** dodo must not "fix up" a status
//!   file it dislikes, and the bundle must not write settings back. There is
//!   nothing enforcing that but this paragraph and the fact that neither module
//!   exposes a writer for the file it does not own.
//! - **A missing file is not an error.** It is the ordinary state before
//!   anything has been saved, and both readers answer it with defaults.
//!
//! # The version rule
//!
//! Both files carry an explicit `"version"` from their very first write, and
//! both parsers **refuse a version above the one this build knows** rather than
//! reading whatever fields happen to line up. This is `environments.json`'s
//! pattern, not `collections.json`'s, and it matters more here than anywhere
//! else in dodo: the two processes are *versioned independently*. A user can
//! update `Dodo.app` and leave a months-old bundle in `~/Library/Input Methods`,
//! or install the bundle from a newer dodo than the one that is running. Half
//! reading the other side's file would mean typing under settings nobody chose.
//!
//! [`document::parse_versioned`] is the one implementation of the rule; both
//! files' parsers go through it, and its tests are the version matrix (equal,
//! lower, higher, missing, malformed, truncated).
//!
//! # Privacy
//!
//! `dodo-ime-macos`'s crate docs state the rule this crate has to be read
//! against: **the bundle keeps no typing history, ever.** That did not change
//! here, but one sentence of it did — the bundle used to write no file at all,
//! and now it writes exactly one. [`status::StatusDocument`] is therefore
//! defined by what it may *not* carry, and a test pins its key set so that
//! adding a field is a decision someone makes on purpose.

pub mod bundle;
pub mod document;
pub mod paths;
pub mod settings;
pub mod status;
pub mod tsf;

pub use self::document::{IpcError, write_atomic};
pub use self::settings::{SETTINGS_SCHEMA_VERSION, SettingsDocument, VietnameseSettings};
pub use self::status::{STATUS_SCHEMA_VERSION, StatusDocument};

/// The distributed notification dodo posts after it has written
/// [`settings::SETTINGS_FILE`].
///
/// A `CFNotificationCenter` distributed notification, not a `Darwin` one, and
/// not `NSDistributedNotificationCenter` from Rust — the CF spelling is the one
/// both sides can use, because its observer callback is a plain `extern "C"`
/// function pointer rather than an Objective-C selector or a `block2` closure.
///
/// It carries **no payload**. The file is the payload; the notification only
/// says "read it again". That is what keeps the design robust against a missed
/// ping: the bundle also reads the file when it starts, so the worst a lost
/// notification costs is that a setting applies at the next launch instead of
/// immediately.
///
/// Distributed notification names are global to the login session, so the name
/// is reverse-DNS prefixed like everything else macOS looks up by string.
pub const SETTINGS_CHANGED: &str = "io.github.mrgru.dodo.inputmethod.Dodo.settings-changed";

#[cfg(test)]
mod tests {
    use super::SETTINGS_CHANGED;
    use crate::bundle::BUNDLE_IDENTIFIER;

    /// The notification shares the bundle's namespace, so a future rename moves
    /// both or neither.
    #[test]
    fn the_notification_name_is_in_the_bundles_namespace() {
        assert!(SETTINGS_CHANGED.starts_with(BUNDLE_IDENTIFIER));
    }
}
