//! Which build this is, handed in by the binary.
//!
//! The updater needs exactly two facts about the running executable and no
//! others: its **version**, to decide whether a manifest names something newer,
//! and its **target triple**, to decide which `update.json` entry describes it.
//! Both are `env!` values dodo's own `build.rs` sets into
//! `build_info::VERSION_INFO`, and a library crate is handed none of them —
//! nor may it grow a build script of its own to re-derive one (`AGENTS.md`).
//! So `main.rs` hands them over once, at [`crate::init`], the same way its
//! `paths` module hands `dodo-paths` the one impure input those pure rules
//! take.
//!
//! # The fallbacks are what `cargo test` sees, and only that
//!
//! Nothing calls [`crate::init`] under `cargo test`, and the crate's own tests
//! still have to be able to build a manifest for the platform they are running
//! on — so [`target`] falls back to naming the platform with `cfg!`, the same
//! trick [`crate::paths::current`] uses, and [`version`] falls back to this
//! crate's own `CARGO_PKG_VERSION`, which is *a* version rather than the
//! running app's. dodo's `main.rs` carries the test that keeps the `cfg!`
//! spelling and the embedded triple one answer, and the one that keeps the
//! embedded version parseable — both of which used to live in this crate,
//! where `VERSION_INFO` was reachable.
//!
//! In the application neither fallback is reachable: `run_app` calls
//! [`crate::init`] before it builds a window, and every reader below runs from
//! the dialog or the scheduled check, both of which come after.

use std::sync::OnceLock;

/// What `main.rs` hands over: the two `VERSION_INFO` fields the updater reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildInfo {
    /// `VERSION_INFO.version` — the running app's SemVer.
    pub version: &'static str,
    /// `VERSION_INFO.target` — the triple `build.rs` embedded.
    pub target: &'static str,
}

static BUILD_INFO: OnceLock<BuildInfo> = OnceLock::new();

/// Records what this binary is. Called once, from [`crate::init`]; a second
/// call is ignored rather than a panic, because losing an update check is a
/// worse outcome than a duplicate startup.
pub fn set(info: BuildInfo) {
    let _ = BUILD_INFO.set(info);
}

/// The running app's version — see the module doc for what a test sees.
pub fn version() -> &'static str {
    BUILD_INFO
        .get()
        .map_or(env!("CARGO_PKG_VERSION"), |info| info.version)
}

/// The running app's target triple — see the module doc for what a test sees.
pub fn target() -> &'static str {
    BUILD_INFO
        .get()
        .map_or(compiled_target(), |info| info.target)
}

/// The triple this crate was compiled for, named the way
/// [`crate::paths::current`] names the platform.
///
/// It is deliberately allowed to disagree with the embedded triple in its
/// *spelling* — `x86_64-pc-windows-gnu` and `x86_64-pc-windows-msvc` are one
/// [`PlatformKey`](crate::models::platform::PlatformKey), as are the two Linux
/// libc spellings — because the only thing anything does with this string is
/// classify it. `main.rs`'s test compares the classifications, not the strings.
const fn compiled_target() -> &'static str {
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "aarch64-apple-darwin"
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        "x86_64-apple-darwin"
    } else if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        "x86_64-pc-windows-msvc"
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        "x86_64-unknown-linux-gnu"
    } else {
        // The honest answer for a build dodo publishes no archive for — see
        // `PlatformKey::from_target`, for which `None` is a real answer.
        "unknown"
    }
}

#[cfg(test)]
mod tests {
    use super::{compiled_target, target, version};
    use crate::models::platform::PlatformKey;

    /// The fallback has to be a platform the manifest table knows, on every
    /// machine this suite runs on — the pipeline tests build a manifest for it.
    #[test]
    fn the_compiled_target_is_one_the_manifest_table_knows() {
        assert!(
            PlatformKey::from_target(compiled_target()).is_some(),
            "no manifest key for the target this test was built for: {}",
            compiled_target()
        );
    }

    /// Unset — which is what `cargo test` leaves it — both readers answer from
    /// the fallbacks rather than panicking.
    #[test]
    fn an_unset_build_answers_from_the_fallbacks() {
        assert_eq!(target(), compiled_target());
        assert_eq!(version(), env!("CARGO_PKG_VERSION"));
    }
}
