//! The three pieces of localization that need the UI framework, behind the
//! `gpui` feature.
//!
//! The catalogue in the rest of this crate is plain data and depends on
//! nothing, which is the point of it: a pure model holds a [`Str`] and a test
//! renders it with no `App` and no frame. These three cannot be:
//!
//! - [`ActiveLanguage`], the gpui global holding the current choice;
//! - [`LanguageExt`], which puts `Language::current` / `set` back where every
//!   call site already spells them;
//! - [`t`], which renders a [`Str`] into a `SharedString` for a view.
//!
//! # Why they are here rather than in the binary
//!
//! They were `src/i18n.rs` until `crates/dodo-cleaner` came out of the binary.
//! A gpui `Global` is identified by its *type*, so a second crate that defined
//! its own `ActiveLanguage` would be reading a different global and would not
//! see a language change at all; there can only be one, and it has to be
//! somewhere both the binary and every feature crate can name. This is the only
//! place that is, which is why the feature exists. It is off by default, so
//! `cargo build -p dodo-i18n` still resolves to a crate with no dependencies
//! whatsoever.
//!
//! # Why the two halves are shaped like this
//!
//! `impl Global for Language` cannot be written by a *consumer* of this crate —
//! both the trait and the type would be foreign to it — so the global is the
//! [`ActiveLanguage`] newtype, which is what the shape below is really about.
//! For the same reason `current` and `set` are a trait's associated functions
//! rather than inherent methods, which is what lets `Language::current(cx)`
//! keep working unchanged wherever [`LanguageExt`] is in scope.

use std::borrow::Cow;

use gpui::{App, Global, SharedString};

use crate::{Language, Str};

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
