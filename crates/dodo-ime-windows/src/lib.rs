//! dodo's Windows Text Services Framework host.
//!
//! Windows loads this DLL into the text-service process; it is not linked by
//! dodo and keeps working after dodo exits. It links only the pure engine and
//! IPC settings contract. The optional Keyboard Hook fallback lives in dodo
//! itself and is deliberately a different, dodo-lifetime-only backend.
//!
//! The Windows-only COM code is a thin adapter. `keymap` is the one mapping from
//! Windows virtual keys to the engine vocabulary, and `service` is the one place
//! where an engine result becomes a TSF composition edit session. Neither logs,
//! persists, or transmits text.

pub mod keymap;

#[cfg(target_os = "windows")]
mod registration;
#[cfg(target_os = "windows")]
mod service;

#[cfg(target_os = "windows")]
pub use registration::{
    DllCanUnloadNow, DllGetClassObject, DllRegisterServer, DllUnregisterServer,
};
