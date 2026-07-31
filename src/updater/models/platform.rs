//! Which `update.json` entry describes *this* binary.
//!
//! The manifest keys its `files` map by platform — `macos-arm64`, `macos-x64`,
//! `linux-x64`, `windows-x64` — and those four strings are decided by
//! `scripts/package.sh` / `scripts/package.ps1` and mirrored by
//! `tools/update-manifest`'s own `Platform` table. This is the *reading* half of
//! the same table, and it is deliberately a separate copy: the generator is a
//! standalone crate that is not part of dodo (`AGENTS.md`), so there is nothing
//! to share, and a client that silently agreed with a generator it cannot see
//! would be worse than one whose table is tested against the four triples.
//!
//! # Derived from the target triple, not from `cfg`
//!
//! [`PlatformKey::current`] reads
//! [`VERSION_INFO.target`](crate::build_info::VERSION_INFO) — the triple
//! `build.rs` embedded — for the same reason
//! [`paths::HostOs`](crate::paths::HostOs) does: two of the four release
//! targets cannot be compiled from the machine this is written on, so a
//! `cfg`-split table would ship two branches nobody ever executed. As data,
//! every row is tested.

use crate::build_info::VERSION_INFO;

/// A platform key as it appears in `update.json`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PlatformKey {
    MacosArm64,
    MacosX64,
    LinuxX64,
    WindowsX64,
}

impl PlatformKey {
    /// Every platform dodo publishes for.
    pub const ALL: [PlatformKey; 4] = [
        PlatformKey::MacosArm64,
        PlatformKey::MacosX64,
        PlatformKey::LinuxX64,
        PlatformKey::WindowsX64,
    ];

    /// The string the manifest uses as a `files` key.
    pub fn key(self) -> &'static str {
        match self {
            PlatformKey::MacosArm64 => "macos-arm64",
            PlatformKey::MacosX64 => "macos-x64",
            PlatformKey::LinuxX64 => "linux-x64",
            PlatformKey::WindowsX64 => "windows-x64",
        }
    }

    /// Reads a key out of a manifest. `None` for anything not in the table —
    /// see [`Manifest::file_for`](super::manifest::Manifest::file_for) for why
    /// an unknown key is not a parse error.
    pub fn parse(value: &str) -> Option<PlatformKey> {
        PlatformKey::ALL.into_iter().find(|p| p.key() == value)
    }

    /// The platform a Rust target triple describes, or `None` for a target dodo
    /// publishes no archive for.
    ///
    /// `None` is a real answer, not a failure mode to paper over: somebody
    /// running a `linux-arm64` build they compiled themselves has no manifest
    /// entry to update from, and the honest thing is to say so rather than to
    /// hand them the x64 archive.
    pub fn from_target(target: &str) -> Option<PlatformKey> {
        match target {
            "aarch64-apple-darwin" => Some(PlatformKey::MacosArm64),
            "x86_64-apple-darwin" => Some(PlatformKey::MacosX64),
            "x86_64-unknown-linux-gnu" | "x86_64-unknown-linux-musl" => Some(PlatformKey::LinuxX64),
            "x86_64-pc-windows-msvc" | "x86_64-pc-windows-gnu" => Some(PlatformKey::WindowsX64),
            _ => None,
        }
    }

    /// The platform this binary was built for.
    pub fn current() -> Option<PlatformKey> {
        PlatformKey::from_target(VERSION_INFO.target)
    }

    /// The archive extension this platform's release asset carries. Windows
    /// zips; everything else tars, because a `.zip` does not preserve the
    /// executable bit. Mirrors `tools/update-manifest`'s table.
    pub fn archive_extension(self) -> &'static str {
        match self {
            PlatformKey::WindowsX64 => ".zip",
            _ => ".tar.gz",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::PlatformKey;

    #[test]
    fn every_release_triple_maps_to_its_manifest_key() {
        for (target, expected) in [
            ("aarch64-apple-darwin", PlatformKey::MacosArm64),
            ("x86_64-apple-darwin", PlatformKey::MacosX64),
            ("x86_64-unknown-linux-gnu", PlatformKey::LinuxX64),
            ("x86_64-pc-windows-msvc", PlatformKey::WindowsX64),
        ] {
            assert_eq!(PlatformKey::from_target(target), Some(expected), "{target}");
        }
    }

    /// The keys are a wire contract with `tools/update-manifest` and with every
    /// manifest already published. Changing one silently strands those users.
    #[test]
    fn the_keys_are_the_ones_in_the_published_manifest() {
        assert_eq!(
            PlatformKey::ALL.map(PlatformKey::key),
            ["macos-arm64", "macos-x64", "linux-x64", "windows-x64"]
        );
    }

    #[test]
    fn a_target_with_no_release_archive_has_no_key() {
        for target in [
            "aarch64-unknown-linux-gnu",
            "aarch64-pc-windows-msvc",
            "x86_64-unknown-freebsd",
        ] {
            assert_eq!(
                PlatformKey::from_target(target),
                None,
                "{target} publishes no archive; it must not be handed another platform's"
            );
        }
    }

    #[test]
    fn an_unknown_manifest_key_parses_to_nothing() {
        assert_eq!(
            PlatformKey::parse("macos-arm64"),
            Some(PlatformKey::MacosArm64)
        );
        assert_eq!(PlatformKey::parse("freebsd-x64"), None);
        assert_eq!(PlatformKey::parse(""), None);
        assert_eq!(PlatformKey::parse("MACOS-ARM64"), None);
    }

    #[test]
    fn only_windows_ships_a_zip() {
        for key in PlatformKey::ALL {
            let expected = if key == PlatformKey::WindowsX64 {
                ".zip"
            } else {
                ".tar.gz"
            };
            assert_eq!(key.archive_extension(), expected, "{}", key.key());
        }
    }

    /// This binary must be able to find itself in a manifest, or the updater is
    /// dead code on the platform it was compiled for.
    #[test]
    fn this_build_knows_which_platform_it_is() {
        assert!(
            PlatformKey::current().is_some(),
            "no manifest key for the target this test was built for"
        );
    }
}
