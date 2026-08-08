//! Plain data and rules, with no gpui and no filesystem.
//!
//! One module today: [`install`], which is the install sequence as data. The
//! *settings* have no model here on purpose — they are
//! `dodo_ime_ipc::settings::VietnameseSettings`, because the input-method bundle
//! reads the same type and a dodo-side mirror of a mirror would be one more thing
//! to keep in step.

pub mod install;
