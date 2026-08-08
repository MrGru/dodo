//! Everything that touches the outside world: the two files, the notification,
//! Text Input Sources, `ditto` and `pkill`.
//!
//! The containment rule the rest of dodo follows applies here too — [`tis`] is
//! the only place in the whole binary that names a Carbon function, and
//! [`notify`] the only one that posts a distributed notification. [`installer`]
//! is the driver, and it reaches the outside world only through its own
//! [`InstallOps`](installer::InstallOps) trait, which is what makes the sequence
//! testable without a Mac.

pub mod installer;
pub mod notify;
pub mod store;

#[cfg(target_os = "macos")]
pub mod tis;
