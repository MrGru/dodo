//! Drives the real `DodoInputController` through the real Objective-C runtime,
//! against a mock `IMKTextInput` client that records what it was told.
//!
//! # What this covers that no unit test can
//!
//! [`keymap`], [`ops`], [`session`] and [`text`] are pure and heavily tested,
//! but every one of them is reached through a *string*: `define_class!` takes
//! selector names as literals, and a mistyped
//! `#[unsafe(method(inputText:key:modifiers:client:))]` compiles, registers a
//! method nobody calls, and produces an input method that installs correctly and
//! never types anything. The same is true of the hand-written `msg_send!`s in
//! `client.rs`, which are unchecked by construction because `IMKTextInput` lives
//! in Carbon and has no objc2 binding.
//!
//! So this binary asserts the things that are only true at runtime:
//!
//! - the class registers under the exact name `Info.plist` names;
//! - `initWithServer:delegate:client:` installs the ivars and returns non-nil;
//! - `inputText:key:modifiers:client:` is actually dispatched, and its `BOOL`
//!   return survives the Rust `bool` conversion;
//! - `setMarkedText:selectionRange:replacementRange:` and
//!   `insertText:replacementRange:` reach the client with the right arguments,
//!   in the right order;
//! - `commitComposition:`, `activateServer:` and `deactivateServer:` dispatch.
//!
//! # `harness = false`, and why
//!
//! `DodoInputController` is `MainThreadOnly`, and libtest runs every `#[test]`
//! on a spawned thread — where `MainThreadMarker::new()` correctly refuses. A
//! custom-harness binary owns `main`, so this runs on the genuine main thread
//! rather than asserting it is one.

//! # Why the file is split
//!
//! The body is `controller/imk.rs`, reached through `#[path]`, because a
//! `harness = false` target must have a `main` on **every** platform: a
//! crate-level `#![cfg(target_os = "macos")]` deletes it everywhere else and
//! `cargo check` on the Linux and Windows CI rows fails with *"`main` function
//! not found"*. Cargo does not build files under `tests/<name>/` as test
//! targets of their own, so the module is compiled only through here.

#[cfg(target_os = "macos")]
#[path = "controller/imk.rs"]
mod imk;

fn main() {
    #[cfg(target_os = "macos")]
    imk::run_all();

    #[cfg(not(target_os = "macos"))]
    println!("skipped: the InputMethodKit boundary test only runs on macOS");
}
