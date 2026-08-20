//! Pure policy and direct-output planning for the Accessibility-gated Event Tap.
//!
//! The CoreGraphics callback is deliberately thin: it asks these rules whether
//! to pass, process, or re-enable, then performs the platform call. The
//! [`DirectComposer`] owns the current Telex word plus one just-committed word
//! while known separators still leave the end cursor provable, so it can
//! recompute after a physical edit without reading the focused application.
//! Nothing here touches Accessibility, files, locks, or GPUI.

use dodo_ime_core::core::truncate_graphemes;
use dodo_ime_core::{
    EngineAction, Key, KeyEvent, LanguageEngine as _, OutputMode, VietnameseConfig,
    VietnameseEngine,
};
use dodo_ime_ipc::settings::Backend;

pub use crate::models::direct_output::OutputPlan;

const MAX_RAW_KEYS: usize = 32;
// Bound a separator run as well as the retained raw word on the callback path.
const MAX_REOPEN_SEPARATORS: usize = 32;
const BACKSPACE_SEARCH_TAIL: usize = 8;
const SYNTHETIC_TAG_NAMESPACE: u32 = 0xd0d0_e7a0;

/// What the pane can honestly say about Event Tap.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EventTapStatus {
    /// Event Tap is not the selected Vietnamese backend.
    #[default]
    Inactive,
    /// A live native bundle has not yet adopted the Event Tap selection.
    WaitingForNative,
    /// macOS has not granted Accessibility permission to dodo.
    NeedsAccessibility,
    /// The tap is attached to dodo's main run loop.
    Running,
    /// CoreGraphics refused or later disabled the tap. Keys pass through.
    Failed,
}

/// The next lifecycle state before the platform service touches CoreGraphics.
///
/// The selected *language* is deliberately not an argument. The tap owns the
/// language-switch shortcut while it is running, so a tap that stopped whenever
/// English was selected could never switch back — the shortcut would work in
/// one direction and look broken in the other. It keeps observing keys in every
/// language and stops only *transforming* them; `models::live_switch` is where
/// that distinction lives.
pub fn desired_status(
    backend: Backend,
    native_is_live: bool,
    settings_applied: bool,
    accessibility_trusted: bool,
) -> EventTapStatus {
    if backend != Backend::EventTap {
        EventTapStatus::Inactive
    } else if native_is_live && !settings_applied {
        EventTapStatus::WaitingForNative
    } else if !accessibility_trusted {
        EventTapStatus::NeedsAccessibility
    } else {
        EventTapStatus::Running
    }
}

/// Whether this eligible, untrusted state may ask macOS to show Dodo.
pub fn should_request_accessibility(status: EventTapStatus, already_requested: bool) -> bool {
    status == EventTapStatus::NeedsAccessibility && !already_requested
}

/// A process-local value for `kCGEventSourceUserData`.
///
/// The PID makes this different for simultaneously running Dodo processes, and
/// the namespace keeps it distinct from CoreGraphics' default zero. Users
/// cannot produce this field through keyboard flags or normal input.
pub(crate) fn synthetic_event_tag(process_id: u32) -> i64 {
    ((u64::from(process_id) << 32) | u64::from(SYNTHETIC_TAG_NAMESPACE)) as i64
}

pub(crate) fn is_synthetic_event(user_data: i64, tag: i64) -> bool {
    user_data == tag
}

/// The event facts the callback needs before it touches composition state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TapEvent {
    KeyDown {
        autorepeat: bool,
    },
    KeyUp {
        suppress: bool,
    },
    /// Any physical mouse-down can move the focus or caret.
    MouseDown,
    /// A modifier went down or up. It types nothing, so the only thing it can
    /// mean to this host is a modifier-only language switch.
    ModifiersChanged,
    TapDisabled,
    Other,
}

/// What the callback must do with one event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handling {
    /// Feed a physical key down through the direct composer.
    ProcessKey { autorepeat: bool },
    /// Return CoreGraphics' event pointer unchanged.
    PassThrough,
    /// Consume the matching physical key-up after its key-down was replaced.
    Suppress,
    /// Offer a bare modifier change to the language switch, then return the
    /// event unchanged either way.
    ///
    /// It is never consumed. Swallowing a modifier transition would leave every
    /// application believing the key is still held, and the switch itself needs
    /// no more than to have seen it.
    OfferShortcut,
    /// Re-enable once for CoreGraphics' explicit disabled notification.
    RecoverTap,
}

/// Security-first callback routing.
pub fn handling(event: TapEvent, secure_input: bool) -> Handling {
    match event {
        TapEvent::TapDisabled => Handling::RecoverTap,
        TapEvent::KeyUp { suppress: true } => Handling::Suppress,
        TapEvent::KeyUp { suppress: false } | TapEvent::MouseDown | TapEvent::Other => {
            Handling::PassThrough
        }
        TapEvent::ModifiersChanged if !secure_input => Handling::OfferShortcut,
        TapEvent::ModifiersChanged => Handling::PassThrough,
        TapEvent::KeyDown { autorepeat } if !secure_input => Handling::ProcessKey { autorepeat },
        TapEvent::KeyDown { .. } => Handling::PassThrough,
    }
}

/// Passed-through events that leave the end cursor or focused target unknown.
///
/// A modifier transition is not one of them: `⇧` in the middle of a word is how
/// a capital letter is typed, and ending the syllable there would break Telex.
pub fn invalidates_composer(event: TapEvent) -> bool {
    matches!(
        event,
        TapEvent::KeyDown { .. } | TapEvent::MouseDown | TapEvent::Other
    )
}

/// One just-committed word that a run of known separators may still reopen.
///
/// The raw keys and config are enough to reconstruct the engine through its
/// normal rules; `rendered` remains the document truth that the next plan
/// replaces. This is deliberately one bounded snapshot, not document history.
#[derive(Clone)]
struct ReopenableWord {
    config: VietnameseConfig,
    raw: Vec<char>,
    rendered: String,
    separators: usize,
}

/// Current-word intent and rendering for direct output.
///
/// The engine remains the sole owner of Vietnamese rules. This type retains the
/// physical characters that built the in-flight word only so a physical
/// Backspace can remove one rendered grapheme and replay the remaining intent.
#[derive(Clone)]
pub(crate) struct DirectComposer {
    config: VietnameseConfig,
    engine: VietnameseEngine,
    raw: Vec<char>,
    rendered: String,
    reopenable: Option<ReopenableWord>,
}

impl DirectComposer {
    pub(crate) fn new(config: VietnameseConfig) -> DirectComposer {
        let config = direct_config(config);
        DirectComposer {
            engine: VietnameseEngine::new(config),
            config,
            raw: Vec::with_capacity(MAX_RAW_KEYS),
            rendered: String::with_capacity(MAX_RAW_KEYS * 3),
            reopenable: None,
        }
    }

    pub(crate) fn reconfigure(&mut self, config: VietnameseConfig) {
        self.config = direct_config(config);
        self.forget();
    }

    pub(crate) fn reset(&mut self) {
        self.forget();
    }

    #[cfg(test)]
    fn rendered(&self) -> &str {
        &self.rendered
    }

    /// Plans one physical key down. The caller posts a plan before deciding
    /// whether to suppress the original event.
    pub(crate) fn process(&mut self, event: KeyEvent) -> OutputPlan {
        if event.key == Key::Backspace {
            return self.backspace();
        }

        let separator = is_reopen_separator(&event);
        if self.reopenable.is_some() && !separator {
            // A key after the boundary means the cursor is no longer provably
            // beside the old word, even when that key ultimately passes through.
            self.reopenable = None;
        }
        if self.raw.len() == MAX_RAW_KEYS {
            return self.commit_before_passing();
        }
        let reopen = (separator && !self.raw.is_empty() && !self.rendered.is_empty()).then(|| {
            ReopenableWord {
                config: self.config,
                raw: self.raw.clone(),
                rendered: self.rendered.clone(),
                separators: 1,
            }
        });
        let result = self.engine.process_key(&event);
        let passes_through = result.actions.iter().any(EngineAction::passes_through);
        if !passes_through
            && self.physical_append(&event, self.engine.composition().text())
            && let Some(key) = event.typed()
        {
            // The original event already inserts exactly the next scalar.
            self.rendered.push(key);
            self.raw.push(key);
            return OutputPlan {
                pass_through: true,
                ..OutputPlan::default()
            };
        }

        let Some(next) = (if passes_through {
            direct_actions_after(&self.rendered, &result.actions)
        } else {
            Some(self.engine.composition().text().to_owned())
        }) else {
            self.forget();
            return OutputPlan {
                pass_through: true,
                ..OutputPlan::default()
            };
        };

        let mut plan = OutputPlan::minimal(&self.rendered, &next);
        plan.pass_through = passes_through;
        self.rendered = next;

        if passes_through {
            if let Some(mut word) = reopen {
                word.rendered = self.rendered.clone();
                self.reopenable = Some(word);
            } else if separator {
                if self
                    .reopenable
                    .as_ref()
                    .is_some_and(|word| word.separators == MAX_REOPEN_SEPARATORS)
                {
                    self.reopenable = None;
                } else if let Some(word) = &mut self.reopenable {
                    word.separators += 1;
                }
            }
            self.reset_engine();
            self.rendered.clear();
        } else {
            if separator {
                self.reopenable = None;
            }
            if let Some(key) = event.typed() {
                self.raw.push(key);
            } else {
                // A direct engine can only keep composing after a printable key.
                self.forget();
                plan.pass_through = true;
            }
        }
        if !plan.transforms() && !plan.pass_through {
            // The callback must never consume a key without producing output.
            self.forget();
            plan.pass_through = true;
        }
        plan
    }

    /// Let macOS remove one rendered character itself, then replay the raw
    /// intent needed to describe the remaining current word. No synthetic
    /// Backspace is ever part of this plan.
    fn backspace(&mut self) -> OutputPlan {
        if let Some(separators) = self.reopenable.as_ref().map(|word| word.separators) {
            if separators == 1 {
                self.reopen();
            } else if separators > 1 {
                if let Some(word) = &mut self.reopenable {
                    word.separators -= 1;
                }
            } else {
                self.reopenable = None;
            }
            return OutputPlan {
                pass_through: true,
                ..OutputPlan::default()
            };
        }

        if self.raw.is_empty() || self.rendered.is_empty() {
            self.forget();
            return OutputPlan {
                pass_through: true,
                ..OutputPlan::default()
            };
        }

        let target = truncate_graphemes(&self.rendered, 1);
        let Some((removed, engine)) = self.removal_for(&target) else {
            // ponytail: multi-source deletion searches the last eight raw keys;
            // extend it only if a valid Vietnamese syllable can exceed that reach.
            self.forget();
            return OutputPlan {
                pass_through: true,
                ..OutputPlan::default()
            };
        };

        let mut at = 0;
        self.raw.retain(|_| {
            let keep = removed & (1 << at) == 0;
            at += 1;
            keep
        });
        self.engine = engine;
        self.rendered = target;
        OutputPlan {
            pass_through: true,
            ..OutputPlan::default()
        }
    }

    fn commit_before_passing(&mut self) -> OutputPlan {
        // ponytail: direct mode tracks at most 32 raw keys; a longer run is
        // committed and passed through rather than making the tap unbounded.
        let result = self.engine.commit();
        let Some(next) = direct_actions_after(&self.rendered, &result.actions) else {
            self.forget();
            return OutputPlan {
                pass_through: true,
                ..OutputPlan::default()
            };
        };
        let mut plan = OutputPlan::minimal(&self.rendered, &next);
        plan.pass_through = true;
        self.rendered = next;
        self.reset_engine();
        self.rendered.clear();
        plan
    }

    fn physical_append(&self, event: &KeyEvent, next: &str) -> bool {
        if event.key != Key::Character || !event.modifiers.is_plain() {
            return false;
        }
        let Some(key) = event.typed() else {
            return false;
        };
        let Some(suffix) = next.strip_prefix(&self.rendered) else {
            return false;
        };
        let mut characters = suffix.chars();
        characters.next() == Some(key) && characters.next().is_none()
    }

    /// Finds the smallest raw-source removal whose complete replay equals the
    /// rendered word after one physical Backspace. A single source is ordinary
    /// (`g` in `tiếng`); a composed scalar may require its base, mark and tone.
    fn removal_for(&self, target: &str) -> Option<(u32, VietnameseEngine)> {
        for at in (0..self.raw.len()).rev() {
            let removed = 1_u32 << at;
            if let Some(engine) = self.replay_without(removed)
                && engine.composition().text() == target
            {
                return Some((removed, engine));
            }
        }

        let tail_start = self.raw.len().saturating_sub(BACKSPACE_SEARCH_TAIL);
        let tail_len = self.raw.len() - tail_start;
        for count in 2..=tail_len {
            for subset in 1_u16..(1_u16 << tail_len) {
                if subset.count_ones() as usize != count {
                    continue;
                }
                let removed = u32::from(subset) << tail_start;
                if let Some(engine) = self.replay_without(removed)
                    && engine.composition().text() == target
                {
                    return Some((removed, engine));
                }
            }
        }
        None
    }

    fn replay_without(&self, removed: u32) -> Option<VietnameseEngine> {
        let mut engine = VietnameseEngine::new(self.config);
        for (at, key) in self.raw.iter().copied().enumerate() {
            if removed & (1 << at) != 0 {
                continue;
            }
            let result = engine.process_key(&KeyEvent::character(key));
            if result.actions.iter().any(EngineAction::passes_through) {
                return None;
            }
        }
        Some(engine)
    }

    fn reopen(&mut self) {
        let Some(word) = self.reopenable.take() else {
            return;
        };
        let mut engine = VietnameseEngine::new(word.config);
        for key in &word.raw {
            if engine
                .process_key(&KeyEvent::character(*key))
                .actions
                .iter()
                .any(EngineAction::passes_through)
            {
                self.forget();
                return;
            }
        }
        self.config = word.config;
        self.engine = engine;
        self.raw = word.raw;
        self.rendered = word.rendered;
    }

    fn forget(&mut self) {
        self.reopenable = None;
        self.reset_engine();
        self.rendered.clear();
    }

    fn reset_engine(&mut self) {
        self.engine = VietnameseEngine::new(self.config);
        self.raw.clear();
    }
}

fn is_reopen_separator(event: &KeyEvent) -> bool {
    event.modifiers == dodo_ime_core::Modifiers::NONE
        && (event.key == Key::Space
            || matches!(event.key, Key::Character)
                && event
                    .text
                    .is_some_and(|character| character.is_ascii_punctuation()))
}

fn direct_config(mut config: VietnameseConfig) -> VietnameseConfig {
    config.output = OutputMode::Direct;
    config
}

fn direct_actions_after(before: &str, actions: &[EngineAction]) -> Option<String> {
    let mut text = before.to_owned();
    for action in actions {
        match action {
            EngineAction::PassThrough => {}
            EngineAction::InsertText(inserted) => text.push_str(inserted),
            EngineAction::DeleteBackward(count) => text = truncate_graphemes(&text, *count),
            EngineAction::ReplaceBeforeCursor {
                grapheme_count,
                text: inserted,
            } => {
                text = truncate_graphemes(&text, *grapheme_count);
                text.push_str(inserted);
            }
            EngineAction::SetComposition { .. }
            | EngineAction::CommitComposition
            | EngineAction::ClearComposition
            | EngineAction::ShowCandidates
            | EngineAction::HideCandidates => return None,
        }
    }
    Some(text)
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::{
        DirectComposer, EventTapStatus, Handling, OutputPlan, TapEvent, desired_status, handling,
        invalidates_composer, is_synthetic_event, synthetic_event_tag,
    };
    use dodo_ime_core::{
        Key, KeyEvent, LanguageEngine as _, Modifiers, OutputMode, VietnameseConfig,
        VietnameseEngine,
    };
    use dodo_ime_ipc::settings::Backend;

    fn composer() -> DirectComposer {
        DirectComposer::new(VietnameseConfig::default())
    }

    fn type_keys(composer: &mut DirectComposer, keys: &str) {
        for key in keys.chars() {
            let _ = composer.process(KeyEvent::character(key));
        }
    }

    /// A deterministic end-cursor document. A returned physical event can land
    /// before synthetic output posted by its callback, so mixed rewrite/pass
    /// plans apply the physical key first. A correct plan must not depend on
    /// the opposite ordering.
    struct EndCursorHarness {
        composer: DirectComposer,
        document: String,
        synthetic_events: usize,
    }

    impl EndCursorHarness {
        fn new() -> EndCursorHarness {
            EndCursorHarness {
                composer: composer(),
                document: String::new(),
                synthetic_events: 0,
            }
        }

        fn press(&mut self, event: KeyEvent) -> OutputPlan {
            let plan = self.composer.process(event);
            if plan.pass_through {
                if event.key == Key::Backspace {
                    self.document = dodo_ime_core::core::truncate_graphemes(&self.document, 1);
                } else if let Some(key) = event.typed() {
                    self.document.push(key);
                }
            }
            self.document =
                dodo_ime_core::core::truncate_graphemes(&self.document, plan.delete_before);
            if let Some(insert) = &plan.insert {
                self.document.push_str(insert);
            }
            self.synthetic_events += macos_event_count(&plan);
            plan
        }

        fn type_keys(&mut self, keys: &str) {
            for key in keys.chars() {
                self.press(KeyEvent::character(key));
            }
        }
    }

    fn type_at_end_cursor(keys: &str) -> String {
        let mut harness = EndCursorHarness::new();
        harness.type_keys(keys);
        harness.document
    }

    fn assert_visible_steps(keys: &str, expected: &[&str]) {
        assert_eq!(keys.chars().count(), expected.len(), "{keys:?}");
        let mut harness = EndCursorHarness::new();
        let mut prefix = String::new();
        for (key, expected) in keys.chars().zip(expected) {
            prefix.push(key);
            harness.press(KeyEvent::character(key));
            assert_eq!(harness.document, *expected, "after {prefix:?}");
        }
    }

    /// One Unicode replacement is one synthetic down/up pair, while every
    /// deletion needs its own Backspace pair.
    fn macos_event_count(plan: &OutputPlan) -> usize {
        plan.delete_before * 2 + usize::from(plan.insert.is_some()) * 2
    }

    /// The selection is the only thing that decides ownership. A tap that also
    /// stopped on the selected language could not switch back out of English.
    #[test]
    fn only_the_selected_backend_can_own_transformation() {
        assert_eq!(
            desired_status(Backend::Native, false, false, true),
            EventTapStatus::Inactive
        );
        assert_eq!(
            desired_status(Backend::KeyboardHook, false, false, true),
            EventTapStatus::Inactive
        );
        assert_eq!(
            desired_status(Backend::EventTap, false, false, true),
            EventTapStatus::Running
        );
        assert_eq!(
            desired_status(Backend::EventTap, true, false, true),
            EventTapStatus::WaitingForNative
        );
    }

    #[test]
    fn accessibility_request_is_once_and_a_returning_user_can_start_the_tap() {
        let untrusted = desired_status(Backend::EventTap, false, false, false);
        assert_eq!(untrusted, EventTapStatus::NeedsAccessibility);
        assert!(super::should_request_accessibility(untrusted, false));
        assert!(!super::should_request_accessibility(untrusted, true));
        assert_eq!(
            desired_status(Backend::EventTap, false, false, true),
            EventTapStatus::Running
        );
    }

    #[test]
    fn marked_events_pass_without_reentering_composition() {
        let tag = synthetic_event_tag(123);
        assert_ne!(tag, synthetic_event_tag(124));
        assert!(is_synthetic_event(tag, tag));
        assert!(!is_synthetic_event(0, tag));
    }

    #[test]
    fn secure_input_key_up_and_non_key_events_do_not_reach_the_composer() {
        assert_eq!(
            handling(TapEvent::KeyDown { autorepeat: true }, false),
            Handling::ProcessKey { autorepeat: true }
        );
        assert_eq!(
            handling(TapEvent::KeyDown { autorepeat: false }, true),
            Handling::PassThrough
        );
        assert_eq!(
            handling(TapEvent::KeyUp { suppress: true }, false),
            Handling::Suppress
        );
        assert_eq!(
            handling(TapEvent::KeyUp { suppress: false }, false),
            Handling::PassThrough
        );
        assert_eq!(handling(TapEvent::MouseDown, false), Handling::PassThrough);
        assert!(invalidates_composer(TapEvent::MouseDown));
        assert_eq!(handling(TapEvent::TapDisabled, false), Handling::RecoverTap);

        // A modifier transition reaches the language switch and nothing else,
        // and a secure field keeps even that.
        assert_eq!(
            handling(TapEvent::ModifiersChanged, false),
            Handling::OfferShortcut
        );
        assert_eq!(
            handling(TapEvent::ModifiersChanged, true),
            Handling::PassThrough
        );
        assert!(
            !invalidates_composer(TapEvent::ModifiersChanged),
            "Shift for a capital letter must not end the syllable"
        );
    }

    #[test]
    fn minimal_rewrites_keep_the_unchanged_suffix_and_batch_unicode() {
        let mut composer = composer();
        let ordinary = composer.process(KeyEvent::character('t'));
        assert!(ordinary.pass_through);
        assert!(!ordinary.transforms());
        type_keys(&mut composer, "ie");
        let circumflex = composer.process(KeyEvent::character('e'));
        assert_eq!(circumflex.delete_before, 1);
        assert_eq!(circumflex.insert.as_deref(), Some("ê"));
        assert!(!circumflex.pass_through);
        assert_eq!(macos_event_count(&circumflex), 4);

        type_keys(&mut composer, "ng");
        let tone = composer.process(KeyEvent::character('s'));
        assert_eq!(tone.delete_before, 3);
        assert_eq!(tone.insert.as_deref(), Some("ếng"));
        assert!(!tone.pass_through);
        assert_eq!(macos_event_count(&tone), 8);
        assert_eq!(composer.rendered(), "tiếng");

        let decomposed = OutputPlan::minimal("e\u{0302}\u{0301}", "");
        assert_eq!(decomposed.delete_before, 1);
        assert_eq!(macos_event_count(&decomposed), 2);
    }

    #[test]
    fn contextual_vietnamese_changes_keep_direct_replacements_minimal() {
        let mut pending = EndCursorHarness::new();
        pending.type_keys("thuo");
        let horn = pending.press(KeyEvent::character('w'));
        assert_eq!(horn.delete_before, 1);
        assert_eq!(horn.insert.as_deref(), Some("ơ"));
        assert_eq!(pending.document, "thuơ");

        let coda = pending.press(KeyEvent::character('n'));
        assert_eq!(coda.delete_before, 2);
        assert_eq!(coda.insert.as_deref(), Some("ươn"));
        assert_eq!(pending.document, "thươn");

        let mut foreign = EndCursorHarness::new();
        foreign.type_keys("ne");
        let literal_w = foreign.press(KeyEvent::character('w'));
        assert!(literal_w.pass_through);
        assert!(!literal_w.transforms());
        assert_eq!(foreign.document, "new");

        let mut window = EndCursorHarness::new();
        window.type_keys("win");
        let restore = window.press(KeyEvent::character('d'));
        assert_eq!(restore.delete_before, 3);
        assert_eq!(restore.insert.as_deref(), Some("wind"));
        window.type_keys("ow");
        assert_eq!(window.document, "window");
    }

    /// Every Telex modifier is checked against the document after every key,
    /// not merely against the engine's semantic state or its final action list.
    #[test]
    fn repeated_telex_modifiers_are_visible_after_every_press() {
        assert_visible_steps("[[[", &["ơ", "[", "[ơ"]);
        assert_visible_steps("]]]", &["ư", "]", "]ư"]);
        assert_visible_steps("{{{", &["Ơ", "{", "{Ơ"]);
        assert_visible_steps("}}}", &["Ư", "}", "}Ư"]);
        assert_visible_steps("o[[", &["o", "oơ", "o["]);

        assert_visible_steps("www", &["ư", "w", "wư"]);
        assert_visible_steps("aaa", &["a", "â", "aa"]);
        assert_visible_steps("eee", &["e", "ê", "ee"]);
        assert_visible_steps("ooo", &["o", "ô", "oo"]);
        assert_visible_steps("ddd", &["d", "đ", "dd"]);
        assert_visible_steps("aww", &["a", "ă", "aw"]);
        assert_visible_steps("oww", &["o", "ơ", "ow"]);
        assert_visible_steps("uww", &["u", "ư", "uw"]);
        assert_visible_steps("mass", &["m", "ma", "má", "mas"]);
        assert_visible_steps("maff", &["m", "ma", "mà", "maf"]);
        assert_visible_steps("marr", &["m", "ma", "mả", "mar"]);
        assert_visible_steps("maxx", &["m", "ma", "mã", "max"]);
        assert_visible_steps("majj", &["m", "ma", "mạ", "maj"]);
        assert_visible_steps("maszz", &["m", "ma", "má", "ma", "maz"]);
    }

    #[test]
    fn direct_document_simulator_converges_dd_and_repeated_w_cancellation() {
        // The case follows the *stroke* key, which is the shared engine's rule
        // and not something either fallback decides — see the Vietnamese
        // module's "Whose shift decides a marked letter's case".
        for (keys, expected) in [("dd", "đ"), ("DD", "Đ"), ("dD", "Đ"), ("Dd", "đ")] {
            assert_eq!(type_at_end_cursor(keys), expected, "{keys}");
        }
        for (keys, expected) in [("ww", "w"), ("wW", "W"), ("uww", "uw")] {
            assert_eq!(type_at_end_cursor(keys), expected, "{keys}");
        }
    }

    #[test]
    fn physical_backspace_passes_once_and_updates_current_word_intent() {
        let mut current = composer();
        type_keys(&mut current, "tieengs");
        assert_eq!(current.rendered(), "tiếng");

        let plan = current.process(KeyEvent::special(Key::Backspace));
        assert!(plan.pass_through);
        assert!(!plan.transforms());
        assert_eq!(macos_event_count(&plan), 0);
        assert_eq!(current.rendered(), "tiến");

        type_keys(&mut current, "g");
        assert_eq!(current.rendered(), "tiếng");

        let mut composed_scalar = composer();
        type_keys(&mut composed_scalar, "ee");
        assert_eq!(composed_scalar.rendered(), "ê");
        let plan = composed_scalar.process(KeyEvent::special(Key::Backspace));
        assert!(plan.pass_through);
        assert!(!plan.transforms());
        assert_eq!(composed_scalar.rendered(), "");
        assert!(
            composed_scalar
                .process(KeyEvent::special(Key::Backspace))
                .pass_through
        );
    }

    #[test]
    fn separator_backspace_reopens_only_the_immediately_preceding_word() {
        let mut first = EndCursorHarness::new();
        first.type_keys("ddee ");
        let backspace = first.press(KeyEvent::special(Key::Backspace));
        assert!(backspace.pass_through);
        assert!(!backspace.transforms());
        assert_eq!(macos_event_count(&backspace), 0);
        first.type_keys("f");
        assert_eq!(first.document, "đề");

        let mut second = EndCursorHarness::new();
        second.type_keys("dde ");
        second.press(KeyEvent::special(Key::Backspace));
        second.type_keys("e");
        assert_eq!(second.document, "đê");

        // The uninterrupted paths use the same replayed Telex rules.
        assert_eq!(type_at_end_cursor("ddeef"), "đề");
        assert_eq!(type_at_end_cursor("ddee"), "đê");

        let mut spaces = EndCursorHarness::new();
        spaces.type_keys("ddee  ");
        spaces.press(KeyEvent::special(Key::Backspace));
        assert_eq!(spaces.document, "đê ");
        spaces.press(KeyEvent::special(Key::Backspace));
        spaces.type_keys("f");
        assert_eq!(spaces.document, "đề");

        let mut punctuation = EndCursorHarness::new();
        punctuation.type_keys("ddee,");
        punctuation.press(KeyEvent::special(Key::Backspace));
        punctuation.type_keys("f");
        assert_eq!(punctuation.document, "đề");
    }

    #[test]
    fn separator_snapshots_are_discarded_by_unsafe_events() {
        let mut extra_text = EndCursorHarness::new();
        extra_text.type_keys("ddee x");
        extra_text.press(KeyEvent::special(Key::Backspace));
        extra_text.type_keys("f");
        assert_eq!(extra_text.document, "đê f");

        let mut navigation = EndCursorHarness::new();
        navigation.type_keys("ddee ");
        navigation.press(KeyEvent::special(Key::ArrowLeft));
        navigation.press(KeyEvent::special(Key::Backspace));
        navigation.type_keys("f");
        assert_eq!(navigation.document, "đêf");

        let mut shortcut = EndCursorHarness::new();
        shortcut.type_keys("ddee ");
        shortcut.press(KeyEvent::character('s').with_modifiers(Modifiers {
            meta: true,
            ..Modifiers::NONE
        }));
        shortcut.press(KeyEvent::special(Key::Backspace));
        shortcut.type_keys("f");
        assert_eq!(shortcut.document, "đêf");

        let mut secure_transition = EndCursorHarness::new();
        secure_transition.type_keys("ddee ");
        // Secure input, focus changes, tap recovery and config changes route
        // through this same callback reset boundary before passing their event.
        assert!(invalidates_composer(TapEvent::MouseDown));
        secure_transition.composer.reset();
        secure_transition.press(KeyEvent::special(Key::Backspace));
        secure_transition.type_keys("f");
        assert_eq!(secure_transition.document, "đêf");
    }

    #[test]
    fn separator_backspace_autorepeats_and_empty_state_stay_physical() {
        let mut repeated = EndCursorHarness::new();
        repeated.type_keys("ddee  ");
        for expected in ["đê ", "đê", "đ"] {
            let plan = repeated.press(KeyEvent::special(Key::Backspace));
            assert!(plan.pass_through);
            assert_eq!(macos_event_count(&plan), 0);
            assert_eq!(repeated.document, expected);
        }

        let mut empty = EndCursorHarness::new();
        let plan = empty.press(KeyEvent::special(Key::Backspace));
        assert!(plan.pass_through);
        assert_eq!(empty.synthetic_events, 0);
        assert_eq!(empty.document, "");
    }

    #[test]
    fn complete_word_replay_converges_across_modifier_order() {
        assert_eq!(type_at_end_cursor("hoiw"), "hơi");
        assert_eq!(type_at_end_cursor("thienej"), "thiện");
        assert_eq!(type_at_end_cursor("thieenj"), "thiện");
    }

    #[test]
    fn a_foreign_precomposed_scalar_ends_direct_composition_and_passes_through() {
        let mut harness = EndCursorHarness::new();
        harness.type_keys("hoi");
        let plan = harness.press(KeyEvent::character('ư'));

        assert_eq!(
            plan,
            OutputPlan {
                pass_through: true,
                ..OutputPlan::default()
            }
        );
        assert_eq!(harness.document, "hoiư");
        assert_eq!(harness.composer.rendered(), "");
        assert_eq!(harness.synthetic_events, 0);
    }

    #[test]
    fn boundaries_and_shortcuts_commit_then_pass_without_synthetic_input() {
        let mut composer = composer();
        type_keys(&mut composer, "tieengs");
        let navigation = composer.process(KeyEvent::special(Key::ArrowLeft));
        assert!(navigation.pass_through);
        assert!(!navigation.transforms());
        assert_eq!(composer.rendered(), "");

        type_keys(&mut composer, "tieengs");
        let shortcut = composer.process(KeyEvent::character('s').with_modifiers(Modifiers {
            meta: true,
            ..Modifiers::NONE
        }));
        assert!(shortcut.pass_through);
        assert!(!shortcut.transforms());
        assert_eq!(composer.rendered(), "");

        let precomposed = composer.process(KeyEvent::character('ư'));
        assert!(precomposed.pass_through);
        assert!(!precomposed.transforms());
        let unknown = composer.process(KeyEvent::special(Key::Other));
        assert!(unknown.pass_through);
        assert!(!unknown.transforms());
    }

    /// Manual release-mode profile. It exercises only the pure planner and the
    /// number of events its CoreGraphics staging would create; it posts nothing.
    #[test]
    #[ignore = "run with --release -- --ignored --nocapture to report planner timings"]
    fn profile_direct_planner_without_live_input() {
        const ROUNDS: usize = 50_000;
        let mut legacy_events = 0;
        let legacy_started = Instant::now();
        for _ in 0..ROUNDS {
            let mut engine = VietnameseEngine::new(VietnameseConfig {
                output: OutputMode::Direct,
                ..VietnameseConfig::default()
            });
            for key in ['t', 'i', 'e', 'e', 'n', 'g', 's'] {
                let result = engine.process_key(&KeyEvent::character(key));
                let plan = OutputPlan::from_actions(&result.actions).expect("direct actions");
                legacy_events += plan.delete_before * 2 + usize::from(plan.insert.is_some());
            }
            let result = engine.process_key(&KeyEvent::special(Key::Backspace));
            let plan = OutputPlan::from_actions(&result.actions).expect("direct actions");
            legacy_events += plan.delete_before * 2 + usize::from(plan.insert.is_some());
        }
        let legacy_elapsed = legacy_started.elapsed();

        let mut minimal_events = 0;
        let minimal_started = Instant::now();
        for _ in 0..ROUNDS {
            let mut composer = composer();
            for key in ['t', 'i', 'e', 'e', 'n', 'g', 's'] {
                minimal_events += macos_event_count(&composer.process(KeyEvent::character(key)));
            }
            minimal_events +=
                macos_event_count(&composer.process(KeyEvent::special(Key::Backspace)));
        }
        let minimal_elapsed = minimal_started.elapsed();

        let first_reopen_started = Instant::now();
        let mut first_reopen_events = 0;
        for _ in 0..ROUNDS {
            let mut composer = composer();
            for key in ['d', 'd', 'e', 'e', ' '] {
                first_reopen_events +=
                    macos_event_count(&composer.process(KeyEvent::character(key)));
            }
            first_reopen_events +=
                macos_event_count(&composer.process(KeyEvent::special(Key::Backspace)));
            first_reopen_events += macos_event_count(&composer.process(KeyEvent::character('f')));
        }
        let first_reopen_elapsed = first_reopen_started.elapsed();

        let second_reopen_started = Instant::now();
        let mut second_reopen_events = 0;
        for _ in 0..ROUNDS {
            let mut composer = composer();
            for key in ['d', 'd', 'e', ' '] {
                second_reopen_events +=
                    macos_event_count(&composer.process(KeyEvent::character(key)));
            }
            second_reopen_events +=
                macos_event_count(&composer.process(KeyEvent::special(Key::Backspace)));
            second_reopen_events += macos_event_count(&composer.process(KeyEvent::character('e')));
        }
        let second_reopen_elapsed = second_reopen_started.elapsed();

        // `ế` needs its base, circumflex and tone sources removed together,
        // exercising the largest normal physical-Backspace replay.
        let mut legacy_backspaces = Vec::with_capacity(ROUNDS);
        let mut minimal_backspaces = Vec::with_capacity(ROUNDS);
        for _ in 0..ROUNDS {
            let mut engine = VietnameseEngine::new(VietnameseConfig {
                output: OutputMode::Direct,
                ..VietnameseConfig::default()
            });
            for key in ['e', 'e', 's'] {
                let _ = engine.process_key(&KeyEvent::character(key));
            }
            legacy_backspaces.push(engine);

            let mut composer = composer();
            for key in ['e', 'e', 's'] {
                let _ = composer.process(KeyEvent::character(key));
            }
            minimal_backspaces.push(composer);
        }
        let legacy_backspace_started = Instant::now();
        let mut legacy_backspace_events = 0;
        for engine in &mut legacy_backspaces {
            let result = engine.process_key(&KeyEvent::special(Key::Backspace));
            let plan = OutputPlan::from_actions(&result.actions).expect("direct actions");
            legacy_backspace_events += plan.delete_before * 2 + usize::from(plan.insert.is_some());
        }
        let legacy_backspace_elapsed = legacy_backspace_started.elapsed();

        let minimal_backspace_started = Instant::now();
        let mut minimal_backspace_events = 0;
        for composer in &mut minimal_backspaces {
            minimal_backspace_events +=
                macos_event_count(&composer.process(KeyEvent::special(Key::Backspace)));
        }
        let minimal_backspace_elapsed = minimal_backspace_started.elapsed();

        eprintln!(
            "Event Tap pure planner: legacy {legacy_events} staged events in {legacy_elapsed:?}; minimal {minimal_events} staged events in {minimal_elapsed:?}; separator reopen đê + Backspace + f {first_reopen_events} events in {first_reopen_elapsed:?} ({:?}/sequence); đe + Backspace + e {second_reopen_events} events in {second_reopen_elapsed:?} ({:?}/sequence); worst Backspace legacy {legacy_backspace_events} staged events in {legacy_backspace_elapsed:?}, minimal {minimal_backspace_events} in {minimal_backspace_elapsed:?}",
            first_reopen_elapsed / ROUNDS as u32,
            second_reopen_elapsed / ROUNDS as u32,
        );
    }
}
