//! The macOS `CGEventTap` host for the existing Vietnamese engine.
//!
//! It is deliberately direct-typing only: a global event tap has no marked-text
//! client, so it rewrites the short current syllable with tagged synthetic
//! events. Tagged events pass this callback unchanged, which prevents feedback
//! loops. No event text is logged, persisted, transmitted, or exposed.
//!
//! # It owns the language switch while it runs
//!
//! The tap owns the shortcut, so it stays attached in every language and can
//! always switch back to Vietnamese. It
//! therefore stays attached in *every* language, watches for the shortcut
//! first, and only then asks whether the key should reach the Vietnamese
//! engine. `models::live_switch` is the whole of that rule; this file adds the
//! two CoreGraphics facts it needs — `FlagsChanged` is in the mask so a
//! modifier-only shortcut is observable, and a matched modifier transition is
//! still returned unchanged so no application is left believing a key is held.
//!
//! # Browsers, and the one thing this host asks the outside world
//!
//! A browser address bar keeps an inline autocomplete selection alive between
//! keystrokes, so a plain Backspace rewrite lands on the wrong text there.
//! `models::browser_rewrite` is the whole of that rule — which browser needs
//! which of the two strategies, the count arithmetic, and every guard — and it
//! is pure. This file supplies the two platform facts it cannot: the frontmost
//! application's bundle identifier, cached by an
//! `NSWorkspaceDidActivateApplicationNotification` observer so that **nothing
//! asks `NSWorkspace` anything on the keystroke path**, and the extra synthetic
//! events themselves. All of them are posted through the one queue every other
//! synthetic event uses, in staging order, so "before the Backspaces" is a
//! property of the descriptor list rather than of two racing post APIs.

use std::cell::{Cell, RefCell};
use std::ptr::{NonNull, null_mut};
use std::rc::Rc;

use block2::RcBlock;
use dodo_ime_core::{Key, KeyEvent, LanguageId, Modifiers};
use futures_channel::mpsc::UnboundedSender;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObjectProtocol, ProtocolObject};
use objc2_app_kit::{
    NSBeep, NSRunningApplication, NSWorkspace, NSWorkspaceApplicationKey,
    NSWorkspaceDidActivateApplicationNotification,
};
use objc2_core_foundation::{
    CFBoolean, CFDictionary, CFMachPort, CFRetained, CFRunLoop, CFRunLoopMode, CFRunLoopSource,
    CFString, kCFRunLoopCommonModes,
};
use objc2_core_graphics::{
    CGEvent, CGEventField, CGEventFlags, CGEventTapLocation, CGEventTapOptions,
    CGEventTapPlacement, CGEventTapProxy, CGEventType,
};
use objc2_foundation::{NSNotification, NSNotificationCenter};

use crate::models::browser_rewrite::BrowserRewrite;
use crate::models::direct_output::OutputPlan;
use crate::models::event_tap::{
    DirectComposer, EventTapStatus, Handling, TapEvent, handling, invalidates_composer,
    is_synthetic_event, synthetic_event_tag,
};
use crate::models::live_switch::LiveSwitch;
use crate::models::settings::SettingsDocument;

const DELETE_KEY_CODE: u16 = 0x33;
/// `kVK_LeftArrow`, the other half of the Chromium-family workaround.
const LEFT_ARROW_KEY_CODE: u16 = 0x7b;
const MAX_INPUT_UNICODE_UNITS: usize = 8;
const MAX_REPLACEMENT_UNICODE_UNITS: usize = 64;

/// What one synthetic event types, if anything.
///
/// Unicode belongs to key-down only: the matching key-up is a
/// [`SyntheticPayload::Key`] with key code zero, so it ends that key without
/// asking a client to insert the scalar a second time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SyntheticPayload {
    /// A plain key press: `key_code` under `flags`.
    Key,
    /// The invisible character that makes a browser commit and dismiss its
    /// inline suggestion. Its text is
    /// `models::browser_rewrite::SELECTION_COMMIT_CHARACTER` and this host never
    /// names it.
    CommitCharacter,
    /// The engine's replacement text for the current syllable.
    Replacement,
}

/// One synthetic keyboard event, fully described before CoreGraphics allocates
/// it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SyntheticEventDescriptor {
    key_code: u16,
    down: bool,
    payload: SyntheticPayload,
    /// Modifiers this event is posted with. Empty for everything except the
    /// Chromium-family `Shift`+`Left`.
    flags: CGEventFlags,
    tag: i64,
}

/// `Shift`+`Left`, spelled the way macOS spells its own arrow keys.
///
/// `NumericPad` is not decoration: CoreGraphics sets it on every real arrow
/// key, and an arrow event without it is one an application may treat
/// differently from the one the user could have pressed.
const SHIFT_LEFT_FLAGS: CGEventFlags =
    CGEventFlags(CGEventFlags::MaskShift.0 | CGEventFlags::MaskNumericPad.0);

/// Room for the invisible commit character in UTF-16, with slack for a
/// replacement chosen during real-browser testing.
const MAX_COMMIT_UNICODE_UNITS: usize = 4;

/// Why a requested tap cannot start. No platform detail carries user input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartError {
    AccessibilityDenied,
    NoRunLoop,
    TapCreationFailed,
}

/// A tap attached to dodo's main run loop.
///
/// The boxed state outlives both the port and source. CoreGraphics keeps only
/// its raw address, and `Drop` disables and invalidates the port on that same
/// main thread before freeing it.
pub struct EventTap {
    state: Box<State>,
    tap: CFRetained<CFMachPort>,
    source: CFRetained<CFRunLoopSource>,
    run_loop: CFRetained<CFRunLoop>,
    mode: &'static CFRunLoopMode,
    /// Kept only to be dropped, which unregisters it. It writes into the same
    /// [`FrontmostApplication`] the callback reads, and must not outlive it.
    _activation: ActivationObserver,
}

impl EventTap {
    /// Creates and enables an editable session event tap.
    ///
    /// It takes the whole settings document rather than a
    /// [`VietnameseConfig`]: the tap answers the language switch as well as
    /// Vietnamese, and the two must never be configured from different reads.
    /// `language_changes` is how a cycle performed inside the CoreGraphics
    /// callback reaches the state layer — the callback has no `App` and must
    /// not block, so it sends and returns.
    pub fn start(
        document: SettingsDocument,
        language_changes: UnboundedSender<LanguageId>,
        request_accessibility: bool,
    ) -> Result<EventTap, StartError> {
        if !accessibility_trusted() {
            if request_accessibility {
                request_accessibility_permission();
            }
            return Err(StartError::AccessibilityDenied);
        }

        let mode = common_modes().ok_or(StartError::NoRunLoop)?;
        let mut state = Box::new(State::new(document, language_changes));
        // Before the tap exists, so the very first keystroke already knows which
        // application it is going to.
        let activation = ActivationObserver::install(Rc::clone(&state.frontmost));
        let user_info = (&mut *state as *mut State).cast();
        let mask = event_mask();
        // SAFETY: `callback` matches CoreGraphics' declared callback type, and
        // `state` is held by the returned EventTap until its port is invalidated.
        let tap = unsafe {
            CGEvent::tap_create(
                CGEventTapLocation::SessionEventTap,
                CGEventTapPlacement::HeadInsertEventTap,
                CGEventTapOptions::Default,
                mask,
                Some(callback),
                user_info,
            )
        }
        .ok_or(StartError::TapCreationFailed)?;
        let source = CFMachPort::new_run_loop_source(None, Some(&tap), 0)
            .ok_or(StartError::TapCreationFailed)?;
        let run_loop = CFRunLoop::main().ok_or(StartError::NoRunLoop)?;

        state.tap.set(Some(CFRetained::as_ptr(&tap)));
        run_loop.add_source(Some(&source), Some(mode));
        CGEvent::tap_enable(&tap, true);
        if !CGEvent::tap_is_enabled(&tap) {
            // Undo the run-loop registration before `state` can be dropped;
            // CoreGraphics otherwise retains a callback pointing at freed data.
            run_loop.remove_source(Some(&source), Some(mode));
            source.invalidate();
            tap.invalidate();
            return Err(StartError::TapCreationFailed);
        }

        Ok(EventTap {
            state,
            tap,
            source,
            run_loop,
            mode,
            _activation: activation,
        })
    }

    /// Applies new settings without reading disk or retaining old composition.
    ///
    /// Direct output is already in the focused document, so resetting is safer
    /// than attempting an out-of-band rewrite with no original event to pair.
    ///
    /// This is what makes a recorded shortcut live without a restart, and it
    /// replaces rather than adds: there is one tap and one [`LiveSwitch`], so
    /// the combination that was recorded over stops matching in the same call
    /// the replacement starts matching.
    pub fn reconfigure(&self, document: SettingsDocument) {
        self.state.reconfigure(document);
    }

    pub fn status(&self) -> EventTapStatus {
        self.state.status.get()
    }
}

impl Drop for EventTap {
    fn drop(&mut self) {
        CGEvent::tap_enable(&self.tap, false);
        self.run_loop
            .remove_source(Some(&self.source), Some(self.mode));
        self.source.invalidate();
        self.tap.invalidate();
        self.state.tap.set(None);
    }
}

/// The frontmost application's bundle identifier, as last reported by AppKit.
///
/// It is a shared cell rather than a field of [`State`] because two things need
/// it and only one of them is the tap: the notification block writes, the
/// CoreGraphics callback reads, and both run on the main thread. Nothing here
/// asks AppKit anything — [`ActivationObserver`] is the only writer.
#[derive(Default)]
struct FrontmostApplication {
    bundle_id: RefCell<Option<String>>,
}

impl FrontmostApplication {
    fn adopt(&self, bundle_id: Option<String>) {
        if let Ok(mut current) = self.bundle_id.try_borrow_mut() {
            *current = bundle_id;
        }
    }
}

struct State {
    composer: RefCell<DirectComposer>,
    /// The language switch as this listener currently understands it.
    switch: RefCell<LiveSwitch>,
    /// Where a cycle performed on the callback path is reported.
    language_changes: UnboundedSender<LanguageId>,
    /// Whether the browser address-bar workaround is switched on.
    ///
    /// A `Cell` beside the composer rather than a re-read of the document:
    /// [`reconfigure`](State::reconfigure) is the one place settings arrive, and
    /// the callback path must not take a borrow it could fail.
    browser_fix: Cell<bool>,
    frontmost: Rc<FrontmostApplication>,
    synthetic_tag: i64,
    /// Key-up events matching a replaced physical key-down. A bitset avoids a
    /// callback-path allocation while preserving real down/up pairs.
    suppressed_key_ups: Cell<u128>,
    /// The last focused target reported by CoreGraphics on a physical key.
    target_process: Cell<Option<u32>>,
    /// A borrowed tap pointer, valid while [`EventTap`] retains it.
    tap: Cell<Option<NonNull<CFMachPort>>>,
    status: Cell<EventTapStatus>,
}

impl State {
    fn new(document: SettingsDocument, language_changes: UnboundedSender<LanguageId>) -> State {
        State {
            composer: RefCell::new(DirectComposer::new(document.vietnamese.to_config())),
            switch: RefCell::new(LiveSwitch::new(&document)),
            language_changes,
            browser_fix: Cell::new(document.browser_address_bar_fix),
            frontmost: Rc::new(FrontmostApplication::default()),
            synthetic_tag: synthetic_event_tag(std::process::id()),
            suppressed_key_ups: Cell::new(0),
            target_process: Cell::new(None),
            tap: Cell::new(None),
            status: Cell::new(EventTapStatus::Running),
        }
    }

    fn reconfigure(&self, document: SettingsDocument) {
        if let Ok(mut switch) = self.switch.try_borrow_mut() {
            switch.adopt(&document);
        }
        if let Ok(mut composer) = self.composer.try_borrow_mut() {
            composer.reconfigure(document.vietnamese.to_config());
        }
        self.browser_fix.set(document.browser_address_bar_fix);
    }

    /// The adjustment this plan needs in the application it is about to land in.
    ///
    /// A failed borrow answers "no adjustment", for the same reason every other
    /// failed borrow here answers the cautious way: posting the plan exactly as
    /// the engine described it is what this host did before browsers were
    /// special-cased, and it cannot delete anything the engine did not ask for.
    fn browser_rewrite(&self, plan: &OutputPlan) -> BrowserRewrite {
        let Ok(bundle_id) = self.frontmost.bundle_id.try_borrow() else {
            return BrowserRewrite::verbatim(plan);
        };
        BrowserRewrite::plan(self.browser_fix.get(), bundle_id.as_deref(), plan)
    }

    /// Cycles the language when this press is the shortcut, and says whether
    /// the key must be swallowed.
    ///
    /// A failed borrow answers "not the shortcut", which is the same thing the
    /// composer does when it is re-entered: a key reaching the application is
    /// always safer than one that disappears.
    fn cycle(&self, event: &KeyEvent) -> bool {
        let Ok(mut switch) = self.switch.try_borrow_mut() else {
            return false;
        };
        let Some(cycled) = switch.cycle(event) else {
            return false;
        };
        drop(switch);
        // The syllable in flight belonged to the language being left.
        self.reset();
        let _ = self.language_changes.unbounded_send(cycled.language);
        if cycled.beep {
            NSBeep();
        }
        true
    }

    /// Whether the Vietnamese engine should see keys at all right now.
    fn transforms(&self) -> bool {
        self.switch
            .try_borrow()
            .is_ok_and(|switch| switch.transforms())
    }

    fn reset(&self) {
        if let Ok(mut composer) = self.composer.try_borrow_mut() {
            composer.reset();
        }
    }

    fn reset_if_target_changed(&self, target: Option<u32>) -> bool {
        let Some(target) = target else {
            return false;
        };
        let changed = self
            .target_process
            .replace(Some(target))
            .is_some_and(|previous| previous != target);
        if changed {
            self.reset();
        }
        changed
    }

    fn reset_after_pass_through(&self, event: TapEvent) {
        if invalidates_composer(event) {
            self.reset();
        }
    }

    fn is_synthetic(&self, event: &CGEvent) -> bool {
        is_synthetic_event(
            CGEvent::integer_value_field(Some(event), CGEventField::EventSourceUserData),
            self.synthetic_tag,
        )
    }

    fn suppress_key_up(&self, key_code: u16) {
        if key_code < 128 {
            self.suppressed_key_ups
                .set(self.suppressed_key_ups.get() | (1_u128 << key_code));
        }
    }

    fn allow_key_up(&self, key_code: u16) {
        if key_code < 128 {
            self.suppressed_key_ups
                .set(self.suppressed_key_ups.get() & !(1_u128 << key_code));
        }
    }

    fn take_suppressed_key_up(&self, event: &CGEvent) -> bool {
        let Ok(key_code) = u16::try_from(CGEvent::integer_value_field(
            Some(event),
            CGEventField::KeyboardEventKeycode,
        )) else {
            return false;
        };
        if key_code >= 128 {
            return false;
        }
        let bit = 1_u128 << key_code;
        let pending = self.suppressed_key_ups.get();
        if pending & bit == 0 {
            return false;
        }
        self.suppressed_key_ups.set(pending & !bit);
        true
    }

    fn recover(&self) {
        let Some(tap) = self.tap.get() else {
            self.status.set(EventTapStatus::Failed);
            return;
        };
        // SAFETY: EventTap retains this exact port while its callback can run.
        let tap = unsafe { tap.as_ref() };
        CGEvent::tap_enable(tap, true);
        self.status.set(if CGEvent::tap_is_enabled(tap) {
            EventTapStatus::Running
        } else {
            EventTapStatus::Failed
        });
    }
}

/// Keys plus the only pointer events that can move the focused caret.
///
/// Mouse movement and scrolling are deliberately absent: they cannot prove a
/// caret change and would put avoidable traffic on the callback path.
/// `FlagsChanged` is in this mask and no mouse-moved or scroll event is.
///
/// The modifier transitions are what make a modifier-only shortcut observable
/// at all — CoreGraphics reports a bare `⇧` as `FlagsChanged` and never as a
/// key-down — and they are the *only* thing this host reads them for.
fn event_mask() -> u64 {
    [
        CGEventType::KeyDown,
        CGEventType::KeyUp,
        CGEventType::FlagsChanged,
        CGEventType::LeftMouseDown,
        CGEventType::RightMouseDown,
        CGEventType::OtherMouseDown,
    ]
    .into_iter()
    .fold(0, |mask, event| mask | (1_u64 << event.0))
}

fn classify_tap_event(event_type: CGEventType, autorepeat: bool) -> TapEvent {
    match event_type {
        CGEventType::KeyDown => TapEvent::KeyDown { autorepeat },
        CGEventType::FlagsChanged => TapEvent::ModifiersChanged,
        CGEventType::LeftMouseDown | CGEventType::RightMouseDown | CGEventType::OtherMouseDown => {
            TapEvent::MouseDown
        }
        _ => TapEvent::Other,
    }
}

fn target_process(event: &CGEvent) -> Option<u32> {
    u32::try_from(CGEvent::integer_value_field(
        Some(event),
        CGEventField::EventTargetUnixProcessID,
    ))
    .ok()
    .filter(|process| *process != 0)
}

fn common_modes() -> Option<&'static CFRunLoopMode> {
    // SAFETY: CoreFoundation exports this immutable process-lifetime constant.
    unsafe { kCFRunLoopCommonModes }
}

pub(crate) fn accessibility_trusted() -> bool {
    // SAFETY: `AXIsProcessTrusted` has no arguments and only asks TCC for this
    // process' current Accessibility grant. It never prompts or changes it.
    unsafe { AXIsProcessTrusted() != 0 }
}

fn request_accessibility_permission() {
    let Some(prompt_key) = (unsafe { kAXTrustedCheckOptionPrompt }) else {
        return;
    };
    let options = CFDictionary::from_slices(&[prompt_key], &[CFBoolean::new(true)]);
    // SAFETY: `options` is a live typed CoreFoundation dictionary for the call.
    // macOS displays any request asynchronously; its return value is the current
    // trust state and cannot say whether the user will grant permission.
    unsafe {
        let _ = AXIsProcessTrustedWithOptions(options.as_opaque());
    }
}

fn secure_input_enabled() -> bool {
    // SAFETY: called only from dodo's main-run-loop tap callback, matching
    // Carbon's documented non-thread-safe requirement. It only reads the state.
    unsafe { IsSecureEventInputEnabled() != 0 }
}

/// CoreGraphics invokes this on the run loop holding the tap source.
unsafe extern "C-unwind" fn callback(
    _proxy: CGEventTapProxy,
    event_type: CGEventType,
    event: NonNull<CGEvent>,
    user_info: *mut std::ffi::c_void,
) -> *mut CGEvent {
    let event_ptr = event.as_ptr();
    // SAFETY: CoreGraphics supplies a live event for this call.
    let event = unsafe { event.as_ref() };
    // SAFETY: EventTap owns this state for every possible callback lifetime.
    let Some(state) = (unsafe { (user_info as *const State).as_ref() }) else {
        return event_ptr;
    };

    if state.is_synthetic(event) {
        return event_ptr;
    }

    if event_type == CGEventType::KeyDown {
        state.reset_if_target_changed(target_process(event));
    }
    let tap_event = if event_type == CGEventType::TapDisabledByTimeout
        || event_type == CGEventType::TapDisabledByUserInput
    {
        TapEvent::TapDisabled
    } else if event_type == CGEventType::KeyUp {
        TapEvent::KeyUp {
            suppress: state.take_suppressed_key_up(event),
        }
    } else {
        classify_tap_event(
            event_type,
            CGEvent::integer_value_field(Some(event), CGEventField::KeyboardEventAutorepeat) != 0,
        )
    };

    let secure_input = matches!(
        tap_event,
        TapEvent::KeyDown { .. } | TapEvent::ModifiersChanged
    ) && secure_input_enabled();
    match handling(tap_event, secure_input) {
        Handling::PassThrough => {
            state.reset_after_pass_through(tap_event);
            event_ptr
        }
        Handling::Suppress => null_mut(),
        Handling::OfferShortcut => {
            state.cycle(&modifier_event(CGEvent::flags(Some(event))));
            // Always the original: see `Handling::OfferShortcut`.
            event_ptr
        }
        Handling::RecoverTap => {
            // A disabled tap leaves the cursor and focus unknown, so no
            // committed-word snapshot can safely survive recovery.
            state.reset();
            // One enable per explicit CoreGraphics notification; no timer or
            // retry loop can spin while the system keeps the tap disabled.
            state.recover();
            event_ptr
        }
        Handling::ProcessKey { autorepeat } => transform(state, event, autorepeat),
    }
}

fn transform(state: &State, event: &CGEvent, autorepeat: bool) -> *mut CGEvent {
    let original = event as *const CGEvent as *mut CGEvent;
    let Ok(key_code) = u16::try_from(CGEvent::integer_value_field(
        Some(event),
        CGEventField::KeyboardEventKeycode,
    )) else {
        state.reset();
        return original;
    };
    let key = key_event(
        unicode_character(event),
        key_code,
        CGEvent::flags(Some(event)),
    );

    // The shortcut is answered before the engine sees anything, and before the
    // selected language is consulted — that ordering is what lets it switch
    // *out* of a language with no engine as well as into one. A repeat is
    // ignored so a held shortcut cycles once.
    if !autorepeat && state.cycle(&key) {
        // The key-down is being swallowed, so its key-up must be too.
        state.suppress_key_up(key_code);
        return null_mut();
    }
    if !state.transforms() {
        state.reset();
        return original;
    }

    let Ok(mut composer) = state.composer.try_borrow_mut() else {
        return original;
    };
    // Plan against a copy. A failed CoreGraphics allocation must not leave
    // composition claiming text that the original event will now type.
    let mut next = composer.clone();
    let plan = next.process(key);
    if !plan.transforms() {
        *composer = next;
        drop(composer);
        if plan.pass_through {
            // A repeat can switch from a replaced down to an original down before
            // the one physical key-up arrives, so that up must now pass as well.
            state.allow_key_up(key_code);
        }
        return original;
    }
    let Some(output) = staged_output(&plan, &state.browser_rewrite(&plan), state.synthetic_tag)
    else {
        composer.reset();
        drop(composer);
        // The original down is passing, so its up must pass too even if an
        // earlier repeat of this key had been replaced.
        state.allow_key_up(key_code);
        return original;
    };
    // The plan is now fully staged. Commit its state before any post can
    // re-enter this callback through a synthetic event.
    *composer = next;
    drop(composer);

    if plan.pass_through {
        state.allow_key_up(key_code);
    } else {
        state.suppress_key_up(key_code);
    }
    for output in &output {
        CGEvent::post(CGEventTapLocation::SessionEventTap, Some(output));
    }

    if plan.pass_through {
        original
    } else {
        null_mut()
    }
}

fn unicode_character(event: &CGEvent) -> Option<char> {
    let mut actual = 0_u64;
    // SAFETY: the length pointer is valid; a null text buffer is documented.
    unsafe {
        CGEvent::keyboard_get_unicode_string(Some(event), 0, &mut actual, null_mut());
    }
    let length = usize::try_from(actual).ok()?;
    if length > MAX_INPUT_UNICODE_UNITS {
        return None;
    }
    let mut units = [0_u16; MAX_INPUT_UNICODE_UNITS];
    let mut copied = 0_u64;
    // SAFETY: `units` holds `length` UTF-16 units, bounded above.
    unsafe {
        CGEvent::keyboard_get_unicode_string(
            Some(event),
            length as _,
            &mut copied,
            units.as_mut_ptr(),
        );
    }
    (copied == actual).then(|| single_character(&units[..length]))?
}

fn single_character(units: &[u16]) -> Option<char> {
    let mut characters = std::char::decode_utf16(units.iter().copied());
    let character = characters.next()?.ok()?;
    (characters.next().is_none() && !character.is_control()).then_some(character)
}

fn staged_output(
    plan: &OutputPlan,
    rewrite: &BrowserRewrite,
    tag: i64,
) -> Option<Vec<CFRetained<CGEvent>>> {
    // Derive both replacements once, describe every event, then complete every
    // fallible CoreGraphics allocation before posting any of them.
    let descriptors = synthetic_event_descriptors(plan, rewrite, tag)?;
    let mut units = [0_u16; MAX_REPLACEMENT_UNICODE_UNITS];
    let mut length = 0;
    if let Some(text) = &plan.insert {
        for unit in text.encode_utf16() {
            units[length] = unit;
            length += 1;
        }
    }
    let mut commit = [0_u16; MAX_COMMIT_UNICODE_UNITS];
    let mut commit_length = 0;
    if let Some(text) = rewrite.commit_character {
        for unit in text.encode_utf16() {
            *commit.get_mut(commit_length)? = unit;
            commit_length += 1;
        }
        if commit_length == 0 {
            return None;
        }
    }

    let mut output = Vec::with_capacity(descriptors.len());
    for descriptor in descriptors {
        let event = match descriptor.payload {
            SyntheticPayload::Replacement => {
                tagged_unicode_event(&units[..length], descriptor.tag)?
            }
            SyntheticPayload::CommitCharacter => {
                tagged_unicode_event(&commit[..commit_length], descriptor.tag)?
            }
            SyntheticPayload::Key => tagged_key_event(
                descriptor.key_code,
                descriptor.down,
                descriptor.flags,
                descriptor.tag,
            )?,
        };
        output.push(event);
    }
    Some(output)
}

/// Every event this rewrite posts, in the order it posts them.
///
/// The order *is* the fix. Whatever clears a browser's inline selection —
/// `Shift`+`Left` or the invisible character — has to reach the application
/// before the first Backspace, and `rewrite.delete_before` rather than
/// `plan.delete_before` is the count, because clearing that selection changes
/// how many characters the Backspaces still have to remove.
fn synthetic_event_descriptors(
    plan: &OutputPlan,
    rewrite: &BrowserRewrite,
    tag: i64,
) -> Option<Vec<SyntheticEventDescriptor>> {
    let mut output = Vec::with_capacity(rewrite.delete_before.saturating_mul(2) + 6);
    if rewrite.extend_selection {
        for down in [true, false] {
            output.push(SyntheticEventDescriptor {
                key_code: LEFT_ARROW_KEY_CODE,
                down,
                payload: SyntheticPayload::Key,
                flags: SHIFT_LEFT_FLAGS,
                tag,
            });
        }
    }
    if rewrite.commit_character.is_some() {
        output.push(SyntheticEventDescriptor {
            key_code: 0,
            down: true,
            payload: SyntheticPayload::CommitCharacter,
            flags: CGEventFlags::empty(),
            tag,
        });
        output.push(SyntheticEventDescriptor {
            key_code: 0,
            down: false,
            payload: SyntheticPayload::Key,
            flags: CGEventFlags::empty(),
            tag,
        });
    }
    for _ in 0..rewrite.delete_before {
        output.push(SyntheticEventDescriptor {
            key_code: DELETE_KEY_CODE,
            down: true,
            payload: SyntheticPayload::Key,
            flags: CGEventFlags::empty(),
            tag,
        });
        output.push(SyntheticEventDescriptor {
            key_code: DELETE_KEY_CODE,
            down: false,
            payload: SyntheticPayload::Key,
            flags: CGEventFlags::empty(),
            tag,
        });
    }
    if let Some(text) = &plan.insert {
        if text.is_empty() || text.encode_utf16().count() > MAX_REPLACEMENT_UNICODE_UNITS {
            return None;
        }
        output.push(SyntheticEventDescriptor {
            key_code: 0,
            down: true,
            payload: SyntheticPayload::Replacement,
            flags: CGEventFlags::empty(),
            tag,
        });
        output.push(SyntheticEventDescriptor {
            key_code: 0,
            down: false,
            payload: SyntheticPayload::Key,
            flags: CGEventFlags::empty(),
            tag,
        });
    }
    (!output.is_empty()).then_some(output)
}

fn tagged_key_event(
    key_code: u16,
    down: bool,
    flags: CGEventFlags,
    tag: i64,
) -> Option<CFRetained<CGEvent>> {
    let event = CGEvent::new_keyboard_event(None, key_code, down)?;
    if flags != CGEventFlags::empty() {
        CGEvent::set_flags(Some(&event), flags);
    }
    tag_event(&event, tag);
    Some(event)
}

fn tagged_unicode_event(units: &[u16], tag: i64) -> Option<CFRetained<CGEvent>> {
    let event = CGEvent::new_keyboard_event(None, 0, true)?;
    // SAFETY: CoreGraphics copies the live UTF-16 buffer during this call.
    unsafe {
        CGEvent::keyboard_set_unicode_string(Some(&event), units.len() as _, units.as_ptr());
    }
    tag_event(&event, tag);
    Some(event)
}

/// Keeps [`FrontmostApplication`] current, and is the only thing in this host
/// that talks to AppKit.
///
/// Dropping it unregisters the block, which is what makes the shared
/// [`FrontmostApplication`] safe to free afterwards.
struct ActivationObserver {
    center: Retained<NSNotificationCenter>,
    token: Retained<ProtocolObject<dyn NSObjectProtocol>>,
}

impl ActivationObserver {
    /// Seeds the cache and starts watching for application switches.
    ///
    /// The seed is the only `NSWorkspace` query on a keystroke's behalf that is
    /// ever made, and it happens once, at start: the tap can be switched on long
    /// after the user last changed application, and a first keystroke that did
    /// not know where it was going would be the one this whole workaround is
    /// about.
    fn install(frontmost: Rc<FrontmostApplication>) -> ActivationObserver {
        let workspace = NSWorkspace::sharedWorkspace();
        frontmost.adopt(bundle_id(workspace.frontmostApplication().as_deref()));
        let center = workspace.notificationCenter();
        // SAFETY: AppKit exports this immutable process-lifetime notification name.
        let name = unsafe { NSWorkspaceDidActivateApplicationNotification };
        let block = RcBlock::new(move |notification: NonNull<NSNotification>| {
            // SAFETY: AppKit supplies a live notification for this call.
            let notification = unsafe { notification.as_ref() };
            frontmost.adopt(bundle_id(activated_application(notification).as_deref()));
        });
        // SAFETY: a `nil` queue means the block runs synchronously on the thread
        // that posted the notification, and AppKit posts this one on the main
        // thread — the same thread that installs the observer here, runs the tap
        // callback, and drops this observer. Nothing it captures crosses a
        // thread, and `Drop` unregisters it before that capture can be freed.
        let token = unsafe {
            center.addObserverForName_object_queue_usingBlock(Some(name), None, None, &block)
        };
        ActivationObserver { center, token }
    }
}

impl Drop for ActivationObserver {
    fn drop(&mut self) {
        // SAFETY: `token` is exactly what this centre returned, and has not been
        // removed before now.
        unsafe { self.center.removeObserver(self.token.as_ref()) };
    }
}

/// The `NSRunningApplication` a `DidActivateApplication` notification is about.
///
/// AppKit documents `NSWorkspaceApplicationKey` as the carrier, which is the
/// race-free answer: asking for the frontmost application again would read
/// whatever is frontmost *now* rather than what this notification announced.
///
/// `None` — a missing key, or an application with no bundle identifier — clears
/// the cache rather than leaving the previous application's identifier in it.
/// An unknown application gets no workaround, which is the safe direction.
fn activated_application(notification: &NSNotification) -> Option<Retained<NSRunningApplication>> {
    let info = notification.userInfo()?;
    // SAFETY: AppKit exports this immutable process-lifetime key.
    let key: &AnyObject = unsafe { NSWorkspaceApplicationKey }.as_ref();
    info.objectForKey(key)?.downcast().ok()
}

fn bundle_id(application: Option<&NSRunningApplication>) -> Option<String> {
    Some(application?.bundleIdentifier()?.to_string())
}

fn tag_event(event: &CGEvent, tag: i64) {
    CGEvent::set_integer_value_field(Some(event), CGEventField::EventSourceUserData, tag);
}

/// macOS modifier flags normalized into the engine vocabulary.
/// The four command modifiers, out of a CoreGraphics flag mask.
fn modifiers(flags: CGEventFlags) -> Modifiers {
    const SHIFT: u64 = 1 << 17;
    const CONTROL: u64 = 1 << 18;
    const OPTION: u64 = 1 << 19;
    const COMMAND: u64 = 1 << 20;

    Modifiers {
        shift: flags.0 & SHIFT != 0,
        control: flags.0 & CONTROL != 0,
        alt: flags.0 & OPTION != 0,
        meta: flags.0 & COMMAND != 0,
    }
}

/// A `FlagsChanged` event as the shared vocabulary spells it.
///
/// The key code is not read. Which modifier moved does not matter — only which
/// ones are now held, because that is what a modifier-only shortcut is.
fn modifier_event(flags: CGEventFlags) -> KeyEvent {
    KeyEvent {
        key: Key::Modifier,
        text: None,
        modifiers: modifiers(flags),
    }
}

fn key_event(text: Option<char>, key_code: u16, flags: CGEventFlags) -> KeyEvent {
    let key = match key_code {
        0x24 | 0x4c => Key::Enter,
        0x30 => Key::Tab,
        0x31 => Key::Space,
        0x33 => Key::Backspace,
        0x35 => Key::Escape,
        // The eight macOS modifier key codes. They reach this table only through a `FlagsChanged`
        // event, and their sole reading is a modifier-only language switch.
        0x36 | 0x37 | 0x38 | 0x3a | 0x3b | 0x3c | 0x3d | 0x3e => Key::Modifier,
        0x75 => Key::Delete,
        0x73 => Key::Home,
        0x74 => Key::PageUp,
        0x77 => Key::End,
        0x79 => Key::PageDown,
        0x7b => Key::ArrowLeft,
        0x7c => Key::ArrowRight,
        0x7d => Key::ArrowDown,
        0x7e => Key::ArrowUp,
        _ if text.is_some() => Key::Character,
        _ => Key::Other,
    };

    KeyEvent {
        key,
        text: (key == Key::Space).then_some(' ').or(text).filter(|_| {
            !matches!(
                key,
                Key::Enter
                    | Key::Tab
                    | Key::Backspace
                    | Key::Escape
                    | Key::Delete
                    | Key::Home
                    | Key::End
                    | Key::PageUp
                    | Key::PageDown
                    | Key::ArrowLeft
                    | Key::ArrowRight
                    | Key::ArrowUp
                    | Key::ArrowDown
            )
        }),
        modifiers: modifiers(flags),
    }
}

// These public Carbon/ApplicationServices APIs have no generated objc2 module.
// The safe wrappers above confine them to permission and secure-input checks.
#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    static kAXTrustedCheckOptionPrompt: Option<&'static CFString>;

    fn AXIsProcessTrusted() -> u8;
    fn AXIsProcessTrustedWithOptions(options: &CFDictionary) -> u8;
}

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn IsSecureEventInputEnabled() -> u8;
}

#[cfg(test)]
mod tests {
    use super::{
        BrowserRewrite, DELETE_KEY_CODE, DirectComposer, LEFT_ARROW_KEY_CODE,
        MAX_COMMIT_UNICODE_UNITS, MAX_REPLACEMENT_UNICODE_UNITS, OutputPlan, SHIFT_LEFT_FLAGS,
        State, SyntheticEventDescriptor, SyntheticPayload, callback, classify_tap_event,
        event_mask, key_event, modifier_event, single_character, synthetic_event_descriptors,
        tag_event,
    };
    use crate::models::browser_rewrite::SELECTION_COMMIT_CHARACTER;
    use crate::models::settings::{
        LanguageSwitch, SettingsDocument, Shortcut, ShortcutKey, ShortcutModifiers,
    };
    use dodo_ime_core::{ActiveLanguages, Key, KeyEvent, LanguageId, Modifiers, VietnameseConfig};
    use objc2_core_graphics::{CGEvent, CGEventField, CGEventFlags, CGEventType};

    /// A tap state with no channel reader, which is all the composition tests
    /// need: an unbounded send to a dropped receiver simply fails.
    fn typing_state() -> State {
        State::new(
            SettingsDocument {
                language: LanguageId::Vietnamese,
                ..SettingsDocument::default()
            },
            futures_channel::mpsc::unbounded().0,
        )
    }

    /// The mask bit and flag values CoreGraphics uses for the four modifiers.
    const FLAG_SHIFT: u64 = 1 << 17;
    const FLAG_CONTROL: u64 = 1 << 18;
    const FLAG_OPTION: u64 = 1 << 19;
    const FLAG_COMMAND: u64 = 1 << 20;

    fn press(state: &State, document: &mut String, event: KeyEvent) {
        let plan = state.composer.borrow_mut().process(event);
        *document = dodo_ime_core::core::truncate_graphemes(document, plan.delete_before);
        if let Some(insert) = plan.insert {
            document.push_str(&insert);
        }
        if plan.pass_through {
            if event.key == Key::Backspace {
                *document = dodo_ime_core::core::truncate_graphemes(document, 1);
            } else if let Some(key) = event.typed() {
                document.push(key);
            }
        }
    }

    /// The whole point of this round, at the layer that used to have no answer
    /// at all: the tap owns the shortcut, replaces it in place, and cycles only
    /// the enabled languages.
    #[test]
    fn the_tap_switches_language_on_the_recorded_shortcut_and_never_on_the_replaced_one() {
        let (sender, mut received) = futures_channel::mpsc::unbounded();
        let document = SettingsDocument {
            language: LanguageId::English,
            active_languages: ActiveLanguages::from_languages(LanguageId::ALL).unwrap(),
            ..SettingsDocument::default()
        };
        let state = State::new(document, sender);
        assert!(!state.transforms(), "English types through");

        // `⌃⇧Space`, the default, arrives as a key-down.
        let control_shift_space =
            key_event(Some(' '), 0x31, CGEventFlags(FLAG_CONTROL | FLAG_SHIFT));
        assert!(state.cycle(&control_shift_space));
        assert_eq!(received.try_recv().unwrap(), LanguageId::Vietnamese);
        assert!(state.transforms());

        // Record `⌥Space` over it. One listener, reconfigured.
        let replacement = SettingsDocument {
            language: LanguageId::Vietnamese,
            active_languages: ActiveLanguages::from_languages(LanguageId::ALL).unwrap(),
            language_switch: LanguageSwitch {
                shortcut: Shortcut {
                    modifiers: ShortcutModifiers {
                        alt: true,
                        ..ShortcutModifiers::NONE
                    },
                    key: ShortcutKey::Space,
                },
                beep: false,
            },
            ..SettingsDocument::default()
        };
        state.reconfigure(replacement);
        assert!(
            !state.cycle(&control_shift_space),
            "the replaced shortcut must be inert without restarting the tap"
        );
        assert!(state.cycle(&key_event(Some(' '), 0x31, CGEventFlags(FLAG_OPTION))));
        assert_eq!(received.try_recv().unwrap(), LanguageId::Japanese);
        assert!(!state.transforms(), "Japanese has no engine here");
        assert!(state.cycle(&key_event(Some(' '), 0x31, CGEventFlags(FLAG_OPTION))));
        assert_eq!(received.try_recv().unwrap(), LanguageId::English);
    }

    /// A modifier-only shortcut is only reachable through `FlagsChanged`, so
    /// the mask, the key-code table and the flag reading all have to agree.
    #[test]
    fn a_modifier_only_shortcut_fires_from_a_flags_changed_event() {
        let mask = event_mask();
        assert_ne!(
            mask & (1_u64 << CGEventType::FlagsChanged.0),
            0,
            "without this bit a bare modifier is never delivered"
        );
        assert_eq!(
            classify_tap_event(CGEventType::FlagsChanged, false),
            crate::models::event_tap::TapEvent::ModifiersChanged
        );

        let (sender, mut received) = futures_channel::mpsc::unbounded();
        let state = State::new(
            SettingsDocument {
                language_switch: LanguageSwitch {
                    shortcut: Shortcut {
                        modifiers: ShortcutModifiers {
                            control: true,
                            shift: true,
                            ..ShortcutModifiers::NONE
                        },
                        key: ShortcutKey::Modifiers,
                    },
                    beep: false,
                },
                ..SettingsDocument::default()
            },
            sender,
        );

        // Control down, then Shift: only the press completing the set fires.
        assert!(!state.cycle(&modifier_event(CGEventFlags(FLAG_CONTROL))));
        assert!(state.cycle(&modifier_event(CGEventFlags(FLAG_CONTROL | FLAG_SHIFT))));
        assert_eq!(received.try_recv().unwrap(), LanguageId::Vietnamese);
        // Releasing them is two more transitions and neither fires again.
        assert!(!state.cycle(&modifier_event(CGEventFlags(FLAG_CONTROL))));
        assert!(!state.cycle(&modifier_event(CGEventFlags(0))));
        assert!(received.try_recv().is_err(), "no second switch");

        // Command is `meta`, not one of the other three.
        assert_eq!(
            modifier_event(CGEventFlags(FLAG_COMMAND)).modifiers,
            Modifiers {
                meta: true,
                ..Modifiers::NONE
            }
        );
        assert_eq!(
            modifier_event(CGEventFlags(FLAG_OPTION)).modifiers,
            Modifiers {
                alt: true,
                ..Modifiers::NONE
            }
        );
        assert_eq!(
            key_event(None, 0x3b, CGEventFlags(FLAG_CONTROL)).key,
            Key::Modifier
        );
    }

    #[test]
    fn mouse_down_mask_and_classification_reset_a_reopenable_word() {
        let mask = event_mask();
        for event in [
            objc2_core_graphics::CGEventType::KeyDown,
            objc2_core_graphics::CGEventType::KeyUp,
            objc2_core_graphics::CGEventType::LeftMouseDown,
            objc2_core_graphics::CGEventType::RightMouseDown,
            objc2_core_graphics::CGEventType::OtherMouseDown,
        ] {
            assert_ne!(mask & (1_u64 << event.0), 0, "{event:?}");
        }
        for event in [
            objc2_core_graphics::CGEventType::MouseMoved,
            objc2_core_graphics::CGEventType::ScrollWheel,
        ] {
            assert_eq!(mask & (1_u64 << event.0), 0, "{event:?}");
        }

        let state = typing_state();
        let mut document = String::new();
        for key in "ddee ".chars() {
            press(&state, &mut document, KeyEvent::character(key));
        }
        for event in [
            objc2_core_graphics::CGEventType::LeftMouseDown,
            objc2_core_graphics::CGEventType::RightMouseDown,
            objc2_core_graphics::CGEventType::OtherMouseDown,
        ] {
            assert_eq!(
                classify_tap_event(event, false),
                crate::models::event_tap::TapEvent::MouseDown,
            );
        }
        state.reset_after_pass_through(classify_tap_event(
            objc2_core_graphics::CGEventType::LeftMouseDown,
            false,
        ));
        press(&state, &mut document, KeyEvent::special(Key::Backspace));
        press(&state, &mut document, KeyEvent::character('f'));
        assert_eq!(document, "đêf");
    }

    #[test]
    fn target_process_change_resets_a_reopenable_word() {
        let state = typing_state();
        assert!(!state.reset_if_target_changed(Some(1)));
        assert!(!state.reset_if_target_changed(Some(1)));

        let mut document = String::new();
        for key in "ddee ".chars() {
            press(&state, &mut document, KeyEvent::character(key));
        }
        assert!(state.reset_if_target_changed(Some(2)));
        press(&state, &mut document, KeyEvent::special(Key::Backspace));
        press(&state, &mut document, KeyEvent::character('f'));
        assert_eq!(document, "đêf");
    }

    #[test]
    fn a_tagged_callback_cannot_reset_the_physical_target_or_composition() {
        let state = typing_state();
        assert!(!state.reset_if_target_changed(Some(1)));
        assert!(
            state
                .composer
                .borrow_mut()
                .process(KeyEvent::character('D'))
                .pass_through
        );

        // This is created only to call the callback directly; it is never posted.
        let event = CGEvent::new_keyboard_event(None, 0x02, true).unwrap();
        tag_event(&event, state.synthetic_tag);
        CGEvent::set_integer_value_field(Some(&event), CGEventField::EventTargetUnixProcessID, 2);
        let event_ptr = std::ptr::NonNull::from(&*event);
        let returned = unsafe {
            callback(
                std::ptr::null_mut(),
                CGEventType::KeyDown,
                event_ptr,
                (&state as *const State).cast_mut().cast(),
            )
        };
        assert_eq!(returned, event_ptr.as_ptr());
        assert_eq!(state.target_process.get(), Some(1));

        let plan = state
            .composer
            .borrow_mut()
            .process(KeyEvent::character('D'));
        assert_eq!(plan.delete_before, 1);
        assert_eq!(plan.insert.as_deref(), Some("Đ"));
    }

    /// A key press with no modifiers, as every descriptor but `Shift`+`Left`
    /// spells it.
    fn plain(key_code: u16, down: bool, tag: i64) -> SyntheticEventDescriptor {
        SyntheticEventDescriptor {
            key_code,
            down,
            payload: SyntheticPayload::Key,
            flags: CGEventFlags::empty(),
            tag,
        }
    }

    fn backspace_pair(tag: i64) -> Vec<SyntheticEventDescriptor> {
        vec![
            plain(DELETE_KEY_CODE, true, tag),
            plain(DELETE_KEY_CODE, false, tag),
        ]
    }

    fn replacement_pair(tag: i64) -> Vec<SyntheticEventDescriptor> {
        vec![
            SyntheticEventDescriptor {
                key_code: 0,
                down: true,
                payload: SyntheticPayload::Replacement,
                flags: CGEventFlags::empty(),
                tag,
            },
            plain(0, false, tag),
        ]
    }

    #[test]
    fn lowercase_and_uppercase_stroke_replacements_stage_one_backspace_pair_then_one_unicode_key() {
        let tag = 41;
        let mut expected = backspace_pair(tag);
        expected.extend(replacement_pair(tag));

        for (key, replacement) in [('d', "đ"), ('D', "Đ")] {
            let mut composer = DirectComposer::new(VietnameseConfig::default());
            assert!(composer.process(KeyEvent::character(key)).pass_through);
            let plan = composer.process(KeyEvent::character(key));
            assert_eq!(plan.delete_before, 1, "{key}");
            assert_eq!(plan.insert.as_deref(), Some(replacement), "{key}");
            assert!(!plan.pass_through, "{key}");
            assert_eq!(
                synthetic_event_descriptors(&plan, &BrowserRewrite::verbatim(&plan), tag),
                Some(expected.clone())
            );
        }
    }

    #[test]
    fn an_unstageable_replacement_has_no_partial_descriptor() {
        let plan = OutputPlan {
            insert: Some("x".repeat(MAX_REPLACEMENT_UNICODE_UNITS + 1)),
            ..OutputPlan::default()
        };
        assert_eq!(
            synthetic_event_descriptors(&plan, &BrowserRewrite::verbatim(&plan), 1),
            None
        );
    }

    /// The Chromium-family sequence, end to end: one `Shift`+`Left` pair,
    /// carrying both flags macOS puts on a real arrow key, ahead of everything
    /// else — and no Backspace at all, because the replacement overwrites what
    /// that selection now covers.
    #[test]
    fn extending_the_selection_stages_shift_left_first_and_drops_the_single_backspace() {
        let tag = 7;
        let plan = OutputPlan {
            delete_before: 1,
            insert: Some("ế".into()),
            pass_through: false,
        };
        let rewrite = BrowserRewrite::plan(true, Some("com.google.Chrome"), &plan);
        let staged = synthetic_event_descriptors(&plan, &rewrite, tag).unwrap();

        let mut expected = vec![
            SyntheticEventDescriptor {
                key_code: LEFT_ARROW_KEY_CODE,
                down: true,
                payload: SyntheticPayload::Key,
                flags: SHIFT_LEFT_FLAGS,
                tag,
            },
            SyntheticEventDescriptor {
                key_code: LEFT_ARROW_KEY_CODE,
                down: false,
                payload: SyntheticPayload::Key,
                flags: SHIFT_LEFT_FLAGS,
                tag,
            },
        ];
        expected.extend(replacement_pair(tag));
        assert_eq!(staged, expected);
        assert_eq!(LEFT_ARROW_KEY_CODE, 123);
        assert_eq!(
            SHIFT_LEFT_FLAGS,
            CGEventFlags::MaskShift | CGEventFlags::MaskNumericPad
        );

        // Two or more keeps every Backspace, still behind the selection.
        let plan = OutputPlan {
            delete_before: 3,
            ..plan
        };
        let rewrite = BrowserRewrite::plan(true, Some("com.google.Chrome"), &plan);
        let staged = synthetic_event_descriptors(&plan, &rewrite, tag).unwrap();
        assert_eq!(staged[..2], expected[..2]);
        assert_eq!(
            staged
                .iter()
                .filter(|event| event.key_code == DELETE_KEY_CODE)
                .count(),
            6
        );
    }

    /// The WebKit/Gecko sequence: the invisible character is typed before the
    /// Backspaces, and there is exactly one more Backspace than the engine
    /// asked for — the one that takes it away again.
    #[test]
    fn committing_the_suggestion_types_the_invisible_character_before_one_extra_backspace() {
        let tag = 9;
        let plan = OutputPlan {
            delete_before: 2,
            insert: Some("ế".into()),
            pass_through: false,
        };
        let rewrite = BrowserRewrite::plan(true, Some("com.apple.Safari"), &plan);
        let staged = synthetic_event_descriptors(&plan, &rewrite, tag).unwrap();

        let mut expected = vec![
            SyntheticEventDescriptor {
                key_code: 0,
                down: true,
                payload: SyntheticPayload::CommitCharacter,
                flags: CGEventFlags::empty(),
                tag,
            },
            plain(0, false, tag),
        ];
        for _ in 0..3 {
            expected.extend(backspace_pair(tag));
        }
        expected.extend(replacement_pair(tag));
        assert_eq!(staged, expected);
        assert!(
            SELECTION_COMMIT_CHARACTER.encode_utf16().count() <= MAX_COMMIT_UNICODE_UNITS,
            "the commit character must fit the buffer that stages it"
        );
    }

    /// The setting off, and an application nobody listed, both stage exactly
    /// what this host staged before browsers were special-cased.
    #[test]
    fn a_disabled_setting_or_an_unknown_application_stages_the_untouched_sequence() {
        let tag = 3;
        let plan = OutputPlan {
            delete_before: 2,
            insert: Some("ế".into()),
            pass_through: false,
        };
        let verbatim = synthetic_event_descriptors(&plan, &BrowserRewrite::verbatim(&plan), tag);
        for (enabled, bundle_id) in [
            (false, Some("com.google.Chrome")),
            (false, Some("com.apple.Safari")),
            (true, Some("com.apple.TextEdit")),
            (true, None),
        ] {
            assert_eq!(
                synthetic_event_descriptors(
                    &plan,
                    &BrowserRewrite::plan(enabled, bundle_id, &plan),
                    tag
                ),
                verbatim,
                "{enabled} {bundle_id:?}"
            );
        }
    }

    /// The tap reads the setting from the document it was configured with, and
    /// adopts a change without being restarted.
    #[test]
    fn the_browser_workaround_follows_the_setting_through_a_reconfigure() {
        let state = typing_state();
        let plan = OutputPlan {
            delete_before: 1,
            insert: Some("ế".into()),
            pass_through: false,
        };
        state.frontmost.adopt(Some("com.google.Chrome".to_owned()));
        assert_eq!(state.browser_rewrite(&plan).delete_before, 0);

        state.reconfigure(SettingsDocument {
            browser_address_bar_fix: false,
            ..SettingsDocument::default()
        });
        assert_eq!(
            state.browser_rewrite(&plan),
            BrowserRewrite::verbatim(&plan)
        );

        state.reconfigure(SettingsDocument::default());
        assert!(state.browser_rewrite(&plan).extend_selection);

        // A different application, no restart, no notification of our own.
        state.frontmost.adopt(Some("com.apple.Safari".to_owned()));
        assert_eq!(
            state.browser_rewrite(&plan).commit_character,
            Some(SELECTION_COMMIT_CHARACTER)
        );
        state.frontmost.adopt(None);
        assert_eq!(
            state.browser_rewrite(&plan),
            BrowserRewrite::verbatim(&plan)
        );
    }

    #[test]
    fn key_decoding_keeps_precomposed_scalars_and_rejects_combining_input() {
        assert_eq!(single_character(&['ư' as u16]), Some('ư'));
        assert_eq!(single_character(&['e' as u16, 0x0302]), None);
        assert_eq!(single_character(&[]), None);
    }

    #[test]
    fn shortcuts_and_non_text_keys_keep_their_platform_meaning() {
        let shortcut = key_event(Some('s'), 0x01, objc2_core_graphics::CGEventFlags(1 << 20));
        assert_eq!(shortcut.text, Some('s'));
        assert_eq!(shortcut.typed(), None);

        let arrow = key_event(
            None,
            0x7b,
            objc2_core_graphics::CGEventFlags((1 << 21) | (1 << 23)),
        );
        assert_eq!(arrow.key, Key::ArrowLeft);
        assert_eq!(arrow.modifiers, Modifiers::NONE);
    }
}
