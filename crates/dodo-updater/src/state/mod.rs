//! The updater's state, and nothing else.
//!
//! - [`machine`] — [`UpdaterMachine`](machine::UpdaterMachine), which owns
//!   every transition. It is GPUI-free on purpose: the dialog holds one and
//!   applies events to it, so the whole of "what happens next" is a pure
//!   function of `(state, event)` and every rule in it is a unit test.
//!
//! One module, where `api_explorer::state` and `docker::state` have several,
//! because the updater has exactly one thing to remember. If a later round adds
//! a history of checks or a queue of channels, they belong beside it rather
//! than inside it.

pub mod machine;
