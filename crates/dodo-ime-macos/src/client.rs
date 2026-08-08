//! `IMKTextInput`, by hand — the only unchecked FFI in this crate.
//!
//! # Why this is written out rather than imported
//!
//! `objc2-input-method-kit` binds `IMKInputController`, `IMKServer` and
//! `IMKCandidates`, and **not** `IMKTextInput`, which is the protocol every call
//! that actually puts text on screen belongs to. That is not an oversight in the
//! crate and will not be fixed by a version bump: `IMKTextInput` is not declared
//! in `InputMethodKit.framework` at all. It lives in Carbon, at
//! `HIToolbox.framework/Headers/IMKInputSession.h`, and objc2 generates one
//! crate per framework with no `objc2-hi-toolbox` among them. So every `sender:`
//! parameter arrives as `Option<&AnyObject>` and every message to it is a
//! hand-written `msg_send!`.
//!
//! dodo's rule is that a checked signature beats a hand-written one
//! (`Cargo.toml`'s `windows-sys` comment). It cannot be honoured here, so the
//! next best thing is done instead: the unchecked calls are confined to this
//! one file behind a safe typed façade, they compute nothing — every range and
//! every string arrives already decided by [`ops`](crate::ops) and
//! [`text`](crate::text), which are tested without a frame — and every optional
//! message is guarded by `respondsToSelector:` rather than assumed.
//!
//! # `NSNotFound` is `NSIntegerMax`
//!
//! Not `NSUIntegerMax`. The sentinel that means "at the insertion point, replace
//! nothing" is `{NSNotFound, NSNotFound}`, and the investigation found that
//! AppKit *happens to accept* the wrong one — which is the kind of accident that
//! works in the harness and corrupts text in somebody's editor.
//!
//! # What a client may refuse
//!
//! `selectedRange` and `attributedSubstringFromRange:` are optional, and three
//! of the six applications the investigation probed answered at least one of
//! them wrongly: Chrome's omnibox and VS Code's chrome return
//! `{NSNotFound, NSNotFound}`, and three clients return a `length` that is
//! simply false. So `length` is never asked for here, and a refusal to answer
//! `selectedRange` makes [`Client::replace_before`] do **nothing** rather than
//! guess a range — the composing path, which is the only one macOS ever takes,
//! does not need either.

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{msg_send, sel};
use objc2_foundation::{NSAttributedString, NSRange, NSString, NSUInteger};

use crate::ops::ClientOp;
use crate::text::{utf16_len, utf16_len_of_last_graphemes};

/// `NSNotFound`, which is `NSIntegerMax` and not `NSUIntegerMax`.
const NS_NOT_FOUND: NSUInteger = isize::MAX as NSUInteger;

/// The range meaning "at the insertion point, replacing nothing".
fn at_insertion_point() -> NSRange {
    NSRange::new(NS_NOT_FOUND, NS_NOT_FOUND)
}

/// How much text before the caret to read back when a replacement has to be
/// measured.
///
/// A Vietnamese syllable is at most seven letters, and the only thing ever
/// replaced is one syllable. Reading a bounded window rather than the document
/// keeps this off the "how long is the user's file" axis entirely — and the text
/// is dropped at the end of the call, never stored.
const REPLACEMENT_WINDOW: usize = 16;

/// The application's text input session, as much of it as is safe to use.
pub struct Client<'a>(&'a AnyObject);

impl<'a> Client<'a> {
    /// Wrap the `sender:` of an `IMKServerInput` method.
    ///
    /// `None` is a real case — the investigation's bare-binary harness produced
    /// it — and the caller must treat it as "there is nowhere to put text".
    pub fn new(sender: Option<&'a AnyObject>) -> Option<Client<'a>> {
        sender.map(Client)
    }

    fn responds_to(&self, selector: Sel) -> bool {
        unsafe { msg_send![self.0, respondsToSelector: selector] }
    }

    /// The bundle identifier of the application being typed into, when it has
    /// one.
    ///
    /// Not used for anything yet. It is the key per-application language memory
    /// will hang off, and it is documented here because two of its properties
    /// are surprising: it can be a *helper's* identifier rather than the host
    /// app's (System Settings produces `com.apple.systempreferences` and then
    /// `com.apple.Keyboard-Settings.extension`), and it is `None` for a process
    /// with no bundle at all.
    pub fn bundle_identifier(&self) -> Option<String> {
        if !self.responds_to(sel!(bundleIdentifier)) {
            return None;
        }
        let identifier: Option<Retained<NSString>> = unsafe { msg_send![self.0, bundleIdentifier] };
        identifier.map(|identifier| identifier.to_string())
    }

    fn set_marked_text(&self, text: &str, selection: NSRange) {
        let text = NSString::from_str(text);
        unsafe {
            let _: () = msg_send![
                self.0,
                setMarkedText: &*text,
                selectionRange: selection,
                replacementRange: at_insertion_point(),
            ];
        }
    }

    fn insert_text(&self, text: &str, replacement: NSRange) {
        let text = NSString::from_str(text);
        unsafe {
            let _: () = msg_send![self.0, insertText: &*text, replacementRange: replacement];
        }
    }

    /// Where the caret is, if the client will say.
    ///
    /// `None` covers both refusals: the client does not implement the message,
    /// and the client implements it and answers `NSNotFound`.
    fn selected_range(&self) -> Option<NSRange> {
        if !self.responds_to(sel!(selectedRange)) {
            return None;
        }
        let range: NSRange = unsafe { msg_send![self.0, selectedRange] };
        (range.location != NS_NOT_FOUND).then_some(range)
    }

    /// The document's text over `range`, if the client will say.
    fn substring(&self, range: NSRange) -> Option<String> {
        if !self.responds_to(sel!(attributedSubstringFromRange:)) {
            return None;
        }
        let attributed: Option<Retained<NSAttributedString>> =
            unsafe { msg_send![self.0, attributedSubstringFromRange: range] };
        let attributed = attributed?;
        Some(attributed.string().to_string())
    }

    /// Replace the `graphemes` graphemes before the caret with `text`.
    ///
    /// Three questions have to be answered before this can be a range, and any
    /// of them may fail: where the caret is, what the text before it says, and
    /// how many UTF-16 units those graphemes occupy. **A failure does nothing**
    /// — an approximate replacement deletes characters the user typed, and this
    /// path is only ever reached in the direct-typing output mode, which the
    /// macOS host never selects.
    fn replace_before(&self, graphemes: usize, text: &str) {
        if graphemes == 0 {
            if !text.is_empty() {
                self.insert_text(text, at_insertion_point());
            }
            return;
        }

        let Some(caret) = self.selected_range() else {
            return;
        };
        let window = REPLACEMENT_WINDOW.min(caret.location);
        let Some(before) = self.substring(NSRange::new(caret.location - window, window)) else {
            return;
        };
        let Some(units) = utf16_len_of_last_graphemes(&before, graphemes) else {
            return;
        };
        // `units` was measured inside `before`, which was read from the document
        // ending at the caret, so it cannot exceed `caret.location`. The check is
        // here because "cannot" is doing work a client could disprove.
        if units > caret.location {
            return;
        }
        debug_assert!(units <= utf16_len(&before));
        self.insert_text(text, NSRange::new(caret.location - units, units));
    }

    /// Perform one message. This is the whole of what this crate does to a
    /// document.
    pub fn perform(&self, op: &ClientOp) {
        match op {
            ClientOp::SetMarked { text, selection } => {
                self.set_marked_text(text, NSRange::new(selection.0, selection.1));
            }
            ClientOp::ClearMarked => self.set_marked_text("", NSRange::new(0, 0)),
            ClientOp::Insert(text) => self.insert_text(text, at_insertion_point()),
            ClientOp::ReplaceBefore { graphemes, text } => self.replace_before(*graphemes, text),
            // Nothing to send: the application gets the key because
            // `inputText:…` returns NO.
            ClientOp::PassThrough => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{NS_NOT_FOUND, at_insertion_point};
    use objc2_foundation::NSUInteger;

    /// The trap the investigation hit: the wrong sentinel is accepted by AppKit
    /// and is still wrong.
    #[test]
    fn not_found_is_the_signed_maximum() {
        assert_eq!(NS_NOT_FOUND, isize::MAX as NSUInteger);
        assert_ne!(NS_NOT_FOUND, NSUInteger::MAX);

        let range = at_insertion_point();
        assert_eq!(range.location, NS_NOT_FOUND);
        assert_eq!(range.length, NS_NOT_FOUND);
    }
}
