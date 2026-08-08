//! Typing Vietnamese on a keyboard that has none of its letters.
//!
//! # One keystroke, end to end
//!
//! ```text
//!   KeyEvent ──▶ InputScheme::interpret ──▶ Transform ──▶ Syllable ──▶ render ──▶ EngineAction
//!               (telex.rs / vni.rs)                     (semantic state)  (NFC)
//! ```
//!
//! Four steps, no indirection, nothing dynamically dispatched.
//! [`InputScheme`] is an enum with two arms rather than a trait, because Telex
//! and VNI are two spellings of the same transforms — not two implementations
//! of anything. The only trait in sight is
//! [`LanguageEngine`](crate::core::LanguageEngine), implemented
//! once, at the bottom of this file.
//!
//! Reading the code in that order — [`telex`], then [`syllable`], then [`tone`]
//! and [`unicode`] — is the intended way in. [`rules`] is the shape of a
//! Vietnamese syllable and is what the other three keep asking.
//!
//! # What is decided where
//!
//! | question | answered in |
//! |---|---|
//! | which key means *circumflex* | [`telex`], [`vni`] |
//! | what a circumflex does | [`syllable`] |
//! | which vowel the tone is drawn on | [`tone`] |
//! | what any of it looks like | [`unicode`] |
//! | whether this is Vietnamese at all | [`rules`] |
//! | when a syllable ends | [`word_boundary`] |
//!
//! The two input schemes appear in exactly one column of that table. That is
//! the requirement "Telex and VNI share one semantic engine", stated as a
//! layout: neither file contains a rule about Vietnamese, so neither can drift
//! from the other.
//!
//! # Fail safe, and what that costs
//!
//! Every path that cannot do the Vietnamese thing does the *literal* thing. A
//! tone key with no syllable to put a tone on types its letter; a diacritic
//! digit with no vowel types its digit; a key nothing here recognises commits
//! whatever is composed and passes through. [`VietnameseEngine::process_key`]
//! carries the full statement — including why losing a keystroke is worse than
//! any wrong diacritic this could produce.
//!
//! The honest cost of a Telex engine is that English words made of Vietnamese
//! shapes get transformed: `test` really does become `tét`, exactly as it does
//! in Unikey, because `tét` is a Vietnamese syllable and the engine has no way
//! to know which language the user meant.
//! [`VietnameseConfig::spell_check`] recovers the cases where the result is
//! *not* a Vietnamese syllable (`where` stays `where`, `sport` stays `sport`),
//! which is most of them; the rest are why an input method has an off switch.

pub mod rules;
pub mod syllable;
pub mod telex;
pub mod tone;
pub mod unicode;
pub mod vni;
pub mod word_boundary;

#[cfg(test)]
mod corpus;

use crate::core::{
    Composition, EngineAction, EngineResult, Key, KeyEvent, LanguageEngine, LanguageId,
    grapheme_count,
};

use self::syllable::{Mark, MarkOutcome, Syllable, Tone};

pub use self::tone::TonePlacement;

/// How the user spells a diacritic.
///
/// **An enum, deliberately.** The two arms produce the same [`Transform`]
/// values and hand them to the same state machine, so there is nothing to
/// substitute and nothing to extend — a trait here would buy an allocation, a
/// vtable and one more file to open while following a keystroke, in exchange
/// for a flexibility that has no second customer.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum InputScheme {
    /// `aa`, `ow`, `dd`, `s`/`f`/`r`/`x`/`j`. See [`telex`].
    #[default]
    Telex,
    /// `6`, `7`, `8`, `9`, `1`–`5`. See [`vni`].
    Vni,
}

impl InputScheme {
    pub const ALL: [InputScheme; 2] = [InputScheme::Telex, InputScheme::Vni];

    /// A stable identifier, for a settings file a later round writes.
    pub fn code(self) -> &'static str {
        match self {
            InputScheme::Telex => "telex",
            InputScheme::Vni => "vni",
        }
    }
}

/// Where the composed text goes.
///
/// # Why there are two
///
/// [`OutputMode::Composition`] is what a real input method does: the text is
/// *marked*, the application knows it is provisional, and nothing lands in the
/// document until the syllable is finished. Every OS host a later round adds
/// will use it.
///
/// [`OutputMode::Direct`] types for real and rewrites what it typed as the
/// syllable evolves. It is worse in every way that matters — the application
/// sees each intermediate state, undo history fills with them, and an
/// application that reorders keystrokes can corrupt the result. It exists
/// because some hosts have no marked-text channel at all, and typing the right
/// characters clumsily beats refusing to type them.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum OutputMode {
    /// Marked text: `SetComposition` while composing, `CommitComposition` at
    /// the boundary.
    #[default]
    Composition,
    /// Real text, rewritten in place: `InsertText`, `ReplaceBeforeCursor`,
    /// `DeleteBackward`.
    Direct,
}

/// Everything about the engine a user could reasonably want to change.
///
/// A settings page for these is a later round; this round only owes them a home
/// that is not a constant buried in the state machine.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct VietnameseConfig {
    pub scheme: InputScheme,
    /// `hoà` (default) or `hòa`. See [`tone`] for the full table and why only
    /// three nuclei are affected.
    pub tone_placement: TonePlacement,
    pub output: OutputMode,
    /// Whether a syllable that is not Vietnamese is handed back as the keys
    /// that were typed.
    ///
    /// On by default, and it is what keeps `where` from becoming `ưhere` and
    /// `sport` from losing its `r`. It cannot rescue a mangling whose result
    /// *is* a Vietnamese syllable — `test` still becomes `tét` — because the
    /// engine has no way to tell that apart from someone typing `tét` on
    /// purpose.
    ///
    /// `false` means the rendered syllable always stands, which is what a user
    /// who types no English at all wants.
    pub spell_check: bool,
    /// Whether `[` and `]` type `ơ` and `ư` in Telex. On by default, matching
    /// Unikey, and the only way to type `uơ` (`thuở`, `huơ`).
    pub bracket_shortcuts: bool,
}

impl Default for VietnameseConfig {
    /// Telex, modern tone placement, marked-text output, spell check and
    /// bracket shortcuts on — Unikey's defaults, which is what a Vietnamese
    /// typist's fingers already expect.
    fn default() -> VietnameseConfig {
        VietnameseConfig {
            scheme: InputScheme::default(),
            tone_placement: TonePlacement::default(),
            output: OutputMode::default(),
            spell_check: true,
            bracket_shortcuts: true,
        }
    }
}

/// What one key does to the syllable, once the scheme has read it.
///
/// The shared vocabulary between [`telex`] and [`vni`]. Every field that is not
/// the transform itself is a `literal`: the character to type if the transform
/// turns out not to apply. Carrying it here is what lets the undo rule
/// (`aaa` → `aa`, `ss` → `s`, `a11` → `a1`) live in one place for both schemes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Transform {
    /// An ordinary letter, possibly already carrying a diacritic (`w` → `ư`).
    Letter {
        base: char,
        mark: Option<Mark>,
        upper: bool,
    },
    /// Put a diacritic on whichever letter should have it, or take it off if it
    /// is already there.
    Mark { mark: Mark, literal: char },
    /// Give the syllable a tone, or take the tone away if it already has that
    /// one.
    Tone { tone: Tone, literal: char },
    /// Take the tone off — Telex's `z`, VNI's `0`.
    ClearTone { literal: char },
}

/// What applying a transform decided.
enum Applied {
    /// The engine consumed the key and the syllable changed.
    Changed,
    /// The key was not, after all, something this engine can use here: commit
    /// what is composed and let the application have the key.
    Release,
}

/// A Vietnamese input method engine.
///
/// Holds one syllable at a time. Everything before the current syllable has
/// already been committed and is gone from here — see the privacy note on
/// [`crate`].
#[derive(Clone, Debug)]
pub struct VietnameseEngine {
    config: VietnameseConfig,
    syllable: Syllable,
    composition: Composition,
    /// In [`OutputMode::Direct`], how many graphemes of this syllable are
    /// already in the document and would have to be replaced.
    emitted: usize,
}

impl Default for VietnameseEngine {
    fn default() -> VietnameseEngine {
        VietnameseEngine::new(VietnameseConfig::default())
    }
}

impl VietnameseEngine {
    pub fn new(config: VietnameseConfig) -> VietnameseEngine {
        VietnameseEngine {
            config,
            syllable: Syllable::new(),
            composition: Composition::new(),
            emitted: 0,
        }
    }

    pub fn config(&self) -> VietnameseConfig {
        self.config
    }

    /// Change the configuration, committing anything in flight first.
    ///
    /// Switching scheme or tone style mid-syllable would reinterpret keys that
    /// were typed under the old rules, so the syllable is finished under the
    /// rules it was typed with. The returned actions must be performed.
    pub fn set_config(&mut self, config: VietnameseConfig) -> EngineResult {
        let result = self.commit();
        self.config = config;
        result
    }

    fn render(&self) -> String {
        self.syllable.render(self.config.tone_placement)
    }

    /// The text that should actually land in the document.
    ///
    /// Normally the rendered syllable. When the letters do not form a
    /// Vietnamese syllable and spell check is on, the keys as typed instead —
    /// see [`VietnameseConfig::spell_check`] and
    /// [`Syllable::raw_is_trustworthy`](syllable::Syllable::raw_is_trustworthy).
    fn commit_text(&self) -> String {
        let rendered = self.render();
        if self.config.spell_check
            && self.syllable.raw_is_trustworthy()
            && rules::parts(self.syllable.letters()).has_nucleus()
            && !self.syllable.is_valid()
        {
            return unicode::nfc(self.syllable.raw());
        }
        rendered
    }

    /// Show the syllable as it now stands.
    fn show(&mut self) -> Vec<EngineAction> {
        let text = self.render();
        match self.config.output {
            OutputMode::Composition => {
                if text.is_empty() {
                    self.reset_state();
                    return vec![EngineAction::ClearComposition];
                }
                self.composition = Composition::at_end(text.clone());
                vec![EngineAction::SetComposition {
                    cursor: self.composition.cursor(),
                    text,
                    selection: None,
                }]
            }
            OutputMode::Direct => {
                let before = self.emitted;
                self.emitted = grapheme_count(&text);
                self.composition = Composition::at_end(text.clone());
                if text.is_empty() {
                    self.syllable.clear();
                }
                EngineAction::replacement(before, text)
                    .into_iter()
                    .collect()
            }
        }
    }

    /// Accept the syllable as final text and start a new one.
    fn finish(&mut self) -> Vec<EngineAction> {
        if self.syllable.is_empty() {
            self.reset_state();
            return Vec::new();
        }
        let text = self.commit_text();
        let mut actions = Vec::with_capacity(2);
        match self.config.output {
            OutputMode::Composition => {
                if text != self.composition.text() {
                    actions.push(EngineAction::SetComposition {
                        cursor: grapheme_count(&text),
                        text,
                        selection: None,
                    });
                }
                actions.push(EngineAction::CommitComposition);
            }
            OutputMode::Direct => {
                if text != self.composition.text() {
                    actions.extend(EngineAction::replacement(self.emitted, text));
                }
            }
        }
        self.reset_state();
        actions
    }

    fn reset_state(&mut self) {
        self.syllable.clear();
        self.composition.clear();
        self.emitted = 0;
    }

    /// Commit what is composed, then hand the key to the application.
    fn release(&mut self) -> EngineResult {
        let mut actions = self.finish();
        actions.push(EngineAction::PassThrough);
        EngineResult::from_actions(actions)
    }

    fn backspace(&mut self) -> EngineResult {
        if self.syllable.is_empty() {
            return EngineResult::from_actions(vec![EngineAction::PassThrough]);
        }
        self.syllable.pop_letter(self.config.tone_placement);
        EngineResult::from_actions(self.show())
    }

    /// Apply one transform to the syllable.
    ///
    /// This is where the undo rule lives, once, for both schemes: a diacritic
    /// that was already there comes off and the key is typed as itself, and so
    /// does a tone that was already set. Everything that cannot apply falls
    /// through [`VietnameseEngine::fall_back`].
    fn apply(&mut self, transform: Transform) -> Applied {
        match transform {
            Transform::Letter { base, mark, upper } => {
                self.syllable.push_marked_letter(base, mark, upper);
                Applied::Changed
            }
            Transform::Mark { mark, literal } => match self.syllable.apply_mark(mark) {
                MarkOutcome::Applied => Applied::Changed,
                MarkOutcome::Reverted => {
                    self.syllable.distrust_raw();
                    self.fall_back(literal)
                }
                MarkOutcome::NoTarget => self.fall_back(literal),
            },
            Transform::Tone { tone, literal } => {
                if !self.syllable.is_valid() {
                    return self.fall_back(literal);
                }
                if self.syllable.tone() == tone {
                    self.syllable.clear_tone();
                    self.syllable.distrust_raw();
                    return self.fall_back(literal);
                }
                self.syllable.set_tone(tone);
                Applied::Changed
            }
            Transform::ClearTone { literal } => {
                if self.syllable.tone() == Tone::Level {
                    return self.fall_back(literal);
                }
                self.syllable.clear_tone();
                Applied::Changed
            }
        }
    }

    /// Type `literal` as itself.
    ///
    /// A letter joins the syllable — `cass` is `cas`, still one composition. A
    /// digit or a bracket cannot be part of a Vietnamese syllable, so the
    /// syllable ends and the key goes to the application: `a11` is `a` followed
    /// by `1`.
    fn fall_back(&mut self, literal: char) -> Applied {
        if word_boundary::is_syllable_letter(literal) {
            self.syllable
                .push_letter(literal, literal.is_ascii_uppercase());
            Applied::Changed
        } else {
            Applied::Release
        }
    }
}

impl LanguageEngine for VietnameseEngine {
    fn language(&self) -> LanguageId {
        LanguageId::Vietnamese
    }

    /// Decide what one keystroke does.
    ///
    /// # The fallback rule, and why it is the way it is
    ///
    /// **Losing a character the user typed is the worst thing this code can
    /// do.** A wrong diacritic is visible and one backspace from fixed; a
    /// keystroke that simply never arrives is invisible, and the user discovers
    /// it later in a sentence that no longer says what they wrote. So every
    /// branch below that cannot produce Vietnamese ends at
    /// [`EngineAction::PassThrough`] or at the literal character, and there is
    /// no branch that returns an empty action list for a key that types
    /// something:
    ///
    /// - a command shortcut, an arrow, Enter, Escape — commit and pass through;
    /// - a character neither scheme claims — commit and pass through;
    /// - a tone or diacritic key with nowhere to land — type it as itself;
    /// - a syllable that is not Vietnamese — hand back the keys as typed, if
    ///   [`VietnameseConfig::spell_check`] is on;
    /// - a state that should be unreachable — the same, because "should be
    ///   unreachable" inside a key handler is where a panic would take the
    ///   user's application down with it.
    ///
    /// There is no `unwrap`, no `expect` and no indexing without a bound on any
    /// path reachable from here.
    fn process_key(&mut self, event: &KeyEvent) -> EngineResult {
        if word_boundary::is_command(event) || word_boundary::breaks_composition(event.key) {
            return self.release();
        }
        if event.key == Key::Backspace {
            return self.backspace();
        }
        let Some(key) = event.typed() else {
            return self.release();
        };

        let transform = match self.config.scheme {
            InputScheme::Telex => {
                telex::interpret(key, &self.syllable, self.config.bracket_shortcuts)
            }
            InputScheme::Vni => vni::interpret(key, &self.syllable),
        };
        let Some(transform) = transform else {
            return self.release();
        };

        self.syllable.record_key(key);
        match self.apply(transform) {
            Applied::Changed => EngineResult::from_actions(self.show()),
            Applied::Release => self.release(),
        }
    }

    fn composition(&self) -> &Composition {
        &self.composition
    }

    /// Abandon the syllable, inserting nothing.
    ///
    /// In [`OutputMode::Direct`] the text is already in the document and this
    /// cannot take it back — the host asked to forget the composition, not to
    /// edit the document behind the application's back. Only the engine's own
    /// state is dropped.
    fn reset(&mut self) -> EngineResult {
        let had_composition =
            !self.syllable.is_empty() && self.config.output == OutputMode::Composition;
        self.reset_state();
        if had_composition {
            EngineResult::from_actions(vec![EngineAction::ClearComposition])
        } else {
            EngineResult::ignored()
        }
    }

    fn commit(&mut self) -> EngineResult {
        EngineResult::from_actions(self.finish())
    }
}

#[cfg(test)]
mod tests;
