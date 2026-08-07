//! Which language the user is **typing** in — the tray's own setting, and the
//! only thing the menu bar mark depends on.

/// The keyboard input language shown by the menu bar mark.
///
/// # This is not [`i18n::Language`](crate::i18n::Language)
///
/// dodo has two language settings and **they must never be merged**. The
/// captain said so on 2026-08-07, after the first version of this feature
/// blurred them:
///
/// | | Changed where | Controls | Type |
/// |---|---|---|---|
/// | interface language | the Settings dialog | dodo's own text, every `Str` | [`crate::i18n::Language`] |
/// | keyboard input language | the menu bar menu | one thing: which glyph this mark carries | this |
///
/// They share no type, no constant, no conversion and no persistence key —
/// `appearance.language` versus `tray.input_language` in `session.json`. The two
/// lists already differ: this one has Japanese and the interface language does
/// not, and that is the point rather than an oversight. Adding a language here
/// obliges nobody to translate ~550 `Str` variants; adding one there says
/// nothing about what anyone types.
///
/// `code` returning `"en"` for both types is a coincidence of spelling, not a
/// shared vocabulary. `tray::tests::the_two_language_settings_share_no_code`
/// exists to make that stay true.
///
/// # Adding a language
///
/// A variant, a row in [`InputLanguage::ALL`], an arm in [`InputLanguage::code`]
/// and [`InputLanguage::label`], and `assets/icons/tray/dodo-<code>.svg`. That
/// is the whole list: the menu is built by iterating `ALL`, the routing table is
/// a lookup over `ALL`, and the asset path is derived from `code`, so **none of
/// them is edited**. The two `match`es are exhaustive, so the compiler names
/// what is missing, and `icon::tests::every_input_language_has_an_embedded_asset`
/// turns a forgotten asset into a failing test rather than a blank menu bar.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
#[allow(
    dead_code,
    reason = "phase 2 ships only the English mark; the menu that constructs the other two is phase 3. Remove the allow with that menu."
)]
pub enum InputLanguage {
    #[default]
    English,
    Vietnamese,
    Japanese,
}

impl InputLanguage {
    /// Every input language, in the order the menu lists them.
    #[allow(
        dead_code,
        reason = "read by the tests and by phase 3's menu builder. Remove the allow with that menu."
    )]
    pub const ALL: [InputLanguage; 3] = [
        InputLanguage::English,
        InputLanguage::Vietnamese,
        InputLanguage::Japanese,
    ];

    /// The stable identifier: what `session.json` stores and what names the
    /// asset, so the two can never drift apart.
    ///
    /// A code that has shipped may not be reused for a different language —
    /// the same compatibility rule [`View::code`](crate::layout::View::code)
    /// carries, for the same reason.
    pub fn code(self) -> &'static str {
        match self {
            InputLanguage::English => "en",
            InputLanguage::Vietnamese => "vi",
            InputLanguage::Japanese => "ja",
        }
    }

    /// The language's name in that language, as language pickers conventionally
    /// show it.
    ///
    /// **Deliberately not a [`Str`](crate::i18n::Str).** An endonym is not
    /// translated: a picker shows each language in its own language, so
    /// "Tiếng Việt" reads the same to an English user as to a Vietnamese one.
    /// `i18n::Language::label` already applies this rule to the Settings
    /// picker; the two arrived at it independently and share no code.
    #[allow(
        dead_code,
        reason = "the menu row's text, which phase 3 builds. Remove the allow with that menu."
    )]
    pub fn label(self) -> &'static str {
        match self {
            InputLanguage::English => "English",
            InputLanguage::Vietnamese => "Tiếng Việt",
            InputLanguage::Japanese => "日本語",
        }
    }

    /// The embedded SVG for this language's menu bar mark.
    ///
    /// Resolved through [`Assets`](crate::assets::Assets) — see
    /// [`crate::tray::icon`] for why no PNG is involved.
    pub fn asset(self) -> String {
        format!("icons/tray/dodo-{}.svg", self.code())
    }

    /// The language a stored code names, if this build still has it.
    ///
    /// `None` for anything unrecognised — a language a later dodo added and
    /// this one does not have, or a hand-edited file — which the caller turns
    /// into the default rather than a refusal to start. Same shape as
    /// [`View::lookup`](crate::layout::View::lookup).
    #[allow(
        dead_code,
        reason = "the way back from `session.json`, which phase 3 writes and phase 4 reads. Remove the allow when the restore lands."
    )]
    pub fn from_code(code: &str) -> Option<InputLanguage> {
        InputLanguage::ALL
            .into_iter()
            .find(|language| language.code() == code)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_distinct_and_round_trip() {
        for language in InputLanguage::ALL {
            assert_eq!(
                InputLanguage::from_code(language.code()),
                Some(language),
                "{language:?} does not survive a trip through its own code"
            );
        }

        let mut codes: Vec<_> = InputLanguage::ALL.iter().map(|l| l.code()).collect();
        codes.sort_unstable();
        let count = codes.len();
        codes.dedup();
        assert_eq!(codes.len(), count, "two input languages share a code");
    }

    /// A code this build does not know opens dodo rather than breaking it. The
    /// `None` is the caller's cue to fall back to the default, which is what
    /// makes removing a language a safe change for anyone who had it selected.
    #[test]
    fn an_unknown_code_is_rejected_rather_than_guessed() {
        assert_eq!(InputLanguage::from_code("ko"), None);
        assert_eq!(InputLanguage::from_code(""), None);
        assert_eq!(InputLanguage::from_code("EN"), None);
    }

    #[test]
    fn labels_are_endonyms_and_distinct() {
        assert_eq!(InputLanguage::Vietnamese.label(), "Tiếng Việt");
        assert_eq!(InputLanguage::Japanese.label(), "日本語");

        let mut labels: Vec<_> = InputLanguage::ALL.iter().map(|l| l.label()).collect();
        labels.sort_unstable();
        let count = labels.len();
        labels.dedup();
        assert_eq!(labels.len(), count, "two input languages share a label");
    }

    /// The asset path is derived, never written out per language — which is
    /// what makes adding a language one variant and one file.
    #[test]
    fn the_asset_path_is_derived_from_the_code() {
        for language in InputLanguage::ALL {
            assert_eq!(
                language.asset(),
                format!("icons/tray/dodo-{}.svg", language.code())
            );
        }
    }

    #[test]
    fn english_is_the_default() {
        assert_eq!(InputLanguage::default(), InputLanguage::English);
    }
}
