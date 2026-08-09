//! The macOS `CGEventTap` host for the existing Vietnamese engine.
//!
//! It is deliberately direct-typing only: a global event tap has no marked-text
//! client, so it rewrites the short current syllable with tagged synthetic
//! events. Tagged events pass this callback unchanged, which prevents feedback
//! loops. No event text is logged, persisted, transmitted, or exposed.

use std::cell::{Cell, RefCell};
use std::ptr::{NonNull, null_mut};

use dodo_ime_core::{LanguageEngine as _, VietnameseConfig, VietnameseEngine};
use objc2_core_foundation::{
    CFMachPort, CFRetained, CFRunLoop, CFRunLoopMode, CFRunLoopSource, kCFRunLoopCommonModes,
};
use objc2_core_graphics::{
    CGEvent, CGEventField, CGEventFlags, CGEventTapLocation, CGEventTapOptions,
    CGEventTapPlacement, CGEventTapProxy, CGEventType,
};

use crate::input_method::models::event_tap::{
    EventTapStatus, Handling, OutputPlan, TapEvent, handling,
};

const DELETE_KEY_CODE: u16 = 0x33;
const MAX_UNICODE_UNITS: usize = 8;
const SYNTHETIC_EVENT_TAG: i64 = 0x444f_444f_4554_4150;

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
}

impl EventTap {
    /// Creates and enables an editable session event tap.
    pub fn start(config: VietnameseConfig) -> Result<EventTap, StartError> {
        if !accessibility_trusted() {
            return Err(StartError::AccessibilityDenied);
        }

        let mode = common_modes().ok_or(StartError::NoRunLoop)?;
        let mut state = Box::new(State::new(config));
        let user_info = (&mut *state as *mut State).cast();
        let mask = 1_u64 << CGEventType::KeyDown.0;
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
        })
    }

    /// Applies new settings without reading disk or retaining old composition.
    ///
    /// Direct output is already in the focused document, so resetting is safer
    /// than attempting an out-of-band rewrite with no original event to pair.
    pub fn reconfigure(&self, config: VietnameseConfig) {
        if let Ok(mut engine) = self.state.engine.try_borrow_mut() {
            *engine = VietnameseEngine::new(direct_config(config));
        }
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

struct State {
    engine: RefCell<VietnameseEngine>,
    /// A borrowed tap pointer, valid while [`EventTap`] retains it.
    tap: Cell<Option<NonNull<CFMachPort>>>,
    status: Cell<EventTapStatus>,
}

impl State {
    fn new(config: VietnameseConfig) -> State {
        State {
            engine: RefCell::new(VietnameseEngine::new(direct_config(config))),
            tap: Cell::new(None),
            status: Cell::new(EventTapStatus::Running),
        }
    }

    fn reset(&self) {
        if let Ok(mut engine) = self.engine.try_borrow_mut() {
            let _ = engine.reset();
        }
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

fn direct_config(mut config: VietnameseConfig) -> VietnameseConfig {
    config.output = dodo_ime_core::OutputMode::Direct;
    config
}

fn common_modes() -> Option<&'static CFRunLoopMode> {
    // SAFETY: CoreFoundation exports this immutable process-lifetime constant.
    unsafe { kCFRunLoopCommonModes }
}

fn accessibility_trusted() -> bool {
    // SAFETY: `AXIsProcessTrusted` has no arguments and only asks TCC for this
    // process' current Accessibility grant. It never prompts or changes it.
    unsafe { AXIsProcessTrusted() != 0 }
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

    if CGEvent::integer_value_field(Some(event), CGEventField::EventSourceUserData)
        == SYNTHETIC_EVENT_TAG
    {
        return event_ptr;
    }

    let tap_event = if event_type == CGEventType::TapDisabledByTimeout
        || event_type == CGEventType::TapDisabledByUserInput
    {
        TapEvent::TapDisabled
    } else if event_type == CGEventType::KeyDown {
        TapEvent::KeyDown {
            autorepeat: CGEvent::integer_value_field(
                Some(event),
                CGEventField::KeyboardEventAutorepeat,
            ) != 0,
        }
    } else {
        TapEvent::Other
    };

    match handling(tap_event, secure_input_enabled()) {
        Handling::PassThrough => {
            state.reset();
            event_ptr
        }
        Handling::RecoverTap => {
            // One enable per explicit CoreGraphics notification; no timer or
            // retry loop can spin while the system keeps the tap disabled.
            state.recover();
            event_ptr
        }
        Handling::ProcessKey { .. } => transform(state, event),
    }
}

fn transform(state: &State, event: &CGEvent) -> *mut CGEvent {
    let original = event as *const CGEvent as *mut CGEvent;
    let Some(text) = unicode_text(event) else {
        state.reset();
        return original;
    };
    let Ok(key_code) = u16::try_from(CGEvent::integer_value_field(
        Some(event),
        CGEventField::KeyboardEventKeycode,
    )) else {
        state.reset();
        return original;
    };
    let key = key_event(&text, key_code, CGEvent::flags(Some(event)));

    let Ok(mut engine) = state.engine.try_borrow_mut() else {
        return original;
    };
    let result = engine.process_key(&key);
    let Some(plan) = OutputPlan::from_actions(&result.actions) else {
        let _ = engine.reset();
        return original;
    };
    drop(engine);

    if !plan.transforms() {
        return original;
    }
    let Some(output) = staged_output(event, &plan) else {
        state.reset();
        return original;
    };
    for output in &output {
        CGEvent::post(CGEventTapLocation::SessionEventTap, Some(output));
    }

    if plan.pass_through {
        original
    } else {
        null_mut()
    }
}

fn unicode_text(event: &CGEvent) -> Option<String> {
    let mut actual = 0_u64;
    // SAFETY: the length pointer is valid; a null text buffer is documented.
    unsafe {
        CGEvent::keyboard_get_unicode_string(Some(event), 0, &mut actual, null_mut());
    }
    let length = usize::try_from(actual).ok()?;
    if length > MAX_UNICODE_UNITS {
        return None;
    }
    let mut units = [0_u16; MAX_UNICODE_UNITS];
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
    (copied == actual).then(|| String::from_utf16(&units[..length]).ok())?
}

fn staged_output(event: &CGEvent, plan: &OutputPlan) -> Option<Vec<CFRetained<CGEvent>>> {
    let mut output = Vec::with_capacity(plan.delete_before.saturating_mul(2) + 1);
    for _ in 0..plan.delete_before {
        output.push(tagged_key_event(DELETE_KEY_CODE, true)?);
        output.push(tagged_key_event(DELETE_KEY_CODE, false)?);
    }
    if let Some(text) = &plan.insert {
        let text = text.encode_utf16().collect::<Vec<_>>();
        let copy = CGEvent::new_copy(Some(event))?;
        // SAFETY: `text` remains live until CoreGraphics has copied it.
        unsafe {
            CGEvent::keyboard_set_unicode_string(Some(&copy), text.len() as _, text.as_ptr());
        }
        CGEvent::set_integer_value_field(
            Some(&copy),
            CGEventField::EventSourceUserData,
            SYNTHETIC_EVENT_TAG,
        );
        output.push(copy);
    }
    (!output.is_empty()).then_some(output)
}

fn tagged_key_event(key_code: u16, down: bool) -> Option<CFRetained<CGEvent>> {
    let event = CGEvent::new_keyboard_event(None, key_code, down)?;
    CGEvent::set_integer_value_field(
        Some(&event),
        CGEventField::EventSourceUserData,
        SYNTHETIC_EVENT_TAG,
    );
    Some(event)
}

/// The same macOS normalisation the native host uses, kept local because dodo
/// must not link the InputMethodKit bundle just to obtain a key adapter.
fn key_event(text: &str, key_code: u16, flags: CGEventFlags) -> dodo_ime_core::KeyEvent {
    use dodo_ime_core::{Key, KeyEvent, Modifiers};

    const SHIFT: u64 = 1 << 17;
    const CONTROL: u64 = 1 << 18;
    const OPTION: u64 = 1 << 19;
    const COMMAND: u64 = 1 << 20;

    let modifiers = Modifiers {
        shift: flags.0 & SHIFT != 0,
        control: flags.0 & CONTROL != 0,
        alt: flags.0 & OPTION != 0,
        meta: flags.0 & COMMAND != 0,
    };
    let text = {
        let mut chars = text.chars();
        match (chars.next(), chars.next()) {
            (Some(character), None) if !character.is_control() => Some(character),
            _ => None,
        }
    };
    let key = match key_code {
        0x24 | 0x4c => Key::Enter,
        0x30 => Key::Tab,
        0x31 => Key::Space,
        0x33 => Key::Backspace,
        0x35 => Key::Escape,
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
        modifiers,
    }
}

// These public Carbon/ApplicationServices APIs have no generated objc2 module.
// The safe wrappers above confine them to permission and secure-input checks.
#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> u8;
}

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn IsSecureEventInputEnabled() -> u8;
}

#[cfg(test)]
mod tests {
    use super::key_event;
    use dodo_ime_core::{Key, Modifiers};

    #[test]
    fn shortcuts_and_non_text_keys_keep_the_native_hosts_meaning() {
        let shortcut = key_event("s", 0x01, objc2_core_graphics::CGEventFlags(1 << 20));
        assert_eq!(shortcut.text, Some('s'));
        assert_eq!(shortcut.typed(), None);

        let arrow = key_event(
            "",
            0x7b,
            objc2_core_graphics::CGEventFlags((1 << 21) | (1 << 23)),
        );
        assert_eq!(arrow.key, Key::ArrowLeft);
        assert_eq!(arrow.modifiers, Modifiers::NONE);
    }
}
