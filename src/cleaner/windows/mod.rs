//! Windows Cleaner implementations: the four generic categories (System
//! Junk, User Cache, Trash Bins, Large & Old Files) that have a meaningful,
//! honest equivalent on Windows. Every macOS-only category — Mail, Xcode,
//! Homebrew, Installed Apps/uninstall review, Orphaned Files, AI Apps,
//! Universal Binaries, Node Tooling Cache, Docker Cache, Language Files —
//! has no scanner here at all; `CleanerView::pending_result` already turns a
//! missing scanner into "planned but not implemented yet" rather than a
//! silent gap, so nothing here needs to say so again.
//!
//! **None of this has been compiled, let alone run, on a real Windows
//! host.** The Windows `cargo check` row is one of the two this Mac cannot
//! even cross-*check* — `aws-lc-sys`'s C build script needs `windows.h` and
//! there is no cross C toolchain for it here (see "Two of the four `cargo
//! check` targets…" in the project's root doc) — so this module has had
//! nothing stronger than careful reading and mirroring already-shipped
//! macOS/generic code against it. Treat every claim below as unverified
//! until a captain builds and runs it, the same posture
//! `docs/windows-input-method.md` already states for the TSF host.

pub mod cleanup;
pub mod platform;
pub mod scanners;

use std::sync::Arc;

use crate::cleaner::core::scanner::CleanerScanner;

pub fn default_scanners() -> Vec<Arc<dyn CleanerScanner>> {
    scanners::default_scanners()
}
