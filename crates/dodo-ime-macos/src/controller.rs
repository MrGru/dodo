//! The `IMKInputController` subclass macOS instantiates, defined in Rust.
//!
//! # The name in `Info.plist` is this file's `#[name = "…"]`
//!
//! `InputMethodServerControllerClass` is a *runtime class name*, looked up as a
//! string, and without `#[name = …]` objc2 would register this class as
//! something like `dodo_ime_macos::DodoInputController0.1.0`. The lookup would
//! fail, macOS would launch the bundle, find no controller and deliver no
//! keystrokes — with no error anywhere. `scripts/macos-input-method-bundle.sh`
//! writes the plist and [`bundle::CONTROLLER_CLASS`](crate::bundle::CONTROLLER_CLASS)
//! is the spelling both sides are checked against.
//!
//! # One controller per input session
//!
//! macOS creates one of these per text field, not one per process, which is why
//! [`Session`] is an ivar and not a global. Two fields compose independently and
//! neither can see the other's syllable.
//!
//! # `IMKServerInput` is informal
//!
//! objc2 models it as a category on `NSObject`, so `inputText:key:modifiers:client:`
//! and `commitComposition:` are declared here as **plain methods** rather than
//! as a protocol conformance. `IMKStateSetting` — `activateServer:`,
//! `deactivateServer:`, `recognizedEvents:` — is a formal protocol and *is*
//! declared with `unsafe impl`.
//!
//! # Nothing here decides anything
//!
//! Each method normalizes its arguments, hands them to [`Session`], and performs
//! the resulting [`ClientOp`](crate::ops::ClientOp)s. There is no Vietnamese in
//! this file, no range arithmetic, and no branch that can lose a key: the
//! `BOOL` returned is [`Response::handled`](crate::session::Response::handled),
//! which is false whenever nothing was performed.

use std::cell::RefCell;

use objc2::rc::{Allocated, Retained};
use objc2::runtime::AnyObject;
use objc2::{DefinedClass, MainThreadOnly, define_class, msg_send};
use objc2_foundation::{NSObject, NSObjectProtocol, NSString, NSUInteger};
use objc2_input_method_kit::{IMKInputController, IMKServer, IMKStateSetting};

use crate::client::Client;
use crate::session::{Response, Session};
use crate::{DEFAULT_CONFIG, keymap};

/// `NSEventMaskKeyDown`.
///
/// Key-down only, which is what makes the InputMethodKit's default mouse
/// handling apply: a click outside a live composition area produces a
/// `commitComposition:` for free. Widening this mask would silently take that
/// over — see `IMKStateSetting`'s own documentation of `recognizedEvents:`.
const EVENT_MASK_KEY_DOWN: NSUInteger = 1 << 10;

/// Per-session state. A `RefCell` because Objective-C hands out `&self` and the
/// engine needs `&mut`; there is no contention to lose, since InputMethodKit
/// runs every one of these on the main thread.
pub struct Ivars {
    session: RefCell<Session>,
}

impl Default for Ivars {
    fn default() -> Ivars {
        Ivars {
            session: RefCell::new(Session::new(DEFAULT_CONFIG)),
        }
    }
}

define_class!(
    #[unsafe(super(IMKInputController, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "DodoInputController"]
    #[ivars = Ivars]
    pub struct DodoInputController;

    /// `IMKServerInput` is an informal protocol, so these are plain methods.
    impl DodoInputController {
        #[unsafe(method(inputText:key:modifiers:client:))]
        fn input_text_key_modifiers_client(
            &self,
            string: Option<&NSString>,
            key_code: isize,
            flags: NSUInteger,
            sender: Option<&AnyObject>,
        ) -> bool {
            // A single tail expression on purpose: in a `bool`-returning objc2
            // method the conversion to `BOOL` happens here and nowhere else, so
            // an early `return false;` does not compile.
            self.on_key(string, key_code, flags, sender)
        }

        /// The client wants whatever is composed, now. Chrome sends this
        /// unprompted when focus moves between its chrome and its web content.
        #[unsafe(method(commitComposition:))]
        fn commit_composition(&self, sender: Option<&AnyObject>) {
            self.run(sender, Session::commit);
        }

        #[unsafe(method_id(initWithServer:delegate:client:))]
        fn init_with_server_delegate_client(
            this: Allocated<Self>,
            server: Option<&IMKServer>,
            delegate: Option<&AnyObject>,
            client: Option<&AnyObject>,
        ) -> Option<Retained<Self>> {
            let this = this.set_ivars(Ivars::default());
            unsafe {
                msg_send![super(this), initWithServer: server, delegate: delegate, client: client]
            }
        }
    }

    /// `IMKStateSetting` is a formal protocol.
    unsafe impl IMKStateSetting for DodoInputController {
        /// A new text field. Anything left over belongs to a document this
        /// session can no longer address, so it is dropped rather than typed
        /// somewhere new.
        #[unsafe(method(activateServer:))]
        fn activate_server(&self, sender: Option<&AnyObject>) {
            self.run(sender, Session::activate);
        }

        /// The session is over: commit, then forget. This is also how macOS
        /// reports a password field — `activateServer:`, `commitComposition:`,
        /// `deactivateServer:`, and no keystrokes in between.
        #[unsafe(method(deactivateServer:))]
        fn deactivate_server(&self, sender: Option<&AnyObject>) {
            self.run(sender, Session::deactivate);
        }

        #[unsafe(method(recognizedEvents:))]
        fn recognized_events(&self, _sender: Option<&AnyObject>) -> NSUInteger {
            EVENT_MASK_KEY_DOWN
        }
    }

    unsafe impl NSObjectProtocol for DodoInputController {}
);

impl DodoInputController {
    /// Ask the session what to do, then do it.
    ///
    /// Every boundary method funnels through here so that "borrow the session,
    /// call it, release the borrow, then touch the client" is written once. The
    /// borrow is dropped before the client is messaged because an Objective-C
    /// call can re-enter — a `setMarkedText:` that makes the client change focus
    /// would come back through `deactivateServer:` and panic on the second
    /// `borrow_mut`.
    fn run(
        &self,
        sender: Option<&AnyObject>,
        action: impl FnOnce(&mut Session) -> Response,
    ) -> bool {
        let response = match self.ivars().session.try_borrow_mut() {
            Ok(mut session) => action(&mut session),
            // Re-entered while the session was already borrowed. Doing nothing
            // and handing the key back is the only safe answer; panicking here
            // would take the user's application down with it.
            Err(_) => Response::unhandled(),
        };

        if let Some(client) = Client::new(sender) {
            for op in &response.ops {
                client.perform(op);
            }
        }
        response.handled
    }

    fn on_key(
        &self,
        string: Option<&NSString>,
        key_code: isize,
        flags: NSUInteger,
        sender: Option<&AnyObject>,
    ) -> bool {
        // A client that is not there cannot be typed into. Claiming the key
        // would swallow it.
        if sender.is_none() {
            return false;
        }
        let text = string.map(NSString::to_string).unwrap_or_default();
        // `key` is documented as a key code; anything that does not fit one is a
        // key this table would not have named anyway.
        let key_code = u16::try_from(key_code).unwrap_or(u16::MAX);
        let event = keymap::key_event(&text, key_code, flags as u64);
        self.run(sender, |session| session.key(&event))
    }
}

#[cfg(test)]
mod tests {
    use super::EVENT_MASK_KEY_DOWN;
    use crate::bundle::CONTROLLER_CLASS;

    /// `define_class!` takes a string literal, so the constant cannot be used
    /// inside it — which makes this the only thing keeping the two in step.
    #[test]
    fn the_defined_class_is_the_one_the_plist_will_name() {
        assert_eq!(CONTROLLER_CLASS, "DodoInputController");
    }

    #[test]
    fn the_event_mask_is_key_down_alone() {
        assert_eq!(EVENT_MASK_KEY_DOWN, 1 << 10);
    }
}
