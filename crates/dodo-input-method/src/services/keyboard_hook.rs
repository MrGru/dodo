//! Windows' `WH_KEYBOARD_LL` fallback for the Vietnamese engine.
//!
//! It is installed while dodo is running. The callback
//! is intentionally smaller than the policy around it: injected, repeated,
//! shortcut, secure-desktop-uncertain, and untranslatable input all go unchanged
//! to the next hook. A key-up is consumed only when its physical down was. It
//! never logs, persists, transmits, or exposes a keystroke.
//!
//! # It owns the language switch while it runs
//!
//! The hook owns the shortcut, so it stays installed in *every*
//! language, matches the shortcut before anything else, and only then asks
//! whether the key should reach the Vietnamese engine.
//! `models::live_switch` is that rule and `models::keyboard_hook::key_event` is
//! the virtual-key table, both of them pure so they are tested on hosts that
//! cannot compile this file. A paired `WH_MOUSE_LL` hook observes button-downs
//! only to forget retained text before a same-control focus or caret move.
//!
//! # The keyboard state is built here, never fetched
//!
//! `GetKeyboardState` answers about the **calling thread**, and this callback
//! runs on dodo's — in the background, where that answer stopped advancing the
//! moment dodo lost focus. Reading Shift from it made every capital letter
//! lowercase and made every shortcut with a modifier in it unmatchable, which
//! was one defect wearing two faces. `models::keyboard_hook::PhysicalKeys`
//! carries the reasoning; the physical keys come from `GetAsyncKeyState`, the
//! arriving key folds itself in, and caps lock is tracked rather than asked.
//!
//! The shortcut is also answered **before** anything asks where text would go.
//! A window with no focused control is still a window the user may switch
//! language in, and the switch is answered before any focus check.

use std::collections::HashSet;
use std::ptr::null_mut;
use std::sync::{Arc, Mutex, OnceLock};

use dodo_ime_core::{Key, LanguageId};
use futures_channel::mpsc::UnboundedSender;
use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::Diagnostics::Debug::MessageBeep;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, GetKeyState, GetKeyboardLayout, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT,
    KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, SendInput, ToUnicodeEx, VK_CAPITAL,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GUITHREADINFO, GetForegroundWindow, GetGUIThreadInfo, GetWindowThreadProcessId,
    HC_ACTION, HHOOK, KBDLLHOOKSTRUCT, LLKHF_INJECTED, MB_OK, SetWindowsHookExW,
    UnhookWindowsHookEx, WH_KEYBOARD_LL, WH_MOUSE_LL, WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDOWN,
    WM_MBUTTONDOWN, WM_RBUTTONDOWN, WM_XBUTTONDOWN,
};

use crate::models::direct_output::OutputPlan;
use crate::models::event_tap::DirectComposer;
use crate::models::keyboard_hook::{
    CapsLock, Handling, HookEvent, KeyboardHookStatus, PhysicalKeys, SuppressedKeyUps,
    TargetIdentity, adopt_after_send, handling, input_event_count, key_event as windows_key_event,
    layout_state, physical_modifiers, target_changed, vk, with_key_down,
};
use crate::models::live_switch::LiveSwitch;
use crate::models::settings::SettingsDocument;

const SYNTHETIC_EVENT_TAG: usize = 0x444f_444f_5748_4f4f;

/// Why the hook could not start. It carries no input data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartError {
    AlreadyRunning,
    HookCreationFailed,
}

/// A live global hook. Dropping it always unregisters before callback state can
/// be freed, so shutdown cannot leave a key observer behind.
pub struct KeyboardHook {
    keyboard_hook: HHOOK,
    mouse_hook: HHOOK,
    state: Arc<Mutex<State>>,
}

struct State {
    composer: DirectComposer,
    /// The language switch as this listener currently understands it.
    switch: LiveSwitch,
    /// Where a cycle performed on the callback path is reported.
    language_changes: UnboundedSender<LanguageId>,
    pressed: HashSet<u32>,
    suppressed_key_ups: SuppressedKeyUps,
    target: Option<TargetIdentity>,
    /// Followed rather than asked for, because a background thread's answer is
    /// a snapshot. See `models::keyboard_hook::CapsLock`.
    caps: CapsLock,
    status: KeyboardHookStatus,
}

impl KeyboardHook {
    /// Installs the hook.
    ///
    /// It takes the whole settings document rather than a
    /// [`dodo_ime_core::VietnameseConfig`]: the hook answers the language switch as well as
    /// Vietnamese, and the two must never be configured from different reads.
    pub fn start(
        document: SettingsDocument,
        language_changes: UnboundedSender<LanguageId>,
    ) -> Result<Self, StartError> {
        let state = Arc::new(Mutex::new(State {
            composer: DirectComposer::new(document.vietnamese.to_config()),
            switch: LiveSwitch::new(&document),
            language_changes,
            pressed: HashSet::new(),
            suppressed_key_ups: SuppressedKeyUps::default(),
            target: None,
            // Taken during startup while dodo still owns the foreground.
            caps: CapsLock::new((unsafe { GetKeyState(VK_CAPITAL as i32) }) & 1 != 0),
            status: KeyboardHookStatus::Running,
        }));
        let mut active = active_hook()
            .lock()
            .map_err(|_| StartError::AlreadyRunning)?;
        if active.is_some() {
            return Err(StartError::AlreadyRunning);
        }
        *active = Some(state.clone());
        // A process-local low-level hook needs no injected DLL or elevated
        // helper. Windows delivers callbacks to dodo's existing UI loop.
        let keyboard_hook =
            unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(callback), null_mut(), 0) };
        if keyboard_hook.is_null() {
            *active = None;
            return Err(StartError::HookCreationFailed);
        }
        let mouse_hook =
            unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_callback), null_mut(), 0) };
        if mouse_hook.is_null() {
            unsafe { UnhookWindowsHookEx(keyboard_hook) };
            *active = None;
            return Err(StartError::HookCreationFailed);
        }
        drop(active);
        Ok(Self {
            keyboard_hook,
            mouse_hook,
            state,
        })
    }

    pub fn status(&self) -> KeyboardHookStatus {
        self.state
            .lock()
            .map(|state| state.status)
            .unwrap_or(KeyboardHookStatus::Failed)
    }

    /// Applies new settings to the one live hook.
    ///
    /// This is what makes a recorded shortcut live without a restart, and it
    /// replaces rather than adds: `SetWindowsHookExW` is never called a second
    /// time, so no listener is left behind still matching the combination that
    /// was recorded over.
    pub fn reconfigure(&self, document: SettingsDocument) {
        if let Ok(mut state) = self.state.lock() {
            state.composer.reconfigure(document.vietnamese.to_config());
            state.switch.adopt(&document);
            state.pressed.clear();
            state.target = None;
        }
    }
}

impl Drop for KeyboardHook {
    fn drop(&mut self) {
        // No retry loop: an unhook failure makes status honest, but state is
        // still detached so a later callback cannot transform anything.
        let keyboard_unhooked = unsafe { UnhookWindowsHookEx(self.keyboard_hook) } != 0;
        let mouse_unhooked = unsafe { UnhookWindowsHookEx(self.mouse_hook) } != 0;
        if !keyboard_unhooked || !mouse_unhooked {
            if let Ok(mut state) = self.state.lock() {
                state.status = KeyboardHookStatus::Failed;
            }
        }
        if let Ok(mut active) = active_hook().lock()
            && active
                .as_ref()
                .is_some_and(|current| Arc::ptr_eq(current, &self.state))
        {
            *active = None;
        }
    }
}

fn active_hook() -> &'static Mutex<Option<Arc<Mutex<State>>>> {
    static ACTIVE: OnceLock<Mutex<Option<Arc<Mutex<State>>>>> = OnceLock::new();
    ACTIVE.get_or_init(|| Mutex::new(None))
}

unsafe extern "system" fn callback(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code != HC_ACTION as i32 || lparam == 0 {
        return unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) };
    }
    let event = unsafe { (lparam as *const KBDLLHOOKSTRUCT).as_ref() };
    let Some(event) = event else {
        return unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) };
    };
    // Own output and all other injected input pass before either callback lock.
    if event.flags & LLKHF_INJECTED != 0 || event.dwExtraInfo == SYNTHETIC_EVENT_TAG {
        return unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) };
    }

    let Ok(active) = active_hook().try_lock() else {
        return unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) };
    };
    let Some(state) = active.as_ref() else {
        return unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) };
    };
    let Ok(mut state) = state.try_lock() else {
        return unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) };
    };

    if wparam == WM_KEYUP as usize {
        state.pressed.remove(&event.vkCode);
        let route = handling(HookEvent::KeyUp {
            suppress: state.suppressed_key_ups.take(event.vkCode),
        });
        return if route == Handling::Suppress {
            1
        } else {
            unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) }
        };
    }
    if wparam != WM_KEYDOWN as usize {
        return unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) };
    }
    let repeat = !state.pressed.insert(event.vkCode);
    if repeat {
        // This physical down reaches the application, so its eventual up must.
        state.suppressed_key_ups.allow(event.vkCode);
    }
    // Windows toggles caps lock on key-down, and this callback is the only place
    // that reliably sees the press — but it toggles once per press, not once per
    // autorepeat. See `models::keyboard_hook::CapsLock`.
    if !repeat {
        state.caps.observe_key_down(event.vkCode);
    }
    let caps_lock = state.caps.on();
    let window = foreground_window();
    let Some(key) = normalized_key(event, window.map(|(_, thread)| thread), caps_lock) else {
        state.composer.reset();
        target_changed(&mut state.target, None);
        return unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) };
    };
    // The shortcut is answered before the composer sees anything, before the
    // selected language is consulted, and before anything asks where text would
    // go — that ordering is what lets it switch *out* of a language with no
    // engine as well as into one, and what keeps it working in a window with no
    // focused edit control. A repeat is ignored so a held shortcut cycles once.
    let cycled = (!repeat).then(|| state.switch.cycle(&key)).flatten();
    if let Some(cycled) = cycled {
        state.composer.reset();
        let _ = state.language_changes.unbounded_send(cycled.language);
        if cycled.beep {
            // `MB_OK` is the system default sound.
            unsafe { MessageBeep(MB_OK) };
        }
        // A modifier-only shortcut must still reach applications, or every one
        // of them believes the key is held. Only a real key is swallowed.
        if key.key == Key::Modifier {
            return unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) };
        }
        state.suppressed_key_ups.suppress(event.vkCode);
        return 1;
    }
    // From here the key is text, so where it would land has to be known.
    let Some(target) = window.and_then(|(foreground, thread)| target_identity(foreground, thread))
    else {
        state.composer.reset();
        target_changed(&mut state.target, None);
        return unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) };
    };
    if target_changed(&mut state.target, Some(target)) {
        state.composer.reset();
    }
    if !state.switch.transforms() {
        state.composer.reset();
        return unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) };
    }
    if key.key == Key::Other || key.key == Key::Modifier {
        state.composer.reset();
        return unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) };
    }
    let route = handling(HookEvent::KeyDown {
        injected: false,
        repeat,
        shortcut: !key.modifiers.is_plain(),
        text_is_known: true,
    });
    if route != Handling::Process {
        state.composer.reset();
        return unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) };
    }

    // Plan against a copy. The retained text is adopted only after Windows
    // accepts the complete staged event array.
    let mut next = state.composer.clone();
    let plan = next.process(key);
    if !plan.transforms() {
        state.composer = next;
        return unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) };
    }
    let (sent, requested) = send_output(&plan);
    if !adopt_after_send(&mut state.composer, next, sent, requested) {
        state.suppressed_key_ups.allow(event.vkCode);
        return unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) };
    }
    if plan.pass_through {
        state.suppressed_key_ups.allow(event.vkCode);
        unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) }
    } else {
        state.suppressed_key_ups.suppress(event.vkCode);
        1
    }
}

/// Any mouse button can move a caret inside the same focused control. There is
/// no cheap cross-application caret position contract, so forget rather than
/// applying a later rewrite at a guessed end cursor.
unsafe extern "system" fn mouse_callback(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code != HC_ACTION as i32
        || lparam == 0
        || !matches!(
            wparam as u32,
            WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_XBUTTONDOWN
        )
    {
        return unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) };
    }
    let Ok(active) = active_hook().try_lock() else {
        return unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) };
    };
    let Some(state) = active.as_ref() else {
        return unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) };
    };
    let Ok(mut state) = state.try_lock() else {
        return unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) };
    };
    state.composer.reset();
    state.target = None;
    unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) }
}

/// The foreground window and its thread, when Windows names both.
fn foreground_window() -> Option<(usize, u32)> {
    let foreground = unsafe { GetForegroundWindow() };
    if foreground.is_null() {
        return None;
    }
    let thread = unsafe { GetWindowThreadProcessId(foreground, null_mut()) };
    (thread != 0).then_some((foreground as usize, thread))
}

/// The focused control retained text belongs to, when there is one.
///
/// Only the composing path needs this. Requiring it before the language switch
/// was matched made the shortcut depend on there being somewhere to type.
fn target_identity(foreground: usize, thread: u32) -> Option<TargetIdentity> {
    let mut gui = GUITHREADINFO {
        cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
        ..GUITHREADINFO::default()
    };
    if unsafe { GetGUIThreadInfo(thread, &mut gui) } == 0 || gui.hwndFocus.is_null() {
        return None;
    }
    Some(TargetIdentity::new(
        foreground,
        thread,
        gui.hwndFocus as usize,
        gui.hwndCaret as usize,
    ))
}

/// The physical keyboard, as opposed to what dodo's own message queue remembers.
///
/// `GetAsyncKeyState`'s high bit is "held", and it is not synchronised to any
/// thread's queue — which is the whole reason it is asked here. The arriving key
/// folds itself in because a low-level hook runs before Windows records it.
fn physical_keys(vkey: u32, caps_lock: bool) -> PhysicalKeys {
    let held = |key: u32| (unsafe { GetAsyncKeyState(key as i32) }) < 0;
    with_key_down(
        PhysicalKeys {
            left_shift: held(vk::LSHIFT),
            right_shift: held(vk::RSHIFT),
            left_control: held(vk::LCONTROL),
            right_control: held(vk::RCONTROL),
            left_alt: held(vk::LMENU),
            right_alt: held(vk::RMENU),
            left_windows: held(vk::LWIN),
            right_windows: held(vk::RWIN),
            caps_lock,
        },
        vkey,
    )
}

/// Maps a Windows event only when its foreground layout supplies exactly one
/// printable character or it is a well-known editing/navigation key.
///
/// The character and the modifier flags come from one `PhysicalKeys` read, so
/// they cannot disagree about Shift — a `shift` flag beside a lowercase letter
/// is the same defect as the reverse.
fn normalized_key(
    event: &KBDLLHOOKSTRUCT,
    layout_thread: Option<u32>,
    caps_lock: bool,
) -> Option<dodo_ime_core::KeyEvent> {
    let physical = physical_keys(event.vkCode, caps_lock);
    let keyboard = layout_state(event.vkCode, physical);
    // The layout of the application being typed into, not dodo's: a Dvorak or
    // French user's `w` has to be their `w`. Zero asks for this thread's own,
    // which is the best remaining answer when there is no foreground window.
    let layout = unsafe { GetKeyboardLayout(layout_thread.unwrap_or(0)) };
    let mut units = [0_u16; 4];
    let count = if layout.is_null() {
        0
    } else {
        unsafe {
            // Windows 10's no-state-change flag avoids consuming a dead-key
            // state.
            ToUnicodeEx(
                event.vkCode,
                event.scanCode,
                keyboard.as_ptr(),
                units.as_mut_ptr(),
                units.len() as i32,
                0x4,
                layout,
            )
        }
    };
    let text = match count {
        0 => None,
        1 => one_character(&units[..1]),
        // A dead key or a ligature has no single-character reading, so the key
        // goes back to the application untouched.
        _ => return None,
    };
    Some(windows_key_event(
        event.vkCode,
        text,
        physical_modifiers(physical),
    ))
}

fn one_character(units: &[u16]) -> Option<char> {
    let text = std::char::decode_utf16(units.iter().copied())
        .collect::<Result<String, _>>()
        .ok()?;
    let mut characters = text.chars();
    let character = characters.next()?;
    (characters.next().is_none() && !character.is_control()).then_some(character)
}

fn send_output(plan: &OutputPlan) -> (usize, usize) {
    let mut events = Vec::with_capacity(input_event_count(plan));
    for _ in 0..plan.delete_before {
        events.push(key_input(0x08, 0, 0));
        events.push(key_input(0x08, 0, KEYEVENTF_KEYUP));
    }
    if let Some(text) = &plan.insert {
        for unit in text.encode_utf16() {
            events.push(key_input(0, unit, KEYEVENTF_UNICODE));
            events.push(key_input(0, unit, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP));
        }
    }
    let requested = events.len();
    let Ok(requested_u32) = u32::try_from(requested) else {
        return (0, requested);
    };
    let sent = if requested == 0 {
        0
    } else {
        unsafe {
            SendInput(
                requested_u32,
                events.as_ptr(),
                std::mem::size_of::<INPUT>() as i32,
            ) as usize
        }
    };
    (sent, requested)
}

fn key_input(vk: u16, scan: u16, flags: u32) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: scan,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: SYNTHETIC_EVENT_TAG,
            },
        },
    }
}
