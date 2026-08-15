//! dodo's end of [`dodo_i18n`]: the three pieces of localization that need the
//! UI framework, and a re-export of everything that does not.
//!
//! The catalogue itself — [`Language`], every area's `Text` enum, [`Str`] and
//! the lookup — lives in `crates/dodo-i18n`, which depends on nothing at all so
//! a pure model can hold a translated message and a test can render it without
//! a window. What stays here is what `gpui` is needed for:
//!
//! - [`ActiveLanguage`], the global holding the current choice;
//! - [`LanguageExt`], which puts `Language::current` / `set` back where every
//!   call site already spells them;
//! - [`t`], which renders a [`Str`] into a `SharedString` for a view.
//!
//! The `pub use` below is what keeps `use crate::i18n::{cleaner, t}` reading
//! exactly as it did when all of this was one module: every area is still
//! reachable as `crate::i18n::<area>`.
//!
//! # Why the two halves are shaped like this
//!
//! `impl Global for Language` cannot be written here — both the trait and the
//! type are foreign to this crate — so the global is the [`ActiveLanguage`]
//! newtype instead. For the same reason `current` and `set` cannot be inherent
//! methods on `Language` any more; they are a trait's associated functions,
//! which is what lets `Language::current(cx)` keep working unchanged wherever
//! [`LanguageExt`] is in scope.

use std::borrow::Cow;

use gpui::{App, Global, SharedString};

pub use dodo_i18n::*;

/// The active language, as a gpui global.
///
/// A newtype rather than [`Language`] itself only because of the orphan rule —
/// see the module doc. Nothing but [`LanguageExt`] should read or write it.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct ActiveLanguage(pub Language);

impl Global for ActiveLanguage {}

/// `Language::current` and `Language::set`, which need an `App`.
pub trait LanguageExt {
    /// The active language. Defaults to English until [`LanguageExt::set`]
    /// runs.
    fn current(cx: &App) -> Language;

    /// Switches language and repaints every window so already-rendered strings
    /// pick the new column up.
    fn set(self, cx: &mut App);
}

impl LanguageExt for Language {
    fn current(cx: &App) -> Language {
        cx.try_global::<ActiveLanguage>()
            .map(|active| active.0)
            .unwrap_or_default()
    }

    fn set(self, cx: &mut App) {
        cx.set_global(ActiveLanguage(self));
        cx.refresh_windows();
    }
}

/// Translates `str` into the active language.
///
/// Takes `impl Into<Str>` so a call site names its own area's catalogue —
/// `t(cleaner::Text::Scan, cx)` — rather than the sum type.
pub fn t(str: impl Into<Str>, cx: &App) -> SharedString {
    match str.into().text(Language::current(cx)) {
        Cow::Borrowed(text) => SharedString::new_static(text),
        Cow::Owned(text) => SharedString::from(text),
    }
}
