//! TSF's COM callbacks adapted to one `VietnameseEngine` per text service.
//!
//! The only edit path is a synchronous TSF composition edit session. There is
//! no keyboard injection in this DLL: if TSF cannot give a writable context, it
//! returns the key unchanged. Settings are re-read before each decision so a
//! newly selected Keyboard Hook makes this host pass through before dodo starts
//! that fallback; only configuration metadata is read, never a keystroke.

use std::cell::RefCell;
use std::mem::ManuallyDrop;
use std::rc::Rc;

use dodo_ime_core::{
    EngineAction, LanguageEngine as _, LanguageId, OutputMode, VietnameseConfig, VietnameseEngine,
};
use dodo_ime_ipc::paths;
use dodo_ime_ipc::settings::{Backend, SETTINGS_FILE, SettingsDocument};
use windows::Win32::Foundation::{BOOL, E_FAIL, LPARAM, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyboardLayout, GetKeyboardState, ToUnicodeEx, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN,
    VK_SHIFT,
};
use windows::Win32::UI::TextServices::{
    ITfComposition, ITfCompositionSink, ITfContext, ITfContextComposition, ITfEditSession,
    ITfEditSession_Impl, ITfKeyEventSink, ITfKeyEventSink_Impl, ITfKeystrokeMgr,
    ITfTextInputProcessor, ITfTextInputProcessor_Impl, ITfThreadMgr, TF_CONTEXT_EDIT_CONTEXT_FLAGS,
    TF_DEFAULT_SELECTION, TF_ES_READWRITE, TF_ES_SYNC, TF_SELECTION, TS_SD_READONLY,
};
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
use windows::core::{Error, Interface, Result, implement};

use crate::keymap;

/// A TSF processor remains in one COM apartment. `RefCell` expresses that and
/// avoids pretending the per-context composition may cross threads safely.
#[implement(ITfTextInputProcessor, ITfKeyEventSink)]
pub struct TextService {
    state: RefCell<State>,
}

#[derive(Clone)]
struct State {
    manager: Option<ITfThreadMgr>,
    client_id: u32,
    engine: Option<VietnameseEngine>,
    composition: Option<ITfComposition>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            manager: None,
            client_id: 0,
            engine: None,
            composition: None,
        }
    }
}

impl TextService {
    pub fn new() -> Self {
        Self {
            state: RefCell::new(State::default()),
        }
    }

    /// Load only when this host owns the selected backend. Missing/refused
    /// settings intentionally produce English pass-through, never Telex beside
    /// dodo's Keyboard Hook.
    fn configured_engine() -> Option<VietnameseEngine> {
        let directory = paths::support_dir_from_env()?;
        let (document, _) = SettingsDocument::read_or_default(&directory.join(SETTINGS_FILE));
        (document.backend == Backend::Native && document.language == LanguageId::Vietnamese).then(
            || {
                let mut config: VietnameseConfig = document.vietnamese.to_config();
                config.output = OutputMode::Composition;
                VietnameseEngine::new(config)
            },
        )
    }

    fn input_event(vkey: u32, lparam: LPARAM) -> Option<dodo_ime_core::KeyEvent> {
        // A held key is a repeat. The application owns repeats unchanged; this
        // prevents a single physical key from being rewritten more than once.
        if (lparam.0 as usize) & (1 << 30) != 0 {
            return None;
        }

        let foreground = unsafe { GetForegroundWindow() };
        if foreground.0 == 0 {
            return None;
        }
        let thread = unsafe { GetWindowThreadProcessId(foreground, None) };
        if thread == 0 {
            return None;
        }

        let mut keyboard = [0_u8; 256];
        if unsafe { GetKeyboardState(&mut keyboard) }.is_err() {
            return None;
        }
        let index = usize::try_from(vkey).ok()?;
        let state = keyboard.get_mut(index)?;
        *state |= 0x80; // callback arrives before this key is committed to state.

        let layout = unsafe { GetKeyboardLayout(thread) };
        if layout.0 == 0 {
            return None;
        }
        let mut units = [0_u16; 4];
        // 0x4 is TO_UNICODE_NO_STATE_CHANGE. Dead keys and ligatures are not a
        // single character and therefore return to the application untouched.
        let count = unsafe { ToUnicodeEx(vkey, 0, &keyboard, &mut units, 0x4, layout) };
        let text = match count {
            0 => None,
            1 => Some(keymap::one_character(&units[..1])?),
            _ => return None,
        };
        let active = |vk: u32| {
            keyboard
                .get(vk as usize)
                .is_some_and(|byte| byte & 0x80 != 0)
        };
        let modifiers = dodo_ime_core::Modifiers {
            shift: active(VK_SHIFT.0 as u32),
            control: active(VK_CONTROL.0 as u32),
            alt: active(VK_MENU.0 as u32),
            meta: active(VK_LWIN.0 as u32) || active(VK_RWIN.0 as u32),
        };
        Some(keymap::key_event(vkey, text, modifiers))
    }

    fn writable(context: &ITfContext) -> bool {
        // TSF exposes readonly contexts. It does not send a separate password
        // bit to a key sink; password-aware text stores deactivate the service.
        unsafe { context.GetStatus() }
            .map(|status| status.dwStaticFlags & TS_SD_READONLY == 0)
            .unwrap_or(false)
    }

    fn action_changes_text(actions: &[EngineAction]) -> bool {
        actions.iter().any(|action| {
            matches!(
                action,
                EngineAction::SetComposition { .. }
                    | EngineAction::CommitComposition
                    | EngineAction::ClearComposition
            )
        })
    }

    fn prepare(
        &self,
        context: &ITfContext,
        vkey: u32,
        lparam: LPARAM,
    ) -> Option<(State, Vec<EngineAction>, bool)> {
        if !Self::writable(context) {
            return None;
        }
        let event = Self::input_event(vkey, lparam)?;
        let mut next = self.state.borrow().clone();
        next.engine = Self::configured_engine();
        let result = next.engine.as_mut()?.process_key(&event);
        Self::action_changes_text(&result.actions).then_some((next, result.actions, result.handled))
    }

    fn apply(&self, context: &ITfContext, vkey: u32, lparam: LPARAM) -> Result<BOOL> {
        let Some((next, actions, handled)) = self.prepare(context, vkey, lparam) else {
            return Ok(BOOL(0));
        };
        let shared = Rc::new(RefCell::new(next));
        let edit: ITfEditSession = ApplyEdit {
            context: context.clone(),
            actions,
            state: shared.clone(),
        }
        .into();
        let client_id = self.state.borrow().client_id;
        let requested = unsafe {
            context.RequestEditSession(
                client_id,
                &edit,
                TF_CONTEXT_EDIT_CONTEXT_FLAGS(TF_ES_SYNC.0 | TF_ES_READWRITE.0),
            )
        }?;
        if requested.is_ok() {
            *self.state.borrow_mut() = shared.borrow().clone();
            Ok(BOOL(handled as i32))
        } else {
            // The original key is passed on. Forgetting the engine avoids any
            // later rewrite based on text the client declined to edit.
            self.state.borrow_mut().engine = None;
            Ok(BOOL(0))
        }
    }
}

impl ITfTextInputProcessor_Impl for TextService {
    fn Activate(&self, manager: Option<&ITfThreadMgr>, client_id: u32) -> Result<()> {
        let Some(manager) = manager else {
            return Err(Error::from(E_FAIL));
        };
        let sink: ITfKeyEventSink = unsafe { self.cast()? };
        let keystrokes: ITfKeystrokeMgr = manager.cast()?;
        unsafe { keystrokes.AdviseKeyEventSink(client_id, &sink, BOOL(1))? };
        let mut state = self.state.borrow_mut();
        state.manager = Some(manager.clone());
        state.client_id = client_id;
        state.engine = Self::configured_engine();
        Ok(())
    }

    fn Deactivate(&self) -> Result<()> {
        let mut state = self.state.borrow_mut();
        if let Some(manager) = state.manager.take()
            && let Ok(keystrokes) = manager.cast::<ITfKeystrokeMgr>()
        {
            let _ = unsafe { keystrokes.UnadviseKeyEventSink(state.client_id) };
        }
        *state = State::default();
        Ok(())
    }
}

impl ITfKeyEventSink_Impl for TextService {
    fn OnSetFocus(&self, _foreground: BOOL) -> Result<()> {
        // A focus change must not carry an unfinished syllable into another
        // context. TSF owns the composition range and ends it on deactivation.
        self.state.borrow_mut().engine = None;
        Ok(())
    }

    fn OnTestKeyDown(
        &self,
        context: Option<&ITfContext>,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> Result<BOOL> {
        Ok(context
            .and_then(|context| self.prepare(context, wparam.0 as u32, lparam))
            .map_or(BOOL(0), |_| BOOL(1)))
    }

    fn OnTestKeyUp(
        &self,
        _context: Option<&ITfContext>,
        _wparam: WPARAM,
        _lparam: LPARAM,
    ) -> Result<BOOL> {
        Ok(BOOL(0))
    }

    fn OnKeyDown(
        &self,
        context: Option<&ITfContext>,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> Result<BOOL> {
        context.map_or(Ok(BOOL(0)), |context| {
            self.apply(context, wparam.0 as u32, lparam)
        })
    }

    fn OnKeyUp(
        &self,
        _context: Option<&ITfContext>,
        _wparam: WPARAM,
        _lparam: LPARAM,
    ) -> Result<BOOL> {
        Ok(BOOL(0))
    }

    fn OnPreservedKey(
        &self,
        _context: Option<&ITfContext>,
        _guid: *const windows::core::GUID,
    ) -> Result<BOOL> {
        Ok(BOOL(0))
    }
}

#[implement(ITfEditSession)]
struct ApplyEdit {
    context: ITfContext,
    actions: Vec<EngineAction>,
    state: Rc<RefCell<State>>,
}

impl ITfEditSession_Impl for ApplyEdit {
    fn DoEditSession(&self, edit_cookie: u32) -> Result<()> {
        let mut state = self.state.borrow_mut();
        for action in &self.actions {
            match action {
                EngineAction::SetComposition { text, .. } => {
                    let composition = if let Some(composition) = &state.composition {
                        composition.clone()
                    } else {
                        let mut selection = [TF_SELECTION::default()];
                        let mut fetched = 0;
                        unsafe {
                            self.context.GetSelection(
                                edit_cookie,
                                TF_DEFAULT_SELECTION,
                                &mut selection,
                                &mut fetched,
                            )?
                        };
                        let range = unsafe { ManuallyDrop::take(&mut selection[0].range) }
                            .ok_or_else(|| Error::from(E_FAIL))?;
                        let composition_context: ITfContextComposition = self.context.cast()?;
                        let composition = unsafe {
                            composition_context.StartComposition(
                                edit_cookie,
                                &range,
                                None::<&ITfCompositionSink>,
                            )?
                        };
                        state.composition = Some(composition.clone());
                        composition
                    };
                    let range = unsafe { composition.GetRange()? };
                    let text: Vec<u16> = text.encode_utf16().collect();
                    unsafe { range.SetText(edit_cookie, 0, &text)? };
                }
                EngineAction::CommitComposition => {
                    if let Some(composition) = state.composition.take() {
                        unsafe { composition.EndComposition(edit_cookie)? };
                    }
                }
                EngineAction::ClearComposition => {
                    if let Some(composition) = state.composition.take() {
                        let range = unsafe { composition.GetRange()? };
                        unsafe { range.SetText(edit_cookie, 0, &[])? };
                        unsafe { composition.EndComposition(edit_cookie)? };
                    }
                }
                // The TSF host always selects composition mode. A direct action
                // here means a future engine contract changed; refuse instead
                // of trying to inject or approximate text.
                EngineAction::PassThrough => {}
                EngineAction::InsertText(_)
                | EngineAction::DeleteBackward(_)
                | EngineAction::ReplaceBeforeCursor { .. }
                | EngineAction::ShowCandidates
                | EngineAction::HideCandidates => return Err(Error::from(E_FAIL)),
            }
        }
        Ok(())
    }
}
