//! The language selected for keyboard input.
//!
//! [`LanguageId`] is the one stable identity shared by dodo's menu bar and its
//! native input method. It is deliberately plain Rust so the engine remains
//! independent of either UI or IPC.

/// A keyboard input language dodo knows about.
///
/// Only Vietnamese has an engine today. English and Japanese still name real
/// selections: the native host passes their keys through until their engines
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

#[cfg(test)]
mod tests {
    use super::LanguageId;

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
}
