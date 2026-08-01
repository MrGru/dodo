//! The few small elements the widget library does not have.
//!
//! Everything here takes text that is **already translated** and has no opinion
//! about localization; the caller does the `t()`. That is the same contract
//! `docker::components` follows, and it is what keeps these reusable between a
//! tree, a form and a result footer without any of them sharing a `Str`.

pub mod notice;
pub mod states;
