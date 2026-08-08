//! The bundle's executable: register the controller class, create the
//! `IMKServer`, run the event loop.
//!
//! macOS launches this — `launchd` does, with `PPID 1` — the first time a user
//! selects the input source. It is never double-clicked, never started by
//! `Dodo.app`, and has no window.
//!
//! # The order is the whole file
//!
//! The controller class must be registered with the Objective-C runtime
//! **before** `IMKServer` looks its name up out of `Info.plist`. objc2 registers
//! a `define_class!` type lazily, on first use, so `DodoInputController::class()`
//! is not a debug line — it is the registration, and moving it below the server
//! creation produces a bundle that launches and never receives a keystroke.
//!
//! # Everything here is deliberately silent
//!
//! No log file, no `println!` of anything a user typed. The two `eprintln!`s
//! below fire only when the process cannot start at all, and say nothing about
//! any keystroke — there has not been one yet. See the privacy note on
//! [`dodo_ime_macos`].

#[cfg(target_os = "macos")]
fn main() {
    use objc2::{AnyThread, ClassType};
    use objc2_app_kit::NSApplication;
    use objc2_foundation::{MainThreadMarker, NSBundle, NSString};
    use objc2_input_method_kit::IMKServer;

    use dodo_ime_macos::controller::DodoInputController;

    let Some(mtm) = MainThreadMarker::new() else {
        eprintln!("DodoVietnamese: not on the main thread");
        std::process::exit(1);
    };

    // Registers the class under the name `Info.plist` will be searched for.
    // Must happen before IMKServer exists.
    let _ = DodoInputController::class();

    let bundle = NSBundle::mainBundle();
    let bundle_identifier = bundle.bundleIdentifier();
    // Read back rather than repeated: `scripts/macos-input-method-bundle.sh`
    // writes this key and `-[IMKServer initWithName:…]` must be handed the same
    // string. Two spellings of one name is a bug waiting for a rename.
    let connection: Option<objc2::rc::Retained<NSString>> = bundle
        .objectForInfoDictionaryKey(&NSString::from_str("InputMethodConnectionName"))
        .and_then(|value| value.downcast::<NSString>().ok());

    let server = unsafe {
        IMKServer::initWithName_bundleIdentifier(
            IMKServer::alloc(),
            connection.as_deref(),
            bundle_identifier.as_deref(),
        )
    };
    if server.is_none() {
        eprintln!("DodoVietnamese: IMKServer could not be created");
        std::process::exit(1);
    }
    // The server must outlive `main`; nothing else holds it.
    std::mem::forget(server);

    NSApplication::sharedApplication(mtm).run();
}

/// The crate is a workspace `default-member` so that `cargo test` and `cargo
/// clippy --all-targets` cover its pure half everywhere, which means this target
/// is built on Linux and Windows too. There is nothing for it to do there: an
/// InputMethodKit bundle is a macOS object and the Objective-C dependencies are
/// not even resolved off Apple platforms.
#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("DodoVietnamese is a macOS input method and does not run on this platform");
    std::process::exit(1);
}
