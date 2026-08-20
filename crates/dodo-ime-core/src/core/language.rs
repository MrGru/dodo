//! The language selected for keyboard input.
//!
//! [`LanguageId`] is the one stable identity shared by dodo's menu bar and its
//! Input method. It is deliberately plain Rust so the engine remains independent
//! of the UI and persisted settings.

/// A keyboard input language dodo knows about.
///
/// Only Vietnamese has an engine today. English and Japanese still name real
/// selections: the input listener passes their keys through until their engines
/// exist.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub enum LanguageId {
    #[default]
    English,
    Vietnamese,
    Japanese,
}

impl LanguageId {
    /// Every supported selection, in menu order.
    pub const ALL: [LanguageId; 3] = [
        LanguageId::English,
        LanguageId::Vietnamese,
        LanguageId::Japanese,
    ];

    /// The stable identifier persisted in the input-method settings file.
    pub fn code(self) -> &'static str {
        match self {
            LanguageId::English => "en",
            LanguageId::Vietnamese => "vi",
            LanguageId::Japanese => "ja",
        }
    }

    /// The language a stored code names, if this build supports it.
    pub fn from_code(code: &str) -> Option<LanguageId> {
        LanguageId::ALL
            .into_iter()
            .find(|language| language.code() == code)
    }
}

/// The keyboard input languages a person has enabled.
///
/// This is a compact set rather than a second enum: adding a language means
/// adding one [`LanguageId`] variant, while selection, cycling, and persistence
/// continue to use this one type.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ActiveLanguages(u8);

impl Default for ActiveLanguages {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl ActiveLanguages {
    /// English and Vietnamese are enabled until a person chooses otherwise.
    pub const DEFAULT: Self = Self(3);

    /// Builds a non-empty set of known, distinct languages.
    pub fn from_languages(languages: impl IntoIterator<Item = LanguageId>) -> Option<Self> {
        let mut enabled = Self(0);
        for language in languages {
            if enabled.contains(language) {
                return None;
            }
            enabled.0 |= Self::bit(language);
        }
        (enabled.0 != 0).then_some(enabled)
    }

    /// Whether one language is available for selection.
    pub fn contains(self, language: LanguageId) -> bool {
        self.0 & Self::bit(language) != 0
    }

    /// The enabled languages in the stable menu order.
    pub fn iter(self) -> impl Iterator<Item = LanguageId> {
        LanguageId::ALL
            .into_iter()
            .filter(move |language| self.contains(*language))
    }

    /// Enables or disables a language, preserving the one-language minimum.
    pub fn with(self, language: LanguageId, enabled: bool) -> Self {
        if enabled {
            Self(self.0 | Self::bit(language))
        } else if self.contains(language) && self.iter().count() == 1 {
            self
        } else {
            Self(self.0 & !Self::bit(language))
        }
    }

    /// The enabled language after `current`, wrapping in menu order.
    pub fn next(self, current: LanguageId) -> LanguageId {
        let first = self
            .iter()
            .next()
            .expect("ActiveLanguages is constructed non-empty");
        let mut after_current = false;
        for language in self.iter() {
            if after_current {
                return language;
            }
            after_current = language == current;
        }
        first
    }

    fn bit(language: LanguageId) -> u8 {
        match language {
            LanguageId::English => 1,
            LanguageId::Vietnamese => 2,
            LanguageId::Japanese => 4,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ActiveLanguages, LanguageId};

    #[test]
    fn codes_are_distinct_and_round_trip() {
        let mut codes = Vec::new();
        for language in LanguageId::ALL {
            assert_eq!(LanguageId::from_code(language.code()), Some(language));
            codes.push(language.code());
        }
        codes.sort_unstable();
        let count = codes.len();
        codes.dedup();
        assert_eq!(codes.len(), count, "two input languages share a code");
    }

    #[test]
    fn english_is_the_default() {
        assert_eq!(LanguageId::default(), LanguageId::English);
    }

    #[test]
    fn active_languages_default_to_english_and_vietnamese_and_cycle_in_order() {
        let active = ActiveLanguages::default();
        assert_eq!(
            active.iter().collect::<Vec<_>>(),
            vec![LanguageId::English, LanguageId::Vietnamese]
        );
        assert_eq!(active.next(LanguageId::English), LanguageId::Vietnamese);
        assert_eq!(active.next(LanguageId::Vietnamese), LanguageId::English);

        let all = ActiveLanguages::from_languages(LanguageId::ALL).unwrap();
        assert_eq!(all.next(LanguageId::Vietnamese), LanguageId::Japanese);
        assert_eq!(all.next(LanguageId::Japanese), LanguageId::English);
    }

    #[test]
    fn active_languages_reject_invalid_sets_and_keep_one_language_enabled() {
        assert!(ActiveLanguages::from_languages([]).is_none());
        assert!(
            ActiveLanguages::from_languages([LanguageId::English, LanguageId::English]).is_none()
        );

        let only_english = ActiveLanguages::from_languages([LanguageId::English]).unwrap();
        assert_eq!(
            only_english.with(LanguageId::English, false),
            only_english,
            "the settings UI cannot leave the shortcut without a destination"
        );
        assert_eq!(
            only_english.next(LanguageId::Vietnamese),
            LanguageId::English
        );
    }
}
