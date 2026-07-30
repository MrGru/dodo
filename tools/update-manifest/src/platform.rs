//! Which file is which platform — decided by exact filename, nothing else.
//!
//! The rule this module exists to enforce: a release version determines the
//! complete set of filenames the release may contain, so classification is a
//! lookup in a table built from `--version`, not a pattern match against
//! whatever happens to be on disk.
//!
//! That matters for two reasons.
//!
//! * **The `-app` selection has to be by name.** Each macOS platform publishes
//!   two archives — `dodo-v1.2.3-macos-arm64.tar.gz` (the bare binary) and
//!   `dodo-v1.2.3-macos-arm64-app.tar.gz` (the `.app` bundle). The manifest must
//!   point at the bundle, because the in-app updater swaps the `.app`. A glob or
//!   a "last one wins" walk gets this right only by accident of ordering;
//!   [`Platform::manifest_kind`] states it.
//! * **An unrecognised file must fail the run**, and "unrecognised" has to
//!   include a *plausible* name — a leftover `dodo-v1.2.2-linux-x64.tar.gz` from
//!   a previous version in the same directory is exactly the kind of thing that
//!   should stop a release, and a loose parse would happily accept it as
//!   `linux-x64`.
//!
//! The names themselves come from `scripts/package.sh` and
//! `scripts/package.ps1`; those two files are the source of truth and this table
//! mirrors them.

use std::collections::BTreeMap;
use std::fmt;

/// A platform key as it appears in `update.json` and on the `--expect-platform`
/// command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Platform {
    MacosArm64,
    MacosX64,
    LinuxX64,
    WindowsX64,
}

impl Platform {
    /// Every platform dodo releases for, in manifest order.
    pub const ALL: [Platform; 4] = [
        Platform::MacosArm64,
        Platform::MacosX64,
        Platform::LinuxX64,
        Platform::WindowsX64,
    ];

    /// The string form used everywhere a human or a JSON document sees it.
    pub fn key(self) -> &'static str {
        match self {
            Platform::MacosArm64 => "macos-arm64",
            Platform::MacosX64 => "macos-x64",
            Platform::LinuxX64 => "linux-x64",
            Platform::WindowsX64 => "windows-x64",
        }
    }

    /// Parses a `--expect-platform` value. The error lists the valid keys,
    /// because a typo here is otherwise indistinguishable from a genuinely
    /// missing artifact.
    pub fn parse(value: &str) -> Result<Platform, String> {
        Platform::ALL
            .into_iter()
            .find(|p| p.key() == value)
            .ok_or_else(|| {
                let valid: Vec<&str> = Platform::ALL.iter().map(|p| p.key()).collect();
                format!(
                    "unknown platform key `{value}` (valid keys: {})",
                    valid.join(", ")
                )
            })
    }

    /// The archive format this platform ships in. Windows zips; everything else
    /// tars, because a `.zip` does not preserve the executable bit.
    fn archive_extension(self) -> &'static str {
        match self {
            Platform::WindowsX64 => ".zip",
            _ => ".tar.gz",
        }
    }

    /// Whether this platform also publishes a `.app` bundle archive.
    fn has_app_bundle(self) -> bool {
        matches!(self, Platform::MacosArm64 | Platform::MacosX64)
    }

    /// Which of a platform's archives the manifest points at.
    ///
    /// macOS points at the bundle: the updater replaces `dodo.app`, so handing
    /// it the bare binary would install something the user cannot launch from
    /// the Dock. Every other platform has exactly one archive.
    pub fn manifest_kind(self) -> ArtifactKind {
        if self.has_app_bundle() {
            ArtifactKind::AppBundle
        } else {
            ArtifactKind::Plain
        }
    }
}

impl fmt::Display for Platform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.key())
    }
}

/// Which of a platform's two possible archives a file is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ArtifactKind {
    /// The binary and its documentation.
    Plain,
    /// macOS only: `dodo.app`.
    AppBundle,
}

impl fmt::Display for ArtifactKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ArtifactKind::Plain => f.write_str("binary archive"),
            ArtifactKind::AppBundle => f.write_str("app bundle"),
        }
    }
}

/// One archive a release is expected to contain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpectedArtifact {
    pub platform: Platform,
    pub kind: ArtifactKind,
    pub file_name: String,
}

/// Every archive filename a release of `version` may legitimately contain.
///
/// This is the whole vocabulary: a file in the artifact directory that is not
/// one of these, and is not one of their `.sha256` sidecars or a known
/// generated file, fails the run.
pub fn expected_artifacts(version: &str) -> Vec<ExpectedArtifact> {
    let mut out = Vec::new();
    for platform in Platform::ALL {
        out.push(ExpectedArtifact {
            platform,
            kind: ArtifactKind::Plain,
            file_name: format!(
                "dodo-v{version}-{}{}",
                platform.key(),
                platform.archive_extension()
            ),
        });
        if platform.has_app_bundle() {
            out.push(ExpectedArtifact {
                platform,
                kind: ArtifactKind::AppBundle,
                file_name: format!("dodo-v{version}-{}-app.tar.gz", platform.key()),
            });
        }
    }
    out
}

/// The `expected_artifacts` list keyed by filename, for classifying a directory
/// entry in one lookup.
pub fn artifact_index(version: &str) -> BTreeMap<String, ExpectedArtifact> {
    expected_artifacts(version)
        .into_iter()
        .map(|a| (a.file_name.clone(), a))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_round_trip_through_parse() {
        for platform in Platform::ALL {
            assert_eq!(Platform::parse(platform.key()), Ok(platform));
        }
    }

    #[test]
    fn unknown_platform_key_names_the_valid_ones() {
        let err = Platform::parse("macos-arm").expect_err("should reject");
        assert!(err.contains("macos-arm"), "{err}");
        assert!(err.contains("macos-arm64"), "{err}");
        assert!(err.contains("windows-x64"), "{err}");
    }

    #[test]
    fn macos_points_at_the_app_bundle_and_the_others_at_the_binary() {
        assert_eq!(
            Platform::MacosArm64.manifest_kind(),
            ArtifactKind::AppBundle
        );
        assert_eq!(Platform::MacosX64.manifest_kind(), ArtifactKind::AppBundle);
        assert_eq!(Platform::LinuxX64.manifest_kind(), ArtifactKind::Plain);
        assert_eq!(Platform::WindowsX64.manifest_kind(), ArtifactKind::Plain);
    }

    #[test]
    fn expected_names_match_the_packaging_scripts() {
        let index = artifact_index("0.2.0");
        let names: Vec<&str> = index.keys().map(String::as_str).collect();
        assert_eq!(
            names,
            vec![
                "dodo-v0.2.0-linux-x64.tar.gz",
                "dodo-v0.2.0-macos-arm64-app.tar.gz",
                "dodo-v0.2.0-macos-arm64.tar.gz",
                "dodo-v0.2.0-macos-x64-app.tar.gz",
                "dodo-v0.2.0-macos-x64.tar.gz",
                "dodo-v0.2.0-windows-x64.zip",
            ]
        );
    }

    #[test]
    fn the_bare_macos_archive_and_the_bundle_are_different_artifacts() {
        let index = artifact_index("0.2.0");
        let bare = &index["dodo-v0.2.0-macos-arm64.tar.gz"];
        let app = &index["dodo-v0.2.0-macos-arm64-app.tar.gz"];
        assert_eq!(bare.platform, Platform::MacosArm64);
        assert_eq!(app.platform, Platform::MacosArm64);
        assert_eq!(bare.kind, ArtifactKind::Plain);
        assert_eq!(app.kind, ArtifactKind::AppBundle);
    }

    #[test]
    fn an_archive_from_another_version_is_not_in_the_vocabulary() {
        let index = artifact_index("0.2.0");
        assert!(!index.contains_key("dodo-v0.1.9-linux-x64.tar.gz"));
    }

    #[test]
    fn linux_and_windows_publish_no_app_bundle() {
        let index = artifact_index("0.2.0");
        assert!(!index.contains_key("dodo-v0.2.0-linux-x64-app.tar.gz"));
        assert!(!index.contains_key("dodo-v0.2.0-windows-x64-app.tar.gz"));
    }
}
