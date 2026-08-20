//! The language switch as one of dodo's own key listeners sees it.
//!
//! Event Tap (macOS) and Keyboard Hook (Windows) run inside dodo but reach it
//! through a raw OS callback: no `App`, no `&mut` state layer, and no chance to
//! borrow a global. Each one therefore holds a [`LiveSwitch`] — a copy of the
//! three settings fields a keystroke can be answered from — and this module is
//! all the rules that copy obeys.
//!
//! # It is a copy, and `input-method.json` is still the one source of truth
//!
//! [`InputMethod::edit`](crate::InputMethod::edit) hands the
//! *whole* document to [`LiveSwitch::adopt`] before it returns, so replacing the
//! shortcut takes effect on the next key rather than after the write, the
//! notification, or a restart. Nothing is registered twice and nothing is
//! unregistered: one listener answers with whatever [`adopt`](LiveSwitch::adopt)
//! last gave it, so the previous shortcut stops matching in the same instant the
//! new one starts.
//!
//! A cycle performed here is reported back to the state layer, which writes it
//! to the file; the file is what every other host reads. The listener updating
//! its own copy first is not a second truth, it is the same value arriving
//! sooner than a round trip through the disk could deliver it.

use dodo_ime_core::{ActiveLanguages, KeyEvent, LanguageId};

use crate::models::settings::{LanguageSwitch, SettingsDocument};

/// What one keystroke did to the selected language.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cycled {
    pub language: LanguageId,
    pub beep: bool,
}

/// The settings a dodo-owned key listener answers one press from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LiveSwitch {
    language: LanguageId,
    active: ActiveLanguages,
    switch: LanguageSwitch,
}

impl LiveSwitch {
    pub fn new(document: &SettingsDocument) -> LiveSwitch {
        LiveSwitch {
            language: document.language,
            active: document.active_languages,
            switch: document.language_switch,
        }
    }

    /// Replaces every field at once.
    ///
    /// All three move together deliberately. A listener holding the new
    /// shortcut beside the old enabled-language set could cycle to a language
    /// the user has just switched off.
    pub fn adopt(&mut self, document: &SettingsDocument) {
        *self = LiveSwitch::new(document);
    }

    /// Whether this listener should hand the key to the Vietnamese engine.
    ///
    /// English and Japanese have no engine yet, so they type through. This is
    /// asked *after* [`cycle`](LiveSwitch::cycle) has declined the key, which is
    /// what keeps the shortcut working in every language: a listener that
    /// stopped observing keys while English was selected could never switch
    /// back.
    pub fn transforms(self) -> bool {
        self.language == LanguageId::Vietnamese
    }

    /// Advances to the next enabled language when this press is the shortcut.
    ///
    /// Answers `None` for every other key, including a repeat — the caller is
    /// responsible for not offering one, because a held shortcut must cycle once
    /// and not once per autorepeat.
    pub fn cycle(&mut self, event: &KeyEvent) -> Option<Cycled> {
        if !self.switch.matches(event) {
            return None;
        }
        self.language = self.active.next(self.language);
        Some(Cycled {
            language: self.language,
            beep: self.switch.beep,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{Cycled, LiveSwitch};
    use dodo_ime_core::{ActiveLanguages, KeyEvent, LanguageId, Modifiers};

    use crate::models::settings::{
        LanguageSwitch, SettingsDocument, Shortcut, ShortcutKey, ShortcutModifiers,
    };

    fn document(shortcut: Shortcut, beep: bool, languages: &[LanguageId]) -> SettingsDocument {
        SettingsDocument {
            language: languages[0],
            active_languages: ActiveLanguages::from_languages(languages.iter().copied()).unwrap(),
            language_switch: LanguageSwitch { shortcut, beep },
            ..SettingsDocument::default()
        }
    }

    fn control_shift_space() -> KeyEvent {
        KeyEvent::character(' ').with_modifiers(Modifiers {
            control: true,
            shift: true,
            ..Modifiers::NONE
        })
    }

    fn meta_space() -> KeyEvent {
        KeyEvent::character(' ').with_modifiers(Modifiers {
            meta: true,
            ..Modifiers::NONE
        })
    }

    /// The bug this whole model exists for: a listener that keeps answering the
    /// shortcut it started with, after the user has recorded another one.
    #[test]
    fn adopting_a_replacement_deactivates_the_previous_shortcut_immediately() {
        let mut live = LiveSwitch::new(&document(
            Shortcut::DEFAULT,
            false,
            &[LanguageId::English, LanguageId::Vietnamese],
        ));
        assert_eq!(
            live.cycle(&control_shift_space()),
            Some(Cycled {
                language: LanguageId::Vietnamese,
                beep: false,
            })
        );

        let replacement = Shortcut {
            modifiers: ShortcutModifiers {
                meta: true,
                ..ShortcutModifiers::NONE
            },
            key: ShortcutKey::Space,
        };
        let mut next = document(
            replacement,
            true,
            &[LanguageId::English, LanguageId::Vietnamese],
        );
        next.language = LanguageId::Vietnamese;
        live.adopt(&next);

        assert_eq!(
            live.cycle(&control_shift_space()),
            None,
            "the shortcut that was replaced must no longer switch"
        );
        assert_eq!(
            live.cycle(&meta_space()),
            Some(Cycled {
                language: LanguageId::English,
                beep: true,
            }),
            "the recorded replacement switches on the very next key"
        );
    }

    /// Two enabled languages alternate; three walk the menu order and wrap.
    #[test]
    fn the_shortcut_cycles_exactly_the_enabled_languages() {
        let mut pair = LiveSwitch::new(&document(
            Shortcut::DEFAULT,
            false,
            &[LanguageId::English, LanguageId::Vietnamese],
        ));
        let seen: Vec<_> = (0..4)
            .filter_map(|_| pair.cycle(&control_shift_space()).map(|c| c.language))
            .collect();
        assert_eq!(
            seen,
            [
                LanguageId::Vietnamese,
                LanguageId::English,
                LanguageId::Vietnamese,
                LanguageId::English,
            ]
        );

        let mut all = LiveSwitch::new(&document(Shortcut::DEFAULT, false, &LanguageId::ALL));
        let seen: Vec<_> = (0..4)
            .filter_map(|_| all.cycle(&control_shift_space()).map(|c| c.language))
            .collect();
        assert_eq!(
            seen,
            [
                LanguageId::Vietnamese,
                LanguageId::Japanese,
                LanguageId::English,
                LanguageId::Vietnamese,
            ]
        );

        // A language switched off is never cycled to, even from a document that
        // still selects it.
        let mut without_vietnamese = LiveSwitch::new(&document(
            Shortcut::DEFAULT,
            false,
            &[LanguageId::English, LanguageId::Japanese],
        ));
        assert_eq!(
            without_vietnamese
                .cycle(&control_shift_space())
                .map(|c| c.language),
            Some(LanguageId::Japanese)
        );
        assert_eq!(
            without_vietnamese
                .cycle(&control_shift_space())
                .map(|c| c.language),
            Some(LanguageId::English)
        );
    }

    /// The listener keeps observing keys in every language, and only stops
    /// *transforming* them. Without this the shortcut would be a one-way trip.
    #[test]
    fn a_non_vietnamese_language_stops_transformation_but_not_the_shortcut() {
        let mut live = LiveSwitch::new(&document(
            Shortcut::DEFAULT,
            false,
            &[LanguageId::English, LanguageId::Vietnamese],
        ));
        assert!(!live.transforms(), "English types through");
        live.cycle(&control_shift_space()).unwrap();
        assert!(live.transforms());
        assert_eq!(
            live.cycle(&control_shift_space()).map(|c| c.language),
            Some(LanguageId::English)
        );
        assert!(!live.transforms());
    }

    #[test]
    fn an_unrelated_key_never_cycles() {
        let mut live = LiveSwitch::new(&document(
            Shortcut::DEFAULT,
            false,
            &[LanguageId::English, LanguageId::Vietnamese],
        ));
        assert_eq!(live.cycle(&KeyEvent::character(' ')), None);
        assert_eq!(live.cycle(&KeyEvent::character('a')), None);
        assert_eq!(
            live.cycle(&KeyEvent::character('s').with_modifiers(Modifiers {
                control: true,
                shift: true,
                ..Modifiers::NONE
            })),
            None
        );
        assert!(!live.transforms(), "English is still selected");
    }
}
