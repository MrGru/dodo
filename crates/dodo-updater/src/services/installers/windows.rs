//! Replacing the running Windows executable.
//!
//! Windows can rename a running `.exe`, so it uses the same extract,
//! rename-aside, replace, restart, and next-launch sweep as the Linux binary.
//! The shared implementation matches the replacement by the running file's own
//! name, so a renamed `dodo.exe` is still updated in place.
//!
//! The sequence is tested on every host in [`super::linux`]. Whether Windows
//! permits the rename for this executable remains a Windows runtime check.

pub use super::linux::LinuxInstaller as WindowsInstaller;
