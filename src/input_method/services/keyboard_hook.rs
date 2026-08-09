//! Windows' `WH_KEYBOARD_LL` fallback for the Vietnamese engine.
//!
//! It is installed only while dodo owns the Keyboard Hook backend. The callback
//! is intentionally smaller than the policy around it: injected, repeated,
//! shortcut, key-up, secure-desktop-uncertain, and untranslatable input all go
//! unchanged to the next hook. It never logs, persists, transmits, or exposes a
//! keystroke.

use std::collections::HashSet;
use std::ptr::null_mut;
use std::sync::{Arc, Mutex, OnceLock};

use dodo_ime_core::{Key, LanguageEngine as _, Modifiers, VietnameseConfig, VietnameseEngine};
use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyboardLayout, GetKeyboardState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT,
    KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, SendInput, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetForegroundWindow, GetWindowThreadProcessId, HC_ACTION, HHOOK,
    KBDLLHOOKSTRUCT, LLKHF_INJECTED, SetWindowsHookExW, UnhookWindowsHookEx, WH_KEYBOARD_LL,
    WM_KEYDOWN, WM_KEYUP,
};

use crate::input_method::models::direct_output::OutputPlan;
use crate::input_method::models::keyboard_hook::{
    Handling, HookEvent, KeyboardHookStatus, handling,
};

const SYNTHETIC_EVENT_TAG: usize = 0x444f_444f_5748_4f4f;

/// Why the hook could not start. It carries no input data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartError {
    AlreadyRunning,
    HookCreationFailed,
}

/// A live global hook. Dropping it always unregisters before callback state can
/// be freed, so backend shutdown cannot leave a key observer behind.
pub struct KeyboardHook {
    hook: HHOOK,
    state: Arc<Mutex<State>>,
}

struct State {
    engine: VietnameseEngine,
    pressed: HashSet<u32>,
    status: KeyboardHookStatus,
}

impl KeyboardHook {
    pub fn start(config: VietnameseConfig) -> Result<Self, StartError> {
        let state = Arc::new(Mutex::new(State {
            engine: VietnameseEngine::new(direct_config(config)),
            pressed: HashSet::new(),
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
        let hook = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(callback), null_mut(), 0) };
        if hook.is_null() {
            *active = None;
            return Err(StartError::HookCreationFailed);
        }
        drop(active);
        Ok(Self { hook, state })
    }

    pub fn status(&self) -> KeyboardHookStatus {
        self.state
            .lock()
            .map(|state| state.status)
            .unwrap_or(KeyboardHookStatus::Failed)
    }

    /// Applies a setting after the hook was deliberately stopped for its write.
    /// This method remains fail-safe if a caller later reuses a live hook.
    pub fn reconfigure(&self, config: VietnameseConfig) {
        if let Ok(mut state) = self.state.lock() {
            state.engine = VietnameseEngine::new(direct_config(config));
            state.pressed.clear();
        }
    }
}

impl Drop for KeyboardHook {
    fn drop(&mut self) {
        // No retry loop: an unhook failure makes status honest, but state is
        // still detached so a later callback cannot transform anything.
        if unsafe { UnhookWindowsHookEx(self.hook) } == 0 {
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

fn direct_config(mut config: VietnameseConfig) -> VietnameseConfig {
    config.output = dodo_ime_core::OutputMode::Direct;
    config
}

unsafe extern "system" fn callback(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code != HC_ACTION as i32 || lparam == 0 {
        return unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) };
    }
    let event = unsafe { (lparam as *const KBDLLHOOKSTRUCT).as_ref() };
    let Some(event) = event else {
        return unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) };
    };
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
        return unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) };
    }
    if wparam != WM_KEYDOWN as usize {
        return unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) };
    }
    let repeat = !state.pressed.insert(event.vkCode);
    let Some(key) = key_event(event) else {
        let _ = state.engine.reset();
        return unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) };
    };
    if key.key == Key::Other {
        let _ = state.engine.reset();
        return unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) };
    }
    let route = handling(HookEvent::KeyDown {
        injected: false,
        repeat,
        shortcut: !key.modifiers.is_plain(),
        text_is_known: true,
    });
    if route != Handling::Process {
        if !repeat {
            let _ = state.engine.reset();
        }
        return unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) };
    }

    let result = state.engine.process_key(&key);
    let Some(plan) = OutputPlan::from_actions(&result.actions) else {
        let _ = state.engine.reset();
        return unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) };
    };
    if !plan.transforms() {
        return unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) };
    }
    if !send_output(&plan) {
        let _ = state.engine.reset();
        return unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) };
    }
    if plan.pass_through {
        unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) }
    } else {
        1
    }
}

/// Maps a Windows event only when its foreground layout supplies exactly one
/// printable character or it is a well-known editing/navigation key.
fn key_event(event: &KBDLLHOOKSTRUCT) -> Option<dodo_ime_core::KeyEvent> {
    let foreground = unsafe { GetForegroundWindow() };
    if foreground.is_null() {
        return None;
    }
    let thread = unsafe { GetWindowThreadProcessId(foreground, null_mut()) };
    if thread == 0 {
        return None;
    }
    let mut keyboard = [0_u8; 256];
    if unsafe { GetKeyboardState(keyboard.as_mut_ptr()) } == 0 {
        return None;
    }
    let current = keyboard.get_mut(event.vkCode as usize)?;
    *current |= 0x80;
    let layout = unsafe { GetKeyboardLayout(thread) };
    if layout.is_null() {
        return None;
    }
    let mut units = [0_u16; 4];
    let count = unsafe {
        // Windows 10's no-state-change flag avoids consuming a dead-key state.
        windows_sys::Win32::UI::Input::KeyboardAndMouse::ToUnicodeEx(
            event.vkCode,
            event.scanCode,
            keyboard.as_ptr(),
            units.as_mut_ptr(),
            units.len() as i32,
            0x4,
            layout,
        )
    };
    let text = match count {
        0 => None,
        1 => one_character(&units[..1]),
        _ => return None,
    };
    let active = |vk: u16| keyboard[vk as usize] & 0x80 != 0;
    let modifiers = Modifiers {
        shift: active(VK_SHIFT),
        control: active(VK_CONTROL),
        alt: active(VK_MENU),
        meta: active(VK_LWIN) || active(VK_RWIN),
    };
    Some(windows_key_event(event.vkCode, text, modifiers))
}

fn one_character(units: &[u16]) -> Option<char> {
    let text = std::char::decode_utf16(units.iter().copied())
        .collect::<Result<String, _>>()
        .ok()?;
    let mut characters = text.chars();
    let character = characters.next()?;
    (characters.next().is_none() && !character.is_control()).then_some(character)
}

fn windows_key_event(
    vkey: u32,
    text: Option<char>,
    modifiers: Modifiers,
) -> dodo_ime_core::KeyEvent {
    let key = match vkey {
        0x08 => Key::Backspace,
        0x09 => Key::Tab,
        0x0d => Key::Enter,
        0x1b => Key::Escape,
        0x20 => Key::Space,
        0x21 => Key::PageUp,
        0x22 => Key::PageDown,
        0x23 => Key::End,
        0x24 => Key::Home,
        0x25 => Key::ArrowLeft,
        0x26 => Key::ArrowUp,
        0x27 => Key::ArrowRight,
        0x28 => Key::ArrowDown,
        0x2e => Key::Delete,
        _ if text.is_some() => Key::Character,
        _ => Key::Other,
    };
    dodo_ime_core::KeyEvent {
        key,
        text: if key == Key::Space {
            Some(' ')
        } else if key == Key::Character {
            text
        } else {
            None
        },
        modifiers,
    }
}

fn send_output(plan: &OutputPlan) -> bool {
    let mut events = Vec::with_capacity(plan.delete_before.saturating_mul(2) + 2);
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
    !events.is_empty()
        && unsafe {
            SendInput(
                events.len() as u32,
                events.as_ptr(),
                std::mem::size_of::<INPUT>() as i32,
            ) == events.len() as u32
        }
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
