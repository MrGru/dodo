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

/// Delivers a native host's explicit language-switch command to dodo.
///
/// The receiver parks when idle; it is a notification callback, never a file
/// poll. Dodo remains the sole `input-method.json` writer.
#[cfg(target_os = "macos")]
pub fn language_changes() -> futures_channel::mpsc::UnboundedReceiver<()> {
    observer::install()
}

#[cfg(target_os = "windows")]
pub fn language_changes() -> futures_channel::mpsc::UnboundedReceiver<()> {
    use futures_channel::mpsc::unbounded;
    use windows_sys::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
    use windows_sys::Win32::System::Threading::{CreateEventW, INFINITE, WaitForSingleObject};

    let (sender, receiver) = unbounded();
    std::thread::spawn(move || {
        let name = dodo_ime_ipc::WINDOWS_LANGUAGE_CHANGED
            .encode_utf16()
            .chain(Some(0))
            .collect::<Vec<_>>();
        // SAFETY: the terminated name lives through CreateEventW; the unnamed
        // security attributes request Windows' current-user defaults.
        let event = unsafe { CreateEventW(std::ptr::null(), 0, 0, name.as_ptr()) };
        if event.is_null() {
            return;
        }
        while unsafe { WaitForSingleObject(event, INFINITE) } == WAIT_OBJECT_0 {
            if sender.unbounded_send(()).is_err() {
                break;
            }
        }
        // SAFETY: this thread owns the event handle it created.
        unsafe { CloseHandle(event) };
    });
    receiver
}

#[cfg(target_os = "macos")]
mod observer {
    use std::ffi::c_void;
    use std::ptr::null;
    use std::sync::OnceLock;

    use futures_channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
    use objc2_core_foundation::{
        CFDictionary, CFNotificationCenter, CFNotificationName, CFNotificationSuspensionBehavior,
        CFString,
    };

    static SENDER: OnceLock<UnboundedSender<()>> = OnceLock::new();

    unsafe extern "C-unwind" fn language_changed(
        _center: *mut CFNotificationCenter,
        _observer: *mut c_void,
        _name: *const CFNotificationName,
        _object: *const c_void,
        _user_info: *const CFDictionary,
    ) {
        if let Some(sender) = SENDER.get() {
            let _ = sender.unbounded_send(());
        }
    }

    pub fn install() -> UnboundedReceiver<()> {
        let (sender, receiver) = unbounded();
        if SENDER.set(sender).is_err() {
            return receiver;
        }
        let Some(center) = CFNotificationCenter::distributed_center() else {
            return receiver;
        };
        let name = CFString::from_str(dodo_ime_ipc::LANGUAGE_CHANGED);
        // SAFETY: the callback reads no pointers; the global sender remains live
        // until dodo exits, and a distributed notification carries no payload.
        unsafe {
            center.add_observer(
                null(),
                Some(language_changed),
                Some(&name),
                null(),
                CFNotificationSuspensionBehavior::DeliverImmediately,
            );
        }
        receiver
    }
}
