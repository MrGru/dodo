//! Typing a language the physical keyboard does not have — dodo's own input
//! method, as pure logic.
//!
//! Round 1 ships the part that has nothing to do with any operating system: a
//! normalized key/action vocabulary ([`core`]) and one language engine
//! ([`languages::vietnamese`]) that speaks it. Later rounds add settings, tray
//! wiring, per-application language memory, abbreviations, and three native
//! hosts — a macOS InputMethodKit bundle, a Windows TSF DLL, an IBus engine on
//! Linux. **Nothing here touches the OS, the UI, the tray or settings**, and
//! [`purity_lint`] is a test that keeps it that way.
//!
//! # The shape of it
//!
//! ```text
//!   OS keystroke ──normalize──▶ KeyEvent ──▶ LanguageEngine ──▶ EngineAction
//!   (NSEvent, WM_KEYDOWN,                    (per language)      (what the
//!    IBus keysym)                                                 host does)
//! ```
//!
//! A host normalizes; an engine decides; the host performs. The engine never
//! learns which host it is under, and the host never learns any Vietnamese.
//! Following one keystroke from [`core::KeyEvent`] to a [`core::EngineAction`]
//! is two hops and no indirection: [`languages::vietnamese::InputScheme`] is a
//! plain enum, not a trait object, and the only trait in the module is
//! [`core::LanguageEngine`] — which exists because Korean, Japanese and Chinese
//! are a genuine future substitution at that seam, and nothing else is.
//!
//! # No typing history. Ever.
//!
//! An input method sees every password, every private message and every
//! half-written thought its user has. So this module **persists nothing, logs
//! nothing, and prints nothing**. There is no file under `data_dir()` for it,
//! no `eprintln!` of composition text on any path, no counter of what was
//! typed, no crash report carrying a syllable. The only thing that ever leaves
//! an engine is the [`core::EngineAction`] list for the keystroke in front of
//! it, and the only thing an engine remembers is the syllable currently being
//! composed — discarded at the next word boundary. Anything that would keep
//! user text beyond that boundary is a bug, not a feature, whatever it would
//! be useful for.
//!
//! # Losing a keystroke is the worst thing this code can do
//!
//! Every fallback in here resolves towards *pass the key through*. A syllable
//! the engine cannot represent, a mark with no vowel to land on, a state that
//! should be unreachable: all of them emit the key as typed rather than
//! swallowing it. See [`languages::vietnamese::VietnameseEngine::process_key`]
//! for where that rule actually lives.

// The whole module is round 1 of a long feature: nothing in dodo calls it yet,
// because the caller is the tray/settings wiring and the three OS hosts that
// later rounds add. Every item here is exercised by this module's own tests, so
// "dead" means *not yet wired*, not *unreachable* — the repo's rule for a module
// under construction (AGENTS.md: "annotate, do not delete"). Remove this the
// round something outside `input_method` constructs an engine; the compiler will
// then name anything that really did go unused.
#![allow(dead_code)]

pub mod core;
pub mod languages;

/// Guards the rule that makes this module extractable into its own crate;
/// test-only.
#[cfg(test)]
mod purity_lint;

/// A host simulator, so a key sequence can be replayed without dodo running.
#[cfg(test)]
mod testing;

// The module's public face, for the tray/settings wiring and the OS hosts that
// later rounds add. Nothing outside `input_method` imports it yet, which is the
// same "not wired" state the `dead_code` allow above covers; both come off
// together.
#[allow(unused_imports)]
pub use self::core::{
    Candidate, CandidateList, Composition, EngineAction, EngineResult, Key, KeyEvent,
    LanguageEngine, LanguageId, Modifiers,
};
#[allow(unused_imports)]
pub use self::languages::vietnamese::{
    InputScheme, OutputMode, TonePlacement, VietnameseConfig, VietnameseEngine,
};
