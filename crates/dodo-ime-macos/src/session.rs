//! One input session: an engine, the preedit it is showing, and the four
//! moments macOS can interrupt it.
//!
//! `IMKInputController` is instantiated **once per input session** — roughly
//! once per text field — so this is per-controller state and never a global.
//! That is not an optimisation: two text fields being composed independently is
//! the normal case, and a shared engine would leak half of one field's syllable
//! into the other.
//!
//! # The four entry points, and why the last three exist at all
//!
//! [`Session::key`] is the interesting one. The other three are the protocol
//! telling the input method that the world moved underneath it, and an engine
//! that assumes it only ever commits on its own schedule drops characters:
//!
//! - [`Session::commit`] — `commitComposition:`. **A client can send this at any
//!   time**, and Chrome does: the investigation caught it arriving unprompted
//!   right after a `setMarkedText:`, when focus moved between browser chrome and
//!   web content. Whatever is composed becomes real text now.
//! - [`Session::deactivate`] — `deactivateServer:`. The session is over. Commit,
//!   then forget everything. This is also **how macOS tells an input method
//!   about a password field**: focus a secure field and the sequence is
//!   `activateServer:` → `commitComposition:` → `deactivateServer:`, with the
//!   keystrokes never arriving. Honouring it is the whole difference between
//!   being blind-but-told and blind-and-silent.
//! - [`Session::activate`] — `activateServer:`. A fresh field. Anything left
//!   over belongs to a document this session cannot see any more, so it is
//!   dropped rather than committed: inserting it here would type one field's
//!   text into another.
//!
//! # `handled` is not the engine's word for it
//!
//! [`Response::handled`] is what `inputText:…` returns, and returning `YES` for
//! a key that produced no client message loses that keystroke outright — the
//! application is told the input method dealt with it, and nothing did. So the
//! host adds a guard the engine cannot: a claimed key with an empty op list is
//! handed back. See [`Response::for_ops`].

use dodo_ime_core::{EngineResult, KeyEvent, LanguageEngine, VietnameseConfig, VietnameseEngine};

use crate::ops::{ClientOp, Pending, translate};

/// What one call into a [`Session`] wants done.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Response {
    /// The client messages, in order.
    pub ops: Vec<ClientOp>,
    /// What `-[IMKInputController inputText:key:modifiers:client:]` should
    /// return. `false` means the application types the key itself.
    pub handled: bool,
}

impl Response {
    /// Wrap an op list, deciding `handled` so it can never claim a key it did
    /// nothing with.
    ///
    /// Two conditions, and the second is the host's own:
    ///
    /// - the engine said it handled the key (no [`ClientOp::PassThrough`] among
    ///   the actions it returned), and
    /// - the translation produced at least one message.
    ///
    /// The second matters because an action list can translate to nothing —
    /// today only a bare `ShowCandidates`, which Vietnamese never emits, but
    /// "unreachable" inside a key handler is exactly where a swallowed keystroke
    /// hides. Nothing was performed, so handing the key back is safe.
    pub fn for_ops(ops: Vec<ClientOp>, engine_handled: bool) -> Response {
        Response {
            handled: engine_handled && !ops.is_empty(),
            ops,
        }
    }

    /// A response that asks for nothing and lets the application have the key.
    pub fn unhandled() -> Response {
        Response::default()
    }
}

/// One `IMKInputController`'s worth of state.
#[derive(Debug)]
pub struct Session {
    engine: VietnameseEngine,
    pending: Pending,
}

impl Session {
    pub fn new(config: VietnameseConfig) -> Session {
        Session {
            engine: VietnameseEngine::new(config),
            pending: Pending::new(),
        }
    }

    /// The text the client is currently showing as marked. Empty when nothing
    /// is being composed.
    pub fn pending(&self) -> &str {
        self.pending.text()
    }

    /// One keystroke.
    pub fn key(&mut self, event: &KeyEvent) -> Response {
        let result = self.engine.process_key(event);
        self.respond(result)
    }

    /// `commitComposition:` — accept whatever is in flight, right now.
    ///
    /// Always safe to call, including when nothing is composed: the engine
    /// returns no actions and the translation produces none.
    pub fn commit(&mut self) -> Response {
        let result = self.engine.commit();
        self.respond(result)
    }

    /// `deactivateServer:` — commit, then forget.
    ///
    /// The commit comes first because the user's half-typed syllable is theirs;
    /// the forgetting is unconditional because after this the engine must hold
    /// no record of what was typed, whatever the client did with the text.
    pub fn deactivate(&mut self) -> Response {
        let response = self.commit();
        self.forget();
        response
    }

    /// `activateServer:` — a new field, and nothing carries over into it.
    ///
    /// Deliberately *not* a commit. Anything still pending belongs to a document
    /// this session can no longer address, and inserting it into the newly
    /// focused one would put a syllable somewhere the user never typed it.
    pub fn activate(&mut self) -> Response {
        self.forget();
        Response::unhandled()
    }

    /// Drop every trace of what was being composed, asking the client for
    /// nothing.
    fn forget(&mut self) {
        let _ = self.engine.reset();
        self.pending.clear();
    }

    fn respond(&mut self, result: EngineResult) -> Response {
        let ops = translate(&result.actions, &mut self.pending);
        Response::for_ops(ops, result.handled)
    }
}

#[cfg(test)]
mod tests {
    use super::{ClientOp, Response, Session};
    use crate::DEFAULT_CONFIG;
    use crate::keymap::key_event;
    use dodo_ime_core::{Key, KeyEvent};

    /// A pretend client, so a key sequence can be asserted on as text.
    ///
    /// The counterpart of `dodo_ime_core::testing::Host`, one layer down: this
    /// one performs [`ClientOp`]s, so it is also the only thing that checks a
    /// commit's two calls arrive in the right order.
    #[derive(Default)]
    struct Client {
        document: String,
        marked: String,
        /// UTF-16 offset of the caret within `marked`.
        marked_caret: usize,
    }

    impl Client {
        fn perform(&mut self, ops: &[ClientOp], typed: Option<char>) {
            for op in ops {
                match op {
                    ClientOp::SetMarked { text, selection } => {
                        self.marked = text.clone();
                        self.marked_caret = selection.0;
                    }
                    ClientOp::ClearMarked => {
                        self.marked.clear();
                        self.marked_caret = 0;
                    }
                    ClientOp::Insert(text) => self.document.push_str(text),
                    ClientOp::ReplaceBefore { graphemes, text } => {
                        let keep = crate::text::grapheme_prefix(
                            &self.document,
                            dodo_ime_core::core::grapheme_count(&self.document) - graphemes,
                        )
                        .to_string();
                        self.document = keep + text;
                    }
                    ClientOp::PassThrough => {
                        if let Some(character) = typed {
                            self.document.push(character);
                        }
                    }
                }
            }
        }

        fn visible(&self) -> String {
            format!("{}{}", self.document, self.marked)
        }
    }

    /// Type `keys` as if each were a plain letter press, and return everything
    /// on screen.
    fn type_keys(session: &mut Session, keys: &str) -> (Client, Vec<bool>) {
        let mut client = Client::default();
        let mut handled = Vec::new();
        for character in keys.chars() {
            let event = KeyEvent::character(character);
            let response = session.key(&event);
            client.perform(&response.ops, event.typed());
            handled.push(response.handled);
        }
        (client, handled)
    }

    fn session() -> Session {
        Session::new(DEFAULT_CONFIG)
    }

    #[test]
    fn a_syllable_composes_and_never_touches_the_document_until_it_ends() {
        let mut session = session();
        let (client, _) = type_keys(&mut session, "tieengs");
        assert_eq!(client.marked, "tiếng");
        assert_eq!(client.document, "", "nothing is committed mid-syllable");
        assert_eq!(client.visible(), "tiếng");
        assert_eq!(session.pending(), "tiếng");
    }

    #[test]
    fn a_space_commits_the_syllable_and_still_reaches_the_application() {
        let mut session = session();
        let (client, handled) = type_keys(&mut session, "tieengs ");
        assert_eq!(client.document, "tiếng ");
        assert_eq!(client.marked, "");
        assert_eq!(
            handled.last(),
            Some(&false),
            "the space itself belongs to the application"
        );
        assert_eq!(session.pending(), "");
    }

    /// The caret sits after the whole preedit, in the client's own unit.
    #[test]
    fn the_caret_follows_the_composition() {
        let mut session = session();
        let (client, _) = type_keys(&mut session, "dduowngf");
        assert_eq!(client.marked, "đường");
        assert_eq!(client.marked_caret, 5);
    }

    /// The Chrome case: a commit arriving from the client mid-syllable.
    #[test]
    fn a_client_can_force_a_commit_at_any_time() {
        let mut session = session();
        let (mut client, _) = type_keys(&mut session, "tieengs");
        assert_eq!(client.document, "");

        let response = session.commit();
        client.perform(&response.ops, None);
        assert_eq!(client.document, "tiếng");
        assert_eq!(client.marked, "");
        assert_eq!(session.pending(), "");
    }

    /// Committing twice must not type the syllable twice.
    #[test]
    fn a_second_commit_inserts_nothing() {
        let mut session = session();
        let (mut client, _) = type_keys(&mut session, "vieet");
        client.perform(&session.commit().ops, None);
        let first = client.document.clone();
        client.perform(&session.commit().ops, None);
        assert_eq!(client.document, first);
    }

    /// The password-field sequence, exactly as the investigation logged it.
    #[test]
    fn deactivation_commits_and_then_forgets() {
        let mut session = session();
        let (mut client, _) = type_keys(&mut session, "chaof");
        assert_eq!(client.marked, "chào");

        let response = session.deactivate();
        client.perform(&response.ops, None);
        assert_eq!(client.document, "chào");
        assert_eq!(
            session.pending(),
            "",
            "nothing about the syllable may survive deactivation"
        );

        // And the session is genuinely empty afterwards, not merely flushed.
        let response = session.commit();
        assert!(response.ops.is_empty());
    }

    /// Activation is the other half: a leftover syllable belongs to a document
    /// this session can no longer address.
    #[test]
    fn activation_drops_anything_left_over_rather_than_typing_it_somewhere_new() {
        let mut session = session();
        type_keys(&mut session, "tieengs");
        assert_eq!(session.pending(), "tiếng");

        let response = session.activate();
        assert!(response.ops.is_empty(), "a new field is asked for nothing");
        assert!(!response.handled);
        assert_eq!(session.pending(), "");
    }

    /// The rule the whole crate is written around, at the one seam where the
    /// host could break it: no key that types something is ever claimed without
    /// something being done about it.
    #[test]
    fn no_keystroke_is_ever_swallowed() {
        let mut session = session();
        let mut client = Client::default();

        // A mixture of Vietnamese, English, punctuation and digits.
        for character in "Xin chaof, dodo v1.0! nheenj ".chars() {
            let event = KeyEvent::character(character);
            let response = session.key(&event);
            assert!(
                !response.handled || !response.ops.is_empty(),
                "a claimed key must have produced at least one client message"
            );
            client.perform(&response.ops, event.typed());
        }
        client.perform(&session.commit().ops, None);

        assert_eq!(client.visible(), "Xin chào, dodo v1.0! nhện ");
    }

    /// `handled` is derived, and the derivation is what stops a keystroke from
    /// being claimed by an action list that translated to nothing.
    #[test]
    fn a_claimed_key_with_nothing_to_do_is_handed_back() {
        assert!(!Response::for_ops(Vec::new(), true).handled);
        assert!(Response::for_ops(vec![ClientOp::ClearMarked], true).handled);
        assert!(!Response::for_ops(vec![ClientOp::PassThrough], false).handled);
    }

    /// Backspace edits the composition rather than reaching the application,
    /// and reaches the application once there is no composition left to edit.
    #[test]
    fn backspace_edits_the_preedit_and_then_gets_out_of_the_way() {
        let mut session = session();
        let (mut client, _) = type_keys(&mut session, "tieengs");

        let backspace = KeyEvent::special(Key::Backspace);
        let response = session.key(&backspace);
        client.perform(&response.ops, None);
        assert!(response.handled);
        // One whole letter, not one combining mark: `ng` loses its `g`.
        assert_eq!(client.marked, "tiến");

        for _ in 0..4 {
            let response = session.key(&backspace);
            client.perform(&response.ops, None);
        }
        assert_eq!(client.marked, "");

        // Nothing left: the application's own backspace must happen.
        let response = session.key(&backspace);
        assert!(!response.handled);
        assert_eq!(response.ops, vec![ClientOp::PassThrough]);
    }

    /// A real `NSEvent` shape, not a synthesised `KeyEvent`: the arrow key that
    /// arrives with the function-key flags set still ends the syllable and still
    /// reaches the application.
    #[test]
    fn an_arrow_key_commits_the_syllable_and_moves_the_caret() {
        let mut session = session();
        let (mut client, _) = type_keys(&mut session, "vieetj");

        let event = key_event("", 0x7B, (1 << 23) | (1 << 21));
        let response = session.key(&event);
        client.perform(&response.ops, event.typed());
        assert_eq!(client.document, "việt");
        assert!(
            !response.handled,
            "the caret movement itself belongs to the application"
        );
    }

    /// A command shortcut commits the syllable and is handed straight on, so
    /// the user's save dialog opens and their half-typed word is not lost.
    #[test]
    fn a_command_shortcut_commits_and_passes_through() {
        let mut session = session();
        let (mut client, _) = type_keys(&mut session, "chaof");

        let event = key_event("s", 0x01, 1 << 20);
        let response = session.key(&event);
        client.perform(&response.ops, event.typed());
        // The `s` of Cmd+S is a shortcut, not a letter: passing the key through
        // hands the application an event, and types nothing.
        assert_eq!(client.document, "chào");
        assert!(!response.handled);
        assert_eq!(session.pending(), "");
    }

    /// The engine's own spell check, reaching the client: an English word that
    /// Telex would mangle comes back as it was typed.
    #[test]
    fn english_that_telex_would_mangle_survives() {
        let mut session = session();
        let (client, _) = type_keys(&mut session, "where sport ");
        assert_eq!(client.document, "where sport ");
    }
}
