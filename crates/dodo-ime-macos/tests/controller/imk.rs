//! The macOS half of the boundary test. See `../controller.rs` for what it
//! covers and why it lives in a subdirectory (cargo does not build files
//! under `tests/<name>/` as their own test targets).

use std::cell::RefCell;

use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyClass, AnyObject, Bool};
use objc2::{AnyThread, ClassType, DefinedClass, MainThreadOnly, define_class, msg_send};
use objc2_foundation::{
    MainThreadMarker, NSAttributedString, NSObject, NSObjectProtocol, NSRange, NSString, NSUInteger,
};
use objc2_input_method_kit::IMKServer;

use dodo_ime_macos::bundle::CONTROLLER_CLASS;
use dodo_ime_macos::controller::DodoInputController;

/// What the mock client was asked to do, in order.
#[derive(Default)]
struct Recorder {
    calls: RefCell<Vec<String>>,
    /// The caret the mock reports for `selectedRange`, in UTF-16 units.
    caret: RefCell<NSUInteger>,
    /// The document the mock reports for `attributedSubstringFromRange:`.
    document: RefCell<String>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = AnyThread]
    #[name = "DodoMockTextInputClient"]
    #[ivars = Recorder]
    struct MockClient;

    impl MockClient {
        #[unsafe(method(setMarkedText:selectionRange:replacementRange:))]
        fn set_marked_text(
            &self,
            text: Option<&NSString>,
            selection: NSRange,
            _replacement: NSRange,
        ) {
            let text = text.map(NSString::to_string).unwrap_or_default();
            self.ivars().calls.borrow_mut().push(format!(
                "setMarkedText({text:?}, selection={{{},{}}})",
                selection.location, selection.length
            ));
        }

        #[unsafe(method(insertText:replacementRange:))]
        fn insert_text(&self, text: Option<&NSString>, replacement: NSRange) {
            let text = text.map(NSString::to_string).unwrap_or_default();
            // `NSNotFound` is `NSIntegerMax`; print it as a name so a wrong
            // sentinel is visible in the assertion rather than as a huge number.
            let range = if replacement.location == isize::MAX as NSUInteger {
                "atCaret".to_string()
            } else {
                format!("{{{},{}}}", replacement.location, replacement.length)
            };
            self.ivars()
                .calls
                .borrow_mut()
                .push(format!("insertText({text:?}, {range})"));
        }

        #[unsafe(method(selectedRange))]
        fn selected_range(&self) -> NSRange {
            NSRange::new(*self.ivars().caret.borrow(), 0)
        }

        #[unsafe(method_id(attributedSubstringFromRange:))]
        fn attributed_substring(&self, range: NSRange) -> Option<Retained<NSAttributedString>> {
            let document = self.ivars().document.borrow();
            let units: Vec<u16> = document.encode_utf16().collect();
            let start = range.location.min(units.len());
            let end = (range.location + range.length).min(units.len());
            let text = String::from_utf16_lossy(&units[start..end]);
            Some(NSAttributedString::from_nsstring(&NSString::from_str(&text)))
        }

        #[unsafe(method_id(bundleIdentifier))]
        fn bundle_identifier(&self) -> Option<Retained<NSString>> {
            Some(NSString::from_str("dev.dodo.tests.mock"))
        }

        #[unsafe(method_id(init))]
        fn init(this: Allocated<Self>) -> Option<Retained<Self>> {
            let this = this.set_ivars(Recorder::default());
            unsafe { msg_send![super(this), init] }
        }
    }

    unsafe impl NSObjectProtocol for MockClient {}
);

impl MockClient {
    fn new() -> Retained<Self> {
        unsafe { msg_send![Self::alloc(), init] }
    }

    fn take_calls(&self) -> Vec<String> {
        std::mem::take(&mut *self.ivars().calls.borrow_mut())
    }
}

/// One assertion, reported rather than panicked, so a failing run prints
/// everything that went wrong instead of only the first thing.
struct Checks {
    failures: Vec<String>,
}

impl Checks {
    fn eq<T: std::fmt::Debug + PartialEq>(&mut self, what: &str, got: T, want: T) {
        if got == want {
            println!("ok   {what}");
        } else {
            println!("FAIL {what}\n       got:  {got:?}\n       want: {want:?}");
            self.failures.push(what.to_string());
        }
    }

    fn that(&mut self, what: &str, condition: bool) {
        if condition {
            println!("ok   {what}");
        } else {
            println!("FAIL {what}");
            self.failures.push(what.to_string());
        }
    }
}

/// Send one key the way InputMethodKit does.
fn press(
    controller: &DodoInputController,
    client: &MockClient,
    text: &str,
    key_code: isize,
    flags: NSUInteger,
) -> bool {
    let text = NSString::from_str(text);
    let handled: Bool = unsafe {
        msg_send![
            controller,
            inputText: &*text,
            key: key_code,
            modifiers: flags,
            client: client,
        ]
    };
    handled.as_bool()
}

pub fn run_all() {
    let mtm = MainThreadMarker::new().expect("a custom-harness test owns the main thread");
    let mut checks = Checks {
        failures: Vec::new(),
    };

    // 1. Registration. objc2 registers a `define_class!` type lazily on first
    //    use, which is exactly what `src/main.rs` forces before `IMKServer`
    //    looks the name up — so this both performs and checks that step.
    let name = DodoInputController::class()
        .name()
        .to_string_lossy()
        .into_owned();
    checks.eq(
        "the runtime class name is the one Info.plist will ask for",
        name,
        CONTROLLER_CLASS.to_string(),
    );
    checks.that(
        "the runtime can find the class by that name",
        AnyClass::get(&std::ffi::CString::new(CONTROLLER_CLASS).unwrap()).is_some(),
    );

    // 2. It can be constructed the way IMKServer constructs it.
    //
    //    Two constraints of `IMKInputController`, both discovered by being
    //    raised at as `NSInvalidArgumentException` here rather than documented:
    //    a **nil server** aborts, so a real `IMKServer` is made (with this
    //    test's own connection name, so it cannot collide with the bundle's);
    //    and a client that is not a real IMK proxy is rejected outright —
    //    *"unexpected client proxy of class DodoMockTextInputClient"* — so the
    //    controller is constructed with a **nil client** and the mock is passed
    //    as the `sender:` of every message instead. That is not a workaround:
    //    `controller.rs` only ever reads `sender`, never `-[self client]`,
    //    because `sender` is the client for the session the message belongs to.
    let server: Option<Retained<IMKServer>> = unsafe {
        IMKServer::initWithName_bundleIdentifier(
            IMKServer::alloc(),
            Some(&NSString::from_str("dev_dodo_tests_controller_Connection")),
            Some(&NSString::from_str("dev.dodo.tests.controller")),
        )
    };
    checks.that("an IMKServer can be created for the test", server.is_some());

    let client = MockClient::new();
    let attempt = unsafe {
        objc2::exception::catch(std::panic::AssertUnwindSafe(|| {
            let controller: Option<Retained<DodoInputController>> = msg_send![
                DodoInputController::alloc(mtm),
                initWithServer: server.as_deref(),
                delegate: std::ptr::null::<AnyObject>(),
                client: std::ptr::null::<AnyObject>(),
            ];
            controller
        }))
    };
    let controller = match attempt {
        Ok(controller) => controller,
        Err(exception) => {
            println!("FAIL initWithServer: raised {exception:?}");
            std::process::exit(1);
        }
    };
    let Some(controller) = controller else {
        println!("FAIL initWithServer:delegate:client: returned nil; nothing else can run");
        std::process::exit(1);
    };
    println!("ok   initWithServer:delegate:client: returned an instance");

    run(&controller, &client, &mut checks);

    if checks.failures.is_empty() {
        println!("\nall Objective-C boundary checks passed");
    } else {
        println!("\n{} failed: {:?}", checks.failures.len(), checks.failures);
        std::process::exit(1);
    }
}

fn run(controller: &DodoInputController, client: &MockClient, checks: &mut Checks) {
    // A fresh field.
    unsafe {
        let _: () = msg_send![controller, activateServer: client];
    }
    checks.eq(
        "activateServer: asks the client for nothing",
        client.take_calls(),
        Vec::<String>::new(),
    );

    // `tieengs` composes `tiếng`, one setMarkedText: per keystroke.
    let keys: [(&str, isize); 7] = [
        ("t", 0x11),
        ("i", 0x22),
        ("e", 0x0E),
        ("e", 0x0E),
        ("n", 0x2D),
        ("g", 0x05),
        ("s", 0x01),
    ];
    let mut handled_every_key = true;
    for (text, key_code) in keys {
        handled_every_key &= press(controller, client, text, key_code, 0);
    }
    checks.that(
        "every letter of a Vietnamese syllable is claimed by the input method",
        handled_every_key,
    );
    checks.eq(
        "the composition is rewritten in place, ending as the toned syllable",
        client.take_calls(),
        vec![
            "setMarkedText(\"t\", selection={1,0})".to_string(),
            "setMarkedText(\"ti\", selection={2,0})".to_string(),
            "setMarkedText(\"tie\", selection={3,0})".to_string(),
            "setMarkedText(\"tiê\", selection={3,0})".to_string(),
            "setMarkedText(\"tiên\", selection={4,0})".to_string(),
            "setMarkedText(\"tiêng\", selection={5,0})".to_string(),
            "setMarkedText(\"tiếng\", selection={5,0})".to_string(),
        ],
    );

    // Space ends the syllable: clear the preedit, insert the text, and let the
    // application have the space.
    let handled = press(controller, client, " ", 0x31, 0);
    checks.that("the space itself reaches the application", !handled);
    checks.eq(
        "a commit clears the marked text before inserting, and inserts at the caret",
        client.take_calls(),
        vec![
            "setMarkedText(\"\", selection={0,0})".to_string(),
            "insertText(\"tiếng\", atCaret)".to_string(),
        ],
    );

    // A client-forced commit mid-syllable — the Chrome case.
    for (text, key_code) in [
        ("c", 0x08),
        ("h", 0x04),
        ("a", 0x00),
        ("o", 0x1F),
        ("f", 0x03),
    ] {
        press(controller, client, text, key_code, 0);
    }
    let _ = client.take_calls();
    unsafe {
        let _: () = msg_send![controller, commitComposition: client];
    }
    checks.eq(
        "commitComposition: flushes the syllable the client is showing",
        client.take_calls(),
        vec![
            "setMarkedText(\"\", selection={0,0})".to_string(),
            "insertText(\"chào\", atCaret)".to_string(),
        ],
    );

    // Deactivation with nothing pending must not insert a stale string — and in
    // fact touches the client not at all: with nothing to commit the engine
    // returns no actions, so there is not even an empty `setMarkedText:`.
    unsafe {
        let _: () = msg_send![controller, deactivateServer: client];
    }
    checks.eq(
        "deactivateServer: with nothing composed leaves the client alone",
        client.take_calls(),
        Vec::<String>::new(),
    );

    // Deactivation *with* something pending commits it rather than losing it.
    unsafe {
        let _: () = msg_send![controller, activateServer: client];
    }
    let _ = client.take_calls();
    for (text, key_code) in [
        ("v", 0x09),
        ("i", 0x22),
        ("e", 0x0E),
        ("e", 0x0E),
        ("t", 0x11),
    ] {
        press(controller, client, text, key_code, 0);
    }
    let _ = client.take_calls();
    unsafe {
        let _: () = msg_send![controller, deactivateServer: client];
    }
    checks.eq(
        "deactivateServer: commits a half-typed syllable rather than dropping it",
        client.take_calls(),
        vec![
            "setMarkedText(\"\", selection={0,0})".to_string(),
            "insertText(\"viêt\", atCaret)".to_string(),
        ],
    );

    // A command shortcut is not typing: nothing is composed and the key goes on.
    unsafe {
        let _: () = msg_send![controller, activateServer: client];
    }
    let _ = client.take_calls();
    let handled = press(controller, client, "s", 0x01, 1 << 20);
    checks.that("Cmd+S reaches the application", !handled);
    checks.eq(
        "Cmd+S composes nothing",
        client.take_calls(),
        Vec::<String>::new(),
    );

    // Backspace with no composition belongs to the application.
    let handled = press(controller, client, "\u{8}", 0x33, 0);
    checks.that(
        "Backspace with nothing composed reaches the application",
        !handled,
    );
    checks.eq(
        "Backspace with nothing composed leaves the client alone",
        client.take_calls(),
        Vec::<String>::new(),
    );

    // The client answers `bundleIdentifier`, which per-application memory will
    // key off in a later round.
    checks.eq(
        "recognizedEvents: is NSEventMaskKeyDown",
        unsafe {
            let mask: NSUInteger = msg_send![controller, recognizedEvents: client];
            mask
        },
        1 << 10,
    );
}
