//! dodo's macOS input method: the InputMethodKit host for
//! [`dodo_ime_core`](dodo_ime_core).
//!
//! macOS does not load an input method into your application. It launches a
//! *second application* — a faceless background agent in `~/Library/Input
//! Methods` — and hands it every keystroke the user types anywhere, once they
//! have selected it in the input-source menu. This crate is that application.
//! `Dodo.app` does not start it, does not talk to it, and does not need to be
//! running for it to type.
//!
//! # The shape of one keystroke
//!
//! ```text
//!   -[IMKInputController inputText:key:modifiers:client:]
//!        │
//!        ├─ keymap::key_event ────▶ KeyEvent          (pure)
//!        ├─ Session::key ────────▶ EngineAction…      (pure, in dodo-ime-core)
//!        ├─ ops::translate ──────▶ ClientOp…          (pure)
//!        └─ client::perform ─────▶ IMKTextInput       (Objective-C)
//! ```
//!
//! Three of those four hops are ordinary Rust that runs in a unit test on any
//! platform, which is deliberate: everything that could get Vietnamese *wrong*
//! — which key is which, where the caret sits inside `tiếng`, how many UTF-16
//! units one grapheme costs, what order a commit's two calls go in — lives in
//! [`keymap`], [`text`], [`ops`] and [`session`], and is tested without a
//! window server. [`client`] and [`controller`] are the Objective-C boundary and
//! hold no decisions.
//!
//! # Why this is its own crate, and where it lives
//!
//! It is `crates/dodo-ime-macos` rather than `platform/…` or a standalone crate
//! with its own lockfile, and the reason is the engine rather than this code: it
//! links `dodo-ime-core`, which dodo also links. A second `Cargo.lock` would
//! resolve the engine and its one dependency independently of dodo's, so "the
//! engine the tests prove" and "the engine the shipped bundle types with" would
//! be two resolutions nothing compares. The root `Cargo.toml`'s `[workspace]`
//! comment carries the same argument from the other side.
//!
//! It must not link gpui, and cannot: gpui is the `dodo` package's dependency
//! and nothing here names `dodo`. `dodo-ime-core`'s own `purity_lint` keeps the
//! engine clean; this crate is allowed AppKit because being AppKit is its job.
//!
//! # Two rules this crate is written around
//!
//! **No typing history, ever.** An input method sees every password and every
//! private message its user writes. Nothing here writes a file, opens a socket,
//! or prints a composition — there is no `println!` of user text on any path,
//! including the error paths, and the only state that outlives a keystroke is
//! the syllable currently being composed. macOS says "stop holding that" with
//! `commitComposition:` and `deactivateServer:`, and
//! [`Session::commit`](session::Session::commit) /
//! [`Session::deactivate`](session::Session::deactivate) honour both.
//!
//! **Never swallow a keystroke.** Every unexpected state resolves towards
//! letting the application have the key. The engine already works this way; the
//! host adds one more guard on top, in [`session::Response::handled`] — an
//! engine that claims a key but asks for nothing to be done hands it back
//! anyway, because a key that produces neither text nor an edit has been lost.
//!
//! # What this round does not do
//!
//! No IPC with `Dodo.app`, no settings, no tray wiring, no per-application
//! language memory, no install action. The engine runs on compiled-in defaults
//! ([`DEFAULT_CONFIG`]). `docs/macos-input-method.md` is the authority on
//! building, installing and enabling it by hand, and on what the next round
//! owes.

pub mod bundle;
pub mod keymap;
pub mod ops;
pub mod session;
pub mod text;

#[cfg(target_os = "macos")]
pub mod client;
#[cfg(target_os = "macos")]
pub mod controller;

pub use self::ops::ClientOp;
pub use self::session::{Response, Session};

use dodo_ime_core::{InputScheme, OutputMode, TonePlacement, VietnameseConfig};

/// The engine configuration the bundle types with, until a later round gives it
/// a settings file to read.
///
/// Telex and modern tone placement are Unikey's defaults, which is what a
/// Vietnamese typist's fingers already expect. The one field that is a *macOS*
/// decision rather than a taste is [`OutputMode::Composition`]: see
/// [`ops`] for why the direct-typing mode exists and why no macOS client
/// ever selects it.
pub const DEFAULT_CONFIG: VietnameseConfig = VietnameseConfig {
    scheme: InputScheme::Telex,
    tone_placement: TonePlacement::Modern,
    output: OutputMode::Composition,
    spell_check: true,
    bracket_shortcuts: true,
};

#[cfg(test)]
mod tests {
    use super::DEFAULT_CONFIG;
    use dodo_ime_core::{InputScheme, OutputMode, TonePlacement, VietnameseConfig};

    /// The brief's "sensible built-in defaults (Telex, modern tone placement)",
    /// as an assertion rather than a comment.
    #[test]
    fn the_built_in_defaults_are_telex_and_modern_tone_placement() {
        // Read through a binding rather than the constant: an `assert!` on a
        // `const` field is a compile-time tautology and clippy rejects it.
        let config = std::hint::black_box(DEFAULT_CONFIG);
        assert_eq!(config.scheme, InputScheme::Telex);
        assert_eq!(config.tone_placement, TonePlacement::Modern);
        assert!(config.spell_check);
        assert!(config.bracket_shortcuts);
    }

    /// macOS always has a marked-text channel, so the host never selects the
    /// direct-typing mode. `ops` explains what that buys; this is the line that
    /// would fail if someone flipped it.
    #[test]
    fn macos_always_composes_and_never_types_directly() {
        assert_eq!(DEFAULT_CONFIG.output, OutputMode::Composition);
    }

    /// A field added to `VietnameseConfig` upstream must be considered here
    /// rather than inherited: this constant is a `const`, so a new field is a
    /// compile error, and this test is where someone reads why.
    #[test]
    fn the_defaults_agree_with_the_engines_own_except_where_stated() {
        assert_eq!(DEFAULT_CONFIG, VietnameseConfig::default());
    }
}
