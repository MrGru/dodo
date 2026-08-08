//! Telling the input method that the settings file changed.
//!
//! One distributed notification, no payload, best effort. See
//! `dodo_ime_ipc::SETTINGS_CHANGED` for why the notification carries nothing and
//! why a lost one costs a setting nothing worse than arriving at the input
//! method's next launch.
//!
//! # Why this is posted and not required
//!
//! The file is the mechanism. The notification only makes a change *immediate*
//! instead of eventual, so every failure here — no distributed centre, no
//! listener, a bundle that is not running — is silent and harmless. That is why
//! [`settings_changed`] returns `()`.
//!
//! # Order matters
//!
//! **Post after the file is written**, never before. A notification that arrives
//! first makes the input method read the *previous* settings and report the
//! previous revision, and nothing would correct it until the next change.

/// Says "the settings file changed" to whoever is listening.
#[cfg(target_os = "macos")]
pub fn settings_changed() {
    use objc2_core_foundation::{CFNotificationCenter, CFString};
    use std::ptr::null;

    let Some(center) = CFNotificationCenter::distributed_center() else {
        return;
    };
    let name = CFString::from_str(dodo_ime_ipc::SETTINGS_CHANGED);

    // SAFETY: the name is a live `CFString` for the duration of the call, and
    // both the sender object and the user info are null — the documented "no
    // sender filter, no payload". `deliver_immediately` is true because the
    // listener is a background agent that may be considered suspended, which is
    // the case where a coalesced notification would simply never arrive.
    unsafe {
        center.post_notification(Some(&name), null(), None, true);
    }
}

/// Nothing to post to. Every other platform reaches this, and the settings file
/// is still written — there is simply no input-method process to tell.
#[cfg(not(target_os = "macos"))]
pub fn settings_changed() {}
