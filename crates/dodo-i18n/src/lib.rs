//! A deliberately small localization mechanism: one enum per area of the app
//! and one file per language inside it.
//!
//! Each area under this crate owns its own strings and its own
//! translations: `<area>/mod.rs` declares the area's `Text` enum, and
//! `<area>/en.rs` and `<area>/vi.rs` are each an exhaustive `match` over it.
//! [`Str`] is the thin sum over those enums, so a call site reads
//! `t(cleaner::Text::Scan, cx)` and a [`Str`] can still be *held* unrendered —
//! a `ConsoleEntry` keeps dodo's own lines that way so they re-translate when
//! the language changes.
//!
//! Adding a string means a variant in the area's `Text` and a row in each of
//! that area's language files; the compiler lists the ones you missed. Adding a
//! **language** means a [`Language`] variant, a row in [`Language::ALL`], one
//! more arm in the `areas!` dispatch below, and one new file per area — no
//! existing string is touched. No catalogue files, no runtime key lookup, no
//! missing-key fallback to get wrong.
//!
//! Messages that carry runtime values — a position, a count, a third-party
//! parser's own text — are variants with fields, so each language owns the
//! whole sentence and word order rather than a translated prefix glued onto an
//! English tail. Third-party error text (serde_json, base64, …) is English and
//! stays English inside the translated frame; there is nothing to translate it
//! with.
//!
//! # What is *not* here
//!
//! The catalogue names no UI framework and never will: a [`Str`] is held by
//! pure models that are tested with no `App` and no frame. By default this
//! crate has no dependencies at all.
//!
//! The three pieces that *do* need the UI — the gpui `Global` holding the
//! active language, `Language::current` / `set`, and [`t`] — are [`ui`], behind
//! the opt-in `gpui` feature. They were dodo's own `src/i18n.rs` until
//! `crates/dodo-cleaner` came out of the binary and needed to render a [`Str`]
//! from outside it; `Cargo.toml` records why a second copy of that global is
//! not an option. Leave the feature off unless you are *drawing* a string.
//!
//! [`t`]: ui::t
//! [`ui`]: ui

use std::borrow::Cow;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum Language {
    #[default]
    English,
    Vietnamese,
}

impl Language {
    pub const ALL: [Language; 2] = [Language::English, Language::Vietnamese];

    /// The stable identifier used as the settings dropdown value.
    pub fn code(self) -> &'static str {
        match self {
            Language::English => "en",
            Language::Vietnamese => "vi",
        }
    }

    pub fn from_code(code: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|language| language.code() == code)
            .unwrap_or_default()
    }

    /// The language's name in that language, as language pickers conventionally
    /// show it.
    pub fn label(self) -> &'static str {
        match self {
            Language::English => "English",
            Language::Vietnamese => "Tiếng Việt",
        }
    }
}

/// Builds one area's sample table for the tests below.
///
/// One entry per `Text` variant, in the area's own `samples.rs`. The macro
/// emits both the list the tests walk *and* an exhaustive `match` over `Text`,
/// so a variant with no entry is a compile error and there is no second table
/// to keep in step — which is what replaced the hand-numbered index table this
/// module used to carry.
///
/// ```ignore
/// samples! {
///     plain Scan;                                   // prose
///     term Docker;                                  // same word in every language
///     with IndentSpaces(NUMBER) [NUMBER_TEXT];      // carries runtime values
/// }
/// ```
#[cfg(test)]
macro_rules! samples {
    ($(
        $kind:ident $name:ident
        $( ( $($tuple:expr),* $(,)? ) )?
        $( { $($field:ident : $value:expr),* $(,)? } )?
        $( [ $($part:expr),* $(,)? ] )?
    );* $(;)?) => {
        /// Every variant of this area's [`Text`], with the sentinel values the
        /// language tests assert on.
        pub(crate) fn samples() -> Vec<Sample> {
            vec![$(
                $kind(
                    Text::$name $( ( $($tuple),* ) )? $( { $($field : $value),* } )?
                    $(, &[$($part),*])?
                )
            ),*]
        }

        /// Exhaustive over [`Text`]: a variant the table above does not list is
        /// a compile error here. Nothing calls this — the compiler is the whole
        /// point of it.
        #[allow(
            dead_code,
            reason = "Exists only so rustc checks the table above is complete."
        )]
        fn covered(text: &Text) {
            match text {
                $(Text::$name { .. } => ()),*
            }
        }
    };
}

/// Declares [`Str`] as the sum of the per-area catalogues.
///
/// This is the only place that lists the areas, and the only place that maps a
/// [`Language`] onto an area's language file. Adding an area is one line here
/// plus its directory; adding a language is one arm in the inner `match` plus
/// one file per area, and the compiler names every area that has not been given
/// one.
macro_rules! areas {
    ($($module:ident => $variant:ident),+ $(,)?) => {
        /// Every string this app localizes, as a thin sum over the areas.
        ///
        /// "Dodo" is the product name and is never translated, so it has no
        /// variant anywhere. Neither do the technical terms that stay put in
        /// both languages — JSON, Base64, hex, JWT, URL — they appear inside
        /// the strings themselves.
        #[derive(Clone, Debug, PartialEq, Eq)]
        pub enum Str {
            $($variant($module::Text),)+
        }

        $(
            impl From<$module::Text> for Str {
                fn from(text: $module::Text) -> Self {
                    Str::$variant(text)
                }
            }
        )+

        impl Str {
            /// The string in one language.
            ///
            /// Public because a [`Str`] can be *held* rather than rendered on
            /// the spot — a `ConsoleEntry` keeps dodo's own lines unrendered so
            /// they re-translate — and those holders have to be testable
            /// without a `Window`. Views go through dodo's own `t()`, which
            /// asks this for the active language.
            pub fn text(self, language: Language) -> Cow<'static, str> {
                match self {
                    $(
                        Str::$variant(text) => match language {
                            Language::English => $module::en::text(text),
                            Language::Vietnamese => $module::vi::text(text),
                        },
                    )+
                }
            }

            /// Every area's sample table, concatenated.
            #[cfg(test)]
            fn samples() -> Vec<tests::Sample> {
                let mut samples = Vec::new();
                $(samples.extend($module::samples::samples());)+
                samples
            }
        }
    };
}

pub mod api_collections;
pub mod api_explorer;
pub mod api_response;
pub mod api_scripts;
pub mod api_variables;
pub mod cleaner;
pub mod database;
pub mod db_catalog;
pub mod db_connection;
pub mod db_query;
pub mod docker;
pub mod encoder_decoder;
pub mod input_method;
pub mod json_formatter;
pub mod mermaid;
pub mod quick_nav;
pub mod session;
pub mod shared;
pub mod shell;
pub mod tray;
pub mod updater;
// The UI half, behind the feature that switches its one dependency on.
#[cfg(feature = "gpui")]
pub mod ui;
#[cfg(feature = "gpui")]
pub use ui::{ActiveLanguage, LanguageExt, t};

areas! {
    api_collections => ApiCollections,
    api_explorer => ApiExplorer,
    api_response => ApiResponse,
    api_scripts => ApiScripts,
    api_variables => ApiVariables,
    cleaner => Cleaner,
    database => Database,
    db_catalog => DbCatalog,
    db_connection => DbConnection,
    db_query => DbQuery,
    docker => Docker,
    encoder_decoder => EncoderDecoder,
    input_method => InputMethod,
    json_formatter => JsonFormatter,
    mermaid => Mermaid,
    quick_nav => QuickNav,
    session => Session,
    shared => Shared,
    shell => Shell,
    tray => Tray,
    updater => Updater,
}

#[cfg(test)]
pub(crate) mod tests;
