//! Plain data and rules, with no gpui and no filesystem.
//!
//! Two modules: [`install`], the install sequence as data, and [`status`], the
//! one sentence the tool says about itself. The *settings* have no model here on
//! purpose — they are `dodo_ime_ipc::settings::VietnameseSettings`, because the
//! input-method bundle reads the same type and a dodo-side mirror of a mirror
//! would be one more thing to keep in step.

pub mod direct_output;
pub mod event_tap;
pub mod install;
pub mod keyboard_hook;
pub mod status;
pub mod windows;
