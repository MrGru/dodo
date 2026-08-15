//! Text Input Sources, by hand — the only unchecked FFI dodo contains.
//!
//! # Why it is written out rather than imported
//!
//! The same reason `dodo-ime-macos`'s `client.rs` hand-writes `IMKTextInput`:
//! these functions live in **Carbon's HIToolbox**, not in a framework objc2
//! generates a crate for. There is no `objc2-hi-toolbox` and there will not be
//! one. `TISRegisterInputSource` and its three siblings are C functions taking
//! CoreFoundation types, so everything *except* the five declarations below is
//! checked by `objc2-core-foundation`, which dodo already links.
//!
//! dodo's rule is that a checked signature beats a hand-written one
//! (`Cargo.toml`'s `windows-sys` comment). It cannot be honoured here, so the
//! next best thing is done instead: the declarations are confined to this file
//! behind a safe typed façade, nothing here decides anything — which identifier
//! to enable is
//! [`models::install`](crate::models::install)'s answer, tested
//! without a machine — and every pointer that crosses the boundary is either a
//! `CFRetained` this function owns or a borrow that outlives the call.
//!
//! # Four behaviours of this API that are not in its signatures
//!
//! All four are measured, and `docs/macos-input-method.md` §2 and §5 are the
//! authority:
//!
//! - **Two concurrent `TISCreateInputSourceList` calls abort the process.** Not
//!   an error return, not a wrong answer: `SIGABRT` from
//!   `islGetInputSourceListWithAdditions.cold.3` inside HIToolbox, with three
//!   threads in the crash report standing in `TISCreateInputSourceList`. Found
//!   while writing this round, by running this module's own tests in parallel —
//!   which is exactly what `cargo test` does. Calling it from a *non-main* thread
//!   is fine; calling it from two threads at once is not.
//!
//!   Hence [`LOCK`]: every function here takes it, so dodo can never make two of
//!   these calls at the same time. That is only half the answer, because AppKit
//!   calls TIS on the main thread whenever it feels like it and no lock of ours
//!   can serialise against that — which is why
//!   [`SystemOps`](super::installer::SystemOps) performs all four of these on the
//!   main queue. See its `on_main`.
//!
//! - **`TISRegisterInputSource` returns `0` for a source that does not appear.**
//!   Its return value cannot be believed; [`is_visible`] is the real answer, and
//!   the retry loop belongs to the caller.
//! - **`TISCreateInputSourceList(NULL, true)` returns sources that cannot be
//!   handed to `TISSelectInputSource`.** So [`enable`] looks the source up
//!   including uninstalled ones — it has to, since a source that is not enabled
//!   yet is exactly what it is about to enable — and [`select`] looks it up
//!   *excluding* them, which is the list whose members are selectable.
//! - **`TISSelectInputSource` can fail with `-50` for reasons that have nothing
//!   to do with the source.** On the machine this was written on it does so for
//!   Apple's own Vietnamese Telex too. This module reports the status and forms
//!   no opinion; the caller's is in
//!   [`InstallOutcome`](crate::models::install::InstallOutcome).

use std::path::Path;
use std::ptr::NonNull;
use std::sync::Mutex;

use objc2_core_foundation::{CFArray, CFDictionary, CFRetained, CFString, CFURL};

/// Held across every call in this module, because two concurrent
/// `TISCreateInputSourceList` calls abort the process — see the module docs for
/// the crash and how it was found.
///
/// It guards nothing of *ours*: there is no shared state here. It exists purely
/// so that HIToolbox is never re-entered concurrently by dodo, and it is a
/// `Mutex<()>` rather than anything cleverer because the calls it serialises are
/// rare (an install, and this module's tests) and short.
///
/// A poisoned lock is stepped over rather than propagated: there is no invariant
/// to have been broken, and refusing to look at the input-source database because
/// an unrelated test panicked would be worse than the panic.
static LOCK: Mutex<()> = Mutex::new(());

fn locked<T>(work: impl FnOnce() -> T) -> T {
    let _guard = LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    work()
}

/// Carbon's `OSStatus`. `0` is `noErr`; `-50` is `paramErr`.
pub type OSStatus = i32;

/// `paramErr`, which is what selecting an input source fails with on a session
/// where input-source switching does not work at all. Named because a bare `-50`
/// in a log is unreadable, and because §5's control experiment — Apple's own
/// input methods fail identically — is the reason dodo must not treat it as its
/// own defect.
// Used by this module's own test rather than by its code: nothing here branches
// on the status, deliberately — see the third bullet above. It is kept because a
// bare `-50` in a report is unreadable and this is where a reader looks it up.
#[allow(dead_code)]
pub const PARAM_ERR: OSStatus = -50;

/// An opaque `TISInputSourceRef`.
///
/// Declared here rather than as a type alias for `c_void` so that the two
/// pointer kinds crossing this boundary — a source and a property key — cannot be
/// swapped by accident.
#[repr(C)]
pub struct TISInputSource {
    _opaque: [u8; 0],
}

// SAFETY of this block is the whole point of the module: these are the five
// HIToolbox declarations, transcribed from `Carbon/HIToolbox/TextInputSources.h`.
// Each signature is checked against that header and against nothing else, which
// is why every caller below is a small safe wrapper rather than an inline call.
#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    /// `CFStringRef` key: the input source's unique identifier, which for a mode
    /// is the mode's id and not the bundle's.
    static kTISPropertyInputSourceID: Option<&'static CFString>;

    /// Every input source matching `properties`, or every one when it is null.
    ///
    /// `include_all_installed` is a `Boolean`, which is an `unsigned char`.
    /// Returns a `+1` reference, and null when nothing matches.
    fn TISCreateInputSourceList(
        properties: Option<&CFDictionary>,
        include_all_installed: u8,
    ) -> Option<NonNull<CFArray>>;

    /// Tells the system to look at the bundle at `location`.
    fn TISRegisterInputSource(location: &CFURL) -> OSStatus;

    fn TISEnableInputSource(source: &TISInputSource) -> OSStatus;

    fn TISSelectInputSource(source: &TISInputSource) -> OSStatus;
}

/// The sources whose `kTISPropertyInputSourceID` is `source_id`.
///
/// Returns the array rather than the source so the caller can hold it: the
/// sources inside are owned by the array, so a `TISInputSourceRef` outliving it
/// is a dangling pointer.
fn matching(source_id: &str, include_all_installed: bool) -> Option<CFRetained<CFArray>> {
    let key = unsafe { kTISPropertyInputSourceID }?;
    let value = CFString::from_str(source_id);
    let filter = CFDictionary::from_slices(&[key], &[&*value]);

    // SAFETY: the filter is a live CF dictionary of CF types for the duration of
    // the call, and the returned array is a Create-rule +1 reference this
    // function takes ownership of.
    let array = unsafe {
        TISCreateInputSourceList(Some(filter.as_opaque()), u8::from(include_all_installed))
    }?;
    Some(unsafe { CFRetained::from_raw(array) })
}

/// The first source in `array`, if there is one.
///
/// # Safety
///
/// The returned reference borrows `array`, which is what keeps the source alive.
fn first(array: &CFArray) -> Option<&TISInputSource> {
    if array.count() < 1 {
        return None;
    }
    // SAFETY: index 0 exists (checked above), and a `TISCreateInputSourceList`
    // array holds `TISInputSourceRef`s by definition.
    let source = unsafe { array.value_at_index(0) };
    NonNull::new(source.cast_mut())
        .map(|source| unsafe { source.cast::<TISInputSource>().as_ref() })
}

/// Whether the system can see an input source with this id at all.
///
/// The real answer to "did registration work", because
/// `TISRegisterInputSource`'s return value is not one — see the module docs.
/// `include_all_installed` is true, so a source that is present but not yet
/// enabled counts as visible, which is the state a fresh install is in.
pub fn is_visible(source_id: &str) -> bool {
    locked(|| matching(source_id, true).is_some_and(|array| array.count() > 0))
}

/// `TISRegisterInputSource` on a bundle directory.
///
/// `None` when the path cannot be expressed as a URL at all, which is a
/// programming error rather than a system refusal and is why it is not an
/// `OSStatus`.
pub fn register(bundle: &Path) -> Option<OSStatus> {
    // A directory URL, because an input method *is* a directory. CoreFoundation
    // distinguishes the two and a file URL for a bundle is not the same string.
    let url = CFURL::from_directory_path(bundle)?;
    // SAFETY: the URL is live for the duration of the call.
    Some(locked(|| unsafe { TISRegisterInputSource(&url) }))
}

/// `TISEnableInputSource`, on whichever source carries this id.
///
/// `None` means no source with that id exists, which is a different answer from
/// "the system refused" and the caller has to tell them apart: the first means
/// registration has not taken effect yet, the second that it has.
pub fn enable(source_id: &str) -> Option<OSStatus> {
    locked(|| {
        let array = matching(source_id, true)?;
        let source = first(&array)?;
        // SAFETY: `source` borrows `array`, which is alive here.
        Some(unsafe { TISEnableInputSource(source) })
    })
}

/// `TISSelectInputSource`, on whichever *enabled* source carries this id.
///
/// The lookup excludes uninstalled sources deliberately: §5 records that
/// `TISCreateInputSourceList(NULL, true)` returns sources that cannot be handed
/// to this function, so asking the selectable list is the only way to pass it
/// something it will accept. `None` therefore means "enabling did not take",
/// which is worth reporting differently from a refusal.
pub fn select(source_id: &str) -> Option<OSStatus> {
    locked(|| {
        let array = matching(source_id, false)?;
        let source = first(&array)?;
        // SAFETY: as `enable`.
        Some(unsafe { TISSelectInputSource(source) })
    })
}

#[cfg(test)]
mod tests {
    use super::{PARAM_ERR, is_visible, locked, matching};
    use crate::models::install::{parent_input_method, selectable_source};

    #[test]
    fn param_err_is_the_number_the_docs_record() {
        assert_eq!(PARAM_ERR, -50);
    }

    /// Reads the real Text Input Sources database, and touches nothing.
    ///
    /// The assertion is deliberately weak — this machine's database is not this
    /// project's to arrange — but the call itself is the test: a wrong signature
    /// or a wrong `Boolean` width crashes or returns garbage here rather than in
    /// front of a user. Apple's own input method is used as the probe because it
    /// is present on every macOS install.
    #[test]
    fn the_database_can_be_queried_for_a_source_apple_ships() {
        let apple_abc = "com.apple.keylayout.ABC";
        assert!(
            is_visible(apple_abc),
            "every macOS install has the ABC keyboard layout"
        );
    }

    /// A source id nothing can have. Proves the filter is applied rather than
    /// ignored — `TISCreateInputSourceList` with a filter it does not understand
    /// would otherwise hand back every source on the machine.
    #[test]
    fn a_source_that_cannot_exist_is_not_visible() {
        assert!(!is_visible("io.github.mrgru.dodo.inputmethod.NotAThing"));
    }

    /// The regression this module's `LOCK` exists for: several threads asking the
    /// database at once used to abort the process from inside HIToolbox. There is
    /// nothing to assert — the test *finishing* is the assertion.
    #[test]
    fn concurrent_queries_do_not_abort_the_process() {
        std::thread::scope(|scope| {
            for _ in 0..8 {
                scope.spawn(|| {
                    for _ in 0..4 {
                        is_visible("com.apple.keylayout.ABC");
                    }
                });
            }
        });
    }

    /// dodo's own identifiers, whether or not anything is installed. This asserts
    /// only that asking is safe and that the two ids are asked about separately —
    /// the machine's actual state is not fixed, so neither answer can be
    /// asserted.
    #[test]
    fn asking_about_dodos_own_source_is_safe_either_way() {
        let mode = locked(|| matching(selectable_source(), true).map(|array| array.count()));
        let parent = locked(|| matching(parent_input_method(), true).map(|array| array.count()));
        // Both are `Option<isize>`; the point is that neither call faulted.
        let _ = (mode, parent);
    }
}
