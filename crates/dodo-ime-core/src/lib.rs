//! Typing a language the physical keyboard does not have — dodo's own input
//! method, as pure logic.
//!
//! This crate ships the part that has nothing to do with any operating system:
//! a normalized key/action vocabulary ([`core`]) and one language engine
//! ([`languages::vietnamese`]) that speaks it. Later rounds add settings, tray
//! wiring, per-application language memory, abbreviations, and three native
//! hosts — a macOS InputMethodKit bundle, a Windows TSF DLL, an IBus engine on
//! Linux. **Nothing here touches the OS, the UI, the tray or settings**, and
//! [`purity_lint`] is a test that keeps it that way.
//!
//! # Why this is a crate and not a module of dodo
//!
//! It was a module — `src/input_method/` — for exactly one round. The macOS
//! investigation settled that the input method has to be a **separate `.app`
//! bundle** that macOS launches, and Windows and Linux load their hosts into
//! *other people's processes*. All three link this code; none of them may link
//! gpui, tree-sitter, bollard or reqwest. A module of the `dodo` binary crate
//! cannot be linked by anything, so the boundary that `purity_lint` had been
//! asserting on paper is now the crate graph. dodo depends on this crate; this
//! crate depends on `unicode-normalization` and nothing else.
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
//! # Typing at it, today, with nothing installed
//!
//! No OS host exists yet, so nothing on the machine can route a keystroke here.
//! `examples/telex.rs` is the way in — it performs the actions the engine
//! returns against a `String`, which is all an OS host really does:
//!
//! ```text
//! cargo run -p dodo-ime-core --example telex                    # interactive
//! cargo run -p dodo-ime-core --example telex -- --keys tieengs   # → tiếng
//! cargo run -p dodo-ime-core --example telex -- --keys w -v      # + the actions
//! ```
//!
//! `--scheme vni`, `--tones traditional`, `--output direct` and
//! `--no-spell-check` reach the rest of [`VietnameseConfig`]; `--help` lists
//! them. It is an `examples/` target, so it is compiled only when asked for and
//! costs the shipped binary nothing; its header comment is the authority on the
//! rest, including why it reads lines rather than raw keystrokes.
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

// The module-wide `#![allow(dead_code)]` this file carried as `src/input_method/
// mod.rs` is gone, and its removal condition was never met — the crate boundary
// simply made it unnecessary. Everything here is `pub` in a library, so it is
// reachable by definition and nothing is dead; a genuinely unused private helper
// will now be reported rather than hidden.

pub mod core;
pub mod languages;

/// Guards the rule that makes the OS hosts linkable; test-only.
#[cfg(test)]
mod purity_lint;

/// A host simulator, so a key sequence can be replayed without dodo running.
#[cfg(test)]
mod testing;

// The crate's public face, for the tray/settings wiring and the OS hosts that
// later rounds add.
pub use self::core::{
    Candidate, CandidateList, Composition, EngineAction, EngineResult, Key, KeyEvent,
    LanguageEngine, LanguageId, Modifiers,
};
pub use self::languages::vietnamese::{
    InputScheme, OutputMode, TonePlacement, VietnameseConfig, VietnameseEngine,
};
