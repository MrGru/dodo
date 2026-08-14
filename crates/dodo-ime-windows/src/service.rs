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
use dodo_ime_ipc::status::{STATUS_FILE, StatusDocument};
use windows::Win32::Foundation::{BOOL, CloseHandle, E_FAIL, LPARAM, S_OK, WPARAM};
use windows::Win32::System::Diagnostics::Debug::MessageBeep;
use windows::Win32::System::Threading::{EVENT_MODIFY_STATE, OpenEventW, SetEvent};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, GetKeyboardLayout, GetKeyboardState, ToUnicodeEx,
};
use windows::Win32::UI::TextServices::{
    ITfComposition, ITfCompositionSink, ITfContext, ITfContextComposition, ITfEditSession,
    ITfEditSession_Impl, ITfKeyEventSink, ITfKeyEventSink_Impl, ITfKeystrokeMgr,
    ITfTextInputProcessor_Impl, ITfTextInputProcessorEx, ITfTextInputProcessorEx_Impl,
    ITfThreadMgr, TF_CONTEXT_EDIT_CONTEXT_FLAGS, TF_DEFAULT_SELECTION, TF_ES_READWRITE, TF_ES_SYNC,
    TF_SELECTION, TS_SD_READONLY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowThreadProcessId, MB_OK,
};
use windows::core::{Error, Interface, PCWSTR, Result, implement};

use crate::keymap;

/// A TSF processor remains in one COM apartment. `RefCell` expresses that and
/// avoids pretending the per-context composition may cross threads safely.
#[implement(ITfTextInputProcessorEx, ITfKeyEventSink)]
pub struct TextService {
    state: RefCell<State>,
}

#[derive(Clone)]
struct State {
    manager: Option<ITfThreadMgr>,
    client_id: u32,
    engine: Option<VietnameseEngine>,
    composition: Option<ITfComposition>,
    language: LanguageId,
    settings_revision: u64,
}

impl Default for State {
    fn default() -> Self {
        Self {
            manager: None,
            client_id: 0,
            engine: None,
            composition: None,
            language: LanguageId::English,
            // No settings revision is loaded yet, so the first key must adopt
            // even a hand-written revision-0 document.
            settings_revision: u64::MAX,
        }
    }
}

impl TextService {
    pub fn new() -> Self {
        Self {
            state: RefCell::new(State::default()),
        }
    }

    /// Reads the existing settings once for this key decision. The native host
    /// already did this before every key; keeping the document together avoids
    /// a second hot-path read for language-switch metadata.
    fn configured_document() -> Option<SettingsDocument> {
        let directory = paths::support_dir_from_env()?;
        Some(SettingsDocument::read_or_default(&directory.join(SETTINGS_FILE)).0)
    }

    fn vietnamese_config(document: SettingsDocument) -> VietnameseConfig {
        let mut config = document.vietnamese.to_config();
        config.output = OutputMode::Composition;
        config
    }

    /// Preserve an in-flight syllable while settings stay the same. A changed
    /// revision deliberately adopts dodo's selected language; a shortcut keeps
    /// its local selection until dodo has persisted that command.
    fn refresh_engine(state: &mut State, document: SettingsDocument) -> bool {
        if document.backend != Backend::Native {
            state.engine = None;
            return false;
        }
        if state.settings_revision != document.revision {
            state.language = document.language;
            state.settings_revision = document.revision;
        }
        if state.language != LanguageId::Vietnamese {
            state.engine = None;
            return true;
        }
        let config = Self::vietnamese_config(document);
        if state
            .engine
            .as_ref()
            .is_none_or(|engine| engine.config() != config)
        {
            state.engine = Some(VietnameseEngine::new(config));
        }
        true
    }

    /// Reports only an explicit language command, never ordinary typing.
    fn report_language(language: LanguageId, revision: u64) {
        let Some(directory) = paths::support_dir_from_env() else {
            return;
        };
        let status = StatusDocument::now(env!("CARGO_PKG_VERSION"), revision)
            .with_selected_language(language);
        let _ = status.write(&directory.join(STATUS_FILE));
        Self::signal_dodo();
    }

    /// Wakes dodo's event-driven status reader when its tray process is alive.
    fn signal_dodo() {
        let name = dodo_ime_ipc::WINDOWS_LANGUAGE_CHANGED
            .encode_utf16()
            .chain(Some(0))
            .collect::<Vec<_>>();
        // SAFETY: OpenEventW only opens dodo's current-session event; a missing
        // dodo process is normal and status remains available at its next start.
        let Ok(event) = (unsafe { OpenEventW(EVENT_MODIFY_STATE, false, PCWSTR(name.as_ptr())) })
        else {
            return;
        };
        // SAFETY: event is the valid handle OpenEventW returned above.
        let _ = unsafe { SetEvent(event) };
        // SAFETY: event is still the valid handle OpenEventW returned above.
        let _ = unsafe { CloseHandle(event) };
    }

    /// One key-down in the engine's vocabulary.
    ///
    /// The character and the modifier flags come out of **one** keyboard-state
    /// array, which `keymap::merge_physical` has reconciled with the physical
    /// keyboard first — see its docs for why a snapshot alone is not enough and
    /// what merging can and cannot repair.
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
        keymap::merge_physical(&mut keyboard, vkey, |key| {
            // SAFETY: a virtual key code is the whole argument, and the call has
            // no side effect on any thread's input state.
            (unsafe { GetAsyncKeyState(key as i32) }) < 0
        });
        let modifiers = keymap::state_modifiers(&keyboard);

        let layout = unsafe { GetKeyboardLayout(thread) };
        if layout.0 == 0 {
            return None;
        }
        let mut units = [0_u16; 4];
        // 0x4 is TO_UNICODE_NO_STATE_CHANGE. Dead keys and ligatures are not a
        // single character and therefore return to the application untouched.
        // The scan code is bits 16-23 of `lParam`: `ToUnicodeEx` documents it as
        // a parameter and a zero there is not the same key on every layout.
        let scan_code = ((lparam.0 >> 16) & 0xff) as u32;
        let count = unsafe { ToUnicodeEx(vkey, scan_code, &keyboard, &mut units, 0x4, layout) };
        let text = match count {
            0 => None,
            1 => Some(keymap::one_character(&units[..1])?),
            _ => return None,
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

    /// What one key-down should do, without performing any of it.
    ///
    /// The language switch is answered **before** the context is asked whether
    /// it can be written to. A read-only text store is still a place the user
    /// may change input language from — the shortcut is a command about the
    /// input method, not an edit — and requiring a writable context first made
    /// the switch depend on where the caret happened to be. Only an in-flight
    /// composition has to be ended, and only then is an edit session needed.
    fn prepare(
        &self,
        context: &ITfContext,
        vkey: u32,
        lparam: LPARAM,
    ) -> Option<(State, Vec<EngineAction>, bool, Option<bool>)> {
        let event = Self::input_event(vkey, lparam)?;
        let document = Self::configured_document()?;
        let mut next = self.state.borrow().clone();
        if !Self::refresh_engine(&mut next, document) {
            return None;
        }
        if document.language_switch.matches(&event) {
            next.language = document.active_languages.next(next.language);
            next.engine = (next.language == LanguageId::Vietnamese)
                .then(|| VietnameseEngine::new(Self::vietnamese_config(document)));
            let commit = next.composition.is_some() && Self::writable(context);
            return Some((
                next,
                if commit {
                    vec![EngineAction::CommitComposition]
                } else {
                    Vec::new()
                },
                true,
                Some(document.language_switch.beep),
            ));
        }
        if !Self::writable(context) {
            return None;
        }
        let result = next.engine.as_mut()?.process_key(&event);
        Self::action_changes_text(&result.actions).then_some((
            next,
            result.actions,
            result.handled,
            None,
        ))
    }

    fn apply(&self, context: &ITfContext, vkey: u32, lparam: LPARAM) -> Result<BOOL> {
        let Some((next, actions, handled, switched)) = self.prepare(context, vkey, lparam) else {
            return Ok(BOOL(0));
        };
        let shared = Rc::new(RefCell::new(next));
        // Nothing to edit is not a failure: a language switch with no
        // composition in flight changes no document and must still happen.
        let requested = if actions.is_empty() {
            S_OK
        } else {
            let edit: ITfEditSession = ApplyEdit {
                context: context.clone(),
                actions,
                state: shared.clone(),
            }
            .into();
            let client_id = self.state.borrow().client_id;
            unsafe {
                context.RequestEditSession(
                    client_id,
                    &edit,
                    TF_CONTEXT_EDIT_CONTEXT_FLAGS(TF_ES_SYNC.0 | TF_ES_READWRITE.0),
                )
            }?
        };
        if requested.is_ok() {
            *self.state.borrow_mut() = shared.borrow().clone();
            if let Some(beep) = switched {
                let state = self.state.borrow();
                Self::report_language(state.language, state.settings_revision);
                if beep {
                    // The sound is advisory; a failure to make it is not an
                    // error the user could act on.
                    let _ = unsafe { MessageBeep(MB_OK) };
                }
            }
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
        if let Some(document) = Self::configured_document() {
            let _ = Self::refresh_engine(&mut state, document);
        }
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

impl ITfTextInputProcessorEx_Impl for TextService {
    fn ActivateEx(
        &self,
        manager: Option<&ITfThreadMgr>,
        client_id: u32,
        _flags: u32,
    ) -> Result<()> {
        ITfTextInputProcessor_Impl::Activate(self, manager, client_id)
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
